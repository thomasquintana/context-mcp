//! Authenticated MCP descriptors and dispatch. Outbound MCP belongs exclusively to HTLK.

use context_core::{
    access::Authentication,
    ids::{AssetId, CollectionId, JobId, TenantId, WorkspaceId},
};
use context_engine::Application;
use rmcp::{
    ErrorData, ServerHandler,
    model::*,
    service::{RequestContext, RoleServer},
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{borrow::Cow, sync::Arc};

/// Per-transport handler; all domain state is shared by the application instance.
#[derive(Clone)]
pub struct McpServer {
    application: Arc<Application>,
    stdio_token: Option<Arc<SecretString>>,
}

impl McpServer {
    /// Construct an authenticated HTTP handler or explicit-token stdio handler.
    pub fn new(application: Arc<Application>, stdio_token: Option<SecretString>) -> Self {
        Self {
            application,
            stdio_token: stdio_token.map(Arc::new),
        }
    }

    async fn actor(
        &self,
        context: &RequestContext<RoleServer>,
    ) -> Result<Authentication, ErrorData> {
        if let Some(parts) = context.extensions.get::<axum::http::request::Parts>() {
            return parts
                .extensions
                .get::<Authentication>()
                .cloned()
                .ok_or_else(|| ErrorData::invalid_request("authentication is required", None));
        }
        let token = self.stdio_token.as_ref().ok_or_else(|| {
            ErrorData::invalid_request("explicit stdio identity is required", None)
        })?;
        self.application
            .authentication()
            .authenticate(token.expose_secret())
            .await
            .map_err(|_| ErrorData::invalid_request("authentication failed", None))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceArguments {
    resource: ResourceArgument,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceArgument {
    kind: String,
    id: uuid::Uuid,
}
impl ResourceArgument {
    fn resource(self) -> Result<context_core::access::Resource, ErrorData> {
        use context_core::access::Resource;
        match self.kind.as_str() {
            "tenant" => Ok(Resource::Tenant(TenantId::from(self.id))),
            "workspace" => Ok(Resource::Workspace(WorkspaceId::from(self.id))),
            "collection" => Ok(Resource::Collection(CollectionId::from(self.id))),
            "asset" => Ok(Resource::Asset(AssetId::from(self.id))),
            _ => Err(ErrorData::invalid_params("invalid resource kind", None)),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionArguments {
    workspace: WorkspaceId,
    label: String,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FinalizeArguments {
    upload: uuid::Uuid,
    idempotency_key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextArguments {
    collection: CollectionId,
    name: String,
    text: String,
    profile: Option<String>,
    idempotency_key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobArguments {
    job: JobId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpandArguments {
    pointer: context_core::context::Pointer,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryArguments {
    asset: AssetId,
    before: Option<context_core::ids::RevisionId>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReprocessArguments {
    asset: AssetId,
    expected_revision: context_core::ids::RevisionId,
    profile: Option<String>,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryReadArguments {
    memory: Option<context_core::ids::MemoryId>,
    workspace: Option<WorkspaceId>,
    subject: Option<String>,
    #[serde(default)]
    history: bool,
    before: Option<i64>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobControlArguments {
    job: JobId,
    idempotency_key: String,
}

impl ServerHandler for McpServer {
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![ProtocolVersion::V_2025_11_25])
    }
    fn get_info(&self) -> ServerConfig {
        server_info()
    }
    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        self.actor(&context).await?;
        context.peer.set_peer_info(request.clone());
        self.negotiate_initialize(&request)
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.actor(&context).await?;
        Ok(ListToolsResult::with_all_items(tools()))
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools().into_iter().find(|tool| tool.name == name)
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let actor = self.actor(&context).await?;
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let result: context_engine::Result<Value> = match request.name.as_ref() {
            "context_identity" => {
                if arguments.as_object().is_none_or(|args| !args.is_empty()) {
                    return Err(ErrorData::invalid_params(
                        "identity accepts no arguments",
                        None,
                    ));
                }
                Ok(self.application.identity(&actor))
            }
            "context_resource" => {
                let args: ResourceArguments = serde_json::from_value(arguments)
                    .map_err(|_| ErrorData::invalid_params("invalid resource arguments", None))?;
                self.application
                    .resource(&actor, args.resource.resource()?)
                    .await
            }
            "context_create_collection" => {
                let args: CollectionArguments = serde_json::from_value(arguments)
                    .map_err(|_| ErrorData::invalid_params("invalid collection arguments", None))?;
                self.application
                    .create_collection(&actor, args.workspace, &args.label, &args.idempotency_key)
                    .await
            }
            "context_create_upload" => {
                let args: context_engine::uploads::CreateUpload = serde_json::from_value(arguments)
                    .map_err(|_| ErrorData::invalid_params("invalid upload arguments", None))?;
                self.application.create_upload(&actor, args).await
            }
            "context_finalize_upload" => {
                let args: FinalizeArguments = serde_json::from_value(arguments).map_err(|_| {
                    ErrorData::invalid_params("invalid finalization arguments", None)
                })?;
                self.application
                    .finalize_upload(&actor, args.upload, &args.idempotency_key)
                    .await
            }
            "context_ingest_text" => {
                let args: TextArguments = serde_json::from_value(arguments)
                    .map_err(|_| ErrorData::invalid_params("invalid text arguments", None))?;
                self.application
                    .ingest_text(
                        &actor,
                        args.collection,
                        &args.name,
                        &args.text,
                        args.profile,
                        &args.idempotency_key,
                    )
                    .await
            }
            "context_ingest_url" => {
                let args: context_engine::urls::IngestUrl = serde_json::from_value(arguments)
                    .map_err(|_| {
                        ErrorData::invalid_params("invalid URL ingestion arguments", None)
                    })?;
                self.application.ingest_url(&actor, args).await
            }
            "context_job_status" => {
                let args: JobArguments = serde_json::from_value(arguments)
                    .map_err(|_| ErrorData::invalid_params("invalid job arguments", None))?;
                self.application.job_status(&actor, args.job).await
            }
            "context_query" => {
                let args: context_engine::retrieval::Query = serde_json::from_value(arguments)
                    .map_err(|_| ErrorData::invalid_params("invalid query arguments", None))?;
                self.application
                    .query(&actor, args)
                    .await
                    .map(|value| json!(value))
            }
            "context_expand" => {
                let args: ExpandArguments = serde_json::from_value(arguments)
                    .map_err(|_| ErrorData::invalid_params("invalid exploration pointer", None))?;
                self.application.expand(&actor, args.pointer).await
            }
            "context_extract_media" => {
                let args: context_engine::media::ExtractMedia =
                    serde_json::from_value(arguments)
                        .map_err(|_| ErrorData::invalid_params("invalid media selection", None))?;
                self.application.extract_media(&actor, args).await
            }
            "context_asset_history" => {
                let args: HistoryArguments = serde_json::from_value(arguments)
                    .map_err(|_| ErrorData::invalid_params("invalid history arguments", None))?;
                self.application
                    .asset_history(&actor, args.asset, args.before, args.limit.unwrap_or(20))
                    .await
            }
            "context_reprocess" => {
                let args: ReprocessArguments = serde_json::from_value(arguments).map_err(|_| {
                    ErrorData::invalid_params("invalid reprocessing arguments", None)
                })?;
                self.application
                    .reprocess(
                        &actor,
                        args.asset,
                        args.expected_revision,
                        args.profile,
                        &args.idempotency_key,
                    )
                    .await
            }
            "context_set_access" => {
                let args: context_engine::sharing::SetAccess = serde_json::from_value(arguments)
                    .map_err(|_| ErrorData::invalid_params("invalid access arguments", None))?;
                self.application.set_access(&actor, args).await
            }
            "context_write_memory" | "context_reconcile_memory" => {
                let args: context_engine::memories::WriteMemory = serde_json::from_value(arguments)
                    .map_err(|_| {
                        ErrorData::invalid_params("invalid semantic memory arguments", None)
                    })?;
                self.application.write_memory(&actor, args).await
            }
            "context_read_memory" => {
                let args: MemoryReadArguments =
                    serde_json::from_value(arguments).map_err(|_| {
                        ErrorData::invalid_params("invalid memory view arguments", None)
                    })?;
                match (args.memory, args.workspace, args.subject) {
                    (Some(memory), None, None) => {
                        self.application
                            .read_memory(
                                &actor,
                                memory,
                                args.history,
                                args.before,
                                args.limit.unwrap_or(20),
                            )
                            .await
                    }
                    (None, Some(workspace), Some(subject))
                        if !args.history && args.before.is_none() =>
                    {
                        self.application
                            .memory_subject(&actor, workspace, &subject)
                            .await
                    }
                    _ => {
                        return Err(ErrorData::invalid_params(
                            "select a memory ID or workspace/subject",
                            None,
                        ));
                    }
                }
            }
            "context_synthesize" => {
                let args: context_engine::synthesis::Synthesize = serde_json::from_value(arguments)
                    .map_err(|_| {
                        ErrorData::invalid_params("invalid cited synthesis arguments", None)
                    })?;
                self.application.synthesize(&actor, args).await
            }
            "context_cancel_job" | "context_retry_job" => {
                let args: JobControlArguments =
                    serde_json::from_value(arguments).map_err(|_| {
                        ErrorData::invalid_params("invalid job control arguments", None)
                    })?;
                if request.name == "context_cancel_job" {
                    self.application
                        .cancel_job(&actor, args.job, &args.idempotency_key)
                        .await
                } else {
                    self.application
                        .retry_job(&actor, args.job, &args.idempotency_key)
                        .await
                }
            }
            _ => return Err(ErrorData::invalid_params("unknown tool", None)),
        };
        let mut result = match result {
            Ok(value) => CallToolResult::structured(value),
            Err(error) => {
                let code = match error {
                    context_engine::Error::Invalid
                    | context_engine::Error::Storage(context_storage::Error::Invalid) => {
                        "invalid_input"
                    }
                    context_engine::Error::Storage(context_storage::Error::Unavailable) => {
                        "unavailable"
                    }
                    context_engine::Error::Storage(context_storage::Error::Conflict) => "conflict",
                    context_engine::Error::MissingCapability => "missing_capability",
                    context_engine::Error::Busy => "overloaded",
                    context_engine::Error::Contract => "processing_contract",
                    context_engine::Error::Cancelled => "cancelled",
                    _ => "operation_failed",
                };
                CallToolResult::structured_error(
                    json!({"error":{"code":code,"message":error.to_string(),"retryable":false}}),
                )
            }
        };
        result.result_type = None;
        Ok(result.into())
    }
}

/// Stable server identity and supported capability contract.
pub fn server_info() -> ServerConfig {
    ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
        .with_server_info(Implementation::new(
            "context-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_protocol_version(ProtocolVersion::V_2025_11_25)
}

/// Exact descriptors for currently implemented application operations.
pub fn tools() -> Vec<Tool> {
    let id = json!({"type":"string","format":"uuid"});
    let source = json!({"type":"object","required":["asset","revision","location","available"],"properties":{"asset":id,"revision":id,"location":{"type":"object"},"available":{"type":"boolean"}},"additionalProperties":false});
    let memory = json!({"type":"object","required":["workspace","subject","description","value","expected_version","idempotency_key"],"properties":{"workspace":id,"memory":id,"subject":{"type":"string","minLength":1,"maxLength":512},"description":{"type":"string","maxLength":8192},"value":{"type":"object","required":["kind"],"properties":{"kind":{"enum":["text","typed"]},"statement":{"type":"string","maxLength":32768},"value_type":{"type":"string","maxLength":128},"value":{}},"additionalProperties":false},"expected_version":{"type":"integer","minimum":0},"derived_from":{"type":"array","maxItems":64,"items":source},"citations":{"type":"array","maxItems":64,"items":source},"derived_content":{"type":"array","maxItems":64,"items":id},"supersedes":{"type":"array","maxItems":64,"items":id},"idempotency_key":{"type":"string","minLength":1,"maxLength":256}},"additionalProperties":false});
    let descriptors = [
        (
            "context_write_memory",
            "Write a subject/description/typed value or statement as an immutable semantic memory revision. Expected version zero creates; conflicts remain explicit unless authorized supersession or configured authority resolves them.",
            memory.clone(),
            false,
        ),
        (
            "context_reconcile_memory",
            "Reconcile a proposed assertion against the known current memory view. Models classify equivalence/complementarity/contradiction; they cannot invent authority or supersession.",
            memory,
            false,
        ),
        (
            "context_read_memory",
            "Read a currently protected memory/conflict view or explicit bounded history. Select a memory ID, or a workspace and natural-language subject.",
            json!({"type":"object","properties":{"memory":id,"workspace":id,"subject":{"type":"string","maxLength":512},"history":{"type":"boolean"},"before":{"type":"integer","minimum":1},"limit":{"type":"integer","minimum":1,"maximum":20}},"additionalProperties":false}),
            true,
        ),
        (
            "context_synthesize",
            "Explicitly generate a cited answer from accessible evidence. Transient by default; save=true requires a stable key and retains all informational dependencies and generated origin.",
            json!({"type":"object","required":["workspace","question"],"properties":{"workspace":id,"question":{"type":"string","minLength":1,"maxLength":4096},"evidence":{"type":"array","maxItems":64,"items":id},"token_budget":{"type":"integer","minimum":1,"maximum":16384},"save":{"type":"boolean"},"name":{"type":"string","maxLength":256},"idempotency_key":{"type":"string","minLength":1,"maxLength":256}},"additionalProperties":false}),
            false,
        ),
        (
            "context_identity",
            "Return the authenticated principal and credential scope ceiling; tenant claims never create resource grants.",
            json!({"type":"object","additionalProperties":false}),
            true,
        ),
        (
            "context_resource",
            "Read current authorized resource metadata using a shared permission/data snapshot.",
            json!({"type":"object","required":["resource"],"properties":{"resource":{"type":"object","required":["kind","id"],"properties":{"kind":{"enum":["tenant","workspace","collection","asset"]},"id":id},"additionalProperties":false}},"additionalProperties":false}),
            true,
        ),
        (
            "context_create_collection",
            "Create an inheriting collection under an authorized workspace. Requires Share scope and resource permission. Stable idempotency key prevents duplicate creation.",
            json!({"type":"object","required":["workspace","label","idempotency_key"],"properties":{"workspace":id,"label":{"type":"string","minLength":1,"maxLength":256},"idempotency_key":{"type":"string","minLength":1,"maxLength":256}},"additionalProperties":false}),
            false,
        ),
        (
            "context_create_upload",
            "Reserve a tracked upload session and immutable source revision. Return signed transfer URL/headers; finalize after transfer. Requires Ingest authority and configured processing capabilities.",
            json!({"type":"object","required":["collection","name","media_type","size_bytes","sha256","idempotency_key"],"properties":{"collection":id,"asset":id,"name":{"type":"string","minLength":1,"maxLength":256},"media_type":{"type":"string","maxLength":128},"size_bytes":{"type":"integer","minimum":1},"sha256":{"type":"string","pattern":"^[0-9a-fA-F]{64}$"},"profile":{"type":"string"},"idempotency_key":{"type":"string","minLength":1,"maxLength":256}},"additionalProperties":false}),
            false,
        ),
        (
            "context_finalize_upload",
            "Verify/freeze uploaded bytes and accept one durable processing job. Failed replacement preserves the previous published revision.",
            json!({"type":"object","required":["upload","idempotency_key"],"properties":{"upload":id,"idempotency_key":{"type":"string","minLength":1,"maxLength":256}},"additionalProperties":false}),
            false,
        ),
        (
            "context_ingest_text",
            "Ingest bounded UTF-8 text through the verified-source and durable publication pipeline. Stable keys prevent duplicate assets/jobs.",
            json!({"type":"object","required":["collection","name","text","idempotency_key"],"properties":{"collection":id,"name":{"type":"string","minLength":1,"maxLength":256},"text":{"type":"string","minLength":1,"maxLength":262144},"profile":{"type":"string"},"idempotency_key":{"type":"string","minLength":1,"maxLength":200}},"additionalProperties":false}),
            false,
        ),
        (
            "context_job_status",
            "Read creator-owned progress/error and cleanup status. Raw inputs, opaque handles and source content are not returned.",
            json!({"type":"object","required":["job"],"properties":{"job":id},"additionalProperties":false}),
            true,
        ),
        (
            "context_ingest_url",
            "Import one exact operator-allowlisted URL into a verified immutable revision. No redirects, crawling or embedded subresource fetching.",
            json!({"type":"object","required":["collection","name","url","media_type","idempotency_key"],"properties":{"collection":id,"asset":id,"name":{"type":"string","maxLength":256},"url":{"type":"string","format":"uri","maxLength":8192},"media_type":{"type":"string","maxLength":128},"profile":{"type":"string"},"idempotency_key":{"type":"string","minLength":1,"maxLength":200}},"additionalProperties":false}),
            false,
        ),
        (
            "context_query",
            "Return a ranked, deduplicated, permission-aware context map with exact citations and an optional model-token evidence budget. History requires explicit opt-in.",
            json!({"type":"object","required":["scope","query"],"properties":{"scope":{"type":"object","required":["kind","id"],"properties":{"kind":{"enum":["tenant","workspace","collection","asset"]},"id":id},"additionalProperties":false},"query":{"type":"string","minLength":1,"maxLength":4096},"limit":{"type":"integer","minimum":1,"maximum":100},"token_budget":{"type":"integer","minimum":1,"maximum":65536},"historical":{"type":"boolean"},"space":{"type":"string"}},"additionalProperties":false}),
            true,
        ),
        (
            "context_expand",
            "Expand a typed exact-revision content, asset, page, media or entity pointer, rechecking all current source protections.",
            json!({"type":"object","required":["pointer"],"properties":{"pointer":{"type":"object","required":["kind"],"properties":{"kind":{"enum":["content","asset","page","media","entity"]},"content":id,"asset":id,"revision":id,"entity":id,"page":{"type":"integer","minimum":1},"location":{"type":"object"}},"additionalProperties":false}},"additionalProperties":false}),
            true,
        ),
        (
            "context_extract_media",
            "Admit durable exact-source PDF-page/image-region/media-clip extraction. Poll the job for its result content pointer, then expand it for a short-lived authorized byte URL.",
            json!({"type":"object","required":["asset","revision","location","idempotency_key"],"properties":{"asset":id,"revision":id,"location":{"type":"object"},"profile":{"type":"string"},"idempotency_key":{"type":"string","minLength":1,"maxLength":256}},"additionalProperties":false}),
            false,
        ),
        (
            "context_asset_history",
            "List bounded published immutable asset revisions and exact-revision pointers under current permissions.",
            json!({"type":"object","required":["asset"],"properties":{"asset":id,"before":id,"limit":{"type":"integer","minimum":1,"maximum":100}},"additionalProperties":false}),
            true,
        ),
        (
            "context_reprocess",
            "Explicitly reprocess the expected current immutable source into a new revision using a selected profile. A failed replacement preserves prior successful evidence.",
            json!({"type":"object","required":["asset","expected_revision","idempotency_key"],"properties":{"asset":id,"expected_revision":id,"profile":{"type":"string"},"idempotency_key":{"type":"string","minLength":1,"maxLength":200}},"additionalProperties":false}),
            false,
        ),
        (
            "context_set_access",
            "Owner-managed complete explicit grant/inheritance replacement under an expected ACL version. Empty grants revoke explicit sharing; ownership is unchanged.",
            json!({"type":"object","required":["resource","expected_acl_version","inherit","grants","idempotency_key"],"properties":{"resource":{"type":"object","required":["kind","id"],"properties":{"kind":{"enum":["tenant","workspace","collection","asset"]},"id":id},"additionalProperties":false},"expected_acl_version":{"type":"integer","minimum":1},"inherit":{"type":"boolean"},"grants":{"type":"array","maxItems":128,"items":{"type":"object","required":["recipient","permissions"],"properties":{"recipient":{"type":"object","required":["kind","id"],"properties":{"kind":{"enum":["principal","tenant","workspace"]},"id":id},"additionalProperties":false},"permissions":{"type":"array","minItems":1,"items":{"enum":["read","ingest","write_memory","synthesize","share","purge"]}}},"additionalProperties":false}},"idempotency_key":{"type":"string","minLength":1,"maxLength":256}},"additionalProperties":false}),
            false,
        ),
        (
            "context_cancel_job",
            "Fence an owned ingestion job and request existing HTLK cancellation/cleanup. Published terminal success cannot be overwritten.",
            json!({"type":"object","required":["job","idempotency_key"],"properties":{"job":id,"idempotency_key":{"type":"string","minLength":1,"maxLength":256}},"additionalProperties":false}),
            false,
        ),
        (
            "context_retry_job",
            "Explicitly retry a settled failed/cancelled revision as a new durable job with retained exact workflow revisions.",
            json!({"type":"object","required":["job","idempotency_key"],"properties":{"job":id,"idempotency_key":{"type":"string","minLength":1,"maxLength":256}},"additionalProperties":false}),
            false,
        ),
    ];
    let output = Arc::new(
        json!({"type":"object","additionalProperties":true})
            .as_object()
            .expect("static schema")
            .clone(),
    );
    descriptors
        .into_iter()
        .map(|(name, description, input, read)| {
            Tool::new(
                name,
                description,
                input.as_object().expect("static schema").clone(),
            )
            .with_raw_output_schema(output.clone())
            .with_annotations(
                ToolAnnotations::new()
                    .read_only(read)
                    .destructive(name == "context_cancel_job")
                    .idempotent(true)
                    .open_world(name == "context_ingest_url"),
            )
        })
        .collect()
}

/// Export exact catalog pins without starting providers or requiring credentials.
pub fn catalog(transport: &str) -> Value {
    json!({
        "tools":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{"context":{"deployment_id":"context-mcp","transport":transport,"implementation_name":"context-mcp","implementation_version":env!("CARGO_PKG_VERSION")}},"schema_documents":{},"tools":tools().into_iter().map(|descriptor|json!({"server":"context","descriptor":descriptor})).collect::<Vec<_>>()},
        "resources":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{},"schema_documents":{},"resources":[],"templates":[]},
        "prompts":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{},"schema_documents":{},"prompts":[]},
    })
}
