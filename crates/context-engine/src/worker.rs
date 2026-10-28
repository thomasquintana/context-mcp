//! Single-instance durable ingestion and per-stage HTLK admission reconciliation.

use crate::{
    Application, Error, Result,
    data::{Document, Enrichment, Extraction, Pipeline, Segment, SegmentKind},
    plane::{Control, Plane},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use context_core::{
    access::{Authentication, Permission, Resource},
    content::{Origin, SourceReference},
    ids::{AssetId, ContentId, JobId, PrincipalId, RevisionId, TenantId, WorkspaceId},
};
use context_storage::{postgres::publish_revision, search::SearchDocument};
use context_workflows::host::{Handle, State};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(crate) struct Job {
    pub(crate) id: JobId,
    pub(crate) asset: AssetId,
    pub(crate) revision: RevisionId,
    principal: PrincipalId,
    pub(crate) tenant: TenantId,
    pub(crate) workspace: WorkspaceId,
    pub(crate) source: ContentId,
    pub(crate) generation: i64,
    pub(crate) media_type: String,
    size: u64,
    sha256: [u8; 32],
    object_key: String,
    pub(crate) pipeline: Pipeline,
    scopes: BTreeSet<Permission>,
    name: String,
    replayable: bool,
    kind: String,
}

impl Job {
    pub(crate) fn actor(&self) -> Authentication {
        Authentication {
            principal: self.principal,
            scopes: self.scopes.clone(),
            credential: None,
        }
    }
}

enum StageMode {
    Process,
    NativeInput,
    Cleanup,
}
pub(crate) struct Stage<'a> {
    name: String,
    role: &'a str,
    request: Value,
    mode: StageMode,
}
impl<'a> Stage<'a> {
    pub(crate) fn new(name: impl Into<String>, role: &'a str, request: Value) -> Self {
        Self {
            name: name.into(),
            role,
            request,
            mode: StageMode::Process,
        }
    }
    pub(crate) fn native(mut self) -> Self {
        self.mode = StageMode::NativeInput;
        self
    }
    fn cleanup(mut self) -> Self {
        self.mode = StageMode::Cleanup;
        self
    }
}

impl Application {
    /// Start the bounded in-process worker pool once after application initialization.
    ///
    /// # Errors
    /// Rejects repeated startup; no ingestion is admitted without a data plane.
    pub async fn start_workers(self: &Arc<Self>) -> Result<()> {
        let Some(plane) = &self.plane else {
            return Ok(());
        };
        let mut tasks = plane.tasks.lock().await;
        if !tasks.is_empty() {
            return Err(Error::Configuration);
        }
        for _ in 0..plane.config.workers {
            let application = Arc::downgrade(self);
            let plane = plane.clone();
            tasks.push(actix::spawn(async move {
                Self::worker_loop(application, plane).await;
            }));
            let application = Arc::downgrade(self);
            let plane = self.data_plane()?.clone();
            tasks.push(actix::spawn(async move {
                Self::operation_loop(application, plane).await;
            }));
        }
        let application = Arc::downgrade(self);
        let plane = plane.clone();
        tasks.push(actix::spawn(async move {
            while !plane.stop.is_cancelled() {
                let Some(application)=application.upgrade() else{break;};
                if let Err(error)=application.maintenance(&plane).await {
                    tracing::warn!(code=%failure_code(&error),"background maintenance remains pending");
                }
                drop(application);
                tokio::select!{_=plane.stop.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(5))=>{}}
            }
        }));
        Ok(())
    }

    async fn worker_loop(application: Weak<Self>, plane: Arc<Plane>) {
        while !plane.stop.is_cancelled() {
            let Some(application) = application.upgrade() else {
                break;
            };
            let claimed = application.claim(&plane).await;
            match claimed {
                Ok(Some((id, control))) => {
                    let result = application.process(&plane, id, &control.cancel).await;
                    if !plane.stop.is_cancelled()
                        && let Err(error) = result
                    {
                        tracing::warn!(job=%id,code=%failure_code(&error),"processing failed");
                        if let Err(error) = application.fail_job(&plane, id, &error).await {
                            tracing::warn!(job=%id,code=%failure_code(&error),"job cleanup remains pending");
                            application.record_cleanup_error(id, &error).await;
                        }
                    }
                    plane.controls.lock().await.remove(&id);
                    control.finished.notify_waiters();
                }
                _ => {
                    tokio::select! {_=plane.stop.cancelled()=>break,_=tokio::time::sleep(Duration::from_millis(200))=>{}}
                }
            }
        }
    }

    async fn claim(&self, plane: &Plane) -> Result<Option<(JobId, Arc<Control>)>> {
        // One process owns workers. Keeping claim+local registration under this
        // mutex resumes running rows without resetting their durable lifecycle.
        let mut controls = plane.controls.lock().await;
        let active = controls.keys().copied().map(Uuid::from).collect::<Vec<_>>();
        let mut tx = self.database.transaction().await?;
        let row=sqlx::query("SELECT j.id FROM context.jobs j JOIN context.workflow_authority a ON a.job=j.id WHERE a.active AND j.kind IN ('ingest','reprocess','media') AND j.state IN ('accepted','running') AND NOT(j.id=ANY($1)) ORDER BY j.created_at FOR UPDATE OF j SKIP LOCKED LIMIT 1")
            .bind(active).fetch_optional(&mut *tx).await.map_err(context_storage::Error::from)?;
        let Some(row) = row else { return Ok(None) };
        let id: Uuid = row.try_get("id").map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.jobs SET state='running',updated_at=now() WHERE id=$1 AND state='accepted'")
            .bind(id).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        tx.commit().await.map_err(context_storage::Error::from)?;
        let id = JobId::from(id);
        let control = Arc::new(Control {
            cancel: CancellationToken::new(),
            finished: Arc::new(tokio::sync::Notify::new()),
        });
        controls.insert(id, control.clone());
        Ok(Some((id, control)))
    }

    async fn job(&self, id: JobId) -> Result<Job> {
        let row=sqlx::query("SELECT j.*,(j.created_at>now()-interval '29 days') AS replayable,r.owner,c.workspace,c.id AS source_id,c.body,v.media_type,v.size_bytes,v.sha256 FROM context.jobs j JOIN context.resources r ON r.id=j.asset JOIN context.revisions v ON v.id=j.revision JOIN context.content c ON c.id=v.content WHERE j.id=$1")
            .bind(Uuid::from(id)).fetch_optional(self.database.pool()).await.map_err(context_storage::Error::from)?.ok_or(context_storage::Error::Unavailable)?;
        let body: Value = row.try_get("body").map_err(context_storage::Error::from)?;
        let scopes = row
            .try_get::<Vec<String>, _>("auth_scopes")
            .map_err(context_storage::Error::from)?
            .into_iter()
            .map(|name| serde_json::from_value(Value::String(name)).map_err(|_| Error::Contract))
            .collect::<Result<_>>()?;
        let hash: Vec<u8> = row
            .try_get("sha256")
            .map_err(context_storage::Error::from)?;
        Ok(Job {
            kind: row.try_get("kind").map_err(context_storage::Error::from)?,
            replayable: row
                .try_get("replayable")
                .map_err(context_storage::Error::from)?,
            id,
            asset: row
                .try_get::<Uuid, _>("asset")
                .map_err(context_storage::Error::from)?
                .into(),
            revision: row
                .try_get::<Uuid, _>("revision")
                .map_err(context_storage::Error::from)?
                .into(),
            principal: row
                .try_get::<Uuid, _>("principal")
                .map_err(context_storage::Error::from)?
                .into(),
            tenant: row
                .try_get::<Uuid, _>("owner")
                .map_err(context_storage::Error::from)?
                .into(),
            workspace: row
                .try_get::<Uuid, _>("workspace")
                .map_err(context_storage::Error::from)?
                .into(),
            source: row
                .try_get::<Uuid, _>("source_id")
                .map_err(context_storage::Error::from)?
                .into(),
            generation: row
                .try_get("generation")
                .map_err(context_storage::Error::from)?,
            media_type: row
                .try_get("media_type")
                .map_err(context_storage::Error::from)?,
            size: row
                .try_get::<i64, _>("size_bytes")
                .map_err(context_storage::Error::from)? as u64,
            sha256: hash.try_into().map_err(|_| Error::Contract)?,
            object_key: body["object_key"].as_str().ok_or(Error::Contract)?.into(),
            name: body["name"].as_str().ok_or(Error::Contract)?.into(),
            pipeline: serde_json::from_str(
                &row.try_get::<String, _>("workflow")
                    .map_err(context_storage::Error::from)?,
            )
            .map_err(|_| Error::Contract)?,
            scopes,
        })
    }

    pub(crate) async fn live(&self, job: &Job) -> Result<()> {
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.jobs j JOIN context.assets a ON a.id=j.asset JOIN context.resources r ON r.id=a.id WHERE j.id=$1 AND j.state='running' AND j.generation=a.generation AND NOT r.purging AND j.deadline>now())")
            .bind(Uuid::from(job.id)).fetch_one(self.database.pool()).await.map_err(context_storage::Error::from)?;
        if !valid {
            return Err(Error::Cancelled);
        }
        // This is the durable admitted intent's scope ceiling, not a cached bearer.
        // Token expiry does not undo accepted jobs; current resource grants still do.
        self.database
            .authorized_view(
                &job.actor(),
                Resource::Asset(job.asset),
                if job.kind == "media" {
                    Permission::Read
                } else {
                    Permission::Ingest
                },
            )
            .await?
            .commit()
            .await?;
        Ok(())
    }

    pub(crate) async fn stage(
        &self,
        plane: &Plane,
        job: &Job,
        spec: Stage<'_>,
        cancel: &CancellationToken,
    ) -> Result<Value> {
        let Stage {
            name,
            role,
            request,
            mode,
        } = spec;
        let stage = name.as_str();
        let cleanup = matches!(mode, StageMode::Cleanup);
        let native_file = matches!(mode, StageMode::NativeInput);
        if plane.stop.is_cancelled() {
            return Err(Error::Stopping);
        }
        if !cleanup {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            self.live(job).await?;
        }
        let fingerprint = job.pipeline.fingerprints.get(role).ok_or(Error::Contract)?;
        if &self.workflows.fingerprint(role)? != fingerprint {
            return Err(Error::MissingCapability);
        }
        let bytes = context_workflows::host_json(
            &serde_json::to_vec(&request).map_err(|_| Error::Contract)?,
        )
        .map_err(|_| Error::Contract)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(Error::Contract);
        }
        let hash: [u8; 32] = Sha256::digest(&bytes).into();
        let mut tx = self.database.transaction().await?;
        let state: String =
            sqlx::query_scalar("SELECT state FROM context.jobs WHERE id=$1 FOR UPDATE")
                .bind(Uuid::from(job.id))
                .fetch_one(&mut *tx)
                .await
                .map_err(context_storage::Error::from)?;
        if !cleanup && state != "running" {
            return Err(Error::Cancelled);
        }
        sqlx::query("INSERT INTO context.workflow_stages(job,stage,role,fingerprint,request_digest,request) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(job,stage) DO NOTHING")
            .bind(Uuid::from(job.id)).bind(stage).bind(role).bind(fingerprint).bind(hash.as_slice()).bind(&request)
            .execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        let row = sqlx::query(
            "SELECT * FROM context.workflow_stages WHERE job=$1 AND stage=$2 FOR UPDATE",
        )
        .bind(Uuid::from(job.id))
        .bind(stage)
        .fetch_one(&mut *tx)
        .await
        .map_err(context_storage::Error::from)?;
        if row
            .try_get::<String, _>("role")
            .map_err(context_storage::Error::from)?
            != role
            || row
                .try_get::<String, _>("fingerprint")
                .map_err(context_storage::Error::from)?
                != *fingerprint
        {
            return Err(Error::Contract);
        }
        let run: Option<String> = row
            .try_get("run_id")
            .map_err(context_storage::Error::from)?;
        let frozen: Option<Value> = row
            .try_get("request")
            .map_err(context_storage::Error::from)?;
        tx.commit().await.map_err(context_storage::Error::from)?;
        let handle = match run {
            Some(run_id) => Handle {
                job: job.id,
                role: role.into(),
                fingerprint: fingerprint.clone(),
                run_id,
            },
            None => {
                let frozen = frozen.ok_or(Error::Contract)?;
                if !job.replayable {
                    return Err(Error::Contract);
                }
                let digest = Sha256::digest(
                    context_workflows::host_json(
                        &serde_json::to_vec(&frozen).map_err(|_| Error::Contract)?,
                    )
                    .map_err(|_| Error::Contract)?,
                );
                if row
                    .try_get::<Vec<u8>, _>("request_digest")
                    .map_err(context_storage::Error::from)?
                    != digest.as_slice()
                {
                    return Err(Error::Contract);
                }
                if !cleanup {
                    self.live(job).await?;
                    if cancel.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                }
                if native_file {
                    write_request(&plane.directory(job.id), &frozen).await?;
                }
                let handle = self
                    .workflows
                    .start(job.id, role, &frozen, &format!("{}:{stage}", job.id))
                    .await?;
                sqlx::query("UPDATE context.workflow_stages SET run_id=$3,request=NULL WHERE job=$1 AND stage=$2")
                    .bind(Uuid::from(job.id)).bind(stage).bind(&handle.run_id).execute(self.database.pool()).await.map_err(context_storage::Error::from)?;
                handle
            }
        };
        sqlx::query("UPDATE context.jobs SET stage=$2,runtime_run=$3,updated_at=now() WHERE id=$1")
            .bind(Uuid::from(job.id))
            .bind(stage)
            .bind(&handle.run_id)
            .execute(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
        loop {
            if plane.stop.is_cancelled() {
                return Err(Error::Stopping);
            }
            if !cleanup && cancel.is_cancelled() {
                let _ = self
                    .workflows
                    .cancel(&handle, &format!("cancel:{stage}"))
                    .await;
                return Err(Error::Cancelled);
            }
            match self.workflows.inspect(&handle).await {
                Ok(State::Running) => {}
                Ok(State::Succeeded(value)) => return Ok(value),
                Ok(State::Failed(code)) => {
                    return Err(context_workflows::host::Error::Failed(code).into());
                }
                Ok(State::Cancelled) => return Err(Error::Cancelled),
                Err(context_workflows::host::Error::Runtime(error))
                    if error.code().to_string() == "E_FENCED" => {}
                Err(error) => return Err(error.into()),
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    async fn process(&self, plane: &Plane, id: JobId, cancel: &CancellationToken) -> Result<()> {
        let job = self.job(id).await?;
        self.live(&job).await?;
        let authority: Value = sqlx::query_scalar(
            "SELECT authority FROM context.workflow_authority WHERE job=$1 AND active",
        )
        .bind(Uuid::from(id))
        .fetch_one(self.database.pool())
        .await
        .map_err(context_storage::Error::from)?;
        let authority: context_workflows::host::JobGrant =
            serde_json::from_value(authority).map_err(|_| Error::Contract)?;
        if authority.job != id
            || authority.principal != job.principal
            || authority.tenant != job.tenant
            || authority.fingerprints != job.pipeline.fingerprints
        {
            return Err(Error::Contract);
        }
        self.workflows.grant_job(authority)?;
        let stage: String = sqlx::query_scalar("SELECT stage FROM context.jobs WHERE id=$1")
            .bind(Uuid::from(id))
            .fetch_one(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
        if stage == "indexed" {
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM context.content WHERE producer_job=$1 AND kind='chunk' AND NOT published")
                .bind(Uuid::from(job.id)).fetch_one(self.database.pool()).await.map_err(context_storage::Error::from)?;
            if count == 0 {
                return Err(Error::Contract);
            }
            let _lock = plane.tenant_lock(job.tenant).await;
            return self.publish_indexed(plane, &job, count as usize).await;
        }
        if job.kind == "media" && stage == "rendered" {
            return self.publish_media(plane, &job).await;
        }
        let directory = plane.directory(id);
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&directory)
            .map_err(context_storage::Error::from)?;
        let input = directory.join("source.input");
        if input.exists() {
            verify_file(&directory, "source.input", job.size, &job.sha256).await?;
        } else {
            let _lock = plane.tenant_lock(job.tenant).await;
            self.live(&job).await?;
            plane
                .blobs
                .download_verified(
                    &job.object_key,
                    &input,
                    job.size,
                    &job.sha256,
                    plane.config.max_upload_bytes,
                )
                .await?;
        }
        if job.kind == "media" {
            return self.process_media(plane, &job, cancel).await;
        }
        let mut chunks = Vec::new();
        let mut cursor = Value::Null;
        let mut seen = BTreeSet::new();
        let mut metadata = Value::Null;
        for page in 0..plane.config.max_segments {
            let url = plane
                .blobs
                .download_url(&job.object_key, Duration::from_secs(900))
                .await?;
            let request = json!({"job":id,"asset":{"id":job.asset,"revision":job.revision,"name":job.name,"media_type":job.media_type,"size_bytes":job.size},
                "directory":directory,"source_file":"source.input","download_url":url,"cursor":cursor,
                "limits":{"max_segments":plane.config.max_segments,"max_text_bytes":262144}});
            let value = self
                .stage(
                    plane,
                    &job,
                    Stage::new(
                        format!("extract:{page}"),
                        &job.pipeline.profile.extract_role,
                        request,
                    )
                    .native(),
                    cancel,
                )
                .await?;
            let extraction: Extraction = document(value)?;
            if crate::uploads::media_type(&extraction.media_type)? != job.media_type {
                return Err(Error::Contract);
            }
            if let Some(value) = extraction.metadata {
                if serde_json::to_vec(&value)
                    .map_err(|_| Error::Contract)?
                    .len()
                    > 16_384
                {
                    return Err(Error::Contract);
                }
                metadata = value;
            }
            for segment in extraction.segments {
                if chunks.len() >= plane.config.max_segments || !segment.location.is_valid() {
                    return Err(Error::Contract);
                }
                let location = segment.location.clone();
                let origin = if segment.kind == SegmentKind::Image {
                    Origin::Generated
                } else {
                    Origin::OriginalEvidence
                };
                let text = match segment.kind {
                    SegmentKind::Text => match &segment.text {
                        Some(text)
                            if segment.file.is_none()
                                && segment.data_base64.is_none()
                                && segment.reference.is_none() =>
                        {
                            text.clone()
                        }
                        None => String::from_utf8(
                            self.segment_bytes(
                                plane,
                                &job,
                                &directory,
                                &segment,
                                chunks.len(),
                                cancel,
                            )
                            .await?,
                        )
                        .map_err(|_| Error::Contract)?,
                        _ => return Err(Error::Contract),
                    },
                    SegmentKind::Image | SegmentKind::Audio => {
                        let bytes = self
                            .segment_bytes(plane, &job, &directory, &segment, chunks.len(), cancel)
                            .await?;
                        let role = if segment.kind == SegmentKind::Image {
                            job.pipeline.profile.describe_role.as_deref()
                        } else {
                            job.pipeline.profile.transcribe_role.as_deref()
                        }
                        .ok_or(Error::MissingCapability)?;
                        let encoded = STANDARD.encode(bytes);
                        let media = segment.media_type.as_deref().ok_or(Error::Contract)?;
                        let request = if segment.kind == SegmentKind::Image {
                            json!({"data_url":format!("data:{media};base64,{encoded}"),"media_type":media,"location":location})
                        } else {
                            json!({"data_base64":encoded,"media_type":media,"location":location})
                        };
                        let value = self
                            .stage(
                                plane,
                                &job,
                                Stage::new(format!("decode:{}", chunks.len()), role, request),
                                cancel,
                            )
                            .await?;
                        value["text"].as_str().ok_or(Error::Contract)?.into()
                    }
                };
                if text.len() > 262_144 {
                    return Err(Error::Contract);
                }
                for text in chunk(&text, job.pipeline.profile.chunk_bytes) {
                    if chunks.len() >= plane.config.max_segments {
                        return Err(Error::Contract);
                    }
                    chunks.push((text, location.clone(), origin));
                }
            }
            match extraction.next_cursor {
                None => break,
                Some(next) => {
                    if !seen.insert(serde_json::to_string(&next).map_err(|_| Error::Contract)?)
                        || page + 1 == plane.config.max_segments
                    {
                        return Err(Error::Contract);
                    }
                    cursor = next;
                }
            }
        }
        if chunks.is_empty() {
            return Err(Error::Contract);
        }
        let mut excerpt = String::new();
        for (text, _, _) in &chunks {
            let remaining = 32_768_usize.saturating_sub(excerpt.len());
            if remaining == 0 {
                break;
            }
            excerpt.push_str(&truncate(text, remaining));
            if excerpt.len() < 32_768 {
                excerpt.push('\n');
            }
        }
        let enriched = self
            .stage(
                plane,
                &job,
                Stage::new(
                    "enrich",
                    &job.pipeline.profile.enrich_role,
                    json!({"text":excerpt,"name":job.name,"media_type":job.media_type}),
                ),
                cancel,
            )
            .await?;
        let enriched: Enrichment = document(enriched)?;
        if enriched.summary.len() > 16_384
            || enriched.entities.len() > 100
            || enriched.relationships.len() > 200
            || enriched.entities.iter().any(|entity| {
                entity.name.is_empty() || entity.name.len() > 512 || entity.entity_type.len() > 128
            })
        {
            return Err(Error::Contract);
        }
        let entity_ids: BTreeMap<_, _> = enriched
            .entities
            .iter()
            .map(|entity| (entity.name.clone(), ContentId::new()))
            .collect();
        if entity_ids.len() != enriched.entities.len()
            || enriched.relationships.iter().any(|r| {
                !entity_ids.contains_key(&r.from)
                    || !entity_ids.contains_key(&r.to)
                    || r.relationship.is_empty()
                    || r.relationship.len() > 128
            })
        {
            return Err(Error::Contract);
        }
        let mut bodies = Vec::new();
        for (index, (text, location, origin)) in chunks.into_iter().enumerate() {
            let embedding = self
                .stage(
                    plane,
                    &job,
                    Stage::new(
                        format!("embed:{index}"),
                        &job.pipeline.space.document_role,
                        json!({"text":text}),
                    ),
                    cancel,
                )
                .await?;
            let embedding = vector(
                embedding,
                &job.pipeline.profile.space,
                job.pipeline.space.dimensions,
            )?;
            let source = SourceReference {
                asset: job.asset,
                revision: job.revision,
                location,
                available: true,
            };
            let id = ContentId::new();
            let tokens = plane.tokenizer.encode_ordinary(&text).len();
            bodies.push((id,json!({"text":text,"sources":[source],"origin":origin,"embedding":embedding,"tokens":tokens}),embedding,origin));
        }
        let _lock = plane.tenant_lock(job.tenant).await;
        self.live(&job).await?;
        // Remove only this job's unpublished derivatives from an interrupted attempt.
        self.clear_staged(plane, &job).await?;
        let mut tx = self.database.transaction().await?;
        for (id, body, _, origin) in &bodies {
            sqlx::query("INSERT INTO context.content(id,workspace,asset,revision,kind,origin,body,embedding_space,producer_job) VALUES($1,$2,$3,$4,'chunk',$5,$6,$7,$8)")
                .bind(Uuid::from(*id)).bind(Uuid::from(job.workspace)).bind(Uuid::from(job.asset)).bind(Uuid::from(job.revision))
                .bind(serde_json::to_value(origin).map_err(|_|Error::Contract)?.as_str().ok_or(Error::Contract)?)
                .bind(body).bind(&job.pipeline.profile.space).bind(Uuid::from(job.id)).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
            sqlx::query("INSERT INTO context.dependencies VALUES($1,$2,'derivation')")
                .bind(Uuid::from(job.source))
                .bind(Uuid::from(*id))
                .execute(&mut *tx)
                .await
                .map_err(context_storage::Error::from)?;
        }
        for entity in &enriched.entities {
            let id = entity_ids[&entity.name];
            let relationships=enriched.relationships.iter().filter(|r|r.from==entity.name ||r.to==entity.name).map(|r|json!({"from":entity_ids[&r.from],"to":entity_ids[&r.to],"relationship":r.relationship})).collect::<Vec<_>>();
            let source = SourceReference {
                asset: job.asset,
                revision: job.revision,
                location: context_core::content::SourceLocation::Asset,
                available: true,
            };
            sqlx::query("INSERT INTO context.content(id,workspace,asset,revision,kind,origin,body,producer_job) VALUES($1,$2,$3,$4,'entity','generated',$5,$6)")
                .bind(Uuid::from(id)).bind(Uuid::from(job.workspace)).bind(Uuid::from(job.asset)).bind(Uuid::from(job.revision))
                .bind(json!({"name":entity.name,"entity_type":entity.entity_type,"relationships":relationships,"sources":[source]})).bind(Uuid::from(job.id)).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
            sqlx::query("INSERT INTO context.dependencies VALUES($1,$2,'derivation')")
                .bind(Uuid::from(job.source))
                .bind(Uuid::from(id))
                .execute(&mut *tx)
                .await
                .map_err(context_storage::Error::from)?;
        }
        sqlx::query(
            "UPDATE context.content SET body=body||$2::jsonb WHERE id=$1 AND NOT published",
        )
        .bind(Uuid::from(job.source))
        .bind(json!({"metadata":metadata,"enrichment":{"origin":"generated","document":enriched}}))
        .execute(&mut *tx)
        .await
        .map_err(context_storage::Error::from)?;
        tx.commit().await.map_err(context_storage::Error::from)?;
        let search = plane
            .search
            .get(&job.pipeline.profile.space)
            .ok_or(Error::Configuration)?;
        for (id, body, embedding, origin) in &bodies {
            self.live(&job).await?;
            search
                .upsert(&SearchDocument {
                    id: *id,
                    workspace: job.workspace,
                    revision: Some(job.revision),
                    text: body["text"].as_str().ok_or(Error::Contract)?.into(),
                    origin: *origin,
                    embedding: embedding.clone(),
                })
                .await?;
        }
        sqlx::query(
            "UPDATE context.jobs SET stage='indexed',detail=$2 WHERE id=$1 AND state='running'",
        )
        .bind(Uuid::from(id))
        .bind(json!({"chunks":bodies.len()}))
        .execute(self.database.pool())
        .await
        .map_err(context_storage::Error::from)?;
        self.publish_indexed(plane, &job, bodies.len()).await
    }

    async fn publish_indexed(&self, plane: &Plane, job: &Job, chunks: usize) -> Result<()> {
        self.live(job).await?;
        match tokio::fs::remove_dir_all(plane.directory(job.id)).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(context_storage::Error::from(error).into()),
        }
        let mut tx = self.database.transaction().await?;
        let state: String =
            sqlx::query_scalar("SELECT state FROM context.jobs WHERE id=$1 FOR UPDATE")
                .bind(Uuid::from(job.id))
                .fetch_one(&mut *tx)
                .await
                .map_err(context_storage::Error::from)?;
        if state != "running" {
            return Err(Error::Cancelled);
        }
        let foreign:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.content WHERE revision=$1 AND kind<>'source' AND NOT published AND producer_job IS DISTINCT FROM $2)")
            .bind(Uuid::from(job.revision)).bind(Uuid::from(job.id)).fetch_one(&mut *tx).await.map_err(context_storage::Error::from)?;
        if foreign {
            return Err(Error::Contract);
        }
        publish_revision(&mut tx, job.asset, job.revision, job.generation).await?;
        sqlx::query("UPDATE context.resources SET label=$2 WHERE id=$1")
            .bind(Uuid::from(job.asset))
            .bind(&job.name)
            .execute(&mut *tx)
            .await
            .map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.jobs SET state='succeeded',stage='published',detail=$2,updated_at=now() WHERE id=$1")
            .bind(Uuid::from(job.id)).bind(json!({"chunks":chunks})).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.workflow_authority SET active=false WHERE job=$1")
            .bind(Uuid::from(job.id))
            .execute(&mut *tx)
            .await
            .map_err(context_storage::Error::from)?;
        tx.commit().await.map_err(context_storage::Error::from)?;
        self.workflows.revoke_job(job.id)?;
        Ok(())
    }

    async fn segment_bytes(
        &self,
        plane: &Plane,
        job: &Job,
        directory: &Path,
        segment: &Segment,
        index: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>> {
        if [
            segment.file.is_some(),
            segment.data_base64.is_some(),
            segment.reference.is_some(),
        ]
        .into_iter()
        .filter(|value| *value)
        .count()
            != 1
        {
            return Err(Error::Contract);
        }
        let bytes = if let Some(file) = &segment.file {
            read_file(directory, file, 2 * 1024 * 1024).await?
        } else if let Some(data) = &segment.data_base64 {
            if data.len() > 3 * 1024 * 1024 {
                return Err(Error::Contract);
            }
            STANDARD.decode(data).map_err(|_| Error::Contract)?
        } else {
            let role = job
                .pipeline
                .profile
                .artifact_role
                .as_deref()
                .ok_or(Error::MissingCapability)?;
            let value = self
                .stage(
                    plane,
                    job,
                    Stage::new(
                        format!("artifact:{index}"),
                        role,
                        json!({"reference":segment.reference,"max_bytes":2*1024*1024}),
                    ),
                    cancel,
                )
                .await?;
            STANDARD
                .decode(value["data_base64"].as_str().ok_or(Error::Contract)?)
                .map_err(|_| Error::Contract)?
        };
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(Error::Contract);
        }
        if segment.file.is_some() && segment.sha256.is_none() {
            return Err(Error::Contract);
        }
        if let Some(hash) = &segment.sha256
            && Sha256::digest(&bytes).as_slice() != crate::uploads::digest(hash)?
        {
            return Err(context_storage::Error::Integrity.into());
        }
        Ok(bytes)
    }

    pub(crate) async fn clear_staged(&self, plane: &Plane, job: &Job) -> Result<()> {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM context.content WHERE producer_job=$1 AND NOT published AND kind<>'source'",
        )
        .bind(Uuid::from(job.id))
        .fetch_all(self.database.pool())
        .await
        .map_err(context_storage::Error::from)?;
        let key =
            std::env::var(&plane.config.typesense_key_env).map_err(|_| Error::Configuration)?;
        let index = context_storage::search::Search::new(
            &plane.config.typesense_endpoint,
            &key,
            &job.pipeline.space.collection,
            job.pipeline.space.dimensions,
        )?;
        for id in &ids {
            index.delete((*id).into()).await?;
        }
        let keys: Vec<String> =
            sqlx::query_scalar("SELECT object_key FROM context.objects WHERE content=ANY($1)")
                .bind(&ids)
                .fetch_all(self.database.pool())
                .await
                .map_err(context_storage::Error::from)?;
        for key in &keys {
            plane.blobs.delete(key).await?;
        }
        sqlx::query("DELETE FROM context.objects WHERE object_key=ANY($1)")
            .bind(keys)
            .execute(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
        sqlx::query("DELETE FROM context.content WHERE id=ANY($1)")
            .bind(ids)
            .execute(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
        Ok(())
    }

    async fn reconcile_normal(&self, job: &Job) -> Result<Vec<(String, Handle)>> {
        let stages=sqlx::query("SELECT * FROM context.workflow_stages WHERE job=$1 AND stage NOT LIKE 'cleanup:%' ORDER BY stage")
            .bind(Uuid::from(job.id)).fetch_all(self.database.pool()).await.map_err(context_storage::Error::from)?;
        let mut handles = Vec::new();
        for stage in stages {
            let name: String = stage
                .try_get("stage")
                .map_err(context_storage::Error::from)?;
            let role: String = stage
                .try_get("role")
                .map_err(context_storage::Error::from)?;
            let fingerprint: String = stage
                .try_get("fingerprint")
                .map_err(context_storage::Error::from)?;
            if job.pipeline.fingerprints.get(&role) != Some(&fingerprint) {
                return Err(Error::Contract);
            }
            let run: Option<String> = stage
                .try_get("run_id")
                .map_err(context_storage::Error::from)?;
            let handle = match run {
                Some(run_id) => Handle {
                    job: job.id,
                    role,
                    fingerprint,
                    run_id,
                },
                None => {
                    // Never mistake a fresh admission for an expired lost receipt.
                    if !job.replayable {
                        return Err(Error::Contract);
                    }
                    let request: Value = stage
                        .try_get::<Option<Value>, _>("request")
                        .map_err(context_storage::Error::from)?
                        .ok_or(Error::Contract)?;
                    let bytes = context_workflows::host_json(
                        &serde_json::to_vec(&request).map_err(|_| Error::Contract)?,
                    )
                    .map_err(|_| Error::Contract)?;
                    if stage
                        .try_get::<Vec<u8>, _>("request_digest")
                        .map_err(context_storage::Error::from)?
                        != Sha256::digest(bytes).as_slice()
                    {
                        return Err(Error::Contract);
                    }
                    // RuntimePolicy is already fenced: replay can recover/create a run,
                    // but cannot dispatch a fresh normal provider operation.
                    let handle = self
                        .workflows
                        .start(job.id, &role, &request, &format!("{}:{name}", job.id))
                        .await?;
                    sqlx::query("UPDATE context.workflow_stages SET run_id=$3,request=NULL WHERE job=$1 AND stage=$2")
                        .bind(Uuid::from(job.id)).bind(&name).bind(&handle.run_id).execute(self.database.pool()).await.map_err(context_storage::Error::from)?;
                    handle
                }
            };
            self.workflows
                .cancel(&handle, &format!("settle:{name}"))
                .await?;
            handles.push((name, handle));
        }
        Ok(handles)
    }

    pub(crate) async fn fail_job(&self, plane: &Plane, id: JobId, error: &Error) -> Result<()> {
        let code = match error {
            Error::Cancelled => "cancelled",
            Error::Contract => "contract",
            _ => "processing_failed",
        };
        let mut tx = self.database.transaction().await?;
        let changed=sqlx::query("UPDATE context.jobs SET state=CASE WHEN state='cancelled' OR $2='cancelled' THEN 'cancelled' ELSE 'failed' END,error_code=COALESCE(error_code,$2),detail=detail||'{\"cleanup_pending\":true}'::jsonb,updated_at=now() WHERE id=$1 AND (state IN ('accepted','running') OR (state IN ('failed','cancelled') AND detail->>'cleanup_pending'='true'))")
            .bind(Uuid::from(id)).bind(code).execute(&mut *tx).await.map_err(context_storage::Error::from)?.rows_affected();
        if changed == 0 {
            return Ok(());
        }
        let authority:Value=sqlx::query_scalar("UPDATE context.workflow_authority SET authority=authority||'{\"fenced\":true}'::jsonb WHERE job=$1 AND active RETURNING authority")
            .bind(Uuid::from(id)).fetch_one(&mut *tx).await.map_err(context_storage::Error::from)?;
        let grant: context_workflows::host::JobGrant =
            serde_json::from_value(authority).map_err(|_| Error::Contract)?;
        tx.commit().await.map_err(context_storage::Error::from)?;
        self.workflows.grant_job(grant)?;
        self.workflows.fence_job(id)?;
        let job = self.job(id).await?;
        let handles = self.reconcile_normal(&job).await?;
        let cleanup_roles = job.pipeline.profile.cleanup_role.iter().cloned().collect();
        self.workflows.allow_cleanup(id, &cleanup_roles)?;
        if let (Some(node), Some(role)) = (
            &job.pipeline.profile.cleanup_node,
            &job.pipeline.profile.cleanup_role,
        ) {
            for (name, handle) in handles
                .iter()
                .filter(|(name, _)| name.starts_with("extract:"))
            {
                if let Some(checkpoint) = self.workflows.checkpoint(handle, node).await? {
                    let value = self
                        .stage(
                            plane,
                            &job,
                            Stage::new(
                                format!("cleanup:{name}"),
                                role,
                                json!({"checkpoint":checkpoint,"job":id}),
                            )
                            .cleanup(),
                            &CancellationToken::new(),
                        )
                        .await?;
                    if value["done"] != true {
                        return Err(Error::Contract);
                    }
                }
            }
        }
        let _lock = plane.tenant_lock(job.tenant).await;
        self.clear_staged(plane, &job).await?;
        match tokio::fs::remove_dir_all(plane.directory(id)).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(context_storage::Error::from(error).into()),
        }
        let mut tx = self.database.transaction().await?;
        sqlx::query("UPDATE context.jobs SET stage='settled',detail=detail||'{\"cleanup_pending\":false,\"provider_cleanup_confirmed\":true}'::jsonb,updated_at=now() WHERE id=$1")
            .bind(Uuid::from(id)).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.workflow_authority SET active=false WHERE job=$1")
            .bind(Uuid::from(id))
            .execute(&mut *tx)
            .await
            .map_err(context_storage::Error::from)?;
        tx.commit().await.map_err(context_storage::Error::from)?;
        self.workflows.revoke_job(id)?;
        Ok(())
    }

    async fn maintenance(&self, plane: &Plane) -> Result<()> {
        self.clear_acquisitions(false).await?;
        let expired=sqlx::query("SELECT u.id,u.asset,r.owner FROM context.uploads u JOIN context.resources r ON r.id=u.asset WHERE u.expires_at<now() AND NOT r.purging ORDER BY u.expires_at LIMIT 16")
            .fetch_all(self.database.pool()).await.map_err(context_storage::Error::from)?;
        for row in expired {
            let upload: Uuid = row.try_get("id").map_err(context_storage::Error::from)?;
            let asset: Uuid = row.try_get("asset").map_err(context_storage::Error::from)?;
            let tenant = TenantId::from(
                row.try_get::<Uuid, _>("owner")
                    .map_err(context_storage::Error::from)?,
            );
            let _lock = plane.tenant_lock(tenant).await;
            let mut tx = self.database.transaction().await?;
            let row=sqlx::query("SELECT u.*,v.content,a.current_revision FROM context.uploads u JOIN context.revisions v ON v.id=u.revision JOIN context.assets a ON a.id=u.asset JOIN context.resources r ON r.id=a.id WHERE u.id=$1 AND u.expires_at<now() AND NOT r.purging FOR UPDATE OF u")
                .bind(upload).fetch_optional(&mut *tx).await.map_err(context_storage::Error::from)?;
            let Some(row) = row else {
                continue;
            };
            let temporary: String = row
                .try_get("object_key")
                .map_err(context_storage::Error::from)?;
            plane.blobs.delete(&temporary).await?;
            sqlx::query("DELETE FROM context.objects WHERE object_key=$1")
                .bind(&temporary)
                .execute(&mut *tx)
                .await
                .map_err(context_storage::Error::from)?;
            if !row
                .try_get::<bool, _>("finalized")
                .map_err(context_storage::Error::from)?
            {
                if let Some(source) = row
                    .try_get::<Option<String>, _>("source_object_key")
                    .map_err(context_storage::Error::from)?
                {
                    plane.blobs.delete(&source).await?;
                    sqlx::query("DELETE FROM context.objects WHERE object_key=$1")
                        .bind(source)
                        .execute(&mut *tx)
                        .await
                        .map_err(context_storage::Error::from)?;
                }
                sqlx::query("DELETE FROM context.content WHERE id=$1 AND NOT published")
                    .bind(
                        row.try_get::<Uuid, _>("content")
                            .map_err(context_storage::Error::from)?,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(context_storage::Error::from)?;
                sqlx::query("DELETE FROM context.resources r WHERE r.id=$1 AND NOT EXISTS(SELECT 1 FROM context.revisions v WHERE v.asset=r.id) AND EXISTS(SELECT 1 FROM context.assets a WHERE a.id=r.id AND a.current_revision IS NULL)")
                    .bind(asset).execute(&mut *tx).await.map_err(context_storage::Error::from)?;
            }
            sqlx::query("DELETE FROM context.uploads WHERE id=$1")
                .bind(upload)
                .execute(&mut *tx)
                .await
                .map_err(context_storage::Error::from)?;
            tx.commit().await.map_err(context_storage::Error::from)?;
        }
        let active = plane
            .controls
            .lock()
            .await
            .keys()
            .copied()
            .map(Uuid::from)
            .collect::<Vec<_>>();
        let pending:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM context.jobs WHERE kind IN ('ingest','reprocess','media') AND state IN ('failed','cancelled') AND detail->>'cleanup_pending'='true' AND NOT(id=ANY($1)) LIMIT 4")
            .bind(active).fetch_all(self.database.pool()).await.map_err(context_storage::Error::from)?;
        for id in pending {
            let id = JobId::from(id);
            let control = {
                let mut controls = plane.controls.lock().await;
                if controls.contains_key(&id) {
                    continue;
                }
                let control = Arc::new(Control {
                    cancel: CancellationToken::new(),
                    finished: Arc::new(tokio::sync::Notify::new()),
                });
                controls.insert(id, control.clone());
                control
            };
            if let Err(error) = self.fail_job(plane, id, &Error::Contract).await {
                tracing::warn!(job=%id,code=%failure_code(&error),"job cleanup remains pending");
                self.record_cleanup_error(id, &error).await;
            }
            plane.controls.lock().await.remove(&id);
            control.finished.notify_waiters();
        }
        Ok(())
    }

    async fn record_cleanup_error(&self, id: JobId, error: &Error) {
        // Only redacted classifications, never backend causes or provider content.
        let _=sqlx::query("UPDATE context.jobs SET detail=detail||$2::jsonb WHERE id=$1 AND detail->>'cleanup_pending'='true'")
            .bind(Uuid::from(id)).bind(json!({"cleanup_error":failure_code(error)})).execute(self.database.pool()).await;
    }
}

fn failure_code(error: &Error) -> String {
    match error {
        Error::Workflow(context_workflows::host::Error::Runtime(error)) => error.code().to_string(),
        Error::Workflow(error) => error.to_string(),
        Error::Storage(context_storage::Error::Database(error)) => error
            .as_database_error()
            .and_then(|e| e.code())
            .map_or_else(|| "database".into(), |code| code.to_string()),
        Error::Storage(error) => error.to_string(),
        error => error.to_string(),
    }
}

fn document<T: DeserializeOwned>(result: Value) -> Result<T> {
    let document: Document =
        serde_json::from_value(result.get("document").cloned().ok_or(Error::Contract)?)
            .map_err(|_| Error::Contract)?;
    document.decode().map_err(|_| Error::Contract)
}

pub(crate) fn vector(value: Value, space: &str, dimensions: usize) -> Result<Vec<f32>> {
    if value["space"] != space {
        return Err(Error::Contract);
    }
    let vector: Vec<f32> =
        serde_json::from_value(value["embedding"].clone()).map_err(|_| Error::Contract)?;
    if vector.len() != dimensions
        || vector.iter().any(|v| !v.is_finite())
        || vector.iter().all(|v| *v == 0.0)
    {
        return Err(Error::Contract);
    }
    Ok(vector)
}

fn truncate(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn chunk(text: &str, max: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        let piece = truncate(remaining, max);
        let size = piece.len();
        if !piece.trim().is_empty() {
            chunks.push(piece.clone());
        }
        remaining = &remaining[size..];
    }
    chunks
}

pub(crate) async fn read_file(directory: &Path, name: &str, max: u64) -> Result<Vec<u8>> {
    if name.is_empty()
        || name.len() > 128
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err(Error::Contract);
    }
    let mut options = tokio::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = options
        .open(directory.join(name))
        .await
        .map_err(context_storage::Error::from)?;
    if !file
        .metadata()
        .await
        .map_err(context_storage::Error::from)?
        .is_file()
    {
        return Err(Error::Contract);
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(context_storage::Error::from)?;
    if bytes.len() as u64 > max {
        return Err(Error::Contract);
    }
    Ok(bytes)
}

async fn write_request(directory: &Path, request: &Value) -> Result<()> {
    let temporary = directory.join(format!("request-{}.tmp", Uuid::now_v7()));
    let mut options = tokio::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&temporary)
        .await
        .map_err(context_storage::Error::from)?;
    file.write_all(&serde_json::to_vec(request).map_err(|_| Error::Contract)?)
        .await
        .map_err(context_storage::Error::from)?;
    file.sync_all()
        .await
        .map_err(context_storage::Error::from)?;
    drop(file);
    tokio::fs::rename(temporary, directory.join("request.json"))
        .await
        .map_err(context_storage::Error::from)?;
    Ok(())
}

async fn verify_file(directory: &Path, name: &str, size: u64, expected: &[u8; 32]) -> Result<()> {
    let mut options = tokio::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(directory.join(name))
        .await
        .map_err(context_storage::Error::from)?;
    let metadata = file
        .metadata()
        .await
        .map_err(context_storage::Error::from)?;
    if !metadata.is_file() || metadata.len() != size {
        return Err(context_storage::Error::Integrity.into());
    }
    let mut buffer = vec![0; 65_536];
    let mut hash = Sha256::new();
    let mut read = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(context_storage::Error::from)?;
        if count == 0 {
            break;
        }
        read = read.checked_add(count as u64).ok_or(Error::Contract)?;
        if read > size {
            return Err(context_storage::Error::Integrity.into());
        }
        hash.update(&buffer[..count]);
    }
    if read != size || hash.finalize().as_slice() != expected {
        return Err(context_storage::Error::Integrity.into());
    }
    Ok(())
}
