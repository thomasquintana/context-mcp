//! Shared authorization/data snapshots, preventing mixed-time membership/grant checks.

use crate::{
    Error, Result,
    postgres::{Database, chain_in, context_in, resource_id},
};
use context_core::access::{AccessContext, Authentication, Permission, Resource};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use std::collections::BTreeSet;
use uuid::Uuid;

/// A protected operation's shared authorization/data snapshot.
pub struct AuthorizedTransaction<'a> {
    transaction: Transaction<'a, Postgres>,
    context: AccessContext,
}

impl Database {
    /// Authorize an internal cache/job mutation using its public operation permission.
    ///
    /// Unlike domain writes, protected read materialization may create derived cache
    /// records. Callers must not use this to grant arbitrary user knowledge writes.
    /// # Errors
    /// Denies missing/currently revoked authority and transaction conflicts.
    pub async fn authorized_materialize(
        &self,
        actor: &Authentication,
        target: Resource,
        permission: Permission,
    ) -> Result<AuthorizedTransaction<'_>> {
        self.authorized(actor, target, permission, true).await
    }
    /// Authorize a read-only control view using its action-specific capability.
    ///
    /// Application services use this for upload/job metadata, never source bodies
    /// unless the selected permission is Read.
    ///
    /// # Errors
    /// Denies missing, revoked, expired or forbidden authority.
    pub async fn authorized_view(
        &self,
        actor: &Authentication,
        target: Resource,
        permission: Permission,
    ) -> Result<AuthorizedTransaction<'_>> {
        self.authorized(actor, target, permission, false).await
    }
    /// Authorize a protected read with credential state, memberships and data in one snapshot.
    ///
    /// # Errors
    /// Missing, revoked, expired and forbidden identities/resources remain unavailable.
    pub async fn authorized_read(
        &self,
        actor: &Authentication,
        target: Resource,
    ) -> Result<AuthorizedTransaction<'_>> {
        self.authorized(actor, target, Permission::Read, false)
            .await
    }

    /// Authorize a mutation under serializable isolation and a target resource lock.
    ///
    /// # Errors
    /// Rejects read-only permission, unavailable authority and transaction conflicts.
    pub async fn authorized_write(
        &self,
        actor: &Authentication,
        target: Resource,
        permission: Permission,
    ) -> Result<AuthorizedTransaction<'_>> {
        if permission == Permission::Read {
            return Err(Error::Invalid);
        }
        self.authorized(actor, target, permission, true).await
    }

    async fn authorized(
        &self,
        actor: &Authentication,
        target: Resource,
        permission: Permission,
        write: bool,
    ) -> Result<AuthorizedTransaction<'_>> {
        let mut tx = self.transaction().await?;
        let isolation = if write {
            "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE"
        } else {
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY"
        };
        sqlx::query(isolation).execute(&mut *tx).await?;
        let mut scopes = actor.scopes.clone();
        if let Some(credential) = actor.credential {
            let stored: Option<Vec<String>> = sqlx::query_scalar("SELECT scopes FROM context.credentials WHERE id=$1 AND principal=$2 AND NOT revoked AND expires_at>now()")
                .bind(Uuid::from(credential)).bind(Uuid::from(actor.principal)).fetch_optional(&mut *tx).await?;
            let stored = stored
                .ok_or(Error::Unavailable)?
                .into_iter()
                .map(|name| serde_json::from_value(Value::String(name)).map_err(Error::from))
                .collect::<Result<BTreeSet<Permission>>>()?;
            scopes = scopes.intersection(&stored).copied().collect();
        }
        let context = context_in(&mut tx, actor.principal, scopes).await?;
        let mut authorized = AuthorizedTransaction {
            transaction: tx,
            context,
        };
        authorized.authorize(target, permission).await?;
        if write {
            sqlx::query("SELECT id FROM context.resources WHERE id=$1 FOR UPDATE")
                .bind(resource_id(target))
                .fetch_one(&mut *authorized.transaction)
                .await?;
        }
        Ok(authorized)
    }
}

impl<'a> AuthorizedTransaction<'a> {
    /// Trusted principal and memberships resolved in this exact transaction.
    pub fn context(&self) -> &AccessContext {
        &self.context
    }

    /// Use this transaction for authorized protected data reads or mutations.
    pub fn transaction(&mut self) -> &mut Transaction<'a, Postgres> {
        &mut self.transaction
    }

    /// Authorize another source using the same snapshot, e.g. synthesis dependencies.
    ///
    /// # Errors
    /// Denies unavailable resources and permissions without exposing their contents.
    pub async fn authorize(&mut self, target: Resource, permission: Permission) -> Result<()> {
        let chain = chain_in(&mut self.transaction, target).await?;
        if !self.context.permits(target, permission, &chain) {
            return Err(Error::Unavailable);
        }
        Ok(())
    }

    /// Commit the protected operation. Dropped operations roll back automatically.
    ///
    /// # Errors
    /// Returns database conflicts; callers must not blindly replay physical effects.
    pub async fn commit(self) -> Result<()> {
        self.transaction.commit().await?;
        Ok(())
    }
}
