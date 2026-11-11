//! Explicit owner-managed inherited ACL replacement and live cross-tenant grants.
use crate::{Application, Error, Result};
use context_core::access::{Authentication, Grant, Permission, Resource};
use context_storage::postgres::{existing_receipt, receipt, resource_id};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Replace one owned resource ACL under an expected version and stable intent key.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetAccess {
    /// Existing target; IDs never manufacture ownership.
    pub resource: Resource,
    /// Optimistic concurrency version from resource metadata.
    pub expected_acl_version: i64,
    /// Whether grants may continue from ancestors.
    pub inherit: bool,
    /// Complete explicit grants; empty revokes all explicit grants.
    pub grants: Vec<Grant>,
    /// Stable principal/operation request key.
    pub idempotency_key: String,
}
impl Application {
    /// Replace an ACL without changing source ownership or copying shared content.
    ///
    /// # Errors
    /// Requires current Share permission and owner-tenant membership. Rejects stale
    /// versions, invalid recipients, excessive grants and conflicting keys.
    pub async fn set_access(&self, actor: &Authentication, args: SetAccess) -> Result<Value> {
        if args.expected_acl_version < 1
            || args.grants.len() > 128
            || args.grants.iter().any(|g| g.permissions.is_empty())
        {
            return Err(Error::Invalid);
        }
        let mut view = self
            .database
            .authorized_write(actor, args.resource, Permission::Share)
            .await?;
        let owner: Uuid = sqlx::query_scalar("SELECT owner FROM context.resources WHERE id=$1")
            .bind(resource_id(args.resource))
            .fetch_one(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        if !view.context().tenants.contains(&owner.into()) {
            return Err(context_storage::Error::Unavailable.into());
        }
        let hash: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&(
                args.resource,
                args.expected_acl_version,
                args.inherit,
                &args.grants,
            ))
            .map_err(|_| Error::Invalid)?,
        )
        .into();
        if let Some(value) = existing_receipt(
            view.transaction(),
            actor.principal,
            "set_access",
            &args.idempotency_key,
            &hash,
        )
        .await?
        {
            view.commit().await?;
            return Ok(value);
        }
        let version:Option<i64>=sqlx::query_scalar("UPDATE context.resources SET acl_version=acl_version+1,inherit=$3 WHERE id=$1 AND acl_version=$2 RETURNING acl_version")
            .bind(resource_id(args.resource)).bind(args.expected_acl_version).bind(args.inherit).fetch_optional(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        let version = version.ok_or(context_storage::Error::Conflict)?;
        sqlx::query("DELETE FROM context.grants WHERE resource=$1")
            .bind(resource_id(args.resource))
            .execute(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        for grant in &args.grants {
            let target = serde_json::to_value(grant.recipient).map_err(|_| Error::Invalid)?;
            let kind = target["kind"].as_str().ok_or(Error::Invalid)?;
            let id = Uuid::parse_str(target["id"].as_str().ok_or(Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
            let exists: bool = if kind == "principal" {
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.principals WHERE id=$1)")
                    .bind(id)
                    .fetch_one(&mut **view.transaction())
                    .await
                    .map_err(context_storage::Error::from)?
            } else {
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.resources WHERE id=$1 AND kind=$2 AND NOT purging)").bind(id).bind(kind).fetch_one(&mut **view.transaction()).await.map_err(context_storage::Error::from)?
            };
            if !exists {
                return Err(Error::Invalid);
            }
            let permissions = grant
                .permissions
                .iter()
                .map(|p| {
                    serde_json::to_value(p)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string()
                })
                .collect::<Vec<_>>();
            sqlx::query("INSERT INTO context.grants VALUES($1,$2,$3,$4)")
                .bind(resource_id(args.resource))
                .bind(kind)
                .bind(id)
                .bind(permissions)
                .execute(&mut **view.transaction())
                .await
                .map_err(context_storage::Error::from)?;
        }
        let value = json!({"resource":args.resource,"acl_version":version,"inherit":args.inherit});
        receipt(
            view.transaction(),
            actor.principal,
            "set_access",
            &args.idempotency_key,
            &hash,
            &value,
        )
        .await?;
        view.commit().await?;
        Ok(value)
    }
}
