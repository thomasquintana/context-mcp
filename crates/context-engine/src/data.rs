//! Operator-owned processing profiles and managed data-plane contracts.

use context_core::content::SourceLocation;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// A retained embedding space; model/tool projection belongs in its role graphs.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Space {
    /// Typesense collection dedicated to this model/space revision.
    pub collection: String,
    /// Required finite vector dimension.
    pub dimensions: usize,
    /// Document-text embedding role.
    pub document_role: String,
    /// Query embedding role in the same space.
    pub query_role: String,
}

/// One operator-selected ingestion profile, frozen in upload/job admission records.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Supported declared MIME types; actual extraction must confirm compatibility.
    pub media_types: BTreeSet<String>,
    /// Extraction role returns a tagged JSON document with segment manifest.
    pub extract_role: String,
    /// Optional operator-declared opaque cleanup-handle checkpoint in that role.
    pub cleanup_node: Option<String>,
    /// Optional cleanup role used after interrupted extraction, with the saved handle.
    pub cleanup_role: Option<String>,
    /// Description/OCR role for image segments, when the profile needs it.
    pub describe_role: Option<String>,
    /// Transcription role for audio segments, when the profile needs it.
    pub transcribe_role: Option<String>,
    /// Optional provider-artifact read role for non-local output references.
    pub artifact_role: Option<String>,
    /// Summary/entity enrichment role.
    pub enrich_role: String,
    /// Selected compatible embedding space.
    pub space: String,
    /// Maximum UTF-8 bytes in one situated evidence chunk.
    pub chunk_bytes: usize,
}

/// Trusted data-plane deployment settings; credential values are environment-only.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// S3-compatible endpoint.
    pub s3_endpoint: String,
    /// Operator-provisioned bucket.
    pub s3_bucket: String,
    /// Signing region.
    pub s3_region: String,
    /// Access-key environment reference.
    pub s3_access_key_env: String,
    /// Secret-key environment reference.
    pub s3_secret_key_env: String,
    /// Explicit local HTTP opt-in.
    pub s3_allow_http: bool,
    /// Typesense endpoint.
    pub typesense_endpoint: String,
    /// Typesense administrator-key environment reference.
    pub typesense_key_env: String,
    /// Retained compatible vector spaces.
    pub spaces: BTreeMap<String, Space>,
    /// Default query space.
    pub default_space: String,
    /// Operator processing profiles.
    pub profiles: BTreeMap<String, Profile>,
    /// Default upload profile.
    pub default_profile: String,
    /// Application-owned staging directory, relative to the config directory.
    pub staging_directory: PathBuf,
    /// Maximum explicit upload bytes.
    pub max_upload_bytes: u64,
    /// Signed upload lifetime; pending purge waits out outstanding write capabilities.
    pub upload_lifetime_seconds: u64,
    /// Maximum unfinished jobs, including pending cleanup (1..=4096).
    pub max_pending_jobs: i64,
    /// In-process processing workers.
    pub workers: usize,
    /// Maximum extracted manifest segments.
    pub max_segments: usize,
}

/// Frozen profile and role revisions; contains no endpoint credentials.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pipeline {
    /// Immutable processing profile chosen at admission.
    pub profile: Profile,
    /// Compatible space configuration chosen at admission.
    pub space: Space,
    /// Exact graph fingerprints for every allowed stage role.
    pub fingerprints: BTreeMap<String, String>,
}

/// Explicit domain document encoding, independent of any provider response schema.
#[derive(Deserialize)]
#[serde(
    tag = "encoding",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Document {
    /// A native JSON object supplied by an operator graph.
    Json(serde_json::Value),
    /// Structured JSON text, e.g. a generated JSON document or native-tool stdout.
    JsonText(String),
}

impl Document {
    /// Decode a bounded structured domain document after HTLK accepts the role result.
    ///
    /// # Errors
    /// Rejects malformed JSON or content that violates the requested application type.
    pub fn decode<T: serde::de::DeserializeOwned>(self) -> Result<T, serde_json::Error> {
        match self {
            Self::Json(value) => serde_json::from_value(value),
            Self::JsonText(text) => serde_json::from_str(&text),
        }
    }
}

/// Native/model extraction manifest; file references are private job basenames only.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extraction {
    /// Source type confirmed by the extractor.
    pub media_type: String,
    /// Bounded ordered evidence segments.
    pub segments: Vec<Segment>,
    /// Optional opaque continuation; each page is a separate idempotent stage.
    pub next_cursor: Option<serde_json::Value>,
    /// Bounded source metadata such as duration, dimensions or page count.
    pub metadata: Option<serde_json::Value>,
}

/// Media segment class, not a provider tool selection.
#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    /// Literal extracted text, inline or in a managed text file.
    Text,
    /// Model-readable image bytes in a managed file.
    Image,
    /// Model-readable audio bytes in a managed file.
    Audio,
}

/// One extraction segment bound to its original source location.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    /// Segment class.
    pub kind: SegmentKind,
    /// Literal text when present.
    pub text: Option<String>,
    /// Private output basename when present.
    pub file: Option<String>,
    /// Bounded inline bytes when the provider does not share local staging.
    pub data_base64: Option<String>,
    /// Opaque provider reference read only through its configured artifact role.
    pub reference: Option<serde_json::Value>,
    /// File digest, preventing mutable staging from changing a saved manifest.
    pub sha256: Option<String>,
    /// Model-readable file media type when relevant.
    pub media_type: Option<String>,
    /// Exact original-source position.
    pub location: SourceLocation,
}

/// Summary/entity enrichment document. Generated origin is assigned by the application.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Enrichment {
    /// Generated bounded source summary.
    pub summary: String,
    /// Explicit bounded entity names/types; relationships are separate application records.
    pub entities: Vec<Entity>,
    /// Generated relationships between declared entity names.
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

/// A generated entity observation retaining source lineage.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    /// Human-readable entity name.
    pub name: String,
    /// Domain/entity type.
    pub entity_type: String,
}

/// Source-grounded generated relationship between entity mentions.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Relationship {
    /// Source entity name in the enrichment document.
    pub from: String,
    /// Target entity name in the enrichment document.
    pub to: String,
    /// Relationship kind.
    pub relationship: String,
}
