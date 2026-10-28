//! Authorized durable ingestion status, explicit cancellation and retry.

use crate::{Application, Error, Result, data::Pipeline};
use context_core::{
    access::{Authentication, Permission, Resource},
    ids::{AssetId, CollectionId, JobId, TenantId},
};
use context_storage::postgres::{existing_receipt, receipt};
use context_workflows::host::JobGrant;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

impl Application {
    /// Poll a creator-owned job's control metadata, never its raw provider inputs/outputs.
    ///
    /// # Errors
    /// Denies nonowners and lost current resource authority without disclosing existence.
    pub async fn job_status(&self, actor: &Authentication, job: JobId) -> Result<Value> {
        let row = sqlx::query("SELECT asset FROM context.jobs WHERE id=$1 AND principal=$2")
            .bind(Uuid::from(job))
            .bind(Uuid::from(actor.principal))
            .fetch_optional(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?
            .ok_or(context_storage::Error::Unavailable)?;
        let asset = AssetId::from(
            row.try_get::<Uuid, _>("asset")
                .map_err(context_storage::Error::from)?,
        );
        let view = match self
            .database
            .authorized_view(actor, Resource::Asset(asset), Permission::Ingest)
            .await
        {
            Ok(view) => view,
            Err(context_storage::Error::Unavailable) => {
                self.database
                    .authorized_read(actor, Resource::Asset(asset))
                    .await?
            }
            Err(error) => return Err(error.into()),
        };
        let mut view = view;
        let row=sqlx::query("SELECT id,asset,revision,state,stage,error_code,detail FROM context.jobs WHERE id=$1 AND principal=$2")
            .bind(Uuid::from(job)).bind(Uuid::from(actor.principal)).fetch_one(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        let detail: Value = row
            .try_get("detail")
            .map_err(context_storage::Error::from)?;
        let result = json!({"job":job,"asset":asset,"revision":row.try_get::<Option<Uuid>,_>("revision").map_err(context_storage::Error::from)?,
            "state":row.try_get::<String,_>("state").map_err(context_storage::Error::from)?,"stage":row.try_get::<String,_>("stage").map_err(context_storage::Error::from)?,
            "error_code":row.try_get::<Option<String>,_>("error_code").map_err(context_storage::Error::from)?,
            "chunks":detail.get("chunks"),"result":detail.get("result"),"cleanup_pending":detail.get("cleanup_pending").and_then(Value::as_bool).unwrap_or(false)});
        view.commit().await?;
        Ok(result)
    }

    /// Commit cancellation/publication fencing, then signal/settle owned execution.
    ///
    /// # Errors
    /// Rejects terminal successes, nonowners, missing Ingest authority and changed keys.
    pub async fn cancel_job(&self, actor: &Authentication, job: JobId, key: &str) -> Result<Value> {
        let plane = self.data_plane()?;
        let row=sqlx::query("SELECT j.asset,j.kind,r.owner FROM context.jobs j JOIN context.resources r ON r.id=j.asset WHERE j.id=$1 AND j.principal=$2")
            .bind(Uuid::from(job)).bind(Uuid::from(actor.principal)).fetch_optional(self.database.pool()).await.map_err(context_storage::Error::from)?.ok_or(context_storage::Error::Unavailable)?;
        let asset = AssetId::from(
            row.try_get::<Uuid, _>("asset")
                .map_err(context_storage::Error::from)?,
        );
        let media = row
            .try_get::<String, _>("kind")
            .map_err(context_storage::Error::from)?
            == "media";
        let permission = if media {
            Permission::Read
        } else {
            Permission::Ingest
        };
        self.database
            .authorized_view(actor, Resource::Asset(asset), permission)
            .await?
            .commit()
            .await?;
        let _lock = plane
            .tenant_lock(TenantId::from(
                row.try_get::<Uuid, _>("owner")
                    .map_err(context_storage::Error::from)?,
            ))
            .await;
        let mut view = self
            .database
            .authorized_materialize(actor, Resource::Asset(asset), permission)
            .await?;
        let tx = view.transaction();
        let hash: [u8; 32] = Sha256::digest(Uuid::from(job).as_bytes()).into();
        if let Some(existing) =
            existing_receipt(tx, actor.principal, "cancel_job", key, &hash).await?
        {
            view.commit().await?;
            return Ok(existing);
        }
        let row = sqlx::query(
            "SELECT state,generation,detail FROM context.jobs WHERE id=$1 AND principal=$2 FOR UPDATE",
        )
        .bind(Uuid::from(job))
        .bind(Uuid::from(actor.principal))
        .fetch_one(&mut **tx)
        .await
        .map_err(context_storage::Error::from)?;
        let state: String = row.try_get("state").map_err(context_storage::Error::from)?;
        if matches!(state.as_str(), "succeeded" | "failed") {
            return Err(context_storage::Error::Conflict.into());
        }
        let detail: Value = row
            .try_get("detail")
            .map_err(context_storage::Error::from)?;
        if state == "cancelled" {
            let result = json!({"job":job,"state":"cancelled","cleanup_pending":detail["cleanup_pending"]==true});
            receipt(tx, actor.principal, "cancel_job", key, &hash, &result).await?;
            view.commit().await?;
            return Ok(result);
        }
        if !media {
            sqlx::query(
                "UPDATE context.assets SET generation=generation+1 WHERE id=$1 AND generation=$2",
            )
            .bind(Uuid::from(asset))
            .bind(
                row.try_get::<i64, _>("generation")
                    .map_err(context_storage::Error::from)?,
            )
            .execute(&mut **tx)
            .await
            .map_err(context_storage::Error::from)?;
        }
        sqlx::query("UPDATE context.jobs SET state='cancelled',detail=detail||'{\"cleanup_pending\":true}'::jsonb WHERE id=$1")
            .bind(Uuid::from(job)).execute(&mut **tx).await.map_err(context_storage::Error::from)?;
        let authority:Value=sqlx::query_scalar("UPDATE context.workflow_authority SET authority=authority||'{\"fenced\":true}'::jsonb WHERE job=$1 AND active RETURNING authority")
            .bind(Uuid::from(job)).fetch_one(&mut **tx).await.map_err(context_storage::Error::from)?;
        let grant: JobGrant = serde_json::from_value(authority).map_err(|_| Error::Contract)?;
        let result = json!({"job":job,"state":"cancelled","cleanup_pending":true});
        receipt(tx, actor.principal, "cancel_job", key, &hash, &result).await?;
        view.commit().await?;
        self.workflows.grant_job(grant)?;
        drop(_lock);
        let control = plane.controls.lock().await.get(&job).cloned();
        if let Some(control) = &control {
            control.cancel.cancel();
        }
        // Non-Send HTLK admission replay and cleanup belong to the Actix worker.
        // The persisted fence protects the acknowledgement gap and process restart.
        Ok(result)
    }

    /// Explicitly retry an owned, settled failed/cancelled revision as a new durable job.
    ///
    /// # Errors
    /// Rejects active cleanup, missing current authority/capabilities and changed keys.
    pub async fn retry_job(
        &self,
        actor: &Authentication,
        previous: JobId,
        key: &str,
    ) -> Result<Value> {
        let plane = self.data_plane()?;
        let row=sqlx::query("SELECT j.*,r.owner FROM context.jobs j JOIN context.resources r ON r.id=j.asset WHERE j.id=$1 AND j.principal=$2")
            .bind(Uuid::from(previous)).bind(Uuid::from(actor.principal)).fetch_optional(self.database.pool()).await.map_err(context_storage::Error::from)?.ok_or(context_storage::Error::Unavailable)?;
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
        let hash: [u8; 32] = Sha256::digest(Uuid::from(previous).as_bytes()).into();
        if let Some(existing) =
            existing_receipt(tx, actor.principal, "retry_job", key, &hash).await?
        {
            permission.commit().await?;
            return Ok(existing);
        }
        let row = sqlx::query("SELECT * FROM context.jobs WHERE id=$1 AND principal=$2 FOR UPDATE")
            .bind(Uuid::from(previous))
            .bind(Uuid::from(actor.principal))
            .fetch_one(&mut **tx)
            .await
            .map_err(context_storage::Error::from)?;
        if row
            .try_get::<String, _>("kind")
            .map_err(context_storage::Error::from)?
            == "media"
        {
            return Err(context_storage::Error::Conflict.into());
        }
        let detail: Value = row
            .try_get("detail")
            .map_err(context_storage::Error::from)?;
        if !matches!(
            row.try_get::<&str, _>("state")
                .map_err(context_storage::Error::from)?,
            "failed" | "cancelled"
        ) || detail["cleanup_pending"].as_bool() == Some(true)
        {
            return Err(context_storage::Error::Conflict.into());
        }
        let conflict:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.jobs WHERE asset=$1 AND state IN ('accepted','running')) OR EXISTS(SELECT 1 FROM context.content WHERE revision=$2 AND published)")
            .bind(Uuid::from(asset)).bind(row.try_get::<Uuid,_>("revision").map_err(context_storage::Error::from)?)
            .fetch_one(&mut **tx).await.map_err(context_storage::Error::from)?;
        if conflict {
            return Err(context_storage::Error::Conflict.into());
        }
        let pending: i64 =
            sqlx::query_scalar("SELECT count(*) FROM context.workflow_authority WHERE active")
                .fetch_one(&mut **tx)
                .await
                .map_err(context_storage::Error::from)?;
        if pending >= plane.config.max_pending_jobs {
            return Err(Error::Busy);
        }
        let workflow: String = row
            .try_get("workflow")
            .map_err(context_storage::Error::from)?;
        let pipeline: Pipeline = serde_json::from_str(&workflow).map_err(|_| Error::Contract)?;
        for (role, fingerprint) in &pipeline.fingerprints {
            if &self.workflows.fingerprint(role)? != fingerprint {
                return Err(Error::MissingCapability);
            }
        }
        self.workflows
            .ready(&pipeline.fingerprints.keys().cloned().collect::<Vec<_>>())
            .await?;
        let generation: i64 = sqlx::query_scalar(
            "UPDATE context.assets SET generation=generation+1 WHERE id=$1 RETURNING generation",
        )
        .bind(Uuid::from(asset))
        .fetch_one(&mut **tx)
        .await
        .map_err(context_storage::Error::from)?;
        let job = JobId::new();
        let revision: Uuid = row
            .try_get("revision")
            .map_err(context_storage::Error::from)?;
        let scopes = actor
            .scopes
            .iter()
            .map(|scope| {
                serde_json::to_value(scope)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .ok_or(Error::Contract)
            })
            .collect::<Result<Vec<_>>>()?;
        sqlx::query("INSERT INTO context.jobs(id,principal,asset,revision,generation,kind,state,workflow,scope,auth_scopes,submitting_credential) VALUES($1,$2,$3,$4,$5,'reprocess','accepted',$6,$3,$7,$8)")
            .bind(Uuid::from(job)).bind(Uuid::from(actor.principal)).bind(Uuid::from(asset)).bind(revision).bind(generation).bind(workflow).bind(scopes)
            .bind(actor.credential.map(Uuid::from)).execute(&mut **tx).await.map_err(context_storage::Error::from)?;
        let grant = JobGrant {
            fenced: false,
            job,
            principal: actor.principal,
            tenant,
            assets: [asset].into(),
            roles: pipeline.fingerprints.keys().cloned().collect(),
            fingerprints: pipeline.fingerprints,
        };
        sqlx::query("INSERT INTO context.workflow_authority(job,authority) VALUES($1,$2)")
            .bind(Uuid::from(job))
            .bind(serde_json::to_value(&grant).map_err(|_| Error::Contract)?)
            .execute(&mut **tx)
            .await
            .map_err(context_storage::Error::from)?;
        let result = json!({"job":job,"asset":asset,"revision":revision,"state":"accepted"});
        receipt(tx, actor.principal, "retry_job", key, &hash, &result).await?;
        permission.commit().await?;
        self.workflows.grant_job(grant)?;
        Ok(result)
    }

    /// Ingest bounded UTF-8 text through the same tracked upload/finalization path.
    ///
    /// # Errors
    /// Rejects invalid input, absent capabilities, failed transfers and denied authority.
    pub async fn ingest_text(
        &self,
        actor: &Authentication,
        collection: CollectionId,
        name: &str,
        text: &str,
        profile: Option<String>,
        key: &str,
    ) -> Result<Value> {
        if text.is_empty() || text.len() > 262_144 || key.is_empty() || key.len() > 200 {
            return Err(Error::Invalid);
        }
        let hash = Sha256::digest(text.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let request_hash: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&(collection, name, &hash, &profile)).map_err(|_| Error::Invalid)?,
        )
        .into();
        let mut permission = self
            .database
            .authorized_write(actor, Resource::Collection(collection), Permission::Ingest)
            .await?;
        if let Some(existing) = existing_receipt(
            permission.transaction(),
            actor.principal,
            "ingest_text",
            key,
            &request_hash,
        )
        .await?
        {
            permission.commit().await?;
            return Ok(existing);
        }
        permission.commit().await?;
        let upload_args = crate::uploads::CreateUpload {
            collection,
            asset: None,
            name: name.into(),
            media_type: "text/plain".into(),
            size_bytes: text.len() as u64,
            sha256: hash.clone(),
            profile: profile.clone(),
            idempotency_key: format!("{key}:upload"),
        };
        // Recover the completed child admission even after its transfer-session
        // row expired, if the process stopped before saving the parent receipt.
        if let Some(row)=sqlx::query("SELECT u.request_digest,u.receipt AS upload_receipt,f.request_digest AS finalize_digest,f.receipt FROM context.receipts u JOIN context.receipts f ON f.principal=u.principal WHERE u.principal=$1 AND u.operation='create_upload' AND u.idempotency_key=encode(sha256(convert_to($2,'UTF8')),'hex') AND f.operation='finalize_upload' AND f.idempotency_key=encode(sha256(convert_to($3,'UTF8')),'hex')")
            .bind(Uuid::from(actor.principal)).bind(format!("{key}:upload")).bind(format!("{key}:finalize"))
            .fetch_optional(self.database.pool()).await.map_err(context_storage::Error::from)? {
            let expected=crate::uploads::intent_digest(&upload_args,"text/plain",&crate::uploads::digest(&hash)?)?;
            if row.try_get::<Vec<u8>,_>("request_digest").map_err(context_storage::Error::from)?!=expected {return Err(context_storage::Error::Conflict.into());}
            let upload:Value=row.try_get("upload_receipt").map_err(context_storage::Error::from)?;
            let upload:Uuid=upload["upload"].as_str().ok_or(Error::Contract)?.parse().map_err(|_|Error::Contract)?;
            if row.try_get::<Vec<u8>,_>("finalize_digest").map_err(context_storage::Error::from)?!=Sha256::digest(upload.as_bytes()).as_slice() {
                return Err(context_storage::Error::Conflict.into());
            }
            let result:Value=row.try_get("receipt").map_err(context_storage::Error::from)?;
            let asset=result["asset"].as_str().ok_or(Error::Contract)?.parse().map_err(|_|Error::Contract)?;
            self.database.authorized_view(actor,Resource::Asset(asset),Permission::Ingest).await?.commit().await?;
            let mut permission=self.database.authorized_write(actor,Resource::Collection(collection),Permission::Ingest).await?;
            let result=receipt(permission.transaction(),actor.principal,"ingest_text",key,&request_hash,&result).await?;
            permission.commit().await?;return Ok(result);
        }
        let transfer = self.create_upload(actor, upload_args).await?;
        if transfer["finalized"] != true {
            let url = transfer["url"].as_str().ok_or(Error::Contract)?;
            let mut request = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(context_storage::Error::from)?
                .put(url)
                .body(text.to_string());
            for (name, value) in transfer["headers"].as_object().ok_or(Error::Contract)? {
                request = request.header(name, value.as_str().ok_or(Error::Contract)?);
            }
            let response = request.send().await.map_err(context_storage::Error::from)?;
            if response.status() != reqwest::StatusCode::PRECONDITION_FAILED {
                response
                    .error_for_status()
                    .map_err(context_storage::Error::from)?;
            }
        }
        let result = self
            .finalize_upload(
                actor,
                transfer["upload"]
                    .as_str()
                    .ok_or(Error::Contract)?
                    .parse()
                    .map_err(|_| Error::Contract)?,
                &format!("{key}:finalize"),
            )
            .await?;
        let mut permission = self
            .database
            .authorized_write(actor, Resource::Collection(collection), Permission::Ingest)
            .await?;
        let result = receipt(
            permission.transaction(),
            actor.principal,
            "ingest_text",
            key,
            &request_hash,
            &result,
        )
        .await?;
        permission.commit().await?;
        Ok(result)
    }
}
