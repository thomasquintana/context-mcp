//! Authorized context application services. Transport handlers never own domain authority.

pub mod data;
pub mod jobs;
pub mod media;
pub mod memories;
mod operations;
mod plane;
pub mod purging;
pub mod retrieval;
pub mod sharing;
pub mod synthesis;
pub mod uploads;
pub mod urls;
mod worker;

use context_auth::{Authenticator, OidcConfig};
use context_core::{
    access::{Authentication, Permission, Resource},
    ids::{CollectionId, PrincipalId, TenantId, WorkspaceId},
};
use context_storage::postgres::{
    Database, existing_receipt, insert_resource, receipt, resource_id,
};
use context_workflows::host::{Config as WorkflowConfig, JobGrant, WorkflowHost};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{collections::BTreeSet, fmt, path::Path, sync::Arc};
use uuid::Uuid;

/// Redacted application failures, mapped consistently by HTTP, MCP and CLI.
#[derive(thiserror::Error)]
pub enum Error {
    /// Input violates the application contract.
    #[error("invalid application input")]
    Invalid,
    /// Trusted configuration or retained authority is invalid.
    #[error("invalid application configuration")]
    Configuration,
    /// Authoritative storage operation failed.
    #[error("application storage operation failed")]
    Storage(#[from] context_storage::Error),
    /// Identity infrastructure/validation failed.
    #[error("authentication failed")]
    Authentication(#[from] context_auth::Error),
    /// Workflow admission, policy or execution failed.
    #[error("workflow operation failed")]
    Workflow(#[from] context_workflows::host::Error),
    /// A required configured processing capability is missing.
    #[error("required processing capability is unavailable")]
    MissingCapability,
    /// A provider result violated its declared application contract.
    #[error("processing result violated its contract")]
    Contract,
    /// Bounded work admission is full.
    #[error("processing capacity is unavailable")]
    Busy,
    /// Explicit caller cancellation won the operation.
    #[error("operation was cancelled")]
    Cancelled,
    /// Application shutdown is preserving work for restart.
    #[error("application is stopping")]
    Stopping,
}
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ApplicationError({self})")
    }
}

/// Result returned by an application service.
pub type Result<T> = std::result::Result<T, Error>;

/// Trusted startup configuration; credentials are referenced by environment name.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Environment variable holding the PostgreSQL URL.
    pub database_url_env: String,
    /// Bounded connection pool.
    pub database_connections: u32,
    /// Optional issuer for human/API access tokens; agents remain authenticated.
    pub oidc: Option<OidcConfig>,
    /// Retained operator graphs and deployment bindings.
    pub workflows: WorkflowConfig,
    /// Optional managed data plane during foundation rollout; ingestion rejects absence.
    pub data: Option<data::Config>,
}

/// The one application instance's shared services.
pub struct Application {
    database: Database,
    authentication: Authenticator,
    workflows: Arc<WorkflowHost>,
    plane: Option<Arc<plane::Plane>>,
}

impl Application {
    /// Initialize persistence, authentication and retained job authority before execution.
    ///
    /// # Errors
    /// Rejects missing secret references, invalid authority or unavailable required providers.
    pub async fn open(config: &Config, base: &Path) -> Result<Self> {
        let url = std::env::var(&config.database_url_env).map_err(|_| Error::Configuration)?;
        let database = Database::connect(&url, config.database_connections).await?;
        let authentication = Authenticator::open(database.clone(), config.oidc.clone()).await?;
        // Persist terminal-job fencing before the runtime can resume any durable run.
        sqlx::query("UPDATE context.workflow_authority a SET authority=authority||'{\"fenced\":true}'::jsonb FROM context.jobs j WHERE a.job=j.id AND a.active AND j.state IN ('failed','cancelled')")
            .execute(database.pool()).await.map_err(context_storage::Error::from)?;
        let rows=sqlx::query("SELECT a.job,a.authority,j.principal,r.owner FROM context.workflow_authority a JOIN context.jobs j ON j.id=a.job LEFT JOIN context.resources r ON r.id=j.scope WHERE a.active AND (j.state IN ('accepted','running') OR (j.state IN ('failed','cancelled') AND j.detail->>'cleanup_pending'='true')) ORDER BY j.created_at LIMIT 4097")
            .fetch_all(database.pool()).await.map_err(context_storage::Error::from)?;
        if rows.len() > 4096 {
            return Err(Error::Configuration);
        }
        let mut restored = Vec::new();
        for row in rows {
            let authority: Value = row
                .try_get("authority")
                .map_err(context_storage::Error::from)?;
            let grant: JobGrant =
                serde_json::from_value(authority).map_err(|_| Error::Configuration)?;
            if grant.roles.is_empty() || grant.fingerprints.len() != grant.roles.len() {
                return Err(Error::Configuration);
            }
            if Uuid::from(grant.job)
                != row
                    .try_get::<Uuid, _>("job")
                    .map_err(context_storage::Error::from)?
                || Uuid::from(grant.principal)
                    != row
                        .try_get::<Uuid, _>("principal")
                        .map_err(context_storage::Error::from)?
                || Some(Uuid::from(grant.tenant))
                    != row
                        .try_get::<Option<Uuid>, _>("owner")
                        .map_err(context_storage::Error::from)?
            {
                return Err(Error::Configuration);
            }
            restored.push(grant);
        }
        if let Some(parent) = base.join(&config.workflows.database).parent() {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(parent).map_err(|_| Error::Configuration)?;
        }
        if !restored.is_empty() && config.data.is_none() {
            return Err(Error::Configuration);
        }
        let workflows = Arc::new(WorkflowHost::open(&config.workflows, base, &restored).await?);
        workflows
            .ready(&config.workflows.roles.keys().cloned().collect::<Vec<_>>())
            .await?;
        let plane = match &config.data {
            Some(data) => Some(Arc::new(plane::Plane::open(data, base, &workflows).await?)),
            None => None,
        };
        let application = Self {
            database,
            authentication,
            workflows,
            plane,
        };
        application.register_indexes().await?;
        application.clear_acquisitions(true).await?;
        Ok(application)
    }

    /// Authentication boundary, used independently on every transport operation.
    pub fn authentication(&self) -> &Authenticator {
        &self.authentication
    }

    /// Trusted repository handle for worker/operator composition.
    pub fn database(&self) -> &Database {
        &self.database
    }

    /// Shared role host; provider-specific operations stay in operator graphs.
    pub fn workflows(&self) -> &WorkflowHost {
        &self.workflows
    }

    fn data_plane(&self) -> Result<&Arc<plane::Plane>> {
        self.plane.as_ref().ok_or(Error::MissingCapability)
    }

    /// Return only the authenticated caller's nonsecret identity and scope ceiling.
    pub fn identity(&self, actor: &Authentication) -> Value {
        json!({"principal":actor.principal,"scopes":actor.scopes,"credential":actor.credential})
    }

    /// Read authorized resource metadata inside the permission/data snapshot.
    ///
    /// # Errors
    /// Denies unavailable resources without returning grant lists or secret configuration.
    pub async fn resource(&self, actor: &Authentication, target: Resource) -> Result<Value> {
        let mut tx = self.database.authorized_read(actor, target).await?;
        let row = sqlx::query(
            "SELECT id,kind,owner,parent,label,acl_version FROM context.resources WHERE id=$1",
        )
        .bind(resource_id(target))
        .fetch_one(&mut **tx.transaction())
        .await
        .map_err(context_storage::Error::from)?;
        let mut result = json!({
            "id":row.try_get::<Uuid,_>("id").map_err(context_storage::Error::from)?,
            "kind":row.try_get::<String,_>("kind").map_err(context_storage::Error::from)?,
            "owner":row.try_get::<Uuid,_>("owner").map_err(context_storage::Error::from)?,
            "parent":row.try_get::<Option<Uuid>,_>("parent").map_err(context_storage::Error::from)?,
            "label":row.try_get::<String,_>("label").map_err(context_storage::Error::from)?,
            "acl_version":row.try_get::<i64,_>("acl_version").map_err(context_storage::Error::from)?,
        });
        if let Resource::Asset(asset) = target {
            let state =
                sqlx::query("SELECT generation,current_revision FROM context.assets WHERE id=$1")
                    .bind(Uuid::from(asset))
                    .fetch_one(&mut **tx.transaction())
                    .await
                    .map_err(context_storage::Error::from)?;
            result["generation"] = json!(
                state
                    .try_get::<i64, _>("generation")
                    .map_err(context_storage::Error::from)?
            );
            result["current_revision"] = json!(
                state
                    .try_get::<Option<Uuid>, _>("current_revision")
                    .map_err(context_storage::Error::from)?
            );
        }
        tx.commit().await?;
        Ok(result)
    }

    /// Create a collection under an authorized workspace with durable idempotency.
    ///
    /// # Errors
    /// Rejects invalid labels, denied Share scope, stale keys or transaction conflicts.
    pub async fn create_collection(
        &self,
        actor: &Authentication,
        workspace: WorkspaceId,
        label: &str,
        key: &str,
    ) -> Result<Value> {
        for attempt in 0..3 {
            match self
                .create_collection_once(actor, workspace, label, key)
                .await
            {
                Err(Error::Storage(context_storage::Error::Database(error)))
                    if error
                        .as_database_error()
                        .and_then(|error| error.code())
                        .is_some_and(|code| matches!(code.as_ref(), "40001" | "40P01")) =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(10 << attempt)).await;
                }
                result => return result,
            }
        }
        Err(context_storage::Error::Conflict.into())
    }

    async fn create_collection_once(
        &self,
        actor: &Authentication,
        workspace: WorkspaceId,
        label: &str,
        key: &str,
    ) -> Result<Value> {
        if label.trim().is_empty() || label.chars().count() > 256 {
            return Err(Error::Invalid);
        }
        let request = serde_json::to_vec(&(workspace, label)).map_err(|_| Error::Invalid)?;
        let digest: [u8; 32] = Sha256::digest(&request).into();
        let mut authorized = self
            .database
            .authorized_write(actor, Resource::Workspace(workspace), Permission::Share)
            .await?;
        let tx = authorized.transaction();
        if let Some(existing) =
            existing_receipt(tx, actor.principal, "create_collection", key, &digest).await?
        {
            authorized.commit().await?;
            return Ok(existing);
        }
        let owner: Uuid = sqlx::query_scalar("SELECT owner FROM context.resources WHERE id=$1")
            .bind(Uuid::from(workspace))
            .fetch_one(&mut **tx)
            .await
            .map_err(context_storage::Error::from)?;
        let collection = CollectionId::new();
        insert_resource(
            tx,
            Resource::Collection(collection),
            owner.into(),
            Some(Resource::Workspace(workspace)),
            true,
        )
        .await?;
        sqlx::query("UPDATE context.resources SET label=$2 WHERE id=$1")
            .bind(Uuid::from(collection))
            .bind(label)
            .execute(&mut **tx)
            .await
            .map_err(context_storage::Error::from)?;
        let result = json!({"collection":collection,"workspace":workspace});
        let result = receipt(
            tx,
            actor.principal,
            "create_collection",
            key,
            &digest,
            &result,
        )
        .await?;
        authorized.commit().await?;
        Ok(result)
    }

    /// Drain execution before closing authoritative application connections.
    ///
    /// # Errors
    /// Returns workflow shutdown failures.
    pub async fn shutdown(&self) -> Result<()> {
        if let Some(plane) = &self.plane {
            plane.stop.cancel();
            let tasks = std::mem::take(&mut *plane.tasks.lock().await);
            for task in tasks {
                let _ = task.await;
            }
        }
        self.workflows.shutdown().await?;
        self.stop_operations().await?;
        self.database.close().await;
        Ok(())
    }
}

/// Nonsecret result of an explicit local operator bootstrap command.
#[derive(Serialize)]
pub struct Bootstrap {
    /// Owner tenant.
    pub tenant: TenantId,
    /// Initial project/workspace.
    pub workspace: WorkspaceId,
    /// Initial collection.
    pub collection: CollectionId,
    /// Explicitly granted principal (no token is implicitly issued).
    pub principal: PrincipalId,
}

/// Create initial hierarchy and explicit principal grants as an operator command.
///
/// # Errors
/// Rejects invalid labels and authoritative persistence failures.
pub async fn bootstrap(
    database: &Database,
    principal: PrincipalId,
    label: &str,
) -> Result<Bootstrap> {
    if label.trim().is_empty() || label.chars().count() > 256 {
        return Err(Error::Invalid);
    }
    let tenant = TenantId::new();
    let workspace = WorkspaceId::new();
    let collection = CollectionId::new();
    let mut tx = database.transaction().await?;
    for (resource, parent) in [
        (Resource::Tenant(tenant), None),
        (
            Resource::Workspace(workspace),
            Some(Resource::Tenant(tenant)),
        ),
        (
            Resource::Collection(collection),
            Some(Resource::Workspace(workspace)),
        ),
    ] {
        insert_resource(&mut tx, resource, tenant, parent, true).await?;
        sqlx::query("UPDATE context.resources SET label=$2 WHERE id=$1")
            .bind(resource_id(resource))
            .bind(label)
            .execute(&mut *tx)
            .await
            .map_err(context_storage::Error::from)?;
    }
    sqlx::query("INSERT INTO context.memberships VALUES($1,$2)")
        .bind(Uuid::from(principal))
        .bind(Uuid::from(tenant))
        .execute(&mut *tx)
        .await
        .map_err(context_storage::Error::from)?;
    let permissions = BTreeSet::from([
        Permission::Read,
        Permission::Ingest,
        Permission::WriteMemory,
        Permission::Synthesize,
        Permission::Share,
        Permission::Purge,
    ]);
    let permissions = permissions
        .iter()
        .map(|permission| {
            serde_json::to_value(permission)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .ok_or(Error::Invalid)
        })
        .collect::<Result<Vec<_>>>()?;
    sqlx::query("INSERT INTO context.grants VALUES($1,'principal',$2,$3)")
        .bind(Uuid::from(tenant))
        .bind(Uuid::from(principal))
        .bind(permissions)
        .execute(&mut *tx)
        .await
        .map_err(context_storage::Error::from)?;
    tx.commit().await.map_err(context_storage::Error::from)?;
    Ok(Bootstrap {
        tenant,
        workspace,
        collection,
        principal,
    })
}
