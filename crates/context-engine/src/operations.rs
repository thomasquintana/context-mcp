//! Bounded synchronous-role execution on the Actix owner of non-Send HTLK admission.
//!
//! These are request-scoped computations, not accepted asynchronous ingestion jobs.
//! No operational output is persisted in application knowledge. Restart restores no
//! transient grant, so orphaned HTLK runs cannot dispatch through application policy.

use crate::{Application, Error, Result, plane::Plane};
use context_core::{
    access::{Authentication, Permission, Resource},
    ids::{ContentId, JobId},
};
use context_workflows::host::{JobGrant, State};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(crate) struct Pending {
    pub sources: BTreeSet<ContentId>,
    pub cancel: CancellationToken,
    pub finished: Arc<tokio::sync::Notify>,
}
pub(crate) struct Invocation {
    id: JobId,
    cancel: CancellationToken,
    actor: Authentication,
    scope: Resource,
    permission: Permission,
    role: String,
    request: Value,
    sources: BTreeSet<ContentId>,
    reply: oneshot::Sender<Result<Value>>,
}
pub(crate) struct Call<'a> {
    pub actor: &'a Authentication,
    pub scope: Resource,
    pub permission: Permission,
    pub role: &'a str,
    pub request: Value,
    pub sources: BTreeSet<ContentId>,
}
impl Application {
    pub(crate) async fn stop_operations(&self) -> Result<()> {
        if let Some(plane) = &self.plane {
            let mut queue = plane.receiver.lock().await;
            while let Ok(invocation) = queue.try_recv() {
                if let Some(pending) = plane.requests.lock().await.remove(&invocation.id) {
                    pending.cancel.cancel();
                    pending.finished.notify_waiters();
                }
                let _ = invocation.reply.send(Err(Error::Stopping));
            }
        }
        Ok(())
    }
    pub(crate) async fn role(&self, call: Call<'_>) -> Result<Value> {
        let plane = self.data_plane()?;
        if plane.stop.is_cancelled() {
            return Err(Error::Stopping);
        }
        let (reply, result) = oneshot::channel();
        let id = JobId::new();
        let cancel = CancellationToken::new();
        plane.requests.lock().await.insert(
            id,
            Pending {
                sources: call.sources.clone(),
                cancel: cancel.clone(),
                finished: Arc::new(tokio::sync::Notify::new()),
            },
        );
        if plane
            .calls
            .try_send(Invocation {
                id,
                cancel,
                actor: call.actor.clone(),
                scope: call.scope,
                permission: call.permission,
                role: call.role.into(),
                request: call.request,
                sources: call.sources,
                reply,
            })
            .is_err()
        {
            plane.requests.lock().await.remove(&id);
            return Err(Error::Busy);
        }
        tokio::time::timeout(Duration::from_secs(150), result)
            .await
            .map_err(|_| Error::Busy)?
            .map_err(|_| Error::Stopping)?
    }

    pub(crate) async fn operation_loop(application: Weak<Self>, plane: Arc<Plane>) {
        loop {
            let invocation = {
                let mut queue = plane.receiver.lock().await;
                tokio::select! {_=plane.stop.cancelled()=>break,value=queue.recv()=>value}
            };
            let Some(invocation) = invocation else {
                break;
            };
            let Some(application) = application.upgrade() else {
                break;
            };
            let id = invocation.id;
            let outcome = application
                .invoke(id, &invocation, &invocation.cancel, &plane)
                .await;
            let _ = application.workflows.revoke_job(id);
            let _ = application.workflows.release_job(id);
            if let Some(pending) = plane.requests.lock().await.remove(&id) {
                pending.cancel.cancel();
                pending.finished.notify_waiters();
                tracing::debug!(job=%id,sources=pending.sources.len(),"transient role finished");
            }
            let _ = invocation.reply.send(outcome);
        }
    }

    async fn invoke(
        &self,
        id: JobId,
        call: &Invocation,
        cancel: &CancellationToken,
        plane: &Plane,
    ) -> Result<Value> {
        let mut permission = self
            .database
            .authorized_view(&call.actor, call.scope, call.permission)
            .await?;
        let mut assets = BTreeSet::new();
        for source in &call.sources {
            let protected = self.content_in(&mut permission, *source).await?;
            assets.extend(protected.assets);
        }
        let tenant: Uuid = sqlx::query_scalar("SELECT owner FROM context.resources WHERE id=$1")
            .bind(context_storage::postgres::resource_id(call.scope))
            .fetch_one(&mut **permission.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        permission.commit().await?;
        if cancel.is_cancelled() || call.reply.is_closed() {
            return Err(Error::Cancelled);
        }
        let fingerprint = self.workflows.fingerprint(&call.role)?;
        self.workflows.grant_job(JobGrant {
            job: id,
            principal: call.actor.principal,
            tenant: tenant.into(),
            assets,
            roles: BTreeSet::from([call.role.clone()]),
            fingerprints: [(call.role.clone(), fingerprint)].into(),
            fenced: false,
        })?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let handle = self
            .workflows
            .start(id, &call.role, &call.request, &id.to_string())
            .await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        loop {
            if cancel.is_cancelled()
                || plane.stop.is_cancelled()
                || call.reply.is_closed()
                || tokio::time::Instant::now() > deadline
            {
                self.workflows.fence_job(id)?;
                self.workflows.cancel(&handle, "request-finished").await?;
                return Err(Error::Cancelled);
            }
            match self.workflows.inspect(&handle).await {
                Ok(State::Running) => {}
                Ok(State::Succeeded(value)) => {
                    let mut view = self
                        .database
                        .authorized_view(&call.actor, call.scope, call.permission)
                        .await?;
                    for source in &call.sources {
                        self.content_in(&mut view, *source).await?;
                    }
                    view.commit().await?;
                    return Ok(value);
                }
                Ok(State::Failed(code)) => {
                    return Err(context_workflows::host::Error::Failed(code).into());
                }
                Ok(State::Cancelled) => return Err(Error::Cancelled),
                Err(context_workflows::host::Error::Runtime(error))
                    if error.code().to_string() == "E_FENCED" => {}
                Err(error) => return Err(error.into()),
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}
