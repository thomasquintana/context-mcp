//! Explicit upload sessions, immutable-source admission and durable job receipts.

use crate::{Application, Error, Result, data::Pipeline};
use context_core::{
    access::{Authentication, Permission, Resource},
    ids::{AssetId, CollectionId, ContentId, JobId, RevisionId, TenantId},
};
use context_storage::postgres::{existing_receipt, insert_resource, receipt};
use context_workflows::host::JobGrant;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

/// Explicit expected upload metadata; no caller authority or storage key is accepted.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateUpload {
    /// Owning collection.
    pub collection: CollectionId,
    /// Optional existing logical asset for an immutable replacement revision.
    pub asset: Option<AssetId>,
    /// Human-readable source filename/label, never a filesystem path.
    pub name: String,
    /// Declared source MIME type, confirmed by extraction.
    pub media_type: String,
    /// Exact transfer length.
    pub size_bytes: u64,
    /// Expected SHA-256 as 64 hexadecimal digits.
    pub sha256: String,
    /// Optional operator-defined profile name.
    pub profile: Option<String>,
    /// Stable request key scoped to principal/operation.
    pub idempotency_key: String,
}

pub(crate) fn digest(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Invalid);
    }
    let mut digest = [0; 32];
    for (index, out) in digest.iter_mut().enumerate() {
        *out =
            u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| Error::Invalid)?;
    }
    Ok(digest)
}

pub(crate) fn media_type(value: &str) -> Result<String> {
    let value = value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if value.len() > 128
        || value.split('/').count() != 2
        || value.split('/').any(str::is_empty)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/!#$&^_.+-".contains(&byte))
    {
        return Err(Error::Invalid);
    }
    Ok(value)
}

pub(crate) fn intent_digest(args: &CreateUpload, media: &str, hash: &[u8; 32]) -> Result<[u8; 32]> {
    let request = serde_json::to_vec(&(
        args.collection,
        args.asset,
        &args.name,
        media,
        args.size_bytes,
        hash,
        &args.profile,
    ))
    .map_err(|_| Error::Invalid)?;
    Ok(Sha256::digest(request).into())
}

impl Application {
    /// Explicitly reprocess a successful immutable source as a new revision/profile.
    ///
    /// # Errors
    /// Requires Ingest authority and the expected current revision; never overwrites
    /// an existing revision or automatically refreshes a remote resource.
    pub async fn reprocess(
        &self,
        actor: &Authentication,
        asset: AssetId,
        expected: RevisionId,
        profile: Option<String>,
        key: &str,
    ) -> Result<Value> {
        if key.is_empty() || key.len() > 200 {
            return Err(Error::Invalid);
        }
        let hash: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&(asset, expected, &profile)).map_err(|_| Error::Invalid)?,
        )
        .into();
        let mut view = self
            .database
            .authorized_write(actor, Resource::Asset(asset), Permission::Ingest)
            .await?;
        if let Some(value) =
            existing_receipt(view.transaction(), actor.principal, "reprocess", key, &hash).await?
        {
            view.commit().await?;
            return Ok(value);
        }
        let resumed = existing_receipt(
            view.transaction(),
            actor.principal,
            "reprocess_intent",
            key,
            &hash,
        )
        .await?
        .is_some();
        receipt(
            view.transaction(),
            actor.principal,
            "reprocess_intent",
            key,
            &hash,
            &json!({}),
        )
        .await?;
        let row=sqlx::query("SELECT a.current_revision,v.sha256,v.size_bytes,v.media_type,c.body,r.parent,r.owner FROM context.assets a JOIN context.resources r ON r.id=a.id JOIN context.revisions v ON v.asset=a.id JOIN context.content c ON c.id=v.content WHERE a.id=$1 AND v.id=$2 AND c.published AND NOT c.purging")
            .bind(Uuid::from(asset)).bind(Uuid::from(expected)).fetch_optional(&mut **view.transaction()).await.map_err(context_storage::Error::from)?.ok_or(context_storage::Error::Conflict)?;
        let body: Value = row.try_get("body").map_err(context_storage::Error::from)?;
        let bytes: Vec<u8> = row
            .try_get("sha256")
            .map_err(context_storage::Error::from)?;
        let digest: [u8; 32] = bytes.try_into().map_err(|_| Error::Contract)?;
        let size: i64 = row
            .try_get("size_bytes")
            .map_err(context_storage::Error::from)?;
        let args = CreateUpload {
            collection: row
                .try_get::<Uuid, _>("parent")
                .map_err(context_storage::Error::from)?
                .into(),
            asset: Some(asset),
            name: body["name"].as_str().ok_or(Error::Contract)?.into(),
            media_type: row
                .try_get("media_type")
                .map_err(context_storage::Error::from)?,
            size_bytes: size as u64,
            sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
            profile,
            idempotency_key: format!("{key}:reprocess-upload"),
        };
        if resumed &&let Some(child)=sqlx::query("SELECT u.receipt AS upload,u.request_digest AS upload_digest,f.receipt,f.request_digest AS finalize_digest FROM context.receipts u JOIN context.receipts f ON f.principal=u.principal WHERE u.principal=$1 AND u.operation='create_upload' AND u.idempotency_key=encode(sha256(convert_to($2,'UTF8')),'hex') AND f.operation='finalize_upload' AND f.idempotency_key=encode(sha256(convert_to($3,'UTF8')),'hex')")
            .bind(Uuid::from(actor.principal)).bind(&args.idempotency_key).bind(format!("{key}:reprocess-finalize")).fetch_optional(&mut **view.transaction()).await.map_err(context_storage::Error::from)? {
                let uploaded:Value=child.try_get("upload").map_err(context_storage::Error::from)?;
                let upload:Uuid=uploaded["upload"].as_str().ok_or(Error::Contract)?.parse().map_err(|_|Error::Contract)?;
                if child.try_get::<Vec<u8>,_>("upload_digest").map_err(context_storage::Error::from)?!=intent_digest(&args,&args.media_type,&digest)? ||child.try_get::<Vec<u8>,_>("finalize_digest").map_err(context_storage::Error::from)?!=Sha256::digest(upload.as_bytes()).as_slice(){return Err(context_storage::Error::Conflict.into());}
                let value:Value=child.try_get("receipt").map_err(context_storage::Error::from)?;
                receipt(view.transaction(),actor.principal,"reprocess",key,&hash,&value).await?;view.commit().await?;return Ok(value);
            }
        if row
            .try_get::<Option<Uuid>, _>("current_revision")
            .map_err(context_storage::Error::from)?
            != Some(Uuid::from(expected))
        {
            return Err(context_storage::Error::Conflict.into());
        }
        view.commit().await?;
        let plane = self.data_plane()?;
        let _publication = plane.publications.lock().await;
        let transfer = self.create_upload(actor, args).await?;
        if transfer["finalized"] != true {
            let plane = self.data_plane()?;
            let owner = row
                .try_get::<Uuid, _>("owner")
                .map_err(context_storage::Error::from)?;
            let _lock = plane.tenant_lock(owner.into()).await;
            self.database
                .authorized_view(actor, Resource::Asset(asset), Permission::Ingest)
                .await?
                .commit()
                .await?;
            let safe:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.revisions v JOIN context.content c ON c.id=v.content WHERE v.id=$1 AND v.asset=$2 AND NOT c.purging)").bind(Uuid::from(expected)).bind(Uuid::from(asset)).fetch_one(self.database.pool()).await.map_err(context_storage::Error::from)?;
            if !safe {
                return Err(context_storage::Error::Unavailable.into());
            }
            sqlx::query("INSERT INTO context.dependencies(source,dependent,relationship) SELECT v.content,n.content,'derivation' FROM context.revisions v,context.revisions n WHERE v.id=$1 AND n.id=$2 ON CONFLICT DO NOTHING")
                .bind(Uuid::from(expected)).bind(Uuid::parse_str(transfer["revision"].as_str().ok_or(Error::Contract)?).map_err(|_|Error::Contract)?).execute(self.database.pool()).await.map_err(context_storage::Error::from)?;
            let target: String = sqlx::query_scalar(
                "SELECT object_key FROM context.uploads WHERE id=$1 AND principal=$2",
            )
            .bind(
                Uuid::parse_str(transfer["upload"].as_str().ok_or(Error::Contract)?)
                    .map_err(|_| Error::Contract)?,
            )
            .bind(Uuid::from(actor.principal))
            .fetch_one(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
            self.data_plane()?
                .blobs
                .promote_verified(
                    body["object_key"].as_str().ok_or(Error::Contract)?,
                    &target,
                    size as u64,
                    &digest,
                    self.data_plane()?.config.max_upload_bytes,
                )
                .await?;
        }
        let value = self
            .finalize_upload_expected(
                actor,
                transfer["upload"]
                    .as_str()
                    .ok_or(Error::Contract)?
                    .parse()
                    .map_err(|_| Error::Contract)?,
                &format!("{key}:reprocess-finalize"),
                Some(expected),
            )
            .await?;
        let mut view = self
            .database
            .authorized_write(actor, Resource::Asset(asset), Permission::Ingest)
            .await?;
        receipt(
            view.transaction(),
            actor.principal,
            "reprocess",
            key,
            &hash,
            &value,
        )
        .await?;
        view.commit().await?;
        Ok(value)
    }

    /// Reserve a tracked upload and separately reserved immutable source destination.
    ///
    /// # Errors
    /// Rejects absent capabilities, invalid metadata, denied Ingest authority or stale keys.
    pub async fn create_upload(&self, actor: &Authentication, args: CreateUpload) -> Result<Value> {
        let plane = self.data_plane()?;
        if args.name.trim().is_empty()
            || args.name.chars().count() > 256
            || args.size_bytes == 0
            || args.size_bytes > plane.config.max_upload_bytes
        {
            return Err(Error::Invalid);
        }
        let hash = digest(&args.sha256)?;
        let media = media_type(&args.media_type)?;
        let target = args
            .asset
            .map(Resource::Asset)
            .unwrap_or(Resource::Collection(args.collection));
        let mut permission = self
            .database
            .authorized_view(actor, target, Permission::Ingest)
            .await?;
        let owner: Uuid = sqlx::query_scalar("SELECT owner FROM context.resources WHERE id=$1")
            .bind(context_storage::postgres::resource_id(target))
            .fetch_one(&mut **permission.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        permission.commit().await?;
        let pipeline = plane.pipeline(&self.workflows, args.profile.as_deref(), Some(&media))?;
        self.workflows
            .ready(&pipeline.fingerprints.keys().cloned().collect::<Vec<_>>())
            .await?;
        let request_hash = intent_digest(&args, &media, &hash)?;
        let _lock = plane.tenant_lock(TenantId::from(owner)).await;
        let mut permission = self
            .database
            .authorized_write(actor, target, Permission::Ingest)
            .await?;
        let tx = permission.transaction();
        if let Some(existing) = existing_receipt(
            tx,
            actor.principal,
            "create_upload",
            &args.idempotency_key,
            &request_hash,
        )
        .await?
        {
            permission.commit().await?;
            return self
                .upload_transfer(
                    actor,
                    existing["upload"]
                        .as_str()
                        .ok_or(Error::Contract)?
                        .parse()
                        .map_err(|_| Error::Contract)?,
                )
                .await;
        }
        let scope=sqlx::query("SELECT c.owner,c.parent AS workspace FROM context.resources c WHERE c.id=$1 AND c.kind='collection' AND NOT c.purging")
            .bind(Uuid::from(args.collection)).fetch_one(&mut **tx).await.map_err(context_storage::Error::from)?;
        let workspace: Uuid = scope
            .try_get("workspace")
            .map_err(context_storage::Error::from)?;
        if scope
            .try_get::<Uuid, _>("owner")
            .map_err(context_storage::Error::from)?
            != owner
        {
            return Err(Error::Invalid);
        }
        let asset = match args.asset {
            Some(asset) => {
                let parent: Uuid =
                    sqlx::query_scalar("SELECT parent FROM context.resources WHERE id=$1")
                        .bind(Uuid::from(asset))
                        .fetch_one(&mut **tx)
                        .await
                        .map_err(context_storage::Error::from)?;
                if parent != Uuid::from(args.collection) {
                    return Err(Error::Invalid);
                }
                asset
            }
            None => {
                let asset = AssetId::new();
                insert_resource(
                    tx,
                    Resource::Asset(asset),
                    owner.into(),
                    Some(Resource::Collection(args.collection)),
                    true,
                )
                .await?;
                sqlx::query("UPDATE context.resources SET label=$2 WHERE id=$1")
                    .bind(Uuid::from(asset))
                    .bind(&args.name)
                    .execute(&mut **tx)
                    .await
                    .map_err(context_storage::Error::from)?;
                sqlx::query("INSERT INTO context.assets(id) VALUES($1)")
                    .bind(Uuid::from(asset))
                    .execute(&mut **tx)
                    .await
                    .map_err(context_storage::Error::from)?;
                asset
            }
        };
        let generation: i64 =
            sqlx::query_scalar("SELECT generation FROM context.assets WHERE id=$1")
                .bind(Uuid::from(asset))
                .fetch_one(&mut **tx)
                .await
                .map_err(context_storage::Error::from)?;
        let upload = Uuid::now_v7();
        let revision = RevisionId::new();
        let content = ContentId::new();
        let upload_key = format!("uploads/{owner}/{upload}");
        let source_key = format!("content/{owner}/{revision}/source");
        sqlx::query("INSERT INTO context.content(id,workspace,asset,revision,kind,origin,body) VALUES($1,$2,$3,$4,'source','original_evidence',$5)")
            .bind(Uuid::from(content)).bind(workspace).bind(Uuid::from(asset)).bind(Uuid::from(revision))
            .bind(json!({"name":args.name,"object_key":source_key})).execute(&mut **tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("INSERT INTO context.revisions(id,asset,content,author,media_type,size_bytes,sha256) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(Uuid::from(revision)).bind(Uuid::from(asset)).bind(Uuid::from(content)).bind(Uuid::from(actor.principal)).bind(&media)
            .bind(args.size_bytes as i64).bind(hash.as_slice()).execute(&mut **tx).await.map_err(context_storage::Error::from)?;
        for key in [&upload_key, &source_key] {
            sqlx::query(
                "INSERT INTO context.objects(object_key,content,size_bytes) VALUES($1,$2,$3)",
            )
            .bind(key)
            .bind(Uuid::from(content))
            .bind(args.size_bytes as i64)
            .execute(&mut **tx)
            .await
            .map_err(context_storage::Error::from)?;
        }
        sqlx::query("INSERT INTO context.uploads(id,principal,asset,revision,object_key,source_object_key,generation,profile,expected_size,expected_sha256,media_type,workflow,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,now()+($13::bigint*interval '1 second'))")
            .bind(upload).bind(Uuid::from(actor.principal)).bind(Uuid::from(asset)).bind(Uuid::from(revision)).bind(&upload_key).bind(&source_key)
            .bind(generation).bind(serde_json::to_value(&pipeline).map_err(|_|Error::Configuration)?).bind(args.size_bytes as i64).bind(hash.as_slice())
            .bind(media).bind(args.profile.unwrap_or_else(||plane.config.default_profile.clone())).bind(plane.config.upload_lifetime_seconds as i64)
            .execute(&mut **tx).await.map_err(context_storage::Error::from)?;
        receipt(
            tx,
            actor.principal,
            "create_upload",
            &args.idempotency_key,
            &request_hash,
            &json!({"upload":upload}),
        )
        .await?;
        permission.commit().await?;
        self.upload_transfer(actor, upload).await
    }

    async fn upload_transfer(&self, actor: &Authentication, upload: Uuid) -> Result<Value> {
        let plane = self.data_plane()?;
        let row=sqlx::query("SELECT u.*,floor(greatest(0,extract(epoch FROM(u.expires_at-now()))))::bigint AS seconds_left FROM context.uploads u JOIN context.revisions v ON v.id=u.revision JOIN context.content c ON c.id=v.content WHERE u.id=$1 AND u.principal=$2 AND NOT c.purging")
            .bind(upload).bind(Uuid::from(actor.principal)).fetch_optional(self.database.pool()).await.map_err(context_storage::Error::from)?.ok_or(context_storage::Error::Unavailable)?;
        let asset = AssetId::from(
            row.try_get::<Uuid, _>("asset")
                .map_err(context_storage::Error::from)?,
        );
        let permission = self
            .database
            .authorized_view(actor, Resource::Asset(asset), Permission::Ingest)
            .await?;
        let seconds: i64 = row
            .try_get("seconds_left")
            .map_err(context_storage::Error::from)?;
        if row
            .try_get::<bool, _>("finalized")
            .map_err(context_storage::Error::from)?
        {
            permission.commit().await?;
            return Ok(
                json!({"upload":upload,"asset":asset,"revision":row.try_get::<Uuid,_>("revision").map_err(context_storage::Error::from)?,
                "finalized":true,"job":row.try_get::<Option<Uuid>,_>("job").map_err(context_storage::Error::from)?}),
            );
        }
        if seconds <= 0 {
            return Err(context_storage::Error::Conflict.into());
        }
        let hash: Vec<u8> = row
            .try_get("expected_sha256")
            .map_err(context_storage::Error::from)?;
        let hash: [u8; 32] = hash.try_into().map_err(|_| Error::Contract)?;
        let size: i64 = row
            .try_get("expected_size")
            .map_err(context_storage::Error::from)?;
        let key: String = row
            .try_get("object_key")
            .map_err(context_storage::Error::from)?;
        // Reissue only within the original capability window; never extend admission expiry.
        let (url, headers) = plane
            .blobs
            .bounded_upload_url(
                &key,
                size as u64,
                &hash,
                Duration::from_secs(seconds as u64),
            )
            .await?;
        permission.commit().await?;
        Ok(
            json!({"upload":upload,"asset":asset,"revision":row.try_get::<Uuid,_>("revision").map_err(context_storage::Error::from)?,
            "url":url,"headers":headers,"expires_in_seconds":seconds}),
        )
    }

    /// Verify and freeze uploaded bytes before admitting a durable processing job.
    ///
    /// # Errors
    /// Rejects wrong content, expired sessions, stale generations and unavailable capacity.
    pub async fn finalize_upload(
        &self,
        actor: &Authentication,
        upload: Uuid,
        key: &str,
    ) -> Result<Value> {
        self.finalize_upload_expected(actor, upload, key, None)
            .await
    }

    async fn finalize_upload_expected(
        &self,
        actor: &Authentication,
        upload: Uuid,
        key: &str,
        expected: Option<RevisionId>,
    ) -> Result<Value> {
        let plane = self.data_plane()?;
        let request_hash: [u8; 32] = Sha256::digest(upload.as_bytes()).into();
        if let Some(row)=sqlx::query("SELECT request_digest,receipt FROM context.receipts WHERE principal=$1 AND operation='finalize_upload' AND idempotency_key=encode(sha256(convert_to($2,'UTF8')),'hex')")
            .bind(Uuid::from(actor.principal)).bind(key).fetch_optional(self.database.pool()).await.map_err(context_storage::Error::from)? {
            if row.try_get::<Vec<u8>,_>("request_digest").map_err(context_storage::Error::from)?!=request_hash {return Err(context_storage::Error::Conflict.into());}
            let receipt:Value=row.try_get("receipt").map_err(context_storage::Error::from)?;
            let asset=receipt["asset"].as_str().ok_or(Error::Contract)?.parse().map_err(|_|Error::Contract)?;
            self.database.authorized_view(actor,Resource::Asset(asset),Permission::Ingest).await?.commit().await?;
            return Ok(receipt);
        }
        let row=sqlx::query("SELECT u.*,r.owner FROM context.uploads u JOIN context.resources r ON r.id=u.asset WHERE u.id=$1 AND u.principal=$2")
            .bind(upload).bind(Uuid::from(actor.principal)).fetch_optional(self.database.pool()).await.map_err(context_storage::Error::from)?.ok_or(context_storage::Error::Unavailable)?;
        let asset = AssetId::from(
            row.try_get::<Uuid, _>("asset")
                .map_err(context_storage::Error::from)?,
        );
        let tenant = TenantId::from(
            row.try_get::<Uuid, _>("owner")
                .map_err(context_storage::Error::from)?,
        );
        self.database
            .authorized_view(actor, Resource::Asset(asset), Permission::Ingest)
            .await?
            .commit()
            .await?;
        let _lock = plane.tenant_lock(tenant).await;
        let mut permission = self
            .database
            .authorized_write(actor, Resource::Asset(asset), Permission::Ingest)
            .await?;
        let tx = permission.transaction();
        let request_hash: [u8; 32] = Sha256::digest(upload.as_bytes()).into();
        if let Some(existing) =
            existing_receipt(tx, actor.principal, "finalize_upload", key, &request_hash).await?
        {
            permission.commit().await?;
            return Ok(existing);
        }
        let row =
            sqlx::query("SELECT * FROM context.uploads WHERE id=$1 AND principal=$2 FOR UPDATE")
                .bind(upload)
                .bind(Uuid::from(actor.principal))
                .fetch_optional(&mut **tx)
                .await
                .map_err(context_storage::Error::from)?
                .ok_or(context_storage::Error::Unavailable)?;
        if row
            .try_get::<bool, _>("finalized")
            .map_err(context_storage::Error::from)?
        {
            let job: Option<Uuid> = row.try_get("job").map_err(context_storage::Error::from)?;
            let result = json!({"job":job.ok_or(Error::Contract)?,"asset":asset,"revision":row.try_get::<Uuid,_>("revision").map_err(context_storage::Error::from)?,"state":"accepted"});
            receipt(
                tx,
                actor.principal,
                "finalize_upload",
                key,
                &request_hash,
                &result,
            )
            .await?;
            permission.commit().await?;
            return Ok(result);
        }
        if let Some(expected) = expected {
            let matches: bool =
                sqlx::query_scalar("SELECT current_revision=$2 FROM context.assets WHERE id=$1")
                    .bind(Uuid::from(asset))
                    .bind(Uuid::from(expected))
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(context_storage::Error::from)?;
            if !matches {
                return Err(context_storage::Error::Conflict.into());
            }
        }
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.uploads u JOIN context.assets a ON a.id=u.asset JOIN context.revisions v ON v.id=u.revision JOIN context.content c ON c.id=v.content WHERE u.id=$1 AND u.principal=$2 AND u.expires_at>now() AND a.generation=u.generation AND NOT u.finalized AND NOT c.purging)")
            .bind(upload).bind(Uuid::from(actor.principal)).fetch_one(&mut **tx).await.map_err(context_storage::Error::from)?;
        if !valid {
            return Err(context_storage::Error::Conflict.into());
        }
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM context.workflow_authority WHERE active")
                .fetch_one(&mut **tx)
                .await
                .map_err(context_storage::Error::from)?;
        if count >= plane.config.max_pending_jobs {
            return Err(Error::Busy);
        }
        let pipeline: Pipeline = serde_json::from_value(
            row.try_get("profile")
                .map_err(context_storage::Error::from)?,
        )
        .map_err(|_| Error::Contract)?;
        for (role, fingerprint) in &pipeline.fingerprints {
            if &self.workflows.fingerprint(role)? != fingerprint {
                return Err(Error::MissingCapability);
            }
        }
        self.workflows
            .ready(&pipeline.fingerprints.keys().cloned().collect::<Vec<_>>())
            .await
            .map_err(|_| Error::MissingCapability)?;
        let source: String = row
            .try_get("object_key")
            .map_err(context_storage::Error::from)?;
        let destination: String = row
            .try_get("source_object_key")
            .map_err(context_storage::Error::from)?;
        let expected: i64 = row
            .try_get("expected_size")
            .map_err(context_storage::Error::from)?;
        let hash: Vec<u8> = row
            .try_get("expected_sha256")
            .map_err(context_storage::Error::from)?;
        let hash: [u8; 32] = hash.try_into().map_err(|_| Error::Contract)?;
        plane
            .blobs
            .promote_verified(
                &source,
                &destination,
                expected as u64,
                &hash,
                plane.config.max_upload_bytes,
            )
            .await?;
        let job = JobId::new();
        let generation:i64=sqlx::query_scalar("UPDATE context.assets SET generation=generation+1 WHERE id=$1 AND generation=$2 RETURNING generation")
            .bind(Uuid::from(asset)).bind(row.try_get::<i64,_>("generation").map_err(context_storage::Error::from)?)
            .fetch_optional(&mut **tx).await.map_err(context_storage::Error::from)?.ok_or(context_storage::Error::Conflict)?;
        let revision: Uuid = row
            .try_get("revision")
            .map_err(context_storage::Error::from)?;
        let grant = JobGrant {
            fenced: false,
            job,
            principal: actor.principal,
            tenant,
            assets: [asset].into(),
            roles: pipeline.fingerprints.keys().cloned().collect(),
            fingerprints: pipeline.fingerprints.clone(),
        };
        let scopes = actor
            .scopes
            .iter()
            .map(|scope| {
                serde_json::to_value(scope)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .ok_or(Error::Invalid)
            })
            .collect::<Result<Vec<_>>>()?;
        sqlx::query("INSERT INTO context.jobs(id,principal,asset,revision,generation,kind,state,workflow,scope,auth_scopes,submitting_credential) VALUES($1,$2,$3,$4,$5,'ingest','accepted',$6,$3,$7,$8)")
            .bind(Uuid::from(job)).bind(Uuid::from(actor.principal)).bind(Uuid::from(asset)).bind(revision).bind(generation)
            .bind(serde_json::to_string(&pipeline).map_err(|_|Error::Contract)?).bind(scopes).bind(actor.credential.map(Uuid::from))
            .execute(&mut **tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("INSERT INTO context.workflow_authority(job,authority) VALUES($1,$2)")
            .bind(Uuid::from(job))
            .bind(serde_json::to_value(&grant).map_err(|_| Error::Contract)?)
            .execute(&mut **tx)
            .await
            .map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.uploads SET finalized=true,job=$2,generation=$3 WHERE id=$1")
            .bind(upload)
            .bind(Uuid::from(job))
            .bind(generation)
            .execute(&mut **tx)
            .await
            .map_err(context_storage::Error::from)?;
        let result = json!({"job":job,"asset":asset,"revision":revision,"state":"accepted"});
        receipt(
            tx,
            actor.principal,
            "finalize_upload",
            key,
            &request_hash,
            &result,
        )
        .await?;
        permission.commit().await?;
        self.workflows.grant_job(grant)?;
        Ok(result)
    }
}
