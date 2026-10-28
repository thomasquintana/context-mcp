//! Authenticated MCP descriptors and dispatch. Outbound MCP belongs exclusively to HTLK.

use context_core::{
    access::Authentication,
    ids::{AssetId, CollectionId, TenantId, WorkspaceId},
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
    let descriptors = [
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
                    .destructive(false)
                    .idempotent(true)
                    .open_world(false),
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
