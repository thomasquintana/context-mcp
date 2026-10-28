//! Typed opaque identifiers. IDs identify records; they do not confer authority.

use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

macro_rules! identifier {
    ($name:ident, $documentation:literal) => {
        #[doc = $documentation]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Allocate a time-ordered opaque identifier.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

identifier!(TenantId, "An owning tenant.");
identifier!(WorkspaceId, "A workspace/project within a tenant.");
identifier!(CollectionId, "A collection within a workspace.");
identifier!(
    AssetId,
    "A stable logical asset, independent of its revisions."
);
identifier!(RevisionId, "An immutable source revision.");
identifier!(ContentId, "A purge-addressable managed content record.");
identifier!(
    PrincipalId,
    "An authenticated human or service-account identity."
);
identifier!(MemoryId, "A stable logical semantic memory.");
identifier!(MemoryRevisionId, "An immutable semantic-memory revision.");
identifier!(
    JobId,
    "A durable ingestion, synthesis, reprocessing, or purge job."
);
