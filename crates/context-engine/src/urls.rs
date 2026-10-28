//! Exact-resource HTTP acquisition with explicit operator egress and tracked staging.

use crate::{Application, Error, Result, uploads::CreateUpload};
use context_core::{
    access::{Authentication, Permission, Resource},
    ids::{AssetId, CollectionId, TenantId},
};
use context_storage::postgres::{existing_receipt, receipt};
use futures_util::TryStreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{net::SocketAddr, time::Duration};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

/// An explicit resource import; redirects and embedded resources are not fetched.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestUrl {
    /// Destination collection.
    pub collection: CollectionId,
    /// Logical asset to replace, when specified.
    pub asset: Option<AssetId>,
    /// Source label, not a host path.
    pub name: String,
    /// Absolute allowlisted HTTP(S) URL without userinfo.
    pub url: String,
    /// Declared media type, verified by the configured extractor.
    pub media_type: String,
    /// Optional operator profile.
    pub profile: Option<String>,
    /// Stable acquisition intent key.
    pub idempotency_key: String,
}
impl Application {
    /// Stream an exact resource through the same verified upload/admission boundary.
    ///
    /// # Errors
    /// Rejects disallowed egress, redirects, oversized resources, lost authorization,
    /// conflicting receipts, unavailable infrastructure and extraction capabilities.
    pub async fn ingest_url(&self, actor: &Authentication, args: IngestUrl) -> Result<Value> {
        let plane = self.data_plane()?;
        if args.url.len() > 8192
            || args.name.chars().count() > 256
            || args.idempotency_key.is_empty()
            || args.idempotency_key.len() > 200
            || args.name.trim().is_empty()
        {
            return Err(Error::Invalid);
        }
        let url = reqwest::Url::parse(&args.url).map_err(|_| Error::Invalid)?;
        let host = url.host_str().ok_or(Error::Invalid)?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || !plane.config.url_hosts.contains(host)
            || !(url.scheme() == "https" || (url.scheme() == "http" && plane.config.url_allow_http))
        {
            return Err(Error::Invalid);
        }
        let hash: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&(
                args.collection,
                args.asset,
                &args.name,
                &args.url,
                &args.media_type,
                &args.profile,
            ))
            .map_err(|_| Error::Invalid)?,
        )
        .into();
        let mut auth = self
            .database
            .authorized_write(
                actor,
                Resource::Collection(args.collection),
                Permission::Ingest,
            )
            .await?;
        if let Some(value) = existing_receipt(
            auth.transaction(),
            actor.principal,
            "ingest_url",
            &args.idempotency_key,
            &hash,
        )
        .await?
        {
            auth.commit().await?;
            return Ok(value);
        }
        let resuming = existing_receipt(
            auth.transaction(),
            actor.principal,
            "ingest_url_intent",
            &args.idempotency_key,
            &hash,
        )
        .await?
        .is_some();
        receipt(
            auth.transaction(),
            actor.principal,
            "ingest_url_intent",
            &args.idempotency_key,
            &hash,
            &json!({}),
        )
        .await?;
        if resuming &&let Some(child)=sqlx::query("SELECT u.receipt AS upload,f.request_digest,f.receipt FROM context.receipts u JOIN context.receipts f ON f.principal=u.principal WHERE u.principal=$1 AND u.operation='create_upload' AND u.idempotency_key=$2 AND f.operation='finalize_upload' AND f.idempotency_key=$3")
            .bind(Uuid::from(actor.principal)).bind(format!("{}:upload",args.idempotency_key)).bind(format!("{}:finalize",args.idempotency_key))
            .fetch_optional(&mut **auth.transaction()).await.map_err(context_storage::Error::from)? {
                let uploaded:Value=child.try_get("upload").map_err(context_storage::Error::from)?;
                let upload:Uuid=uploaded["upload"].as_str().ok_or(Error::Contract)?.parse().map_err(|_|Error::Contract)?;
                if child.try_get::<Vec<u8>,_>("request_digest").map_err(context_storage::Error::from)?!=Sha256::digest(upload.as_bytes()).as_slice(){return Err(context_storage::Error::Conflict.into());}
                let value:Value=child.try_get("receipt").map_err(context_storage::Error::from)?;
                let asset=value["asset"].as_str().ok_or(Error::Contract)?.parse().map_err(|_|Error::Contract)?;
                auth.authorize(Resource::Asset(asset),Permission::Ingest).await?;
                let value=receipt_record(auth.transaction(),actor,&args.idempotency_key,&hash,&value).await?;auth.commit().await?;return Ok(value);
            }
        let owner: Uuid = sqlx::query_scalar("SELECT owner FROM context.resources WHERE id=$1")
            .bind(Uuid::from(args.collection))
            .fetch_one(&mut **auth.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        auth.commit().await?;
        // Collection owner mutation fencing also protects acquisition from purge.
        let lock = plane.tenant_lock(TenantId::from(owner)).await;
        self.database
            .authorized_view(
                actor,
                Resource::Collection(args.collection),
                Permission::Ingest,
            )
            .await?
            .commit()
            .await?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO context.acquisitions(id,scope,principal) VALUES($1,$2,$3)")
            .bind(id)
            .bind(Uuid::from(args.collection))
            .bind(Uuid::from(actor.principal))
            .execute(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
        let file = plane.staging.join(format!("acquire-{id}"));
        let result = async {
            let port = url.port_or_known_default().ok_or(Error::Invalid)?;
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
                .await
                .map_err(context_storage::Error::from)?
                .collect();
            if addresses.is_empty() {
                return Err(Error::Invalid);
            }
            // Pin the resolved allowlisted host for this one request; no DNS rebinding.
            let http = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(120))
                .resolve_to_addrs(host, &addresses)
                .build()
                .map_err(context_storage::Error::from)?;
            let response = http
                .get(url.clone())
                .send()
                .await
                .map_err(context_storage::Error::from)?;
            if !response.status().is_success()
                || response
                    .content_length()
                    .is_some_and(|n| n > plane.config.max_upload_bytes)
            {
                return Err(Error::Invalid);
            }
            let mut options = tokio::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut output = options
                .open(&file)
                .await
                .map_err(context_storage::Error::from)?;
            let mut stream = response.bytes_stream();
            let mut size = 0_u64;
            let mut digest = Sha256::new();
            while let Some(bytes) = stream
                .try_next()
                .await
                .map_err(context_storage::Error::from)?
            {
                size = size.checked_add(bytes.len() as u64).ok_or(Error::Invalid)?;
                if size > plane.config.max_upload_bytes {
                    return Err(Error::Invalid);
                }
                digest.update(&bytes);
                output
                    .write_all(&bytes)
                    .await
                    .map_err(context_storage::Error::from)?;
            }
            output
                .sync_all()
                .await
                .map_err(context_storage::Error::from)?;
            drop(output);
            let sha256 = digest
                .finalize()
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<String>();
            Ok((size, sha256))
        }
        .await;
        // create/finalize acquire this same mutex themselves. The tracked file
        // remains purge-addressable, and each later boundary reauthorizes again.
        drop(lock);
        let admitted=async {
            let (size,sha256)=result?;
            let transfer=self.create_upload(actor,CreateUpload {collection:args.collection,asset:args.asset,name:args.name,media_type:args.media_type,
                size_bytes:size,sha256,profile:args.profile,idempotency_key:format!("{}:upload",args.idempotency_key)}).await?;
            if transfer["finalized"]!=true {
                let input=tokio::fs::File::open(&file).await.map_err(context_storage::Error::from)?;
                let http=reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(120)).build().map_err(context_storage::Error::from)?;
                let mut upload=http.put(transfer["url"].as_str().ok_or(Error::Contract)?).body(reqwest::Body::wrap_stream(tokio_util::io::ReaderStream::new(input)));
                for (key,value) in transfer["headers"].as_object().ok_or(Error::Contract)? {upload=upload.header(key,value.as_str().ok_or(Error::Contract)?);}
                upload.send().await.map_err(context_storage::Error::from)?.error_for_status().map_err(context_storage::Error::from)?;
            }
            let receipt=self.finalize_upload(actor,transfer["upload"].as_str().ok_or(Error::Contract)?.parse().map_err(|_|Error::Contract)?,&format!("{}:finalize",args.idempotency_key)).await?;
            let mut auth=self.database.authorized_write(actor,Resource::Collection(args.collection),Permission::Ingest).await?;
            let asset=receipt["asset"].as_str().ok_or(Error::Contract)?.parse().map_err(|_|Error::Contract)?;
            auth.authorize(Resource::Asset(asset),Permission::Ingest).await?;
            let mut source=url.clone();source.set_query(None);
            sqlx::query("UPDATE context.content c SET body=body||$2::jsonb FROM context.revisions v WHERE v.content=c.id AND v.id=$1")
                .bind(Uuid::parse_str(receipt["revision"].as_str().ok_or(Error::Contract)?).map_err(|_|Error::Contract)?).bind(json!({"source_url":source.as_str()}))
                .execute(&mut **auth.transaction()).await.map_err(context_storage::Error::from)?;
            let value=receipt_record(auth.transaction(),actor,&args.idempotency_key,&hash,&receipt).await?;auth.commit().await?;Ok(value)
        }.await;
        match tokio::fs::remove_file(&file).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(context_storage::Error::from(e).into()),
        }
        sqlx::query("DELETE FROM context.acquisitions WHERE id=$1")
            .bind(id)
            .execute(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
        admitted
    }

    pub(crate) async fn clear_acquisitions(&self, startup: bool) -> Result<()> {
        let Some(plane) = &self.plane else {
            return Ok(());
        };
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM context.acquisitions WHERE $1 OR expires_at<now() LIMIT 128",
        )
        .bind(startup)
        .fetch_all(self.database.pool())
        .await
        .map_err(context_storage::Error::from)?;
        for id in ids {
            match tokio::fs::remove_file(plane.staging.join(format!("acquire-{id}"))).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(context_storage::Error::from(e).into()),
            }
            sqlx::query("DELETE FROM context.acquisitions WHERE id=$1")
                .bind(id)
                .execute(self.database.pool())
                .await
                .map_err(context_storage::Error::from)?;
        }
        Ok(())
    }
}
async fn receipt_record(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &Authentication,
    key: &str,
    hash: &[u8; 32],
    value: &Value,
) -> Result<Value> {
    Ok(receipt(tx, actor.principal, "ingest_url", key, hash, &json!(value)).await?)
}
