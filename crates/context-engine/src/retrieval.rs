//! Current-permission hybrid retrieval and exact-revision, dependency-protected navigation.

use crate::{Application, Error, Result, operations::Call};
use context_core::{
    access::{Authentication, Permission, Resource},
    content::{Origin, SourceReference},
    context::{ContextMap, Evidence, Pointer, TokenAccounting},
    ids::{AssetId, ContentId, RevisionId, WorkspaceId},
};
use context_storage::{authorization::AuthorizedTransaction, postgres::resource_id};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
    time::Duration,
};
use uuid::Uuid;

/// A bounded query over one explicitly authorized resource hierarchy.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// Tenant/workspace/collection/asset scope; never an authority claim.
    pub scope: Resource,
    /// Query text; embedding uses the configured compatible query role.
    pub query: String,
    /// Maximum evidence items (1..=100, default20).
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Optional model-token evidence budget (1..=65536).
    pub token_budget: Option<NonZeroU32>,
    /// Explicit opt-in to retained historical revisions.
    #[serde(default)]
    pub historical: bool,
    /// Optional retained embedding-space identity.
    pub space: Option<String>,
}
fn default_limit() -> usize {
    20
}

pub(crate) struct Protected {
    pub assets: BTreeSet<AssetId>,
    pub id: ContentId,
    pub workspace: WorkspaceId,
    pub asset: Option<AssetId>,
    pub revision: Option<RevisionId>,
    pub kind: String,
    pub origin: Origin,
    pub body: Value,
}

impl Application {
    pub(crate) async fn content_in(
        &self,
        view: &mut AuthorizedTransaction<'_>,
        id: ContentId,
    ) -> Result<Protected> {
        let row =
            sqlx::query("SELECT * FROM context.content WHERE id=$1 AND published AND NOT purging")
                .bind(Uuid::from(id))
                .fetch_optional(&mut **view.transaction())
                .await
                .map_err(context_storage::Error::from)?
                .ok_or(context_storage::Error::Unavailable)?;
        let dependencies=sqlx::query("WITH RECURSIVE upstream(id) AS (SELECT $1::uuid UNION SELECT d.source FROM context.dependencies d JOIN upstream u ON d.dependent=u.id WHERE d.relationship='derivation') SELECT c.id,c.asset,c.workspace,(c.published AND NOT c.purging) AS published FROM upstream u JOIN context.content c ON c.id=u.id LIMIT 10001")
            .bind(Uuid::from(id)).fetch_all(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        if dependencies.len() > 10000 {
            return Err(Error::Busy);
        }
        let mut assets = BTreeSet::new();
        for source in dependencies {
            if !source
                .try_get::<bool, _>("published")
                .map_err(context_storage::Error::from)?
            {
                return Err(context_storage::Error::Unavailable.into());
            }
            let asset: Option<Uuid> = source
                .try_get("asset")
                .map_err(context_storage::Error::from)?;
            if let Some(asset) = asset {
                assets.insert(asset.into());
            }
            let workspace: Uuid = source
                .try_get("workspace")
                .map_err(context_storage::Error::from)?;
            view.authorize(
                asset
                    .map(|id| Resource::Asset(id.into()))
                    .unwrap_or(Resource::Workspace(workspace.into())),
                Permission::Read,
            )
            .await?;
        }
        let mut body: Value = row.try_get("body").map_err(context_storage::Error::from)?;
        for field in ["citations", "sources"] {
            if let Some(sources) = body.get_mut(field).and_then(Value::as_array_mut) {
                for source in sources {
                    let reference: SourceReference =
                        serde_json::from_value(source.clone()).map_err(|_| Error::Contract)?;
                    let exists: bool = sqlx::query_scalar(
                        "SELECT EXISTS(SELECT 1 FROM context.revisions v JOIN context.content c ON c.id=v.content WHERE v.id=$1 AND v.asset=$2 AND c.published AND NOT c.purging)",
                    )
                    .bind(Uuid::from(reference.revision))
                    .bind(Uuid::from(reference.asset))
                    .fetch_one(&mut **view.transaction())
                    .await
                    .map_err(context_storage::Error::from)?;
                    let available = if exists {
                        match view
                            .authorize(Resource::Asset(reference.asset), Permission::Read)
                            .await
                        {
                            Ok(()) => true,
                            Err(context_storage::Error::Unavailable) => false,
                            Err(e) => return Err(e.into()),
                        }
                    } else {
                        false
                    };
                    source["available"] = json!(available);
                }
            }
        }
        Ok(Protected {
            assets,
            id,
            workspace: row
                .try_get::<Uuid, _>("workspace")
                .map_err(context_storage::Error::from)?
                .into(),
            asset: row
                .try_get::<Option<Uuid>, _>("asset")
                .map_err(context_storage::Error::from)?
                .map(Into::into),
            revision: row
                .try_get::<Option<Uuid>, _>("revision")
                .map_err(context_storage::Error::from)?
                .map(Into::into),
            kind: row.try_get("kind").map_err(context_storage::Error::from)?,
            origin: serde_json::from_value(json!(
                row.try_get::<String, _>("origin")
                    .map_err(context_storage::Error::from)?
            ))
            .map_err(|_| Error::Contract)?,
            body,
        })
    }

    /// Return ranked, deduplicated current-permission evidence with real tokenizer budgets.
    ///
    /// # Errors
    /// Rejects unauthorized scopes, excessive corpus/query/budget bounds, incompatible
    /// vector responses and dependency/index failures. No unauthorized hit counts return.
    pub async fn query(&self, actor: &Authentication, args: Query) -> Result<ContextMap> {
        if args.query.trim().is_empty()
            || args.query.len() > 4096
            || !(1..=100).contains(&args.limit)
            || args.token_budget.is_some_and(|n| n.get() > 65536)
        {
            return Err(Error::Invalid);
        }
        let plane = self.data_plane()?;
        let name = args.space.as_ref().unwrap_or(&plane.config.default_space);
        let space = plane
            .config
            .spaces
            .get(name)
            .ok_or(Error::MissingCapability)?;
        let mut view = self.database.authorized_read(actor, args.scope).await?;
        let scope = serde_json::to_value(args.scope).map_err(|_| Error::Contract)?;
        let kind = scope["kind"].as_str().ok_or(Error::Contract)?;
        let rows:Vec<Uuid>=sqlx::query_scalar("SELECT c.id FROM context.content c JOIN context.resources w ON w.id=c.workspace LEFT JOIN context.resources a ON a.id=c.asset WHERE c.published AND c.embedding_space=$3 AND c.kind IN ('chunk','memory','summary') AND (($1='tenant' AND w.owner=$2) OR ($1='workspace' AND c.workspace=$2) OR ($1='collection' AND a.parent=$2) OR ($1='asset' AND c.asset=$2)) AND ($4 OR (c.revision IS NULL OR EXISTS(SELECT 1 FROM context.assets asset WHERE asset.id=c.asset AND asset.current_revision=c.revision))) AND ($4 OR c.kind<>'memory' OR EXISTS(SELECT 1 FROM context.memory_revisions mr JOIN context.memories m ON m.id=mr.memory WHERE mr.content=c.id AND mr.id=ANY(m.current_revisions))) ORDER BY c.id LIMIT 10001")
            .bind(kind).bind(resource_id(args.scope)).bind(name).bind(args.historical).fetch_all(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        if rows.len() > 10000 {
            return Err(Error::Busy);
        }
        let mut allowed = Vec::new();
        for id in rows {
            match self.content_in(&mut view, id.into()).await {
                Ok(_) => allowed.push(ContentId::from(id)),
                Err(Error::Storage(context_storage::Error::Unavailable)) => {}
                Err(e) => return Err(e),
            }
        }
        view.commit().await?;
        if allowed.is_empty() {
            return Ok(ContextMap {
                version: "0.1".into(),
                evidence: Vec::new(),
                pointers: Vec::new(),
                tokens: TokenAccounting {
                    scheme: plane.config.token_encoding.clone(),
                    limit: args.token_budget,
                    used: 0,
                },
                truncated: false,
            });
        }
        let response = self
            .role(Call {
                actor,
                scope: args.scope,
                permission: Permission::Read,
                role: &space.query_role,
                request: json!({"text":args.query}),
                sources: BTreeSet::new(),
            })
            .await?;
        let vector = crate::worker::vector(response, name, space.dimensions)?;
        let index = plane.search.get(name).ok_or(Error::Configuration)?;
        let mut ids = BTreeSet::new();
        for batch in allowed.chunks(512) {
            ids.extend(index.hybrid(&args.query, &vector, batch, 100).await?);
        }
        // Reauthorize and hydrate in one new snapshot after the external index/model
        // boundaries. Ranking is recomputed only from these currently readable bodies.
        let mut view = self.database.authorized_read(actor, args.scope).await?;
        let mut items = Vec::new();
        for id in ids {
            match self.content_in(&mut view, id).await {
                Ok(value) => {
                    let current:bool=sqlx::query_scalar("SELECT (c.revision IS NULL OR EXISTS(SELECT 1 FROM context.assets a WHERE a.id=c.asset AND a.current_revision=c.revision)) AND (c.kind<>'memory' OR EXISTS(SELECT 1 FROM context.memory_revisions mr JOIN context.memories m ON m.id=mr.memory WHERE mr.content=c.id AND mr.id=ANY(m.current_revisions))) FROM context.content c WHERE c.id=$1")
                    .bind(Uuid::from(id)).fetch_one(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
                    if args.historical || current {
                        items.push(value);
                    }
                }
                Err(Error::Storage(context_storage::Error::Unavailable)) => {}
                Err(e) => return Err(e),
            }
        }
        let terms = words(&args.query);
        let mut lexical = Vec::new();
        let mut semantic = Vec::new();
        for item in &items {
            let text = item.body["text"].as_str().ok_or(Error::Contract)?;
            let tokens = words(text);
            lexical.push((item.id, terms.intersection(&tokens).count()));
            let embedded: Vec<f32> = serde_json::from_value(item.body["embedding"].clone())
                .map_err(|_| Error::Contract)?;
            if embedded.len() != vector.len() {
                return Err(Error::Contract);
            }
            semantic.push((item.id, cosine(&vector, &embedded)));
        }
        lexical.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        semantic.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut rank = BTreeMap::<ContentId, f64>::new();
        for list in [
            lexical
                .iter()
                .filter(|v| v.1 > 0)
                .map(|v| v.0)
                .collect::<Vec<_>>(),
            semantic.iter().map(|v| v.0).collect(),
        ] {
            for (index, id) in list.iter().enumerate() {
                *rank.entry(*id).or_default() += 1.0 / (60.0 + index as f64);
            }
        }
        items.sort_by(|a, b| rank[&b.id].total_cmp(&rank[&a.id]).then(a.id.cmp(&b.id)));
        let mut evidence = Vec::new();
        let mut seen = BTreeSet::new();
        let mut used = 0;
        let mut bytes = 0;
        let mut truncated = false;
        for item in items {
            let text = item.body["text"].as_str().ok_or(Error::Contract)?;
            if !seen.insert(text.to_string()) {
                continue;
            }
            let remaining = args
                .token_budget
                .map_or(65536, |n| n.get())
                .saturating_sub(used);
            if evidence.len() >= args.limit || remaining == 0 || bytes + text.len() > 262144 {
                truncated = true;
                continue;
            }
            let mut text = text.to_string();
            let mut tokens = plane.tokenizer.encode_ordinary(&text).len() as u32;
            if tokens > remaining {
                let encoded = plane.tokenizer.encode_ordinary(&text);
                let mut end = remaining as usize;
                text = loop {
                    match plane.tokenizer.decode(&encoded[..end]) {
                        Ok(text) => break text,
                        Err(_) if end > 0 => end -= 1,
                        Err(_) => return Err(Error::Contract),
                    }
                };
                tokens = plane.tokenizer.encode_ordinary(&text).len() as u32;
                truncated = true;
                if tokens > remaining {
                    return Err(Error::Contract);
                }
            }
            if text.is_empty() {
                continue;
            }
            used += tokens;
            bytes += text.len();
            let sources: Vec<SourceReference> = serde_json::from_value(
                item.body
                    .get("sources")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            )
            .map_err(|_| Error::Contract)?;
            let mut pointers = vec![Pointer::Content { content: item.id }];
            for source in sources.iter().filter(|s| s.available) {
                pointers.push(Pointer::Asset {
                    asset: source.asset,
                    revision: source.revision,
                });
            }
            evidence.push(Evidence {
                id: item.id,
                text,
                origin: item.origin,
                sources,
                rank: evidence.len() as u32,
                tokens,
                pointers,
            });
        }
        let revisions = evidence
            .iter()
            .flat_map(|e| {
                e.sources
                    .iter()
                    .filter(|s| s.available)
                    .map(|s| Uuid::from(s.revision))
            })
            .collect::<Vec<_>>();
        let entities:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM context.content WHERE kind='entity' AND published AND revision=ANY($1) ORDER BY id LIMIT 100")
            .bind(revisions).fetch_all(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        let mut pointers = Vec::new();
        for id in entities {
            match self.content_in(&mut view, id.into()).await {
                Ok(_) => pointers.push(Pointer::Entity { entity: id.into() }),
                Err(Error::Storage(context_storage::Error::Unavailable)) => {}
                Err(e) => return Err(e),
            }
        }
        view.commit().await?;
        Ok(ContextMap {
            version: "0.1".into(),
            evidence,
            pointers,
            tokens: TokenAccounting {
                scheme: plane.config.token_encoding.clone(),
                limit: args.token_budget,
                used,
            },
            truncated,
        })
    }

    /// Expand an exact evidence, source or entity pointer under all current protections.
    ///
    /// # Errors
    /// Denies unavailable/revoked dependencies and malformed exact source identities.
    pub async fn expand(&self, actor: &Authentication, pointer: Pointer) -> Result<Value> {
        match pointer {
            Pointer::Content { content } | Pointer::Entity { entity: content } => {
                let row = sqlx::query(
                    "SELECT asset,workspace FROM context.content WHERE id=$1 AND published",
                )
                .bind(Uuid::from(content))
                .fetch_optional(self.database.pool())
                .await
                .map_err(context_storage::Error::from)?
                .ok_or(context_storage::Error::Unavailable)?;
                let target = row
                    .try_get::<Option<Uuid>, _>("asset")
                    .map_err(context_storage::Error::from)?
                    .map(|id| Resource::Asset(id.into()))
                    .unwrap_or(Resource::Workspace(
                        row.try_get::<Uuid, _>("workspace")
                            .map_err(context_storage::Error::from)?
                            .into(),
                    ));
                let mut view = self.database.authorized_read(actor, target).await?;
                let item = self.content_in(&mut view, content).await?;
                let mut body = item.body;
                let url = if item.kind == "media" {
                    Some(
                        self.data_plane()?
                            .blobs
                            .download_url(
                                body["object_key"].as_str().ok_or(Error::Contract)?,
                                Duration::from_secs(60),
                            )
                            .await?,
                    )
                } else {
                    None
                };
                body.as_object_mut()
                    .ok_or(Error::Contract)?
                    .remove("embedding");
                body.as_object_mut()
                    .ok_or(Error::Contract)?
                    .remove("object_key");
                view.commit().await?;
                Ok(
                    json!({"content":item.id,"workspace":item.workspace,"asset":item.asset,"revision":item.revision,"kind":item.kind,"origin":item.origin,"body":body,"url":url,"expires_in_seconds":if url.is_some(){Some(60)}else{None}}),
                )
            }
            Pointer::Asset { asset, revision } => self.asset_revision(actor, asset, revision).await,
            Pointer::Page {
                asset,
                revision,
                page,
            } => {
                self.location_evidence(
                    actor,
                    asset,
                    revision,
                    json!({"kind":"pages","first":page,"last":page}),
                )
                .await
            }
            Pointer::Media {
                asset,
                revision,
                location,
            } => {
                if !location.is_valid() {
                    return Err(Error::Invalid);
                }
                self.location_evidence(
                    actor,
                    asset,
                    revision,
                    serde_json::to_value(location).map_err(|_| Error::Invalid)?,
                )
                .await
            }
        }
    }

    /// Read an explicit retained revision and issue a short-lived original-byte URL.
    ///
    /// # Errors
    /// Denies lost authority, unpublished revisions and mismatched asset/revision pairs.
    pub async fn asset_revision(
        &self,
        actor: &Authentication,
        asset: AssetId,
        revision: RevisionId,
    ) -> Result<Value> {
        let mut view = self
            .database
            .authorized_read(actor, Resource::Asset(asset))
            .await?;
        let row=sqlx::query("SELECT v.*,c.id AS source,a.current_revision FROM context.revisions v JOIN context.content c ON c.id=v.content JOIN context.assets a ON a.id=v.asset WHERE v.id=$1 AND v.asset=$2 AND c.published")
            .bind(Uuid::from(revision)).bind(Uuid::from(asset)).fetch_optional(&mut **view.transaction()).await.map_err(context_storage::Error::from)?.ok_or(context_storage::Error::Unavailable)?;
        let source = self
            .content_in(
                &mut view,
                row.try_get::<Uuid, _>("source")
                    .map_err(context_storage::Error::from)?
                    .into(),
            )
            .await?;
        let url = self
            .data_plane()?
            .blobs
            .download_url(
                source.body["object_key"].as_str().ok_or(Error::Contract)?,
                Duration::from_secs(60),
            )
            .await?;
        let ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM context.content WHERE revision=$1 AND published AND kind<>'source' ORDER BY id LIMIT 101").bind(Uuid::from(revision)).fetch_all(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        let mut pointers = Vec::new();
        for id in ids.iter().take(100) {
            if self.content_in(&mut view, (*id).into()).await.is_ok() {
                pointers.push(Pointer::Content {
                    content: (*id).into(),
                });
            }
        }
        view.commit().await?;
        Ok(
            json!({"asset":asset,"revision":revision,"current":row.try_get::<Option<Uuid>,_>("current_revision").map_err(context_storage::Error::from)?==Some(Uuid::from(revision)),"media_type":row.try_get::<String,_>("media_type").map_err(context_storage::Error::from)?,"size_bytes":row.try_get::<i64,_>("size_bytes").map_err(context_storage::Error::from)?,"url":url,"expires_in_seconds":60,"metadata":source.body.get("metadata"),"enrichment":source.body.get("enrichment"),"pointers":pointers,"truncated":ids.len()>100}),
        )
    }

    /// List only published immutable revision metadata under current asset authority.
    ///
    /// # Errors
    /// Denies unavailable assets and invalid pagination bounds.
    pub async fn asset_history(
        &self,
        actor: &Authentication,
        asset: AssetId,
        before: Option<RevisionId>,
        limit: usize,
    ) -> Result<Value> {
        if !(1..=100).contains(&limit) {
            return Err(Error::Invalid);
        }
        let mut view = self
            .database
            .authorized_read(actor, Resource::Asset(asset))
            .await?;
        let rows=sqlx::query("SELECT v.id,v.media_type,v.size_bytes,a.current_revision FROM context.revisions v JOIN context.content c ON c.id=v.content JOIN context.assets a ON a.id=v.asset WHERE v.asset=$1 AND c.published AND NOT c.purging AND ($2::uuid IS NULL OR v.id<$2) ORDER BY v.id DESC LIMIT $3")
            .bind(Uuid::from(asset)).bind(before.map(Uuid::from)).bind((limit+1) as i64).fetch_all(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        let mut revisions = Vec::new();
        for row in rows.iter().take(limit) {
            let id: Uuid = row.try_get("id").map_err(context_storage::Error::from)?;
            revisions.push(json!({"revision":id,"media_type":row.try_get::<String,_>("media_type").map_err(context_storage::Error::from)?,"size_bytes":row.try_get::<i64,_>("size_bytes").map_err(context_storage::Error::from)?,"current":row.try_get::<Option<Uuid>,_>("current_revision").map_err(context_storage::Error::from)?==Some(id),"pointer":Pointer::Asset{asset,revision:id.into()}}));
        }
        let next = if rows.len() > limit {
            revisions.last().map(|v| v["revision"].clone())
        } else {
            None
        };
        view.commit().await?;
        Ok(json!({"asset":asset,"revisions":revisions,"next_cursor":next}))
    }

    async fn location_evidence(
        &self,
        actor: &Authentication,
        asset: AssetId,
        revision: RevisionId,
        location: Value,
    ) -> Result<Value> {
        let mut view = self
            .database
            .authorized_read(actor, Resource::Asset(asset))
            .await?;
        let ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM context.content WHERE asset=$1 AND revision=$2 AND published AND kind='chunk' ORDER BY id LIMIT 10000")
            .bind(Uuid::from(asset)).bind(Uuid::from(revision)).fetch_all(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        let mut evidence = Vec::new();
        for id in ids {
            let item = self.content_in(&mut view, id.into()).await?;
            let matches = item.body["sources"]
                .as_array()
                .is_some_and(|sources| sources.iter().any(|s| overlaps(&s["location"], &location)));
            if matches && evidence.len() < 100 {
                evidence.push(json!({"content":item.id,"text":item.body["text"],"origin":item.origin,"sources":item.body["sources"]}));
            }
        }
        view.commit().await?;
        Ok(
            json!({"asset":asset,"revision":revision,"location":location,"evidence":evidence,"extract":{"tool":"context_extract_media","asset":asset,"revision":revision,"location":location}}),
        )
    }
}

fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect()
}
fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let dot = a
        .iter()
        .zip(b)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum::<f64>();
    let norm = |v: &[f32]| v.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>().sqrt();
    let n = norm(a) * norm(b);
    if n > 0.0 { dot / n } else { 0.0 }
}
fn overlaps(a: &Value, b: &Value) -> bool {
    if a["kind"] != b["kind"] {
        return a["kind"] == "asset";
    }
    let n = |v: &Value, key: &str| v[key].as_u64().unwrap_or(0);
    match a["kind"].as_str() {
        Some("pages") => n(a, "first") <= n(b, "last") && n(b, "first") <= n(a, "last"),
        Some("time") => n(a, "start_ms") < n(b, "end_ms") && n(b, "start_ms") < n(a, "end_ms"),
        Some("region") => {
            n(a, "x") < n(b, "x") + n(b, "width")
                && n(b, "x") < n(a, "x") + n(a, "width")
                && n(a, "y") < n(b, "y") + n(b, "height")
                && n(b, "y") < n(a, "y") + n(a, "height")
        }
        _ => a == b,
    }
}
