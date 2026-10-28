//! User/agent HTTP CLI client. Every MCP operation belongs to an admitted HTLK graph.
use context_core::ids::{JobId, PrincipalId, TenantId};
use context_workflows::host::{JobGrant, State, WorkflowHost};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Read, path::Path, time::Duration};

/// Redacted explicit CLI operation result.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
/// One local HTLK host retaining exact server descriptors and a scoped HTTP endpoint.
pub struct Client {
    host: WorkflowHost,
    job: JobId,
    _directory: tempfile::TempDir,
}
impl Client {
    /// Open a client using a bearer credential inherited from the named environment.
    ///
    /// # Errors
    /// Rejects malformed/credential-bearing URLs, unavailable tokens and descriptor drift.
    pub async fn open(endpoint: &str, token_env: &str, allow_http: bool) -> Result<Self> {
        let url = url::Url::parse(endpoint).map_err(|_| "invalid MCP endpoint")?;
        if !(url.scheme() == "https" || (url.scheme() == "http" && allow_http))
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err("invalid MCP endpoint".into());
        }
        let token = std::env::var(token_env)
            .map_err(|_| "bearer-token environment reference is required")?;
        let directory = tempfile::tempdir()?;
        let mut roles = serde_json::Map::new();
        std::fs::write(
            directory.path().join("catalogs.json"),
            serde_json::to_vec(&crate::mcp::catalog("streamable_http"))?,
        )?;
        for tool in crate::mcp::tools() {
            let name = tool.name.as_ref();
            let file = format!("{name}.htlk");
            std::fs::write(
                directory.path().join(&file),
                format!(
                    r#"
ir_version = "0.1"
graph client.{name} {{
    inputs = {{request = json}}
    outputs = {{result = json}}
    nodes {{operation = call(mcp.tool("context","{name}"))}}
    edges {{edge request {{from = inputs.request to = operation.inputs.arguments}} edge result {{from = operation.outputs.value to = outputs.result}}}}
    limits {{max_mcp_calls = 1}}
}}
"#
                ),
            )?;
            roles.insert(
                name.to_string(),
                json!({"source":file,"catalogs":"catalogs.json"}),
            );
        }
        let config = serde_json::from_value(
            json!({"database":"client.sqlite","roles":roles,"endpoints":{"context-mcp":{"transport":"streamable_http","url":url.as_str(),"headers":{"Authorization":format!("Bearer {token}")}}},"required_deployments":["context-mcp"],"workers":2,"max_runs":8}),
        )?;
        let host = WorkflowHost::open(&config, directory.path(), &[]).await?;
        let job = JobId::new();
        host.grant_job(JobGrant {
            job,
            principal: PrincipalId::new(),
            tenant: TenantId::new(),
            assets: BTreeSet::new(),
            roles: roles.keys().cloned().collect(),
            fingerprints: Default::default(),
            fenced: false,
        })?;
        Ok(Self {
            host,
            job,
            _directory: directory,
        })
    }

    /// Call an exported application tool through the shared public HTLK host.
    ///
    /// # Errors
    /// Returns redacted admission/protocol/tool failures and bounded deadline errors.
    pub async fn call(&self, tool: &str, arguments: &Value) -> Result<Value> {
        let handle = self
            .host
            .start(self.job, tool, arguments, &JobId::new().to_string())
            .await?;
        let deadline = std::time::Instant::now() + Duration::from_secs(180);
        loop {
            match self.host.inspect(&handle).await {
            Ok(State::Succeeded(value))=>return Ok(value),Ok(State::Running)=>{},
            Err(context_workflows::host::Error::Runtime(error)) if error.code().to_string()=="E_FENCED"=>{},
            _=>return Err("MCP operation failed; check credential scopes, current resource authority and input contract".into()),
        }
            if std::time::Instant::now() > deadline {
                self.host.cancel(&handle, "cli-deadline").await?;
                return Err("MCP operation deadline exceeded".into());
            }
            actix::clock::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Stream a verified local file through upload/finalize contracts and optional polling.
    ///
    /// # Errors
    /// Rejects nonfiles, unreadable/changed bytes, failed transfers and failed processing.
    pub async fn upload(
        &self,
        file: &Path,
        collection: context_core::ids::CollectionId,
        profile: Option<String>,
        media_type: Option<String>,
        key: &str,
        wait: bool,
    ) -> Result<Value> {
        let mut input = std::fs::File::open(file)?;
        if !input.metadata()?.is_file() {
            return Err("upload requires a regular file".into());
        }
        let mut buffer = [0_u8; 65536];
        let mut hash = Sha256::new();
        let mut size = 0_u64;
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            size = size.checked_add(read as u64).ok_or("file size overflow")?;
            hash.update(&buffer[..read]);
        }
        let hash = hash
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let name = file
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("invalid upload filename")?;
        let media = media_type.unwrap_or_else(|| {
            mime_guess::from_path(file)
                .first_or_octet_stream()
                .essence_str()
                .into()
        });
        let mut args = json!({"collection":collection,"name":name,"media_type":media,"size_bytes":size,"sha256":hash,"idempotency_key":format!("{key}:upload")});
        if let Some(profile) = profile {
            args["profile"] = json!(profile);
        }
        let session = self.call("context_create_upload", &args).await?;
        if session["finalized"] != true {
            let input = tokio::fs::File::open(file).await?;
            let http = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(900))
                .build()
                .map_err(|_| "upload client failed")?;
            let mut transfer = http
                .put(session["url"].as_str().ok_or("invalid upload receipt")?)
                .body(reqwest::Body::wrap_stream(
                    tokio_util::io::ReaderStream::new(input),
                ));
            for (name, value) in session["headers"]
                .as_object()
                .ok_or("invalid upload headers")?
            {
                transfer = transfer.header(name, value.as_str().ok_or("invalid upload header")?);
            }
            let response = transfer
                .send()
                .await
                .map_err(|_| "upload transfer failed")?;
            if !response.status().is_success()
                && response.status() != reqwest::StatusCode::PRECONDITION_FAILED
            {
                return Err("upload transfer rejected".into());
            }
        }
        let receipt = self
            .call(
                "context_finalize_upload",
                &json!({"upload":session["upload"],"idempotency_key":format!("{key}:finalize")}),
            )
            .await?;
        if !wait {
            return Ok(receipt);
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(3600);
        loop {
            let state = self
                .call("context_job_status", &json!({"job":receipt["job"]}))
                .await?;
            match state["state"].as_str() {
                Some("succeeded") => return Ok(state),
                Some("failed" | "cancelled") => {
                    return Err(
                        "processing did not succeed; inspect the durable job receipt".into(),
                    );
                }
                _ => {}
            }
            if std::time::Instant::now() > deadline {
                return Err("processing wait deadline exceeded; poll the durable job".into());
            }
            actix::clock::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Close native sessions/store before removing the client's temporary execution files.
    /// # Errors
    /// Returns existing public HTLK shutdown failures.
    pub async fn shutdown(&self) -> Result<()> {
        self.host.shutdown().await?;
        Ok(())
    }
}

/// Read one bounded JSON argument object from a file or explicit stdin (`-`).
/// # Errors
/// Rejects oversized, malformed and nonobject inputs.
pub fn arguments(path: &Path) -> Result<Value> {
    let input: Box<dyn Read> = if path == Path::new("-") {
        Box::new(std::io::stdin())
    } else {
        Box::new(std::fs::File::open(path)?)
    };
    let mut bytes = Vec::new();
    input.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err("arguments exceed1 MiB".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid JSON arguments")?;
    if !value.is_object() {
        return Err("arguments must be an object".into());
    }
    Ok(value)
}
