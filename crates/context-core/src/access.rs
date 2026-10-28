//! Explicit resource grants, inherited permissions, and tenant-safe authorization.

use crate::ids::{AssetId, CollectionId, CredentialId, PrincipalId, TenantId, WorkspaceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A resource identity, resolved independently of caller-supplied tenant claims.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Resource {
    /// A tenant root.
    Tenant(TenantId),
    /// A workspace/project.
    Workspace(WorkspaceId),
    /// A collection.
    Collection(CollectionId),
    /// A logical asset.
    Asset(AssetId),
}

/// An application capability; credentials must independently permit this scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Read/search source evidence and content.
    Read,
    /// Ingest new assets or revisions.
    Ingest,
    /// Write semantic memories.
    WriteMemory,
    /// Invoke model-backed synthesis.
    Synthesize,
    /// Grant or revoke resource access.
    Share,
    /// Permanently purge authorized managed content.
    Purge,
}

/// A resolved grant recipient. Membership is read from authoritative storage.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Recipient {
    /// A specific human or service account.
    Principal(PrincipalId),
    /// Members of a tenant, including an explicitly shared recipient tenant.
    Tenant(TenantId),
    /// Members of a workspace, including an explicitly shared recipient workspace.
    Workspace(WorkspaceId),
}

/// An explicit allow grant on one resource. Absence of a grant denies access.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// Recipient of the grant.
    pub recipient: Recipient,
    /// Explicit capabilities granted.
    pub permissions: BTreeSet<Permission>,
}

/// One authoritative node of a leaf-to-root permission chain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccessNode {
    /// Resource whose ACL this node contains.
    pub resource: Resource,
    /// Owning tenant, which sharing never changes.
    pub owner: TenantId,
    /// Whether grants from the next ancestor may also authorize this resource.
    pub inherit: bool,
    /// Explicit grants attached to this resource.
    pub grants: Vec<Grant>,
}

/// Host-authenticated identity and independently verified service-token scopes.
#[derive(Clone, Debug)]
pub struct AccessContext {
    /// Authenticated principal, never taken from an operation's ordinary arguments.
    pub principal: PrincipalId,
    /// Authoritatively resolved tenant memberships.
    pub tenants: BTreeSet<TenantId>,
    /// Authoritatively resolved workspace memberships.
    pub workspaces: BTreeSet<WorkspaceId>,
    /// Effective credential capabilities, even when a resource ACL is broader.
    pub scopes: BTreeSet<Permission>,
}

/// Host-authenticated session facts; never construct from ordinary tool arguments.
///
/// There is intentionally no Deserialize implementation. Token validation
/// establishes these facts; PostgreSQL rechecks service credential state and
/// resolves memberships/grants in the protected operation's transaction.
#[derive(Clone, Debug)]
pub struct Authentication {
    /// Authenticated human or service-account identity.
    pub principal: PrincipalId,
    /// Upper bound on effective API capabilities.
    pub scopes: BTreeSet<Permission>,
    /// Service credential whose live database state must also permit the operation.
    pub credential: Option<CredentialId>,
}

impl AccessContext {
    /// Authorize a validated leaf-to-root chain owned by one tenant.
    ///
    /// The host must resolve resource hierarchy from storage. This additionally
    /// rejects mismatched targets, mixed-owner chains and repeated/cyclic nodes.
    /// Tenant membership alone never grants access to an asset.
    pub fn permits(&self, target: Resource, permission: Permission, chain: &[AccessNode]) -> bool {
        let Some(first) = chain.first() else {
            return false;
        };
        if first.resource != target
            || !self.scopes.contains(&permission)
            || chain.iter().any(|node| node.owner != first.owner)
            || chain
                .iter()
                .enumerate()
                .any(|(index, node)| chain[..index].iter().any(|n| n.resource == node.resource))
        {
            return false;
        }
        for node in chain {
            if node.grants.iter().any(|grant| {
                grant.permissions.contains(&permission)
                    && match grant.recipient {
                        Recipient::Principal(id) => self.principal == id,
                        Recipient::Tenant(id) => self.tenants.contains(&id),
                        Recipient::Workspace(id) => self.workspaces.contains(&id),
                    }
            }) {
                return true;
            }
            if !node.inherit {
                break;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inheritance_cross_tenant_grants_and_credential_scopes_are_independent() {
        let owner = TenantId::new();
        let guest = TenantId::new();
        let asset = Resource::Asset(AssetId::new());
        let mut context = AccessContext {
            principal: PrincipalId::new(),
            tenants: BTreeSet::from([guest]),
            workspaces: BTreeSet::new(),
            scopes: BTreeSet::from([Permission::Read]),
        };
        let mut chain = vec![
            AccessNode {
                resource: asset,
                owner,
                inherit: true,
                grants: vec![],
            },
            AccessNode {
                resource: Resource::Collection(CollectionId::new()),
                owner,
                inherit: true,
                grants: vec![Grant {
                    recipient: Recipient::Tenant(guest),
                    permissions: BTreeSet::from([Permission::Read, Permission::Purge]),
                }],
            },
        ];
        assert!(context.permits(asset, Permission::Read, &chain));
        assert!(!context.permits(asset, Permission::Purge, &chain));
        chain[0].inherit = false;
        assert!(!context.permits(asset, Permission::Read, &chain));
        chain[0].inherit = true;
        chain[1].owner = guest;
        assert!(!context.permits(asset, Permission::Read, &chain));
        chain[1].owner = owner;
        context.tenants = BTreeSet::from([owner]);
        assert!(!context.permits(asset, Permission::Read, &chain));
    }

    #[test]
    fn wrong_target_and_cycles_fail_closed() {
        let asset = Resource::Asset(AssetId::new());
        let context = AccessContext {
            principal: PrincipalId::new(),
            tenants: BTreeSet::new(),
            workspaces: BTreeSet::new(),
            scopes: BTreeSet::from([Permission::Read]),
        };
        let node = AccessNode {
            resource: asset,
            owner: TenantId::new(),
            inherit: true,
            grants: vec![Grant {
                recipient: Recipient::Principal(context.principal),
                permissions: BTreeSet::from([Permission::Read]),
            }],
        };
        assert!(!context.permits(
            Resource::Asset(AssetId::new()),
            Permission::Read,
            std::slice::from_ref(&node)
        ));
        assert!(!context.permits(asset, Permission::Read, &[node.clone(), node]));
    }
}
