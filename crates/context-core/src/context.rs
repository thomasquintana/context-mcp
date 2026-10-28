//! Bounded evidence maps and typed, authorization-checked exploration pointers.

use crate::{
    content::{Origin, SourceLocation, SourceReference},
    ids::{AssetId, ContentId, RevisionId},
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// A typed exploration request. Resolving a pointer always rechecks authorization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Pointer {
    /// Expand exact managed evidence.
    Content {
        /// Managed content identity.
        content: ContentId,
    },
    /// Inspect source metadata.
    Asset {
        /// Logical asset.
        asset: AssetId,
        /// Exact revision.
        revision: RevisionId,
    },
    /// Retrieve a one-based document page.
    Page {
        /// Logical asset.
        asset: AssetId,
        /// Exact revision.
        revision: RevisionId,
        /// One-based page.
        page: NonZeroU32,
    },
    /// Retrieve an image region or audio/video interval from an exact revision.
    Media {
        /// Logical asset.
        asset: AssetId,
        /// Exact revision.
        revision: RevisionId,
        /// Validated region or time interval.
        location: SourceLocation,
    },
    /// Navigate an application-resolved entity and its authorized relationships.
    Entity {
        /// Purge-addressable entity record.
        entity: ContentId,
    },
}

/// One ranked evidence item; provider raw scores are normalized before assembly.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// Stable managed evidence identity.
    pub id: ContentId,
    /// Selected bounded text for reasoning.
    pub text: String,
    /// Origin class, preserved across generation and retrieval.
    pub origin: Origin,
    /// Exact, authorized supporting sources.
    pub sources: Vec<SourceReference>,
    /// Relative rank within this result; zero is best.
    pub rank: u32,
    /// Token count under the map's declared accounting scheme.
    pub tokens: u32,
    /// Paths for explicitly requesting additional context.
    pub pointers: Vec<Pointer>,
}

/// Token-accounting declaration; this is not an external model-spend guarantee.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TokenAccounting {
    /// Tokenizer/accounting scheme selected by the operator.
    pub scheme: String,
    /// Optional caller-provided evidence budget.
    pub limit: Option<NonZeroU32>,
    /// Total selected evidence tokens under that scheme.
    pub used: u32,
}

/// Evidence-first search response. Generated answers use a separate operation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMap {
    /// Explicit wire-contract version.
    pub version: String,
    /// Ranked, deduplicated, authorized evidence.
    pub evidence: Vec<Evidence>,
    /// Additional lightweight navigation routes.
    pub pointers: Vec<Pointer>,
    /// Selected evidence token accounting.
    pub tokens: TokenAccounting,
    /// Whether bounded selection omitted otherwise eligible evidence.
    pub truncated: bool,
}
