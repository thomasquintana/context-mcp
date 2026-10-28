//! Explicit cited generation, transient by default and dependency-protected when saved.
use crate::{Application, Error, Result, operations::Call, retrieval::Query};
use context_core::{
    access::{Authentication, Permission, Resource},
    content::{Origin, SourceReference},
    ids::{ContentId, WorkspaceId},
};
use context_storage::{
    postgres::{existing_receipt, receipt},
    search::SearchDocument,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, num::NonZeroU32};
use uuid::Uuid;

/// Explicit evidence-first synthesis request; saving must be requested separately.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Synthesize {
    /// Authorized destination/context workspace.
    pub workspace: WorkspaceId,
    /// Bounded user question, never authority or provider configuration.
    pub question: String,
    /// Exact evidence; empty performs bounded current-view context retrieval.
    #[serde(default)]
    pub evidence: BTreeSet<ContentId>,
    /// Evidence-token budget (default8192, at most16384).
    pub token_budget: Option<NonZeroU32>,
    /// Explicitly save a generated knowledge artifact.
    #[serde(default)]
    pub save: bool,
    /// Optional label for a saved artifact.
    pub name: Option<String>,
    /// Required for saving; transient retries are new computations.
    pub idempotency_key: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    text: String,
    citations: Vec<ContentId>,
}

impl Application {
    /// Generate a bounded cited answer from currently accessible exact evidence.
    ///
    /// # Errors
    /// Rejects missing capabilities/evidence, forbidden sources, invented citations,
    /// malformed output and stale/revoked dependencies before delivery/publication.
    pub async fn synthesize(&self, actor: &Authentication, args: Synthesize) -> Result<Value> {
        let plane = self.data_plane()?;
        if args.question.trim().is_empty()
            || args.question.len() > 4096
            || args.evidence.len() > 64
            || args.token_budget.is_some_and(|n| n.get() > 16384)
            || args.name.as_ref().is_some_and(|s| s.len() > 256)
        {
            return Err(Error::Invalid);
        }
        let role = plane
            .config
            .synthesis_role
            .as_deref()
            .ok_or(Error::MissingCapability)?;
        let key = args.idempotency_key.as_deref().unwrap_or("");
        if args.save && (key.is_empty() || key.len() > 256) {
            return Err(Error::Invalid);
        }
        let hash: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&(
                args.workspace,
                &args.question,
                &args.evidence,
                args.token_budget,
                args.save,
                &args.name,
            ))
            .map_err(|_| Error::Invalid)?,
        )
        .into();
        let mut view = self
            .database
            .authorized_write(
                actor,
                Resource::Workspace(args.workspace),
                Permission::Synthesize,
            )
            .await?;
        if args.save
            && let Some(value) = existing_receipt(
                view.transaction(),
                actor.principal,
                "save_synthesis",
                key,
                &hash,
            )
            .await?
        {
            let id = value["content"]
                .as_str()
                .ok_or(Error::Contract)?
                .parse()
                .map_err(|_| Error::Contract)?;
            let saved = self.content_in(&mut view, id).await?;
            view.commit().await?;
            return Ok(
                json!({"content":id,"saved":true,"origin":"generated","text":saved.body["text"],"citations":saved.body["evidence_citations"]}),
            );
        }
        let owner: Uuid = sqlx::query_scalar("SELECT owner FROM context.resources WHERE id=$1")
            .bind(Uuid::from(args.workspace))
            .fetch_one(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        view.commit().await?;
        let budget = args
            .token_budget
            .unwrap_or(NonZeroU32::new(8192).ok_or(Error::Contract)?);
        let mut selected = std::collections::BTreeMap::new();
        let ids = if args.evidence.is_empty() {
            let context = self
                .query(
                    actor,
                    Query {
                        scope: Resource::Workspace(args.workspace),
                        query: args.question.clone(),
                        limit: 32,
                        token_budget: Some(budget),
                        historical: false,
                        space: None,
                    },
                )
                .await?;
            for item in context.evidence {
                selected.insert(item.id, item.text);
            }
            selected.keys().copied().collect::<BTreeSet<_>>()
        } else {
            args.evidence
        };
        if ids.is_empty() {
            return Err(context_storage::Error::Unavailable.into());
        }
        let mut view = self
            .database
            .authorized_view(
                actor,
                Resource::Workspace(args.workspace),
                Permission::Synthesize,
            )
            .await?;
        let mut evidence = Vec::new();
        let mut total = 0;
        let mut sources = Vec::<SourceReference>::new();
        for id in &ids {
            let item = self.content_in(&mut view, *id).await?;
            let original = item.body["text"].as_str().ok_or(Error::Invalid)?;
            let text = selected.get(id).map(String::as_str).unwrap_or(original);
            if !original.starts_with(text) {
                return Err(context_storage::Error::Conflict.into());
            }
            total += plane.tokenizer.encode_ordinary(text).len();
            if total > budget.get() as usize {
                return Err(Error::Invalid);
            }
            evidence.push(json!({"id":id,"text":text,"origin":item.origin,"sources":item.body.get("sources")}));
            let references: Vec<SourceReference> = serde_json::from_value(
                item.body
                    .get("sources")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            )
            .map_err(|_| Error::Contract)?;
            for reference in references {
                if !sources.contains(&reference) {
                    sources.push(reference);
                }
            }
        }
        view.commit().await?;
        let text = serde_json::to_string(&json!({"question":args.question,"evidence":evidence}))
            .map_err(|_| Error::Invalid)?;
        let result = self
            .role(Call {
                actor,
                scope: Resource::Workspace(args.workspace),
                permission: Permission::Synthesize,
                role,
                request: json!({"text":text,"evidence":ids,"question":args.question}),
                sources: ids.clone(),
            })
            .await?;
        let document: crate::data::Document =
            serde_json::from_value(result["document"].clone()).map_err(|_| Error::Contract)?;
        let answer: Answer = document.decode().map_err(|_| Error::Contract)?;
        if answer.text.trim().is_empty()
            || answer.text.len() > 32768
            || answer.citations.is_empty()
            || answer.citations.len() > 64
            || answer.citations.iter().any(|id| !ids.contains(id))
        {
            return Err(Error::Contract);
        }
        let mut current = self
            .database
            .authorized_view(
                actor,
                Resource::Workspace(args.workspace),
                Permission::Synthesize,
            )
            .await?;
        for source in &ids {
            self.content_in(&mut current, *source).await?;
        }
        current.commit().await?;
        if !args.save {
            return Ok(
                json!({"saved":false,"origin":"generated","text":answer.text,"citations":answer.citations}),
            );
        }
        let name = &plane.config.default_space;
        let space = plane.config.spaces.get(name).ok_or(Error::Configuration)?;
        let vector = self
            .role(Call {
                actor,
                scope: Resource::Workspace(args.workspace),
                permission: Permission::Synthesize,
                role: &space.document_role,
                request: json!({"text":answer.text}),
                sources: ids.clone(),
            })
            .await?;
        let embedding = crate::worker::vector(vector, name, space.dimensions)?;
        let _publication = plane.publications.lock().await;
        let _owner = plane.tenant_lock(owner.into()).await;
        let mut view = self
            .database
            .authorized_write(
                actor,
                Resource::Workspace(args.workspace),
                Permission::Synthesize,
            )
            .await?;
        if let Some(value) = existing_receipt(
            view.transaction(),
            actor.principal,
            "save_synthesis",
            key,
            &hash,
        )
        .await?
        {
            let id = value["content"]
                .as_str()
                .ok_or(Error::Contract)?
                .parse()
                .map_err(|_| Error::Contract)?;
            let saved = self.content_in(&mut view, id).await?;
            view.commit().await?;
            return Ok(
                json!({"content":id,"saved":true,"origin":"generated","text":saved.body["text"],"citations":saved.body["evidence_citations"]}),
            );
        }
        for source in &ids {
            self.content_in(&mut view, *source).await?;
        }
        let id = ContentId::new();
        let body = json!({"text":answer.text,"name":args.name,"question":args.question,"origin":"generated","sources":sources,"evidence_citations":answer.citations,"embedding":embedding,"author":actor.principal});
        sqlx::query("INSERT INTO context.content(id,workspace,kind,origin,body,embedding_space) VALUES($1,$2,'summary','generated',$3,$4)").bind(Uuid::from(id)).bind(Uuid::from(args.workspace)).bind(&body).bind(name).execute(&mut **view.transaction()).await.map_err(context_storage::Error::from)?;
        // Depend on ALL model inputs, not only the subset it chose to cite.
        for source in &ids {
            sqlx::query("INSERT INTO context.dependencies VALUES($1,$2,'derivation')")
                .bind(Uuid::from(*source))
                .bind(Uuid::from(id))
                .execute(&mut **view.transaction())
                .await
                .map_err(context_storage::Error::from)?;
        }
        view.commit().await?;
        plane
            .search
            .get(name)
            .ok_or(Error::Configuration)?
            .upsert(&SearchDocument {
                id,
                workspace: args.workspace,
                revision: None,
                text: answer.text.clone(),
                origin: Origin::Generated,
                embedding,
            })
            .await?;
        let mut view = self
            .database
            .authorized_write(
                actor,
                Resource::Workspace(args.workspace),
                Permission::Synthesize,
            )
            .await?;
        for source in &ids {
            self.content_in(&mut view, *source).await?;
        }
        sqlx::query("UPDATE context.content SET published=true WHERE id=$1")
            .bind(Uuid::from(id))
            .execute(&mut **view.transaction())
            .await
            .map_err(context_storage::Error::from)?;
        let receipt_value = json!({"content":id,"saved":true,"origin":"generated"});
        receipt(
            view.transaction(),
            actor.principal,
            "save_synthesis",
            key,
            &hash,
            &receipt_value,
        )
        .await?;
        view.commit().await?;
        Ok(
            json!({"content":id,"saved":true,"origin":"generated","text":answer.text,"citations":answer.citations}),
        )
    }
}
