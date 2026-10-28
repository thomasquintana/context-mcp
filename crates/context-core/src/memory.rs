//! Revisioned semantic memories and explicit reconciliation outcomes.

use crate::{
    content::{Origin, SourceReference},
    ids::{ContentId, MemoryId, MemoryRevisionId, PrincipalId, WorkspaceId},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::num::NonZeroU64;

/// A semantic statement or a typed value; no machine-style key is required.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryValue {
    /// A natural-language statement.
    Text {
        /// Statement content.
        statement: String,
    },
    /// A value with an operator/application-defined semantic type.
    Typed {
        /// Declared type, validated by the configured memory contract.
        value_type: String,
        /// Value matching that type.
        value: Value,
    },
}

/// One immutable revision of a project-scoped memory.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRevision {
    /// Immutable revision identity.
    pub id: MemoryRevisionId,
    /// Logical memory identity.
    pub memory: MemoryId,
    /// Workspace/project scope.
    pub workspace: WorkspaceId,
    /// Purge-addressable body record.
    pub content: ContentId,
    /// Monotonic ordinal, advanced transactionally.
    pub version: NonZeroU64,
    /// Human-readable subject.
    pub subject: String,
    /// Natural-language description/context.
    pub description: String,
    /// Structured value or text statement.
    pub value: MemoryValue,
    /// Authenticated human/service-account author.
    pub author: PrincipalId,
    /// Original assertion versus generated/reconciled content.
    pub origin: Origin,
    /// Content-dependent sources: purge propagates through these dependencies.
    pub derived_from: Vec<SourceReference>,
    /// Citation-only evidence: removal invalidates support, not independent content.
    pub citations: Vec<SourceReference>,
    /// Prior revisions replaced by this explicit reconciliation.
    pub supersedes: Vec<MemoryRevisionId>,
}

/// A current view can explicitly preserve unresolved competing assertions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MemoryView {
    /// All current contributions were permanently removed; retained history is explicit.
    Unavailable,
    /// One accepted current revision.
    Current {
        /// Current revision.
        revision: MemoryRevisionId,
    },
    /// Contradiction unresolved by explicit supersession or authority rules.
    Conflicted {
        /// Competing immutable revisions; application requires at least two.
        revisions: Vec<MemoryRevisionId>,
    },
}

/// Basis permitting automatic publication of a reconciled current revision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationBasis {
    /// Equivalent statements whose combined provenance is retained.
    Equivalent,
    /// Complementary information that does not contradict existing assertions.
    Complementary,
    /// An authorized write explicitly supersedes a known revision.
    ExplicitSupersession,
    /// An operator-configured authority rule establishes precedence.
    AuthorityRule,
    /// A contradiction with no authorized basis for selecting a winner.
    UnresolvedConflict,
}

impl ReconciliationBasis {
    /// Whether this basis allows a single current revision rather than a conflict view.
    pub fn permits_current(self) -> bool {
        self != Self::UnresolvedConflict
    }
}
