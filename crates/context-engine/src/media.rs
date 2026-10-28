//! Durable exact-source page/region/clip materialization with publication fencing.
use crate::{
    Application, Error, Result,
    plane::Plane,
    worker::{Job, Stage},
};
use context_core::{
    access::{Authentication, Permission, Resource},
    content::{SourceLocation, SourceReference},
    ids::{AssetId, ContentId, JobId, RevisionId, TenantId},
};
use context_storage::postgres::{existing_receipt, receipt};
use context_workflows::host::JobGrant;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Explicit bounded binary extraction from an immutable source revision.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractMedia {
    /// Source asset.
    pub asset: AssetId,
    /// Exact published source revision, including explicitly selected history.
    pub revision: RevisionId,
    /// One PDF page, image rectangle or at most60-second audio/video interval.
    pub location: SourceLocation,
    /// Optional operator profile with a media_role.
    pub profile: Option<String>,
    /// Stable request key.
    pub idempotency_key: String,
}
impl Application {
    /// Admit source-protected media extraction as an asynchronous durable job.
    ///
    /// # Errors
    /// Rejects invalid bounds, unpublished sources, unavailable capabilities and lost Read authority.
    pub async fn extract_media(&self, actor: &Authentication, args: ExtractMedia) -> Result<Value> {
        if !args.location.is_valid() {
            return Err(Error::Invalid);
        }
        match args.location {
            SourceLocation::Pages { first, last } if first == last => {}
            SourceLocation::Region {
                x,
                y,
                width,
                height,
            } if x < 16384 && y < 16384 && width.get() <= 8192 && height.get() <= 8192 => {}
            SourceLocation::Time { start_ms, end_ms } if end_ms - start_ms <= 60000 => {}
            _ => return Err(Error::Invalid),
        }
        let plane = self.data_plane()?;
        let mut view = self
            .database
            .authorized_read(actor, Resource::Asset(args.asset))
            .await?;
        let row=sqlx::query("SELECT r.owner,v.media_type,a.generation,v.content FROM context.revisions v JOIN context.content c ON c.id=v.content JOIN context.resources r ON r.id=v.asset JOIN context.assets a ON a.id=v.asset WHERE v.id=$1 AND v.asset=$2 AND c.published")
            .bind(Uuid::from(args.revision)).bind(Uuid::from(args.asset)).fetch_optional(&mut **view.transaction()).await.map_err(context_storage::Error::from)?.ok_or(context_storage::Error::Unavailable)?;
        let media: String = row
            .try_get("media_type")
            .map_err(context_storage::Error::from)?;
        let tenant: Uuid = row.try_get("owner").map_err(context_storage::Error::from)?;
        view.commit().await?;
        let _lock = plane.tenant_lock(TenantId::from(tenant)).await;
        let mut view = self
            .database
            .authorized_materialize(actor, Resource::Asset(args.asset), Permission::Read)
            .await?;
        let hash: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&(args.asset, args.revision, &args.location, &args.profile))
                .map_err(|_| Error::Invalid)?,
        )
        .into();
        if let Some(value) = existing_receipt(
            view.transaction(),
            actor.principal,
            "extract_media",
            &args.idempotency_key,
            &hash,
        )
        .await?
        {
            view.commit().await?;
            return Ok(value);
        }
        let pending: i64 =
            sqlx::query_scalar("SELECT count(*) FROM context.workflow_authority WHERE active")
                .fetch_one(&mut **view.transaction())
                .await
                .map_err(context_storage::Error::from)?;
        if pending >= plane.config.max_pending_jobs {
            return Err(Error::Busy);
        }
        let source:Uuid=sqlx::query_scalar("SELECT v.content FROM context.revisions v JOIN context.content c ON c.id=v.content WHERE v.id=$1 AND v.asset=$2 AND c.published")
            .bind(Uuid::from(args.revision)).bind(Uuid::from(args.asset)).fetch_one(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        self.content_in(&mut view, source.into()).await?;
        let mut pipeline =
            plane.pipeline(&self.workflows, args.profile.as_deref(), Some(&media))?;
        let role = pipeline
            .profile
            .media_role
            .as_ref()
            .ok_or(Error::MissingCapability)?;
        if let Some(node) = &pipeline.profile.cleanup_node
            && !self.workflows.has_checkpoint(role, node)
        {
            return Err(Error::MissingCapability);
        }
        let roles = std::iter::once(role.clone())
            .chain(pipeline.profile.cleanup_role.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        pipeline.fingerprints.retain(|key, _| roles.contains(key));
        self.workflows
            .ready(&roles.iter().cloned().collect::<Vec<_>>())
            .await?;
        let generation: i64 =
            sqlx::query_scalar("SELECT generation FROM context.assets WHERE id=$1")
                .bind(Uuid::from(args.asset))
                .fetch_one(&mut **view.transaction())
                .await
                .map_err(context_storage::Error::from)?;
        let job = JobId::new();
        let scopes = actor
            .scopes
            .iter()
            .map(|p| {
                serde_json::to_value(p)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<Vec<_>>();
        sqlx::query("INSERT INTO context.jobs(id,principal,asset,revision,generation,kind,state,workflow,scope,auth_scopes,submitting_credential,detail) VALUES($1,$2,$3,$4,$5,'media','accepted',$6,$3,$7,$8,$9)")
            .bind(Uuid::from(job)).bind(Uuid::from(actor.principal)).bind(Uuid::from(args.asset)).bind(Uuid::from(args.revision)).bind(generation)
            .bind(serde_json::to_string(&pipeline).map_err(|_|Error::Contract)?).bind(scopes).bind(actor.credential.map(Uuid::from)).bind(json!({"location":args.location})).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        let grant = JobGrant {
            job,
            principal: actor.principal,
            tenant: tenant.into(),
            assets: [args.asset].into(),
            roles,
            fingerprints: pipeline.fingerprints,
            fenced: false,
        };
        sqlx::query("INSERT INTO context.workflow_authority(job,authority) VALUES($1,$2)")
            .bind(Uuid::from(job))
            .bind(serde_json::to_value(&grant).map_err(|_| Error::Contract)?)
            .execute(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        let value =
            json!({"job":job,"asset":args.asset,"revision":args.revision,"state":"accepted"});
        receipt(
            view.transaction(),
            actor.principal,
            "extract_media",
            &args.idempotency_key,
            &hash,
            &value,
        )
        .await?;
        view.commit().await?;
        self.workflows.grant_job(grant)?;
        Ok(value)
    }

    pub(crate) async fn process_media(
        &self,
        plane: &Plane,
        job: &Job,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let detail: Value = sqlx::query_scalar("SELECT detail FROM context.jobs WHERE id=$1")
            .bind(Uuid::from(job.id))
            .fetch_one(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
        let role = job
            .pipeline
            .profile
            .media_role
            .as_deref()
            .ok_or(Error::MissingCapability)?;
        let result=self.stage(plane,job,Stage::new("extract:render",role,json!({"operation":"render","job":job.id,"directory":plane.directory(job.id),"asset":{"id":job.asset,"revision":job.revision,"media_type":job.media_type},"location":detail["location"]})).native(),cancel).await?;
        let document: crate::data::Document =
            serde_json::from_value(result["document"].clone()).map_err(|_| Error::Contract)?;
        let result: Value = document.decode().map_err(|_| Error::Contract)?;
        let file = result["file"].as_str().ok_or(Error::Contract)?;
        let bytes =
            crate::worker::read_file(&plane.directory(job.id), file, 2 * 1024 * 1024).await?;
        if Sha256::digest(&bytes).as_slice()
            != crate::uploads::digest(result["sha256"].as_str().ok_or(Error::Contract)?)?
        {
            return Err(Error::Contract);
        }
        let media =
            crate::uploads::media_type(result["media_type"].as_str().ok_or(Error::Contract)?)?;
        let _lock = plane.tenant_lock(job.tenant).await;
        self.live(job).await?;
        self.clear_staged(plane, job).await?;
        let id = ContentId::new();
        let key = format!("derived/{}/{id}", job.tenant);
        let source = SourceReference {
            asset: job.asset,
            revision: job.revision,
            location: serde_json::from_value(detail["location"].clone())
                .map_err(|_| Error::Contract)?,
            available: true,
        };
        let mut tx = self.database.transaction().await?;
        sqlx::query("INSERT INTO context.content(id,workspace,asset,revision,kind,origin,body,producer_job) VALUES($1,$2,$3,$4,'media','original_evidence',$5,$6)")
            .bind(Uuid::from(id)).bind(Uuid::from(job.workspace)).bind(Uuid::from(job.asset)).bind(Uuid::from(job.revision)).bind(json!({"object_key":key,"media_type":media,"size_bytes":bytes.len(),"sources":[source]})).bind(Uuid::from(job.id)).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("INSERT INTO context.objects VALUES($1,$2,$3)")
            .bind(&key)
            .bind(Uuid::from(id))
            .bind(bytes.len() as i64)
            .execute(&mut *tx)
            .await
            .map_err(context_storage::Error::from)?;
        sqlx::query("INSERT INTO context.dependencies VALUES($1,$2,'derivation')")
            .bind(Uuid::from(job.source))
            .bind(Uuid::from(id))
            .execute(&mut *tx)
            .await
            .map_err(context_storage::Error::from)?;
        tx.commit().await.map_err(context_storage::Error::from)?;
        plane.blobs.put(&key, bytes.into(), 2 * 1024 * 1024).await?;
        sqlx::query("UPDATE context.jobs SET stage='rendered',detail=detail||$2::jsonb WHERE id=$1 AND state='running'").bind(Uuid::from(job.id)).bind(json!({"result":{"content":id}})).execute(self.database.pool()).await.map_err(context_storage::Error::from)?;
        drop(_lock);
        self.publish_media(plane, job).await
    }

    pub(crate) async fn publish_media(&self, plane: &Plane, job: &Job) -> Result<()> {
        let _lock = plane.tenant_lock(job.tenant).await;
        self.live(job).await?;
        match tokio::fs::remove_dir_all(plane.directory(job.id)).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(context_storage::Error::from(e).into()),
        }
        let mut view = self
            .database
            .authorized_materialize(&job.actor(), Resource::Asset(job.asset), Permission::Read)
            .await?;
        let valid:bool=sqlx::query_scalar("SELECT j.state='running' AND j.generation=a.generation FROM context.jobs j JOIN context.assets a ON a.id=j.asset WHERE j.id=$1 FOR UPDATE OF j")
            .bind(Uuid::from(job.id)).fetch_one(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        if !valid {
            return Err(Error::Cancelled);
        }
        sqlx::query("UPDATE context.content SET published=true WHERE producer_job=$1")
            .bind(Uuid::from(job.id))
            .execute(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.jobs SET state='succeeded',stage='published',updated_at=now() WHERE id=$1").bind(Uuid::from(job.id)).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.workflow_authority SET active=false WHERE job=$1")
            .bind(Uuid::from(job.id))
            .execute(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        view.commit().await?;
        self.workflows.revoke_job(job.id)?;
        Ok(())
    }
}
