//! PostgreSQL authority with explicit transactions and permission-chain hydration.

use crate::{Error, Result};
use context_core::{
    access::{AccessContext, AccessNode, Grant, Permission, Recipient, Resource},
    ids::{AssetId, PrincipalId, RevisionId, TenantId, WorkspaceId},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgPoolOptions};
use std::{collections::BTreeSet, time::Duration};
use uuid::Uuid;

/// Authoritative application store; cloning shares a bounded connection pool.
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Connect and apply checked embedded migrations before admitting requests.
    ///
    /// # Errors
    /// Returns redacted connection or schema migration failures.
    pub async fn connect(url: &str, connections: u32) -> Result<Self> {
        if connections == 0 {
            return Err(Error::Invalid);
        }
        let pool = PgPoolOptions::new()
            .max_connections(connections)
            .acquire_timeout(Duration::from_secs(10))
            .after_connect(|connection, _| {
                Box::pin(async move {
                    // Keep SQLx's unqualified migration ledger in a stable schema even
                    // when the login role has the same name as our application schema.
                    sqlx::query("SET search_path=public")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(url)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Access the trusted pool for application-specific transactions.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Begin a transaction. The application owns atomic publication and admission.
    ///
    /// # Errors
    /// Returns a database failure.
    pub async fn transaction(&self) -> Result<Transaction<'_, Postgres>> {
        Ok(self.pool.begin().await?)
    }

    /// Resolve a stable issuer/subject identity without trusting caller-selected IDs.
    ///
    /// # Errors
    /// Rejects empty identities and unknown principal classes.
    pub async fn identity(&self, issuer: &str, subject: &str, kind: &str) -> Result<PrincipalId> {
        if issuer.is_empty() || subject.is_empty() || !matches!(kind, "human" | "service") {
            return Err(Error::Invalid);
        }
        let row = sqlx::query("INSERT INTO context.principals(id,issuer,subject,kind) VALUES($1,$2,$3,$4) ON CONFLICT(issuer,subject) DO UPDATE SET subject=EXCLUDED.subject RETURNING id,kind")
            .bind(Uuid::now_v7()).bind(issuer).bind(subject).bind(kind).fetch_one(&self.pool).await?;
        if row.try_get::<String, _>("kind")? != kind {
            return Err(Error::Conflict);
        }
        Ok(row.try_get::<Uuid, _>("id")?.into())
    }

    /// Hydrate current memberships and independently bounded credential scopes.
    ///
    /// # Errors
    /// Returns corrupt or unavailable identity/membership data.
    pub async fn access_context(
        &self,
        principal: PrincipalId,
        scopes: BTreeSet<Permission>,
    ) -> Result<AccessContext> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let context = context_in(&mut tx, principal, scopes).await?;
        tx.commit().await?;
        Ok(context)
    }

    /// Resolve actual ancestors and grants in a repeatable-read snapshot.
    ///
    /// # Errors
    /// Rejects missing/purging ancestors, malformed ancestry and stored grants.
    pub async fn access_chain(&self, resource: Resource) -> Result<Vec<AccessNode>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let chain = chain_in(&mut tx, resource).await?;
        tx.commit().await?;
        Ok(chain)
    }

    /// Close connections after request/worker shutdown.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// Typed resource to database identity conversion.
pub fn resource_id(resource: Resource) -> Uuid {
    match resource {
        Resource::Tenant(id) => id.into(),
        Resource::Workspace(id) => id.into(),
        Resource::Collection(id) => id.into(),
        Resource::Asset(id) => id.into(),
    }
}

fn decode_resource(kind: &str, id: Uuid) -> Result<Resource> {
    match kind {
        "tenant" => Ok(Resource::Tenant(id.into())),
        "workspace" => Ok(Resource::Workspace(id.into())),
        "collection" => Ok(Resource::Collection(id.into())),
        "asset" => Ok(Resource::Asset(id.into())),
        _ => Err(Error::Invalid),
    }
}

pub(crate) async fn context_in(
    tx: &mut Transaction<'_, Postgres>,
    principal: PrincipalId,
    scopes: BTreeSet<Permission>,
) -> Result<AccessContext> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.principals WHERE id=$1)")
            .bind(Uuid::from(principal))
            .fetch_one(&mut **tx)
            .await?;
    if !exists {
        return Err(Error::Unavailable);
    }
    let rows = sqlx::query("SELECT r.id,r.kind FROM context.memberships m JOIN context.resources r ON r.id=m.resource WHERE m.principal=$1 AND NOT r.purging")
        .bind(Uuid::from(principal)).fetch_all(&mut **tx).await?;
    let mut context = AccessContext {
        principal,
        scopes,
        tenants: BTreeSet::new(),
        workspaces: BTreeSet::new(),
    };
    for row in rows {
        let id: Uuid = row.try_get("id")?;
        match row.try_get::<&str, _>("kind")? {
            "tenant" => {
                context.tenants.insert(id.into());
            }
            "workspace" => {
                context.workspaces.insert(id.into());
            }
            _ => return Err(Error::Invalid),
        }
    }
    Ok(context)
}

pub(crate) async fn chain_in(
    tx: &mut Transaction<'_, Postgres>,
    resource: Resource,
) -> Result<Vec<AccessNode>> {
    let rows = sqlx::query("WITH RECURSIVE chain AS (SELECT *,0 AS depth FROM context.resources WHERE id=$1 UNION ALL SELECT r.*,c.depth+1 FROM context.resources r JOIN chain c ON r.id=c.parent WHERE c.depth<4) SELECT * FROM chain ORDER BY depth")
        .bind(resource_id(resource)).fetch_all(&mut **tx).await?;
    if rows.is_empty() || rows.iter().any(|row| row.get::<bool, _>("purging")) {
        return Err(Error::Unavailable);
    }
    let mut chain = Vec::with_capacity(rows.len());
    for row in rows {
        let id: Uuid = row.try_get("id")?;
        let grants = sqlx::query(
            "SELECT recipient_kind,recipient,permissions FROM context.grants WHERE resource=$1",
        )
        .bind(id)
        .fetch_all(&mut **tx)
        .await?;
        let grants = grants
            .into_iter()
            .map(|row| {
                let recipient: Uuid = row.try_get("recipient")?;
                Ok(Grant {
                    recipient: match row.try_get::<&str, _>("recipient_kind")? {
                        "principal" => Recipient::Principal(recipient.into()),
                        "tenant" => Recipient::Tenant(recipient.into()),
                        "workspace" => Recipient::Workspace(recipient.into()),
                        _ => return Err(Error::Invalid),
                    },
                    permissions: row
                        .try_get::<Vec<String>, _>("permissions")?
                        .into_iter()
                        .map(|name| {
                            serde_json::from_value(Value::String(name)).map_err(Error::from)
                        })
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?;
        chain.push(AccessNode {
            resource: decode_resource(row.try_get("kind")?, id)?,
            owner: row.try_get::<Uuid, _>("owner")?.into(),
            inherit: row.try_get("inherit")?,
            grants,
        });
    }
    if chain.first().map(|n| n.resource) != Some(resource) {
        return Err(Error::Unavailable);
    }
    if !matches!(chain.last().map(|n| n.resource), Some(Resource::Tenant(_))) {
        return Err(Error::Invalid);
    }
    Ok(chain)
}

/// Insert an immutable hierarchy node using an already-authorized transaction.
///
/// # Errors
/// Database constraints reject invalid ancestry, owner changes, and missing parents.
pub async fn insert_resource(
    tx: &mut Transaction<'_, Postgres>,
    resource: Resource,
    owner: TenantId,
    parent: Option<Resource>,
    inherit: bool,
) -> Result<()> {
    let kind = match resource {
        Resource::Tenant(_) => "tenant",
        Resource::Workspace(_) => "workspace",
        Resource::Collection(_) => "collection",
        Resource::Asset(_) => "asset",
    };
    sqlx::query(
        "INSERT INTO context.resources(id,kind,owner,parent,inherit) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(resource_id(resource))
    .bind(kind)
    .bind(Uuid::from(owner))
    .bind(parent.map(resource_id))
    .bind(inherit)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Atomically change a current pointer only for live, matching-generation work.
///
/// The caller must have completed all physical/index writes while retaining the
/// transaction's resource lock. Marks every staged body in this revision visible.
///
/// # Errors
/// Rejects purged/replaced assets, wrong-parent revisions, or stale generations.
pub async fn publish_revision(
    tx: &mut Transaction<'_, Postgres>,
    asset: AssetId,
    revision: RevisionId,
    generation: i64,
) -> Result<()> {
    let changed = sqlx::query("UPDATE context.assets a SET current_revision=$2 WHERE a.id=$1 AND a.generation=$3 AND NOT EXISTS(WITH RECURSIVE ancestors AS (SELECT id,parent,purging FROM context.resources WHERE id=a.id UNION ALL SELECT r.id,r.parent,r.purging FROM context.resources r JOIN ancestors p ON r.id=p.parent) SELECT 1 FROM ancestors WHERE purging) AND EXISTS(SELECT 1 FROM context.revisions v JOIN context.content c ON c.id=v.content WHERE v.id=$2 AND v.asset=a.id AND NOT c.purging)")
        .bind(Uuid::from(asset)).bind(Uuid::from(revision)).bind(generation).execute(&mut **tx).await?;
    if changed.rows_affected() != 1 {
        return Err(Error::Conflict);
    }
    sqlx::query("UPDATE context.content SET published=true WHERE revision=$1 AND NOT purging")
        .bind(Uuid::from(revision))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Claim a pending job with row locking for bounded workers within one application.
///
/// # Errors
/// Returns a database failure. Callers persist stage/run identities before effects.
pub async fn claim_job(tx: &mut Transaction<'_, Postgres>) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar("UPDATE context.jobs SET state='running',updated_at=now() WHERE id=(SELECT id FROM context.jobs WHERE state='accepted' ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING id")
        .fetch_optional(&mut **tx).await?)
}

/// Persist/replay an idempotent operation receipt inside the caller's transaction.
///
/// # Errors
/// Reusing a key with a different request digest is a conflict, never a new effect.
pub async fn receipt(
    tx: &mut Transaction<'_, Postgres>,
    principal: PrincipalId,
    operation: &str,
    key: &str,
    digest: &[u8; 32],
    value: &Value,
) -> Result<Value> {
    if key.is_empty() || key.len() > 256 {
        return Err(Error::Invalid);
    }
    let row = sqlx::query("INSERT INTO context.receipts(principal,operation,idempotency_key,request_digest,receipt) VALUES($1,$2,$3,$4,$5) ON CONFLICT(principal,operation,idempotency_key) DO UPDATE SET idempotency_key=EXCLUDED.idempotency_key RETURNING request_digest,receipt")
        .bind(Uuid::from(principal)).bind(operation).bind(receipt_key(key)).bind(digest.as_slice()).bind(value)
        .fetch_one(&mut **tx).await?;
    if row.try_get::<Vec<u8>, _>("request_digest")? != digest {
        return Err(Error::Conflict);
    }
    Ok(row.try_get("receipt")?)
}

/// Lock one idempotency key and check its receipt before performing any effects.
///
/// Keep this transaction open through authoritative mutation and receipt creation.
/// PostgreSQL's transaction-scoped advisory lock also serializes concurrent first
/// admissions; hash collisions merely serialize unrelated requests.
///
/// # Errors
/// Rejects invalid keys and reuse with a different request digest.
pub async fn existing_receipt(
    tx: &mut Transaction<'_, Postgres>,
    principal: PrincipalId,
    operation: &str,
    key: &str,
    digest: &[u8; 32],
) -> Result<Option<Value>> {
    if key.is_empty() || key.len() > 256 || operation.is_empty() || operation.len() > 128 {
        return Err(Error::Invalid);
    }
    let key = receipt_key(key);
    let lock_key = serde_json::to_string(&(principal, operation, &key))?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut **tx)
        .await?;
    let row = sqlx::query("SELECT request_digest,receipt FROM context.receipts WHERE principal=$1 AND operation=$2 AND idempotency_key=$3")
        .bind(Uuid::from(principal)).bind(operation).bind(key).fetch_optional(&mut **tx).await?;
    match row {
        None => Ok(None),
        Some(row) if row.try_get::<Vec<u8>, _>("request_digest")? == digest => {
            Ok(Some(row.try_get("receipt")?))
        }
        Some(_) => Err(Error::Conflict),
    }
}

/// Opaque lookup handle for a caller-chosen idempotency key; never retain raw key text.
pub fn receipt_key(key: &str) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Look up the owning project and tenant for an actual collection hierarchy.
///
/// # Errors
/// Rejects missing, purging or malformed containers.
pub async fn collection_scope(
    db: &Database,
    collection: Resource,
) -> Result<(WorkspaceId, TenantId)> {
    let chain = db.access_chain(collection).await?;
    let workspace = chain
        .iter()
        .find_map(|node| match node.resource {
            Resource::Workspace(id) => Some(id),
            _ => None,
        })
        .ok_or(Error::Invalid)?;
    Ok((workspace, chain[0].owner))
}
