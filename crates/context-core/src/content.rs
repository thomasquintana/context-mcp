//! Source locations, explicit lineage, and immutable asset revision records.

use crate::ids::{AssetId, CollectionId, ContentId, PrincipalId, RevisionId, TenantId};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// Semantic distinction preserved on every searchable or generated record.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Evidence extracted from an original asset.
    OriginalEvidence,
    /// An independently authored assertion by an authenticated agent.
    AgentAssertion,
    /// Model-generated or otherwise synthesized content.
    Generated,
}

/// Distinguishes informational dependence from a citation-only connection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relationship {
    /// Content contains or derives information from its source; purge cascades.
    Derivation,
    /// Independently authored content merely cites a source; purge invalidates evidence.
    Citation,
}

/// Location of evidence within an immutable source revision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceLocation {
    /// The whole asset.
    Asset,
    /// Exact logical member of an explicitly uploaded document/repository archive.
    File {
        /// Relative archive member path; never a host filesystem path.
        path: String,
    },
    /// Inclusive one-based document page range.
    Pages {
        /// First page.
        first: NonZeroU32,
        /// Last page; validate `last >= first` at ingress.
        last: NonZeroU32,
    },
    /// Half-open millisecond interval within an audio/video asset.
    Time {
        /// Inclusive start.
        start_ms: u64,
        /// Exclusive end; validate `end_ms > start_ms` at ingress.
        end_ms: u64,
    },
    /// Pixel-space image rectangle, anchored at the upper-left corner.
    Region {
        /// Left coordinate.
        x: u32,
        /// Top coordinate.
        y: u32,
        /// Positive width.
        width: NonZeroU32,
        /// Positive height.
        height: NonZeroU32,
    },
    /// Half-open byte range within a source text file.
    Bytes {
        /// Inclusive start.
        start: u64,
        /// Exclusive end; validate `end > start` at ingress.
        end: u64,
    },
}

impl SourceLocation {
    /// Check ordering and coordinate overflow before use with a media processor.
    pub fn is_valid(&self) -> bool {
        match self {
            Self::File { path } => {
                !path.is_empty()
                    && path.len() <= 1024
                    && !path.starts_with('/')
                    && !path.contains('\\')
                    && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
            }
            Self::Asset => true,
            Self::Pages { first, last } => first <= last,
            Self::Time { start_ms, end_ms } => start_ms < end_ms,
            Self::Bytes { start, end } => start < end,
            Self::Region {
                x,
                y,
                width,
                height,
            } => x.checked_add(width.get()).is_some() && y.checked_add(height.get()).is_some(),
        }
    }
}

/// Exact source identity. A citation never silently tracks the current revision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceReference {
    /// Stable logical source.
    pub asset: AssetId,
    /// Exact immutable revision used.
    pub revision: RevisionId,
    /// Location within that revision.
    pub location: SourceLocation,
    /// Whether the target still exists and supporting evidence remains available.
    pub available: bool,
}

/// Stable ownership and successful-current-revision pointer for an asset.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    /// Stable identity.
    pub id: AssetId,
    /// Owning tenant, unaffected by cross-tenant sharing.
    pub owner: TenantId,
    /// Owning collection.
    pub collection: CollectionId,
    /// Last fully published revision, absent until first success.
    pub current_revision: Option<RevisionId>,
    /// Monotonic publication generation, advanced when purging or replacing work.
    pub generation: u64,
}

/// Metadata for one immutable source revision; content lives in managed storage.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRevision {
    /// Immutable identity.
    pub id: RevisionId,
    /// Logical parent asset.
    pub asset: AssetId,
    /// Purge-addressable content record.
    pub content: ContentId,
    /// Host-authenticated submitting identity.
    pub author: PrincipalId,
    /// Verified media type, not an extraction strategy or provider name.
    pub media_type: String,
    /// Verified payload size in bytes.
    pub size_bytes: u64,
    /// SHA-256 digest encoded as lowercase hex; verify at storage ingress.
    pub sha256: String,
    /// Managed object key; never directly returned as an authorization capability.
    pub object_key: String,
    /// Creation time in Unix milliseconds.
    pub created_at_ms: u64,
}
