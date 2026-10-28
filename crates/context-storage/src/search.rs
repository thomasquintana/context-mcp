//! Typesense as a derived index, with application-authorized ID filters.

use crate::{Error, Result};
use context_core::{
    content::Origin,
    ids::{ContentId, RevisionId, WorkspaceId},
};
use futures_util::TryStreamExt;
use reqwest::{
    Client, Method, StatusCode, Url,
    header::{HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

/// A staged index document. PostgreSQL remains authoritative for returned evidence.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchDocument {
    /// Managed content identity, shared with PostgreSQL.
    pub id: ContentId,
    /// Workspace/project scope for indexing/repair.
    pub workspace: WorkspaceId,
    /// Exact source revision, when applicable.
    pub revision: Option<RevisionId>,
    /// Searchable text.
    pub text: String,
    /// Explicit provenance distinction.
    pub origin: Origin,
    /// Finite vector in this collection's declared embedding space.
    pub embedding: Vec<f32>,
}

/// Bounded native hybrid-search index client. Intentionally excludes credentials from Debug.
#[derive(Clone)]
pub struct Search {
    client: Client,
    endpoint: Url,
    collection: String,
    dimensions: usize,
}

impl Search {
    /// Construct an operator-selected collection with finite vectors and request bounds.
    ///
    /// # Errors
    /// Rejects malformed endpoints/collection names, credentials in URLs and invalid dimensions.
    pub fn new(endpoint: &str, api_key: &str, collection: &str, dimensions: usize) -> Result<Self> {
        let mut endpoint = Url::parse(endpoint).map_err(|_| Error::Invalid)?;
        if api_key.is_empty()
            || !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || collection.is_empty()
            || !collection
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
            || !(1..=4096).contains(&dimensions)
        {
            return Err(Error::Invalid);
        }
        if !endpoint.path().ends_with('/') {
            endpoint.set_path(&format!("{}/", endpoint.path()));
        }
        let mut headers = HeaderMap::new();
        let mut key = HeaderValue::from_str(api_key).map_err(|_| Error::Invalid)?;
        key.set_sensitive(true);
        headers.insert("X-TYPESENSE-API-KEY", key);
        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            endpoint,
            collection: collection.into(),
            dimensions,
        })
    }

    fn url(&self, path: &str) -> Result<Url> {
        self.endpoint.join(path).map_err(|_| Error::Invalid)
    }

    async fn response(response: reqwest::Response) -> Result<Value> {
        let response = response.error_for_status()?;
        if response
            .content_length()
            .is_some_and(|size| size > 8 * 1024 * 1024)
        {
            return Err(Error::Integrity);
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(bytes) = stream.try_next().await? {
            if body
                .len()
                .checked_add(bytes.len())
                .is_none_or(|size| size > 8 * 1024 * 1024)
            {
                return Err(Error::Integrity);
            }
            body.extend_from_slice(&bytes);
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Ensure the derived collection exists and has the configured vector dimension.
    ///
    /// # Errors
    /// Rejects incompatible existing schema instead of silently mixing vector spaces.
    pub async fn ensure_collection(&self) -> Result<()> {
        let url = self.url(&format!("collections/{}", self.collection))?;
        let response = self.client.get(url.clone()).send().await?;
        let schema = if response.status() == StatusCode::NOT_FOUND {
            let schema = json!({"name": self.collection, "fields": [
                {"name":"workspace","type":"string","facet":true},
                {"name":"revision","type":"string","optional":true},
                {"name":"text","type":"string"}, {"name":"origin","type":"string","facet":true},
                {"name":"embedding","type":"float[]","num_dim":self.dimensions},
            ]});
            let created = self
                .client
                .post(self.url("collections")?)
                .json(&schema)
                .send()
                .await?;
            if created.status() == StatusCode::CONFLICT {
                Self::response(self.client.get(url).send().await?).await?
            } else {
                Self::response(created).await?
            }
        } else {
            Self::response(response).await?
        };
        let fields = schema["fields"].as_array().ok_or(Error::Invalid)?;
        let vector = fields
            .iter()
            .find(|field| field["name"] == "embedding")
            .ok_or(Error::Invalid)?;
        if vector["num_dim"].as_u64() != Some(self.dimensions as u64) || vector["type"] != "float[]"
        {
            return Err(Error::Invalid);
        }
        for (name, expected) in [
            ("workspace", "string"),
            ("revision", "string"),
            ("text", "string"),
            ("origin", "string"),
        ] {
            if !fields
                .iter()
                .any(|field| field["name"] == name && field["type"] == expected)
            {
                return Err(Error::Invalid);
            }
        }
        Ok(())
    }

    /// Remove one workspace's derived index entries before explicit authoritative repair.
    ///
    /// # Errors
    /// Returns backing-service failures; a missing collection is already empty.
    pub async fn delete_workspace(&self, workspace: WorkspaceId) -> Result<()> {
        let mut url = self.url(&format!("collections/{}/documents", self.collection))?;
        url.query_pairs_mut()
            .append_pair("filter_by", &format!("workspace:={workspace}"));
        let response = self.client.delete(url).send().await?;
        if response.status() != StatusCode::NOT_FOUND {
            Self::response(response).await?;
        }
        Ok(())
    }

    /// Stage/upsert one deterministic content ID before authoritative publication.
    ///
    /// # Errors
    /// Rejects excessive text, vector-space mismatches, nonfinite values and backend errors.
    pub async fn upsert(&self, document: &SearchDocument) -> Result<()> {
        if document.text.len() > 262_144
            || document.embedding.len() != self.dimensions
            || document.embedding.iter().any(|n| !n.is_finite())
        {
            return Err(Error::Invalid);
        }
        let mut url = self.url(&format!("collections/{}/documents", self.collection))?;
        url.query_pairs_mut().append_pair("action", "upsert");
        let mut body = serde_json::to_value(document)?;
        if document.revision.is_none() {
            body.as_object_mut()
                .ok_or(Error::Invalid)?
                .remove("revision");
        }
        let response = Self::response(self.client.post(url).json(&body).send().await?).await?;
        if response["id"] != document.id.to_string() {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    /// Search only explicitly authorized, published candidate IDs.
    ///
    /// Returns ranked IDs for authoritative hydration; raw bodies, scores and
    /// unauthorized hit counts never become application evidence through this API.
    ///
    /// # Errors
    /// Rejects unbounded queries, wrong vectors and unexpected/unauthorized result IDs.
    pub async fn hybrid(
        &self,
        query: &str,
        vector: &[f32],
        allowed: &[ContentId],
        limit: usize,
    ) -> Result<Vec<ContentId>> {
        if allowed.is_empty() {
            return Ok(Vec::new());
        }
        if query.len() > 4096
            || vector.len() != self.dimensions
            || vector.iter().any(|v| !v.is_finite())
            || allowed.len() > 512
            || !(1..=100).contains(&limit)
        {
            return Err(Error::Invalid);
        }
        let allowed: BTreeSet<_> = allowed.iter().copied().collect();
        let ids = allowed
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let vector = serde_json::to_string(vector)?;
        let search = json!({"searches":[{
            "collection":self.collection, "q":query, "query_by":"text",
            "vector_query":format!("embedding:({vector}, alpha:0.5)"),
            "filter_by":format!("id:=[{ids}]"), "per_page":limit,
            "exclude_fields":"embedding,text", "rerank_hybrid_matches":true,
        }]});
        let response = Self::response(
            self.client
                .post(self.url("multi_search")?)
                .json(&search)
                .send()
                .await?,
        )
        .await?;
        let result = response["results"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or(Error::Invalid)?;
        if result.get("error").is_some() {
            return Err(Error::Invalid);
        }
        let hits = result["hits"].as_array().ok_or(Error::Invalid)?;
        if hits.len() > limit {
            return Err(Error::Integrity);
        }
        let mut seen = BTreeSet::new();
        let mut ids = Vec::new();
        for hit in hits {
            let id: ContentId = hit["document"]["id"]
                .as_str()
                .ok_or(Error::Invalid)?
                .parse()
                .map_err(|_| Error::Invalid)?;
            if !allowed.contains(&id) || !seen.insert(id) {
                return Err(Error::Invalid);
            }
            ids.push(id);
        }
        Ok(ids)
    }

    /// Remove index entries idempotently as part of a durable purge worklist.
    ///
    /// # Errors
    /// Returns bounded backend failures; missing documents already count as removed.
    pub async fn delete(&self, id: ContentId) -> Result<()> {
        let response = self
            .client
            .request(
                Method::DELETE,
                self.url(&format!("collections/{}/documents/{id}", self.collection))?,
            )
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        Self::response(response).await?;
        Ok(())
    }
}
