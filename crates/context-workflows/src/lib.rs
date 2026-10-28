//! Operator-controlled integration validation using unchanged public HTLK APIs.
//!
//! This is an integration-spike host, not the production authentication or job
//! service. Configuration is trusted operator input and can launch local programs.
//! MCP discovery and execution both use HTLK's existing `NativeMcpAdapter`.

use htlk::{
    compiler::{
        Compiler, CompilerCapabilities, CompilerConfig, FrontendLimits, MCPCatalogInputs, analyzer,
        executable::{
            EvaluatorLimits, ExecutionLimits, Identifier, JsonDocument, McpBinding, McpBindingKind,
            McpTransport, PolicyDocument, PolicyFields, ServerIdentity,
            cbor::{Limits, Value},
            digest::Digest,
        },
    },
    runtime::{
        Authorization, HostContext, NativeRegistry, Runtime, RuntimeConfig, RuntimePolicy,
        execution::{ExecutionConfig, ExecutionEngine},
        failure::{FailureCode, RuntimeFailure},
        identity::RunId,
        mcp::{McpAdapter, McpCancellation, McpEndpoint, McpRunContext, NativeMcpAdapter},
        records::{InputValue, RunStatus, SecurityLabels},
        store::{SqliteStore, StoreConfig},
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    error::Error,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

/// Integration errors; never include endpoint environment values in diagnostics.
pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Trusted deployment configuration; contains no provider-specific tool selection.
#[derive(Clone, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum Endpoint {
    /// Spawn a local MCP process through HTLK.
    Stdio {
        /// Absolute or operator-PATH program name.
        program: String,
        /// Exact program arguments.
        arguments: Vec<String>,
        /// Nonsecret explicit environment settings; prefer secret environment references.
        environment: BTreeMap<String, String>,
        /// Host environment names whose values are copied without being logged.
        #[serde(default)]
        inherit_environment: Vec<String>,
    },
    /// Connect through HTLK's existing Streamable HTTP client.
    StreamableHttp {
        /// Operator-authorized endpoint.
        url: String,
        /// Nonsecret headers; use host environment references for bearer credentials.
        headers: BTreeMap<String, String>,
        /// Header names mapped to host environment variable names.
        #[serde(default)]
        header_environment: BTreeMap<String, String>,
    },
}

impl Endpoint {
    fn resolve(&self) -> Result<McpEndpoint> {
        match self {
            Self::Stdio {
                program,
                arguments,
                environment,
                inherit_environment,
            } => {
                let mut environment = environment.clone();
                for name in inherit_environment {
                    environment.insert(
                        name.clone(),
                        std::env::var(name).map_err(|_| {
                            format!("required host environment variable is not set: {name}")
                        })?,
                    );
                }
                Ok(McpEndpoint::Stdio {
                    program: program.clone(),
                    arguments: arguments.clone(),
                    environment,
                })
            }
            Self::StreamableHttp {
                url,
                headers,
                header_environment,
            } => {
                let mut headers = headers.clone();
                for (header, variable) in header_environment {
                    headers.insert(
                        header.clone(),
                        std::env::var(variable).map_err(|_| {
                            format!("required host environment variable is not set: {variable}")
                        })?,
                    );
                }
                Ok(McpEndpoint::Http {
                    url: url.clone(),
                    headers,
                })
            }
        }
    }

    fn transport(&self) -> McpTransport {
        match self {
            Self::Stdio { .. } => McpTransport::Stdio,
            Self::StreamableHttp { .. } => McpTransport::StreamableHttp,
        }
    }
}

/// One operator-selected server and tool set for exact live-catalog export.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverySelection {
    /// Trusted deployment-map key.
    pub deployment_id: String,
    /// Expected advertised implementation name; drift is an error.
    pub implementation_name: String,
    /// Expected advertised implementation version; drift is an error.
    pub implementation_version: String,
    /// Exact operator-selected tool names, confined to integration configuration.
    pub tools: Vec<String>,
}

/// Discovery configuration; export is explicit and never rewrites admitted pins.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryConfig {
    /// Source aliases mapped to operator-selected identities/tools.
    pub servers: BTreeMap<String, DiscoverySelection>,
    /// Deployment IDs mapped to trusted endpoints.
    pub endpoints: BTreeMap<String, Endpoint>,
}

fn adapter(endpoints: &BTreeMap<String, Endpoint>) -> Result<Arc<NativeMcpAdapter>> {
    let endpoints = endpoints
        .iter()
        .map(|(name, endpoint)| Ok((name.clone(), endpoint.resolve()?)))
        .collect::<Result<_>>()?;
    Ok(Arc::new(NativeMcpAdapter::new(endpoints, 10_000)?))
}

/// Export complete selected descriptors using HTLK's existing discovery API.
///
/// This performs no tool invocation. The selection binding's provisional digests
/// are discovery metadata only; exported exact descriptors become authoritative
/// compiler/runtime pins during subsequent graph admission.
///
/// # Errors
/// Rejects endpoint/identity/protocol drift, missing tools, and discovery failures.
pub async fn discover(config: &DiscoveryConfig) -> Result<serde_json::Value> {
    let limits = Limits::default();
    let adapter = adapter(&config.endpoints)?;
    let context = McpRunContext {
        run_id: RunId::new("context-explicit-catalog-discovery".into(), &limits)?,
        host: HostContext::new("context-integration-spike".into(), &limits)?,
        fence: 1,
        cancellation: Arc::new(McpCancellation::new()),
    };
    let result = async {
        let provisional = JsonDocument::new(br#"{"type":"object"}"#, &limits)?.digest();
        let mut servers = serde_json::Map::new();
        let mut tools = Vec::new();
        for (alias, selection) in &config.servers {
            let endpoint = config
                .endpoints
                .get(&selection.deployment_id)
                .ok_or("discovery deployment is not configured")?;
            let server = ServerIdentity::new(
                selection.deployment_id.clone(),
                endpoint.transport(),
                selection.implementation_name.clone(),
                selection.implementation_version.clone(),
                &limits,
            )?;
            servers.insert(
                alias.clone(),
                serde_json::json!({
                    "deployment_id": selection.deployment_id,
                    "transport": endpoint.transport().as_str(),
                    "implementation_name": selection.implementation_name,
                    "implementation_version": selection.implementation_version,
                }),
            );
            for tool in &selection.tools {
                let binding = McpBinding::new(
                    server.clone(),
                    provisional,
                    McpBindingKind::Tool {
                        name: tool.clone(),
                        input_schema: provisional,
                        output_schema: provisional,
                    },
                    &limits,
                )?;
                let descriptor = adapter.descriptor(&binding, &context, &limits).await?;
                let descriptor: serde_json::Value = serde_json::from_slice(descriptor.as_bytes())?;
                tools.push(serde_json::json!({"server": alias, "descriptor": descriptor}));
            }
        }
        Ok(serde_json::json!({
            "tools": {
                "catalog_version": "0.1", "mcp_protocol_version": "2025-11-25",
                "servers": servers, "schema_documents": {}, "tools": tools,
            },
            "resources": {
                "catalog_version": "0.1", "mcp_protocol_version": "2025-11-25",
                "servers": {}, "schema_documents": {}, "resources": [], "templates": [],
            },
            "prompts": {
                "catalog_version": "0.1", "mcp_protocol_version": "2025-11-25",
                "servers": {}, "schema_documents": {}, "prompts": [],
            },
        }))
    }
    .await;
    context.cancellation.cancel();
    adapter.shutdown().await?;
    result
}

/// Trusted graph-run configuration for opt-in integration validation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    /// HTLK source, relative to the configuration file.
    pub source: PathBuf,
    /// Exact admitted catalogs, relative to the configuration file.
    pub catalogs: PathBuf,
    /// Typed graph input values represented as JSON.
    pub inputs: BTreeMap<String, serde_json::Value>,
    /// Trusted deployment routing.
    pub endpoints: BTreeMap<String, Endpoint>,
    /// Optional persistent execution store; absent uses isolated in-memory state.
    #[serde(default)]
    pub database: Option<PathBuf>,
    /// Stable admission key within the configured execution store.
    pub idempotency_key: String,
}

/// Successful run summary. Explicitly writing outputs may contain test source data.
#[derive(Serialize)]
pub struct RunReport {
    /// Registered executable identity.
    pub fingerprint: String,
    /// Durable HTLK run identity.
    pub run_id: String,
    /// Accepted graph outputs.
    pub outputs: BTreeMap<String, serde_json::Value>,
}

struct SpikePolicy;
impl RuntimePolicy for SpikePolicy {
    fn authorize(
        &self,
        context: &HostContext,
        _: Authorization<'_>,
    ) -> std::result::Result<(), RuntimeFailure> {
        if context.principal() == "context-integration-spike" {
            Ok(())
        } else {
            Err(RuntimeFailure::new(FailureCode::Policy))
        }
    }
    fn input_labels(
        &self,
        _: &HostContext,
        _: Digest,
        _: &Identifier,
        _: Option<&Value>,
    ) -> std::result::Result<SecurityLabels, RuntimeFailure> {
        Ok(SecurityLabels::default())
    }
}

/// Construct the existing HTLK compiler and matching native registry for the spike.
///
/// # Errors
/// Returns invalid limits, policies, or linked implementation errors.
pub fn compiler(limits: &Limits) -> Result<(Compiler, NativeRegistry)> {
    let policy = PolicyDocument::new(
        PolicyFields {
            cost_unit: "credits".into(),
            defaults: ExecutionLimits::new()
                .with_timeout_ms(60_000)?
                .with_attempt_timeout_ms(30_000)?
                .with_max_concurrency(1)?,
            evaluator_limits: EvaluatorLimits {
                max_expression_depth: 64,
                max_value_bytes: 262_144,
                max_collection_visits: 10_000,
                max_regex_bytes: 4096,
                max_regex_compiled_bytes: 65_536,
                max_output_bytes: 262_144,
                // JSON validation meters both native and JSON representations;
                // bounded inline audio needs more than the tiny scalar-spike budget.
                max_steps: 4_000_000,
            },
            maximum_scope_depth: 32,
            maximum_expanded_nodes: 1000,
        },
        limits,
    )?;
    let mut registry = NativeRegistry::new(limits)?;
    let profile = registry.link_policy(&policy)?;
    let compiler = Compiler::new(CompilerConfig {
        profile,
        policy,
        libraries: BTreeMap::new(),
        limits: limits.clone(),
        frontend: FrontendLimits::default(),
        schema_options: analyzer::NativeSchemaOptions::default(),
        capabilities: CompilerCapabilities::default(),
    })?;
    Ok((compiler, registry))
}

fn object(value: &Value) -> Result<&htlk::compiler::executable::cbor::Map> {
    match value {
        Value::Map(value) => Ok(value),
        _ => Err("expected object".into()),
    }
}

/// Compile, admit, execute, inspect, and read accepted values using public HTLK APIs.
///
/// Run within an Actix system. This deliberately uses a spike-only fixed principal;
/// it must not be exposed as the production authentication/authorization boundary.
///
/// # Errors
/// Returns compilation, admission, transport, execution, or output-reading failures.
pub async fn run(config: &RunConfig, base: &Path) -> Result<RunReport> {
    let limits = Limits::default();
    let catalogs =
        MCPCatalogInputs::from_json(&std::fs::read(base.join(&config.catalogs))?, &limits)?;
    let (compiler, registry) = compiler(&limits)?;
    let graph = compiler
        .compile_source(
            &std::fs::read_to_string(base.join(&config.source))?,
            &catalogs,
        )
        .map_err(|diagnostics| format!("graph compilation failed: {diagnostics:?}"))?;
    let inputs = config
        .inputs
        .iter()
        .map(|(key, value)| {
            let value = JsonDocument::new(&serde_json::to_vec(value)?, &limits)?;
            Ok((key.parse()?, InputValue::Inline(value.value().clone())))
        })
        .collect::<Result<_>>()?;
    let adapter = adapter(&config.endpoints)?;
    let store = match &config.database {
        Some(path) => SqliteStore::open(base.join(path), StoreConfig::default()).await?,
        None => SqliteStore::in_memory(StoreConfig::default()).await?,
    };
    let runtime = Arc::new(
        Runtime::new(
            store,
            Arc::new(registry),
            Arc::new(SpikePolicy),
            RuntimeConfig::default(),
        )?
        .with_mcp(adapter.clone()),
    );
    let context = HostContext::new("context-integration-spike".into(), &limits)?;
    runtime
        .register_graph(&context, &graph.executable_bytes, &[])
        .await?;
    let engine = ExecutionEngine::launch(runtime.clone(), ExecutionConfig::default())?;
    let result = async {
        let run = runtime
            .start_run(
                &context,
                graph.fingerprint,
                &inputs,
                &config.idempotency_key,
                None,
            )
            .await?;
        let deadline = Instant::now() + Duration::from_secs(90);
        let view = loop {
            match runtime.inspect_run(&context, &run.run_id).await {
                Ok(view) if view.header.status.is_terminal() => break view,
                Ok(_) => {}
                Err(error) if error.code() == FailureCode::Fenced => {}
                Err(error) => return Err(error.into()),
            }
            if Instant::now() >= deadline {
                return Err("integration run exceeded its deadline".into());
            }
            actix::clock::sleep(Duration::from_millis(20)).await;
        };
        if view.header.status != RunStatus::Succeeded {
            return Err(format!(
                "integration run {} ended {:?}; failure={:?}",
                run.run_id,
                view.header.status,
                view.failure.as_ref().map(|failure| failure.code())
            )
            .into());
        }
        let mut outputs = BTreeMap::new();
        for (name, entry) in object(view.outputs.as_ref().ok_or("missing run outputs")?)?.iter() {
            let Some(Value::Text(id)) = object(entry)?.get("artifact_id") else {
                return Err("missing accepted output artifact".into());
            };
            let artifact = runtime.read_artifact(&context, id.parse()?).await?;
            let document = JsonDocument::from_value(&artifact.value, &limits)?;
            outputs.insert(
                name.to_owned(),
                serde_json::from_slice(document.as_bytes())?,
            );
        }
        Ok(RunReport {
            fingerprint: graph.fingerprint.to_string(),
            run_id: run.run_id.to_string(),
            outputs,
        })
    }
    .await;
    let engine_shutdown = engine.shutdown().await;
    let adapter_shutdown = adapter.shutdown().await;
    engine_shutdown?;
    adapter_shutdown?;
    result
}
