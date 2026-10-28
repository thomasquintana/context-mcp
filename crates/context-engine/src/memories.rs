//! Immutable semantic assertions, explicit conflict views and authority-bounded reconciliation.
use crate::{Application, Error, Result, operations::Call};
use context_core::{
    access::{Authentication, Permission, Resource},
    content::{Origin, SourceReference},
    ids::{ContentId, MemoryId, MemoryRevisionId, PrincipalId, WorkspaceId},
    memory::{MemoryValue, ReconciliationBasis},
};
use context_storage::{
    authorization::AuthorizedTransaction,
    postgres::{existing_receipt, receipt},
    search::SearchDocument,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::collections::BTreeSet;
use uuid::Uuid;

/// Write/reconcile a human-subject semantic memory; no machine key is required.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteMemory {
    /// Project scope, protected by WriteMemory and current source permissions.
    pub workspace: WorkspaceId,
    /// Optional known logical memory; otherwise normalized subject locates it.
    pub memory: Option<MemoryId>,
    /// Human-readable subject.
    pub subject: String,
    /// Natural-language context.
    pub description: String,
    /// Statement or explicitly typed JSON value.
    pub value: MemoryValue,
    /// Expected current version; zero creates a new memory.
    pub expected_version: i64,
    /// Source-derived assertions, whose purge propagates.
    #[serde(default)]
    pub derived_from: Vec<SourceReference>,
    /// Independently authored support, whose removal only invalidates citations.
    #[serde(default)]
    pub citations: Vec<SourceReference>,
    /// Additional exact managed-content derivations, e.g. memory/synthesis revisions.
    #[serde(default)]
    pub derived_content: BTreeSet<ContentId>,
    /// Explicitly authorized current revisions to supersede.
    #[serde(default)]
    pub supersedes: BTreeSet<MemoryRevisionId>,
    /// Stable principal/operation intent key.
    pub idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Classification {
    relation: Relation,
    rationale: String,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Relation {
    Equivalent,
    Complementary,
    Contradiction,
}

impl Application {
    /// Publish one immutable revision or a preserved explicit conflict view.
    ///
    /// # Errors
    /// Rejects stale versions, denied sources, invalid typed values, ungranted
    /// supersession and malformed/failed model classification or embedding.
    pub async fn write_memory(&self, actor: &Authentication, args: WriteMemory) -> Result<Value> {
        self.write_memory_inner(actor, args)
            .await
            .map_err(write_conflict)
    }

    async fn write_memory_inner(
        &self,
        actor: &Authentication,
        mut args: WriteMemory,
    ) -> Result<Value> {
        let plane = self.data_plane()?;
        if args.subject.trim().is_empty()
            || args.subject.len() > 512
            || args.description.len() > 8192
            || args.expected_version < 0
            || args.derived_from.len() > 64
            || args.citations.len() > 64
            || args.derived_content.len() > 64
            || args.supersedes.len() > 64
        {
            return Err(Error::Invalid);
        }
        validate_value(&args.value, &plane.config.memory_types)?;
        let normalized = args
            .subject
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        let hash: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&(
                args.workspace,
                args.memory,
                &args.subject,
                &args.description,
                &args.value,
                args.expected_version,
                &args.derived_from,
                &args.citations,
                &args.derived_content,
                &args.supersedes,
            ))
            .map_err(|_| Error::Invalid)?,
        )
        .into();
        let mut view = self
            .database
            .authorized_write(
                actor,
                Resource::Workspace(args.workspace),
                Permission::WriteMemory,
            )
            .await?;
        if let Some(value) = existing_receipt(
            view.transaction(),
            actor.principal,
            "write_memory",
            &args.idempotency_key,
            &hash,
        )
        .await?
        {
            view.commit().await?;
            return Ok(value);
        }
        let existing=sqlx::query("SELECT * FROM context.memories WHERE workspace=$1 AND (($2::uuid IS NOT NULL AND id=$2) OR ($2::uuid IS NULL AND subject_normalized=$3))")
            .bind(Uuid::from(args.workspace)).bind(args.memory.map(Uuid::from)).bind(&normalized).fetch_optional(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        let (memory, current) = if let Some(row) = existing {
            if row
                .try_get::<i64, _>("version")
                .map_err(context_storage::Error::from)?
                != args.expected_version
                || row
                    .try_get::<String, _>("subject_normalized")
                    .map_err(context_storage::Error::from)?
                    != normalized
            {
                return Err(context_storage::Error::Conflict.into());
            }
            (
                MemoryId::from(
                    row.try_get::<Uuid, _>("id")
                        .map_err(context_storage::Error::from)?,
                ),
                row.try_get::<Vec<Uuid>, _>("current_revisions")
                    .map_err(context_storage::Error::from)?,
            )
        } else {
            if args.expected_version != 0 || args.memory.is_some() {
                return Err(context_storage::Error::Conflict.into());
            }
            (MemoryId::new(), Vec::new())
        };
        let current_ids = current
            .iter()
            .copied()
            .map(MemoryRevisionId::from)
            .collect::<BTreeSet<_>>();
        if current_ids.len() > 64 {
            return Err(Error::Busy);
        }
        if !args.supersedes.is_subset(&current_ids) {
            return Err(Error::Invalid);
        }
        let mut previous = Vec::new();
        let mut dependencies = args.derived_content.clone();
        for revision in &current {
            let id: Uuid = sqlx::query_scalar(
                "SELECT content FROM context.memory_revisions WHERE id=$1 AND memory=$2",
            )
            .bind(revision)
            .bind(Uuid::from(memory))
            .fetch_one(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
            previous.push(self.content_in(&mut view, id.into()).await?);
        }
        let derived = self
            .references_in(&mut view, &mut args.derived_from)
            .await?;
        dependencies.extend(derived);
        let mut citations = self.references_in(&mut view, &mut args.citations).await?;
        for source in &args.derived_content {
            self.content_in(&mut view, *source).await?;
        }
        let owner: Uuid = sqlx::query_scalar("SELECT owner FROM context.resources WHERE id=$1")
            .bind(Uuid::from(args.workspace))
            .fetch_one(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        view.commit().await?;
        let old_values = previous
            .iter()
            .map(|p| {
                serde_json::from_value::<MemoryValue>(p.body["value"].clone())
                    .map_err(|_| Error::Contract)
            })
            .collect::<Result<Vec<_>>>()?;
        let explicit = !args.supersedes.is_empty();
        let classification = if previous.is_empty()
            || explicit
            || old_values.iter().all(|v| v == &args.value)
        {
            Classification {
                relation: Relation::Equivalent,
                rationale: if explicit {
                    "Explicit supersession"
                } else {
                    "Identical value or initial assertion"
                }
                .into(),
            }
        } else if old_values
            .iter()
            .all(|old| scalar_conflict(old, &args.value))
        {
            Classification {
                relation: Relation::Contradiction,
                rationale: "Different values of the same scalar type".into(),
            }
        } else {
            let role = plane
                .config
                .reconcile_role
                .as_deref()
                .ok_or(Error::MissingCapability)?;
            let sources = dependencies
                .iter()
                .copied()
                .chain(previous.iter().map(|p| p.id))
                .collect();
            let text=serde_json::to_string(&json!({"subject":args.subject,"current":previous.iter().map(|p|json!({"description":p.body["description"],"value":p.body["value"]})).collect::<Vec<_>>(),"proposed":{"description":args.description,"value":args.value}})).map_err(|_|Error::Invalid)?;
            let result = self
                .role(Call {
                    actor,
                    scope: Resource::Workspace(args.workspace),
                    permission: Permission::WriteMemory,
                    role,
                    request: json!({"text":text,"proposed":{"value":args.value,"description":args.description}}),
                    sources,
                })
                .await?;
            let document: crate::data::Document =
                serde_json::from_value(result["document"].clone()).map_err(|_| Error::Contract)?;
            let classification: Classification = document.decode().map_err(|_| Error::Contract)?;
            if classification.rationale.len() > 8192 {
                return Err(Error::Contract);
            }
            classification
        };
        let revision = MemoryRevisionId::new();
        let content = ContentId::new();
        let version = args.expected_version.checked_add(1).ok_or(Error::Invalid)?;
        let mut value = args.value.clone();
        let mut origin = Origin::AgentAssertion;
        let mut supersedes = args.supersedes.clone();
        let mut view_ids = current_ids.clone();
        let mut basis = ReconciliationBasis::Equivalent;
        if explicit {
            for id in &args.supersedes {
                view_ids.remove(id);
            }
            view_ids.insert(revision);
            basis = ReconciliationBasis::ExplicitSupersession;
        } else if !previous.is_empty() && matches!(classification.relation, Relation::Contradiction)
        {
            let rank = |principal: PrincipalId| {
                plane
                    .config
                    .memory_authority
                    .get(&principal)
                    .copied()
                    .unwrap_or(0)
            };
            let old_rank = previous
                .iter()
                .map(|p| {
                    p.body["author"]
                        .as_str()
                        .and_then(|s| s.parse::<PrincipalId>().ok())
                        .map(rank)
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0);
            if rank(actor.principal) > old_rank {
                view_ids.clear();
                view_ids.insert(revision);
                supersedes = current_ids.clone();
                basis = ReconciliationBasis::AuthorityRule;
            } else if rank(actor.principal) < old_rank {
                view_ids = previous
                    .iter()
                    .filter(|p| {
                        p.body["author"]
                            .as_str()
                            .and_then(|s| s.parse::<PrincipalId>().ok())
                            .map(rank)
                            == Some(old_rank)
                    })
                    .map(|p| {
                        p.body["memory_revision"]
                            .as_str()
                            .ok_or(Error::Contract)?
                            .parse()
                            .map_err(|_| Error::Contract)
                    })
                    .collect::<Result<_>>()?;
                basis = ReconciliationBasis::AuthorityRule;
            } else {
                view_ids.insert(revision);
                basis = ReconciliationBasis::UnresolvedConflict;
            }
        } else {
            if !previous.is_empty() {
                origin = Origin::Generated;
                supersedes = current_ids.clone();
                if matches!(classification.relation, Relation::Complementary) {
                    value = merge_values(&old_values, &value)?;
                    basis = ReconciliationBasis::Complementary;
                }
                dependencies.extend(previous.iter().map(|p| p.id));
                for previous in &previous {
                    merge_references(&mut args.derived_from, previous.body.get("sources"))?;
                    merge_references(&mut args.citations, previous.body.get("citations"))?;
                }
                let mut descriptions = previous
                    .iter()
                    .filter_map(|p| p.body["description"].as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if !descriptions.contains(&args.description) {
                    descriptions.push(args.description.clone());
                }
                descriptions.dedup();
                args.description = descriptions.join("\n");
                if args.description.len() > 8192 {
                    return Err(Error::Invalid);
                }
            }
            view_ids.clear();
            view_ids.insert(revision);
        }
        validate_value(&value, &plane.config.memory_types)?;
        if args.derived_from.len() > 256 || args.citations.len() > 256 {
            return Err(Error::Invalid);
        }
        let text = format!(
            "{}\n{}\n{}",
            args.subject,
            args.description,
            serde_json::to_string(&value).map_err(|_| Error::Contract)?
        );
        let name = &plane.config.default_space;
        let space = plane.config.spaces.get(name).ok_or(Error::Configuration)?;
        let all_sources = dependencies
            .iter()
            .copied()
            .chain(previous.iter().map(|p| p.id))
            .collect();
        let embedding = self
            .role(Call {
                actor,
                scope: Resource::Workspace(args.workspace),
                permission: Permission::WriteMemory,
                role: &space.document_role,
                request: json!({"text":text}),
                sources: all_sources,
            })
            .await?;
        let embedding = crate::worker::vector(embedding, name, space.dimensions)?;
        let body = json!({"memory":memory,"memory_revision":revision,"version":version,"subject":args.subject,"description":args.description,"value":value,"author":actor.principal,"origin":origin,"sources":args.derived_from,"citations":args.citations,"supersedes":supersedes,"basis":basis,"rationale":classification.rationale,"text":text,"embedding":embedding});
        let _publication = plane.publications.lock().await;
        let _owner = plane.tenant_lock(owner.into()).await;
        let mut view = self
            .database
            .authorized_write(
                actor,
                Resource::Workspace(args.workspace),
                Permission::WriteMemory,
            )
            .await?;
        if let Some(value) = existing_receipt(
            view.transaction(),
            actor.principal,
            "write_memory",
            &args.idempotency_key,
            &hash,
        )
        .await?
        {
            view.commit().await?;
            return Ok(value);
        }
        let current_version: Option<i64> =
            sqlx::query_scalar("SELECT version FROM context.memories WHERE id=$1 FOR UPDATE")
                .bind(Uuid::from(memory))
                .fetch_optional(&mut **view.transaction())
                .await
                .map_err(context_storage::Error::from)?;
        if current_version.unwrap_or(0) != args.expected_version {
            return Err(context_storage::Error::Conflict.into());
        }
        for source in &dependencies {
            self.content_in(&mut view, *source).await?;
        }
        self.references_in(&mut view, &mut args.derived_from)
            .await?;
        for reference in args.citations.iter_mut().filter(|r| r.available) {
            citations.extend(
                self.references_in(&mut view, std::slice::from_mut(reference))
                    .await?,
            );
        }
        sqlx::query("INSERT INTO context.content(id,workspace,kind,origin,body,embedding_space) VALUES($1,$2,'memory',$3,$4,$5)")
            .bind(Uuid::from(content)).bind(Uuid::from(args.workspace)).bind(serde_json::to_value(origin).map_err(|_|Error::Contract)?.as_str().ok_or(Error::Contract)?).bind(&body).bind(name).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        for (ids, relationship) in [(&dependencies, "derivation"), (&citations, "citation")] {
            for source in ids {
                sqlx::query(
                    "INSERT INTO context.dependencies VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
                )
                .bind(Uuid::from(*source))
                .bind(Uuid::from(content))
                .bind(relationship)
                .execute(&mut **view.transaction())
                .await
                .map_err(context_storage::Error::from)?;
            }
        }
        view.commit().await?;
        let index = plane.search.get(name).ok_or(Error::Configuration)?;
        index
            .upsert(&SearchDocument {
                id: content,
                workspace: args.workspace,
                revision: None,
                text,
                origin,
                embedding,
            })
            .await?;
        let mut view = self
            .database
            .authorized_write(
                actor,
                Resource::Workspace(args.workspace),
                Permission::WriteMemory,
            )
            .await?;
        for source in &dependencies {
            self.content_in(&mut view, *source).await?;
        }
        if args.expected_version == 0 {
            sqlx::query(
                "INSERT INTO context.memories(id,workspace,subject_normalized) VALUES($1,$2,$3)",
            )
            .bind(Uuid::from(memory))
            .bind(Uuid::from(args.workspace))
            .bind(&normalized)
            .execute(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        }
        let changed=sqlx::query("UPDATE context.memories SET version=$3,current_revisions=$4 WHERE id=$1 AND version=$2")
            .bind(Uuid::from(memory)).bind(args.expected_version).bind(version).bind(view_ids.iter().copied().map(Uuid::from).collect::<Vec<_>>()).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?.rows_affected();
        if changed != 1 {
            return Err(context_storage::Error::Conflict.into());
        }
        sqlx::query("INSERT INTO context.memory_revisions(id,memory,content,version,author) VALUES($1,$2,$3,$4,$5)").bind(Uuid::from(revision)).bind(Uuid::from(memory)).bind(Uuid::from(content)).bind(version).bind(Uuid::from(actor.principal)).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        sqlx::query("UPDATE context.content SET published=true WHERE id=$1")
            .bind(Uuid::from(content))
            .execute(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        let current = if view_ids.len() == 1 {
            json!({"state":"current","revision":view_ids.first()})
        } else {
            json!({"state":"conflicted","revisions":view_ids})
        };
        let result = json!({"memory":memory,"revision":revision,"content":content,"version":version,"view":current,"basis":basis});
        receipt(
            view.transaction(),
            actor.principal,
            "write_memory",
            &args.idempotency_key,
            &hash,
            &result,
        )
        .await?;
        view.commit().await?;
        Ok(result)
    }

    pub(crate) async fn references_in(
        &self,
        view: &mut AuthorizedTransaction<'_>,
        references: &mut [SourceReference],
    ) -> Result<BTreeSet<ContentId>> {
        let mut ids = BTreeSet::new();
        for reference in references {
            if !reference.location.is_valid() {
                return Err(Error::Invalid);
            }
            let id: Uuid = sqlx::query_scalar(
                "SELECT content FROM context.revisions WHERE id=$1 AND asset=$2",
            )
            .bind(Uuid::from(reference.revision))
            .bind(Uuid::from(reference.asset))
            .fetch_optional(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?
            .ok_or(context_storage::Error::Unavailable)?;
            self.content_in(view, id.into()).await?;
            reference.available = true;
            ids.insert(id.into());
        }
        Ok(ids)
    }

    /// Read a protected current/conflicted view or explicitly requested bounded history.
    ///
    /// # Errors
    /// Denies missing memories and any inaccessible current contribution.
    pub async fn read_memory(
        &self,
        actor: &Authentication,
        memory: MemoryId,
        history: bool,
        before: Option<i64>,
        limit: usize,
    ) -> Result<Value> {
        if !(1..=20).contains(&limit) {
            return Err(Error::Invalid);
        }
        let workspace: Uuid =
            sqlx::query_scalar("SELECT workspace FROM context.memories WHERE id=$1")
                .bind(Uuid::from(memory))
                .fetch_optional(self.database.pool())
                .await
                .map_err(context_storage::Error::from)?
                .ok_or(context_storage::Error::Unavailable)?;
        let mut view = self
            .database
            .authorized_read(actor, Resource::Workspace(workspace.into()))
            .await?;
        let row = sqlx::query("SELECT version,current_revisions FROM context.memories WHERE id=$1")
            .bind(Uuid::from(memory))
            .fetch_one(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        let current: Vec<Uuid> = row
            .try_get("current_revisions")
            .map_err(context_storage::Error::from)?;
        let current_content: Vec<Uuid> =
            sqlx::query_scalar("SELECT content FROM context.memory_revisions WHERE id=ANY($1)")
                .bind(&current)
                .fetch_all(&mut **view.transaction())
                .await
                .map_err(context_storage::Error::from)?;
        for id in current_content {
            self.content_in(&mut view, id.into()).await?;
        }
        let rows=sqlx::query("SELECT id,content,version FROM context.memory_revisions WHERE memory=$1 AND ($2 OR id=ANY($3)) AND ($4::bigint IS NULL OR version<$4) ORDER BY version DESC LIMIT $5")
            .bind(Uuid::from(memory)).bind(history).bind(&current).bind(before).bind((limit+1) as i64).fetch_all(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        let mut revisions = Vec::new();
        let mut size = 0;
        let mut next = None;
        for row in rows {
            let version: i64 = row
                .try_get("version")
                .map_err(context_storage::Error::from)?;
            if revisions.len() >= limit {
                next = revisions.last().and_then(|r: &Value| r["version"].as_i64());
                break;
            }
            match self
                .content_in(
                    &mut view,
                    row.try_get::<Uuid, _>("content")
                        .map_err(context_storage::Error::from)?
                        .into(),
                )
                .await
            {
                Ok(item) => {
                    let mut body = item.body;
                    body.as_object_mut()
                        .ok_or(Error::Contract)?
                        .remove("embedding");
                    size += serde_json::to_vec(&body)
                        .map_err(|_| Error::Contract)?
                        .len();
                    if size > 262144 {
                        next = revisions.last().and_then(|r: &Value| r["version"].as_i64());
                        break;
                    }
                    revisions.push(body);
                }
                Err(Error::Storage(context_storage::Error::Unavailable)) if history => {}
                Err(e) => return Err(e),
            }
            if version == 0 {
                return Err(Error::Contract);
            }
        }
        let current = if current.len() == 1 {
            json!({"state":"current","revision":current[0]})
        } else {
            json!({"state":"conflicted","revisions":current})
        };
        view.commit().await?;
        Ok(
            json!({"memory":memory,"workspace":workspace,"version":row.try_get::<i64,_>("version").map_err(context_storage::Error::from)?,"view":current,"revisions":revisions,"next_cursor":next}),
        )
    }

    /// Resolve a natural-language subject and read its current protected memory view.
    ///
    /// # Errors
    /// Denies unavailable workspaces/subjects and inaccessible source contributions.
    pub async fn memory_subject(
        &self,
        actor: &Authentication,
        workspace: WorkspaceId,
        subject: &str,
    ) -> Result<Value> {
        if subject.len() > 512 {
            return Err(Error::Invalid);
        }
        let normalized = subject
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        let mut view = self
            .database
            .authorized_read(actor, Resource::Workspace(workspace))
            .await?;
        let id: Uuid = sqlx::query_scalar(
            "SELECT id FROM context.memories WHERE workspace=$1 AND subject_normalized=$2",
        )
        .bind(Uuid::from(workspace))
        .bind(normalized)
        .fetch_optional(&mut **view.transaction())
        .await
        .map_err(context_storage::Error::from)?
        .ok_or(context_storage::Error::Unavailable)?;
        view.commit().await?;
        self.read_memory(actor, id.into(), false, None, 20).await
    }
}

fn validate_value(
    value: &MemoryValue,
    types: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    if serde_json::to_vec(value).map_err(|_| Error::Invalid)?.len() > 32768 {
        return Err(Error::Invalid);
    }
    match value {
        MemoryValue::Text { statement } if !statement.trim().is_empty() => Ok(()),
        MemoryValue::Typed { value_type, value } => {
            let kind = types
                .get(value_type)
                .map(String::as_str)
                .unwrap_or(value_type);
            let valid = match kind {
                "string" => value.is_string(),
                "number" => value.is_number(),
                "integer" => value.is_i64() || value.is_u64(),
                "boolean" => value.is_boolean(),
                "object" => value.is_object(),
                "array" => value.is_array(),
                "null" => value.is_null(),
                _ => false,
            };
            if valid { Ok(()) } else { Err(Error::Invalid) }
        }
        _ => Err(Error::Invalid),
    }
}
fn scalar_conflict(a: &MemoryValue, b: &MemoryValue) -> bool {
    matches!((a,b),(MemoryValue::Typed{value_type:a,value:x},MemoryValue::Typed{value_type:b,value:y}) if a==b &&!x.is_object()&&!x.is_array() &&x!=y)
}
fn merge_values(old: &[MemoryValue], new: &MemoryValue) -> Result<MemoryValue> {
    match new {
        MemoryValue::Text { statement } => {
            let mut statements = Vec::new();
            for value in old.iter().chain(std::iter::once(new)) {
                let MemoryValue::Text { statement } = value else {
                    return Err(Error::Contract);
                };
                if !statements.contains(statement) {
                    statements.push(statement.clone());
                }
            }
            if statements.is_empty() {
                statements.push(statement.clone());
            }
            Ok(MemoryValue::Text {
                statement: statements.join("\n"),
            })
        }
        MemoryValue::Typed {
            value_type,
            value: Value::Object(value),
        } => {
            let mut merged = value.clone();
            for previous in old {
                let MemoryValue::Typed {
                    value_type: kind,
                    value: Value::Object(object),
                } = previous
                else {
                    return Err(Error::Contract);
                };
                if kind != value_type {
                    return Err(Error::Contract);
                }
                for (key, value) in object {
                    if merged.get(key).is_some_and(|existing| existing != value) {
                        return Err(Error::Contract);
                    }
                    merged.insert(key.clone(), value.clone());
                }
            }
            Ok(MemoryValue::Typed {
                value_type: value_type.clone(),
                value: Value::Object(merged),
            })
        }
        _ => Err(Error::Contract),
    }
}
fn merge_references(target: &mut Vec<SourceReference>, value: Option<&Value>) -> Result<()> {
    let items: Vec<SourceReference> =
        serde_json::from_value(value.cloned().unwrap_or_else(|| json!([])))
            .map_err(|_| Error::Contract)?;
    for reference in items {
        if !target.contains(&reference) {
            target.push(reference);
        }
    }
    Ok(())
}
fn write_conflict(error: Error) -> Error {
    match &error {
        Error::Storage(context_storage::Error::Database(e))
            if e.as_database_error()
                .and_then(|e| e.code())
                .is_some_and(|c| matches!(c.as_ref(), "40001" | "40P01" | "23505")) =>
        {
            context_storage::Error::Conflict.into()
        }
        _ => error,
    }
}
