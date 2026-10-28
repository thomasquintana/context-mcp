//! Durable content-free physical purge worklists, publication fences and index repair.
use crate::{Application, Error, Result};
use context_core::{
    access::{Authentication, Permission, Resource},
    ids::{AssetId, CollectionId, ContentId, JobId, MemoryId, RevisionId, TenantId, WorkspaceId},
};
use context_storage::{
    postgres::{existing_receipt, receipt, resource_id},
    search::{Search, SearchDocument},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

/// Exact owned purge target. Derivation cleanup follows every owner/source boundary.
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Target {
    /// Whole tenant hierarchy.
    Tenant(TenantId),
    /// Whole project hierarchy and its knowledge.
    Workspace(WorkspaceId),
    /// Collection hierarchy.
    Collection(CollectionId),
    /// Every revision/derivative of a logical asset.
    Asset(AssetId),
    /// One immutable revision and its derivation closure.
    Revision(RevisionId),
    /// One exact managed record and derivation closure.
    Content(ContentId),
    /// Every immutable revision of a semantic memory.
    Memory(MemoryId),
}
/// An irreversible owned deletion intent with current-generation validation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Purge {
    /// Exact target.
    pub target: Target,
    /// Required asset generation or memory version for those mutable logical targets.
    pub expected_generation: Option<i64>,
    /// Stable intent key; retries never regenerate removed content.
    pub idempotency_key: String,
}
impl Application {
    pub(crate) async fn register_indexes(&self) -> Result<()> {
        let Some(plane) = &self.plane else {
            return Ok(());
        };
        for (name, space) in &plane.config.spaces {
            sqlx::query("INSERT INTO context.index_bindings(endpoint,collection,space,dimensions,key_env) VALUES($1,$2,$3,$4,$5) ON CONFLICT(endpoint,collection) DO NOTHING")
                .bind(&plane.config.typesense_endpoint).bind(&space.collection).bind(name).bind(space.dimensions as i32).bind(&plane.config.typesense_key_env).execute(self.database.pool()).await.map_err(context_storage::Error::from)?;
            let valid:bool=sqlx::query_scalar("SELECT space=$3 AND dimensions=$4 FROM context.index_bindings WHERE endpoint=$1 AND collection=$2").bind(&plane.config.typesense_endpoint).bind(&space.collection).bind(name).bind(space.dimensions as i32).fetch_one(self.database.pool()).await.map_err(context_storage::Error::from)?;
            if !valid {
                return Err(Error::Configuration);
            }
        }
        Ok(())
    }

    async fn purge_scope(&self, target: Target) -> Result<Resource> {
        Ok(match target {
            Target::Tenant(id) => Resource::Tenant(id),
            Target::Workspace(id) => Resource::Workspace(id),
            Target::Collection(id) => Resource::Collection(id),
            Target::Asset(id) => Resource::Asset(id),
            Target::Revision(id) => Resource::Asset(
                sqlx::query_scalar::<_, Uuid>("SELECT asset FROM context.revisions WHERE id=$1")
                    .bind(Uuid::from(id))
                    .fetch_optional(self.database.pool())
                    .await
                    .map_err(context_storage::Error::from)?
                    .ok_or(context_storage::Error::Unavailable)?
                    .into(),
            ),
            Target::Memory(id) => Resource::Workspace(
                sqlx::query_scalar::<_, Uuid>("SELECT workspace FROM context.memories WHERE id=$1")
                    .bind(Uuid::from(id))
                    .fetch_optional(self.database.pool())
                    .await
                    .map_err(context_storage::Error::from)?
                    .ok_or(context_storage::Error::Unavailable)?
                    .into(),
            ),
            Target::Content(id) => {
                let row = sqlx::query(
                    "SELECT asset,workspace FROM context.content WHERE id=$1 AND NOT purging",
                )
                .bind(Uuid::from(id))
                .fetch_optional(self.database.pool())
                .await
                .map_err(context_storage::Error::from)?
                .ok_or(context_storage::Error::Unavailable)?;
                row.try_get::<Option<Uuid>, _>("asset")
                    .map_err(context_storage::Error::from)?
                    .map(|id| Resource::Asset(id.into()))
                    .unwrap_or(Resource::Workspace(
                        row.try_get::<Uuid, _>("workspace")
                            .map_err(context_storage::Error::from)?
                            .into(),
                    ))
            }
        })
    }

    /// Close visibility/admission and persist the full cross-owner deletion closure.
    ///
    /// # Errors
    /// Requires Purge scope, current resource authority and owner-tenant membership.
    /// Rejects stale generations and conflicting keys. Completion requires physical cleanup.
    pub async fn purge(&self, actor: &Authentication, args: Purge) -> Result<Value> {
        let plane = self.data_plane()?;
        if args.idempotency_key.is_empty() || args.idempotency_key.len() > 256 {
            return Err(Error::Invalid);
        }
        let hash: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&(args.target, args.expected_generation))
                .map_err(|_| Error::Invalid)?,
        )
        .into();
        if let Some(row)=sqlx::query("SELECT request_digest,receipt FROM context.receipts WHERE principal=$1 AND operation='purge' AND idempotency_key=encode(sha256(convert_to($2,'UTF8')),'hex')")
            .bind(Uuid::from(actor.principal)).bind(&args.idempotency_key).fetch_optional(self.database.pool()).await.map_err(context_storage::Error::from)? {
            if row.try_get::<Vec<u8>,_>("request_digest").map_err(context_storage::Error::from)?!=hash{return Err(context_storage::Error::Conflict.into());}
            let value:Value=row.try_get("receipt").map_err(context_storage::Error::from)?;let id=Uuid::parse_str(value["purge"].as_str().ok_or(Error::Contract)?).map_err(|_|Error::Contract)?;return self.purge_status(actor,id).await;
        }
        let scope = self.purge_scope(args.target).await?;
        let mut preliminary = self
            .database
            .authorized_view(actor, scope, Permission::Purge)
            .await?;
        let owner: Uuid = sqlx::query_scalar("SELECT owner FROM context.resources WHERE id=$1")
            .bind(resource_id(scope))
            .fetch_one(&mut **preliminary.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        if !preliminary.context().tenants.contains(&owner.into()) {
            return Err(context_storage::Error::Unavailable.into());
        }
        preliminary.commit().await?;
        let _publication = plane.publications.lock().await;
        let _owner = plane.tenant_lock(owner.into()).await;
        let mut view = self
            .database
            .authorized_write(actor, scope, Permission::Purge)
            .await?;
        if let Some(value) = existing_receipt(
            view.transaction(),
            actor.principal,
            "purge",
            &args.idempotency_key,
            &hash,
        )
        .await?
        {
            view.commit().await?;
            return Ok(value);
        }
        match args.target {
            Target::Asset(asset) => {
                let generation: i64 =
                    sqlx::query_scalar("SELECT generation FROM context.assets WHERE id=$1")
                        .bind(Uuid::from(asset))
                        .fetch_one(&mut **view.transaction())
                        .await
                        .map_err(context_storage::Error::from)?;
                if args.expected_generation != Some(generation) {
                    return Err(context_storage::Error::Conflict.into());
                }
            }
            Target::Revision(_) => {
                let generation: i64 =
                    sqlx::query_scalar("SELECT generation FROM context.assets WHERE id=$1")
                        .bind(resource_id(scope))
                        .fetch_one(&mut **view.transaction())
                        .await
                        .map_err(context_storage::Error::from)?;
                if args.expected_generation != Some(generation) {
                    return Err(context_storage::Error::Conflict.into());
                }
            }
            Target::Memory(memory) => {
                let version: i64 =
                    sqlx::query_scalar("SELECT version FROM context.memories WHERE id=$1")
                        .bind(Uuid::from(memory))
                        .fetch_one(&mut **view.transaction())
                        .await
                        .map_err(context_storage::Error::from)?;
                if args.expected_generation != Some(version) {
                    return Err(context_storage::Error::Conflict.into());
                }
            }
            _ => {}
        }
        let id = Uuid::now_v7();
        let target = serde_json::to_value(args.target).map_err(|_| Error::Invalid)?;
        let kind = target["kind"].as_str().ok_or(Error::Contract)?;
        let target_id = Uuid::parse_str(target["id"].as_str().ok_or(Error::Contract)?)
            .map_err(|_| Error::Contract)?;
        sqlx::query(
            "INSERT INTO context.purges(id,principal,manifest,owner,scope) VALUES($1,$2,$3,$4,$5)",
        )
        .bind(id)
        .bind(Uuid::from(actor.principal))
        .bind(json!({"target":target}))
        .bind(owner)
        .bind(resource_id(scope))
        .execute(&mut **view.transaction())
        .await
        .map_err(context_storage::Error::from)?;
        if matches!(
            args.target,
            Target::Tenant(_) | Target::Workspace(_) | Target::Collection(_) | Target::Asset(_)
        ) {
            sqlx::query("WITH RECURSIVE tree(id) AS (SELECT $2::uuid UNION SELECT r.id FROM context.resources r JOIN tree t ON r.parent=t.id) INSERT INTO context.purge_resources SELECT $1,id FROM tree")
                .bind(id).bind(target_id).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        }
        sqlx::query("WITH RECURSIVE affected(id) AS (SELECT c.id FROM context.content c WHERE c.asset IN(SELECT resource FROM context.purge_resources WHERE purge=$1) OR c.workspace IN(SELECT resource FROM context.purge_resources WHERE purge=$1) OR ($2='content' AND c.id=$3) OR ($2='revision' AND c.revision=$3) OR ($2='memory' AND c.id IN(SELECT content FROM context.memory_revisions WHERE memory=$3)) UNION SELECT d.dependent FROM context.dependencies d JOIN affected a ON d.source=a.id WHERE d.relationship='derivation') INSERT INTO context.purge_items SELECT $1,a.id,c.embedding_space FROM affected a JOIN context.content c ON c.id=a.id")
            .bind(id).bind(kind).bind(target_id).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.resources SET purging=true WHERE id IN(SELECT resource FROM context.purge_resources WHERE purge=$1)").bind(id).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.content SET purging=true WHERE id IN(SELECT content FROM context.purge_items WHERE purge=$1)").bind(id).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        sqlx::query("INSERT INTO context.purge_objects SELECT $1,o.object_key FROM context.objects o JOIN context.purge_items i ON i.content=o.content WHERE i.purge=$1").bind(id).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        sqlx::query("INSERT INTO context.purge_jobs SELECT $1,j.id FROM context.jobs j WHERE j.asset IN(SELECT resource FROM context.purge_resources WHERE purge=$1) OR j.scope IN(SELECT resource FROM context.purge_resources WHERE purge=$1) OR j.revision IN(SELECT v.id FROM context.revisions v JOIN context.purge_items i ON i.content=v.content WHERE i.purge=$1) OR j.id IN(SELECT c.producer_job FROM context.content c JOIN context.purge_items i ON i.content=c.id WHERE i.purge=$1)").bind(id).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        sqlx::query("INSERT INTO context.purge_acquisitions SELECT $1,a.id FROM context.acquisitions a WHERE a.scope IN(SELECT resource FROM context.purge_resources WHERE purge=$1)").bind(id).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.jobs SET state='cancelled',error_code='purged',detail=detail||'{\"cleanup_pending\":true}'::jsonb WHERE id IN(SELECT job FROM context.purge_jobs WHERE purge=$1) AND state IN ('accepted','running')").bind(id).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.workflow_authority SET authority=authority||'{\"fenced\":true}'::jsonb WHERE job IN(SELECT job FROM context.purge_jobs WHERE purge=$1) AND active").bind(id).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        let value = json!({"purge":id,"state":"pending","completed":false});
        receipt(
            view.transaction(),
            actor.principal,
            "purge",
            &args.idempotency_key,
            &hash,
            &value,
        )
        .await?;
        view.commit().await?;
        drop(_owner);
        drop(_publication);
        self.signal_purge(id).await?;
        Ok(value)
    }

    /// Poll creator-owned purge control state even after the target no longer exists.
    ///
    /// # Errors
    /// Denies wrong principals and expired/revoked/insufficient credentials.
    pub async fn purge_status(&self, actor: &Authentication, id: Uuid) -> Result<Value> {
        if !actor.scopes.contains(&Permission::Purge) {
            return Err(context_storage::Error::Unavailable.into());
        }
        let mut tx = self.database.transaction().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(context_storage::Error::from)?;
        if let Some(credential) = actor.credential {
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.credentials WHERE id=$1 AND principal=$2 AND NOT revoked AND expires_at>now() AND 'purge'=ANY(scopes))").bind(Uuid::from(credential)).bind(Uuid::from(actor.principal)).fetch_one(&mut *tx).await.map_err(context_storage::Error::from)?;
            if !valid {
                return Err(context_storage::Error::Unavailable.into());
            }
        }
        let row = sqlx::query(
            "SELECT completed,manifest FROM context.purges WHERE id=$1 AND principal=$2",
        )
        .bind(id)
        .bind(Uuid::from(actor.principal))
        .fetch_optional(&mut *tx)
        .await
        .map_err(context_storage::Error::from)?
        .ok_or(context_storage::Error::Unavailable)?;
        let completed: bool = row
            .try_get("completed")
            .map_err(context_storage::Error::from)?;
        tx.commit().await.map_err(context_storage::Error::from)?;
        Ok(
            json!({"purge":id,"completed":completed,"state":if completed{"completed"}else{"pending"}}),
        )
    }

    async fn signal_purge(&self, id: Uuid) -> Result<bool> {
        let plane = self.data_plane()?;
        let jobs: Vec<Uuid> =
            sqlx::query_scalar("SELECT job FROM context.purge_jobs WHERE purge=$1")
                .bind(id)
                .fetch_all(self.database.pool())
                .await
                .map_err(context_storage::Error::from)?;
        for job in jobs {
            let job = JobId::from(job);
            let _ = self.workflows.fence_job(job);
            if let Some(control) = plane.controls.lock().await.get(&job) {
                control.cancel.cancel();
            }
        }
        let requests = plane
            .requests
            .lock()
            .await
            .iter()
            .map(|(id, p)| (*id, p.sources.clone(), p.cancel.clone()))
            .collect::<Vec<_>>();
        let mut waiting = false;
        for (request, sources, cancel) in requests {
            let affected:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.purge_items WHERE purge=$1 AND content=ANY($2))").bind(id).bind(sources.iter().copied().map(Uuid::from).collect::<Vec<_>>()).fetch_one(self.database.pool()).await.map_err(context_storage::Error::from)?;
            if affected {
                cancel.cancel();
                let _ = self.workflows.fence_job(request);
                waiting = true;
            }
        }
        Ok(waiting)
    }

    pub(crate) async fn purge_pass(&self) -> Result<()> {
        let ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM context.purges WHERE NOT completed ORDER BY attempted_at,created_at LIMIT 4").fetch_all(self.database.pool()).await.map_err(context_storage::Error::from)?;
        for id in ids {
            if let Err(error) = self.finish_purge(id).await {
                tracing::warn!(purge=%id,error=%error,"physical purge remains pending");
            }
        }
        self.clear_orphans().await?;
        Ok(())
    }

    async fn finish_purge(&self, id: Uuid) -> Result<()> {
        sqlx::query("UPDATE context.purges SET attempted_at=now() WHERE id=$1")
            .bind(id)
            .execute(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
        if self.signal_purge(id).await? {
            return Ok(());
        }
        let unsettled:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.purge_jobs p JOIN context.jobs j ON j.id=p.job LEFT JOIN context.workflow_authority a ON a.job=j.id WHERE p.purge=$1 AND (a.active OR j.detail->>'cleanup_pending'='true'))")
            .bind(id).fetch_one(self.database.pool()).await.map_err(context_storage::Error::from)?;
        if unsettled {
            return Ok(());
        }
        let plane = self.data_plane()?;
        let _publication = plane.publications.lock().await;
        plane.blobs.ready().await?;
        let objects: Vec<String> =
            sqlx::query_scalar("SELECT object_key FROM context.purge_objects WHERE purge=$1")
                .bind(id)
                .fetch_all(self.database.pool())
                .await
                .map_err(context_storage::Error::from)?;
        for key in &objects {
            if key.starts_with("uploads/") {
                plane.blobs.fence_upload(key).await?;
            } else {
                plane.blobs.delete(key).await?;
            }
        }
        let bindings=sqlx::query("SELECT DISTINCT b.* FROM context.index_bindings b JOIN context.purge_items i ON i.embedding_space=b.space WHERE i.purge=$1").bind(id).fetch_all(self.database.pool()).await.map_err(context_storage::Error::from)?;
        for binding in bindings {
            let key_env: String = binding
                .try_get("key_env")
                .map_err(context_storage::Error::from)?;
            let key = std::env::var(key_env).map_err(|_| Error::Configuration)?;
            let index = Search::new(
                &binding
                    .try_get::<String, _>("endpoint")
                    .map_err(context_storage::Error::from)?,
                &key,
                &binding
                    .try_get::<String, _>("collection")
                    .map_err(context_storage::Error::from)?,
                binding
                    .try_get::<i32, _>("dimensions")
                    .map_err(context_storage::Error::from)? as usize,
            )?;
            let space: String = binding
                .try_get("space")
                .map_err(context_storage::Error::from)?;
            let ids: Vec<Uuid> = sqlx::query_scalar(
                "SELECT content FROM context.purge_items WHERE purge=$1 AND embedding_space=$2",
            )
            .bind(id)
            .bind(space)
            .fetch_all(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
            for content in ids {
                index.delete(content.into()).await?;
            }
        }
        let jobs: Vec<Uuid> =
            sqlx::query_scalar("SELECT job FROM context.purge_jobs WHERE purge=$1")
                .bind(id)
                .fetch_all(self.database.pool())
                .await
                .map_err(context_storage::Error::from)?;
        for job in &jobs {
            remove_directory(&plane.directory((*job).into())).await?;
        }
        let acquisitions: Vec<Uuid> =
            sqlx::query_scalar("SELECT acquisition FROM context.purge_acquisitions WHERE purge=$1")
                .bind(id)
                .fetch_all(self.database.pool())
                .await
                .map_err(context_storage::Error::from)?;
        for acquisition in acquisitions {
            match tokio::fs::remove_file(plane.staging.join(format!("acquire-{acquisition}"))).await
            {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(context_storage::Error::from(e).into()),
            }
        }
        let mut tx = self.database.transaction().await?;
        let deleted: i64 =
            sqlx::query_scalar("SELECT count(*) FROM context.purge_items WHERE purge=$1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.content c SET body=jsonb_set(c.body,'{citations}',COALESCE((SELECT jsonb_agg(CASE WHEN citation->>'revision' IN(SELECT v.id::text FROM context.revisions v JOIN context.purge_items i ON i.content=v.content WHERE i.purge=$1) THEN citation||'{\"available\":false}'::jsonb ELSE citation END) FROM jsonb_array_elements(c.body->'citations') citation),'[]'::jsonb)) WHERE jsonb_typeof(c.body->'citations')='array'").bind(id).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.memories m SET current_revisions=ARRAY(SELECT r FROM unnest(m.current_revisions) r WHERE r NOT IN(SELECT mr.id FROM context.memory_revisions mr JOIN context.purge_items i ON i.content=mr.content WHERE i.purge=$1)),version=version+1 WHERE id IN(SELECT mr.memory FROM context.memory_revisions mr JOIN context.purge_items i ON i.content=mr.content WHERE i.purge=$1)").bind(id).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.resources r SET label='' WHERE id IN(SELECT a.id FROM context.assets a JOIN context.revisions v ON v.id=a.current_revision JOIN context.purge_items i ON i.content=v.content WHERE i.purge=$1)").bind(id).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.assets SET current_revision=NULL WHERE current_revision IN(SELECT v.id FROM context.revisions v JOIN context.purge_items i ON i.content=v.content WHERE i.purge=$1)").bind(id).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("DELETE FROM context.objects WHERE object_key=ANY($1)")
            .bind(&objects)
            .execute(&mut *tx)
            .await
            .map_err(context_storage::Error::from)?;
        sqlx::query("DELETE FROM context.uploads WHERE revision IN(SELECT v.id FROM context.revisions v JOIN context.purge_items i ON i.content=v.content WHERE i.purge=$1) OR asset IN(SELECT resource FROM context.purge_resources WHERE purge=$1)").bind(id).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("DELETE FROM context.jobs WHERE id IN(SELECT job FROM context.purge_jobs WHERE purge=$1)").bind(id).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("DELETE FROM context.content WHERE id IN(SELECT content FROM context.purge_items WHERE purge=$1)").bind(id).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("DELETE FROM context.memories WHERE NOT EXISTS(SELECT 1 FROM context.memory_revisions r WHERE r.memory=context.memories.id)").execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("DELETE FROM context.acquisitions WHERE id IN(SELECT acquisition FROM context.purge_acquisitions WHERE purge=$1)").bind(id).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("DELETE FROM context.resources WHERE id IN(SELECT resource FROM context.purge_resources WHERE purge=$1)").bind(id).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        for sql in [
            "DELETE FROM context.purge_resources WHERE purge=$1",
            "DELETE FROM context.purge_items WHERE purge=$1",
            "DELETE FROM context.purge_objects WHERE purge=$1",
            "DELETE FROM context.purge_jobs WHERE purge=$1",
            "DELETE FROM context.purge_acquisitions WHERE purge=$1",
        ] {
            sqlx::query(sql)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(context_storage::Error::from)?;
        }
        sqlx::query(
            "UPDATE context.purges SET completed=true,manifest=manifest||$2::jsonb WHERE id=$1",
        )
        .bind(id)
        .bind(json!({"deleted_records":deleted}))
        .execute(&mut *tx)
        .await
        .map_err(context_storage::Error::from)?;
        tx.commit().await.map_err(context_storage::Error::from)?;
        Ok(())
    }

    async fn clear_orphans(&self) -> Result<()> {
        let plane = self.data_plane()?;
        let _publication = plane.publications.lock().await;
        let rows=sqlx::query("SELECT id,embedding_space FROM context.content WHERE NOT published AND NOT purging AND producer_job IS NULL AND kind IN ('memory','summary') AND created_at<now()-interval '10 minutes' LIMIT 32").fetch_all(self.database.pool()).await.map_err(context_storage::Error::from)?;
        for row in rows {
            let id: Uuid = row.try_get("id").map_err(context_storage::Error::from)?;
            let space: Option<String> = row
                .try_get("embedding_space")
                .map_err(context_storage::Error::from)?;
            if let Some(space) = space {
                self.delete_index_ids(&space, &[id.into()]).await?;
            }
            sqlx::query("DELETE FROM context.content WHERE id=$1 AND NOT published")
                .bind(id)
                .execute(self.database.pool())
                .await
                .map_err(context_storage::Error::from)?;
        }
        Ok(())
    }

    /// Explicitly repair one derived index from published, protected authoritative bodies.
    ///
    /// # Errors
    /// Requires owner Share authority; cannot regenerate source/model content.
    pub async fn rebuild_index(
        &self,
        actor: &Authentication,
        workspace: WorkspaceId,
        space: Option<String>,
    ) -> Result<Value> {
        let plane = self.data_plane()?;
        let name = space.as_ref().unwrap_or(&plane.config.default_space);
        let index = plane.search.get(name).ok_or(Error::MissingCapability)?;
        let _publication = plane.publications.lock().await;
        let mut view = self
            .database
            .authorized_view(actor, Resource::Workspace(workspace), Permission::Share)
            .await?;
        let owner: Uuid = sqlx::query_scalar("SELECT owner FROM context.resources WHERE id=$1")
            .bind(Uuid::from(workspace))
            .fetch_one(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        if !view.context().tenants.contains(&owner.into()) {
            return Err(context_storage::Error::Unavailable.into());
        }
        index.ensure_collection().await?;
        let rows:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM context.content WHERE workspace=$1 AND published AND NOT purging AND embedding_space=$2 ORDER BY id LIMIT 10001").bind(Uuid::from(workspace)).bind(name).fetch_all(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        if rows.len() > 10000 {
            return Err(Error::Busy);
        }
        let mut documents = Vec::new();
        for id in rows {
            let item = self.content_in(&mut view, id.into()).await?;
            let embedding: Vec<f32> = serde_json::from_value(item.body["embedding"].clone())
                .map_err(|_| Error::Contract)?;
            documents.push(SearchDocument {
                id: item.id,
                workspace: item.workspace,
                revision: item.revision,
                text: item.body["text"].as_str().ok_or(Error::Contract)?.into(),
                origin: item.origin,
                embedding,
            });
        }
        index.delete_workspace(workspace).await?;
        for document in &documents {
            index.upsert(document).await?;
        }
        let count = documents.len();
        view.commit().await?;
        Ok(json!({"workspace":workspace,"space":name,"indexed":count}))
    }

    pub(crate) async fn delete_index_ids(&self, space: &str, ids: &[ContentId]) -> Result<()> {
        let bindings = sqlx::query("SELECT * FROM context.index_bindings WHERE space=$1")
            .bind(space)
            .fetch_all(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
        for binding in bindings {
            let key = std::env::var(
                binding
                    .try_get::<String, _>("key_env")
                    .map_err(context_storage::Error::from)?,
            )
            .map_err(|_| Error::Configuration)?;
            let index = Search::new(
                &binding
                    .try_get::<String, _>("endpoint")
                    .map_err(context_storage::Error::from)?,
                &key,
                &binding
                    .try_get::<String, _>("collection")
                    .map_err(context_storage::Error::from)?,
                binding
                    .try_get::<i32, _>("dimensions")
                    .map_err(context_storage::Error::from)? as usize,
            )?;
            for id in ids {
                index.delete(*id).await?;
            }
        }
        Ok(())
    }
}

async fn remove_directory(path: &std::path::Path) -> Result<()> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(context_storage::Error::from(e).into()),
    }
}
