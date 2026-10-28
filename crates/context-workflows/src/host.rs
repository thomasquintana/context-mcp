//! Long-lived HTLK hosting with trusted job grants and provider-neutral role ports.
//!
//! Role graphs take one required `request: json` and return one required
//! `result: json` object. Operator graphs own provider tool names/argument shapes;
//! host authorization facts are never inferred from ordinary request contents.

use crate::{Endpoint, adapter};
use context_core::ids::{AssetId, JobId, PrincipalId, TenantId};
use htlk::{
    compiler::{
        Compiler, CompilerCapabilities, CompilerConfig, FrontendLimits, MCPCatalogInputs, analyzer,
        executable::{
            BuiltinType, CanonicalDocument, EvaluatorLimits, ExecutionLimits, Identifier,
            JsonDocument, PolicyDocument, PolicyFields, ValueTypeKind,
            cbor::{Limits, Value},
            digest::Digest,
        },
    },
    runtime::{
        Authorization, HostContext, NativeRegistry, Runtime, RuntimeConfig, RuntimeError,
        RuntimePolicy,
        execution::{ExecutionConfig, ExecutionEngine},
        failure::{FailureCode, RuntimeFailure},
        identity::RunId,
        mcp::{McpAdapter, McpCancellation, McpRunContext, NativeMcpAdapter},
        records::{InputValue, RunStatus, SecurityLabels},
        store::{SqliteStore, StoreConfig},
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

/// Redacted production workflow-host failures.
#[derive(thiserror::Error)]
pub enum Error {
    /// Invalid operator configuration or role contract.
    #[error("invalid workflow configuration or role contract")]
    Configuration,
    /// Missing or revoked trusted job authority.
    #[error("workflow access denied")]
    Unauthorized,
    /// Shutdown has closed host admission.
    #[error("workflow host is stopping")]
    Stopped,
    /// Graph execution failed with an existing stable runtime classification.
    #[error("workflow execution failed: {0}")]
    Failed(FailureCode),
    /// Public runtime operation failed.
    #[error("workflow runtime operation failed")]
    Runtime(#[from] RuntimeError),
    /// Startup failed; the cause is for explicit trusted diagnostics only.
    #[error("workflow host initialization failed")]
    Initialization(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WorkflowError({self})")
    }
}

/// Production-host operation result.
pub type Result<T> = std::result::Result<T, Error>;

/// A retained operator workflow revision supplying a logical application role.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    /// Source file, relative to the operator configuration directory.
    pub source: PathBuf,
    /// Exact catalog snapshot, relative to the operator configuration directory.
    pub catalogs: PathBuf,
}

/// Finite operator-selected settings; no provider credentials are serialized.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Persistent HTLK execution database; PostgreSQL owns application state separately.
    pub database: PathBuf,
    /// Logical role names mapped to retained operator graph revisions.
    pub roles: BTreeMap<String, Role>,
    /// Trusted endpoint routing and environment-referenced credentials.
    pub endpoints: BTreeMap<String, Endpoint>,
    /// Operator-selected deployment IDs which must be used by an admitted role.
    pub required_deployments: BTreeSet<String>,
    /// Bounded calculation workers inside the single application instance.
    pub workers: usize,
    /// Maximum simultaneously supervised runs.
    pub max_runs: usize,
}

/// Application-authenticated job identity restored from authoritative records.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JobGrant {
    /// Stable application job identity.
    pub job: JobId,
    /// Initiating principal, independent of the runtime's service-owned job identity.
    pub principal: PrincipalId,
    /// Owning execution tenant.
    pub tenant: TenantId,
    /// Source assets whose current access was checked by the application.
    pub assets: BTreeSet<AssetId>,
    /// Only these logical roles may be dispatched for this job.
    pub roles: BTreeSet<String>,
}

struct AuthorizedJob {
    identity: JobGrant,
    graphs: BTreeSet<Digest>,
    bindings: BTreeSet<Digest>,
    active: bool,
}

struct Policy {
    jobs: RwLock<BTreeMap<JobId, AuthorizedJob>>,
    limits: Limits,
}

fn job_context(job: JobId, limits: &Limits) -> std::result::Result<HostContext, RuntimeError> {
    HostContext::new(format!("context-job:{job}"), limits)
}

fn labels(job: &JobGrant, limits: &Limits) -> std::result::Result<SecurityLabels, RuntimeFailure> {
    let mut labels = vec![
        format!("job:{}", job.job),
        format!("tenant:{}", job.tenant),
        format!("principal:{}", job.principal),
    ];
    labels.extend(job.assets.iter().map(|id| format!("asset:{id}")));
    SecurityLabels::new(labels, limits).map_err(|_| RuntimeFailure::new(FailureCode::Policy))
}

fn protected_by(job: &JobGrant, actual: &SecurityLabels) -> bool {
    actual.as_slice().contains(&format!("job:{}", job.job))
        && actual
            .as_slice()
            .contains(&format!("tenant:{}", job.tenant))
        && actual
            .as_slice()
            .contains(&format!("principal:{}", job.principal))
}

impl RuntimePolicy for Policy {
    fn authorize(
        &self,
        context: &HostContext,
        action: Authorization<'_>,
    ) -> std::result::Result<(), RuntimeFailure> {
        let deny = || RuntimeFailure::new(FailureCode::Policy);
        if context.principal() == "context-operator" {
            return match action {
                Authorization::RegisterGraph { .. } | Authorization::InspectGraph { .. } => Ok(()),
                _ => Err(deny()),
            };
        }
        let id = context
            .principal()
            .strip_prefix("context-job:")
            .ok_or_else(deny)?
            .parse::<JobId>()
            .map_err(|_| deny())?;
        let jobs = self.jobs.read().map_err(|_| deny())?;
        let job = jobs.get(&id).ok_or_else(deny)?;
        let allowed = match action {
            Authorization::CancelRun { .. } => true,
            Authorization::InspectRun { labels, .. } => protected_by(&job.identity, labels),
            _ if !job.active => false,
            Authorization::StartRun { fingerprint, .. }
            | Authorization::InspectGraph { fingerprint } => job.graphs.contains(&fingerprint),
            Authorization::ReadBoundary { labels, .. } => protected_by(&job.identity, labels),
            Authorization::ReadArtifact { metadata } => {
                protected_by(&job.identity, metadata.security_labels())
            }
            Authorization::DispatchMcp {
                binding, labels, ..
            } => {
                protected_by(&job.identity, labels)
                    && job
                        .bindings
                        .contains(&binding.digest(&self.limits).map_err(|_| deny())?)
            }
            Authorization::ReadResource { binding, .. } => job
                .bindings
                .contains(&binding.digest(&self.limits).map_err(|_| deny())?),
            _ => false,
        };
        if allowed { Ok(()) } else { Err(deny()) }
    }

    fn input_labels(
        &self,
        context: &HostContext,
        _: Digest,
        _: &Identifier,
        _: Option<&Value>,
    ) -> std::result::Result<SecurityLabels, RuntimeFailure> {
        let deny = || RuntimeFailure::new(FailureCode::Policy);
        let id = context
            .principal()
            .strip_prefix("context-job:")
            .ok_or_else(deny)?
            .parse::<JobId>()
            .map_err(|_| deny())?;
        let jobs = self.jobs.read().map_err(|_| deny())?;
        let job = jobs.get(&id).filter(|job| job.active).ok_or_else(deny)?;
        labels(&job.identity, &self.limits)
    }
}

struct AdmittedRole {
    fingerprint: Digest,
    document: CanonicalDocument,
}

/// Durable run identity which can be saved in PostgreSQL job/stage records.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Handle {
    /// Owning authenticated application job.
    pub job: JobId,
    /// Exact workflow revision, retained across role changes/restarts.
    pub fingerprint: String,
    /// HTLK durable run identity.
    pub run_id: String,
}

/// Accepted workflow state; only successful object-valued results expose content.
pub enum State {
    /// Work is still executing or awaiting an external prerequisite.
    Running,
    /// Accepted role result, subject to application schema validation/publication.
    Succeeded(serde_json::Value),
    /// Fixed runtime failure classification.
    Failed(FailureCode),
    /// Cancellation won the durable terminal outcome.
    Cancelled,
}

/// One compiler/registry/store/runtime/engine lifetime for the application instance.
pub struct WorkflowHost {
    runtime: Arc<Runtime>,
    adapter: Arc<NativeMcpAdapter>,
    engine: ExecutionEngine,
    store: SqliteStore,
    policy: Arc<Policy>,
    roles: BTreeMap<String, AdmittedRole>,
    limits: Limits,
    stopping: AtomicBool,
}

impl WorkflowHost {
    /// Open/register retained role graphs, restore trusted job grants, launch once.
    ///
    /// Call inside an Actix system after application authentication/persistence is
    /// initialized. Restored grants must come from current authoritative job/ACL
    /// records, never user request bodies. Admission roles retain source/catalog pins.
    ///
    /// # Errors
    /// Rejects invalid role ports, missing required deployments and incompatible stores.
    pub async fn open(config: &Config, base: &Path, restored: &[JobGrant]) -> Result<Self> {
        let limits = Limits {
            max_document_bytes: 32 * 1024 * 1024,
            ..Limits::default()
        };
        let (compiler, registry) = production_compiler(&limits).map_err(Error::Initialization)?;
        let store = SqliteStore::open(
            base.join(&config.database),
            StoreConfig {
                limits: limits.clone(),
                ..StoreConfig::default()
            },
        )
        .await
        .map_err(|e| Error::Initialization(Box::new(e)))?;
        let adapter = adapter(&config.endpoints).map_err(Error::Initialization)?;
        let policy = Arc::new(Policy {
            jobs: RwLock::new(BTreeMap::new()),
            limits: limits.clone(),
        });
        let runtime = Arc::new(
            Runtime::new(
                store.clone(),
                Arc::new(registry),
                policy.clone(),
                RuntimeConfig {
                    limits: limits.clone(),
                    ..RuntimeConfig::default()
                },
            )?
            .with_mcp(adapter.clone()),
        );
        let operator = HostContext::new("context-operator".into(), &limits)?;
        let mut roles = BTreeMap::new();
        let mut deployments = BTreeSet::new();
        for (name, role) in &config.roles {
            if name.is_empty()
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
            {
                return Err(Error::Configuration);
            }
            let source = bounded_file(&base.join(&role.source)).map_err(Error::Initialization)?;
            let catalog =
                bounded_file(&base.join(&role.catalogs)).map_err(Error::Initialization)?;
            let catalogs = MCPCatalogInputs::from_json(&catalog, &limits)
                .map_err(|e| Error::Initialization(Box::new(e)))?;
            let source = std::str::from_utf8(&source).map_err(|_| Error::Configuration)?;
            let compiled = compiler
                .compile_source(source, &catalogs)
                .map_err(|_| Error::Configuration)?;
            let fields = compiled.canonical_document.fields();
            let root = fields
                .scopes
                .get(&fields.root_scope)
                .ok_or(Error::Configuration)?
                .fields();
            for (ports, name) in [(&root.inputs, "request"), (&root.outputs, "result")] {
                let port = ports.get(name).ok_or(Error::Configuration)?;
                if ports.len() != 1
                    || !port.required()
                    || !matches!(
                        port.value_type().kind(),
                        ValueTypeKind::Builtin(BuiltinType::Json)
                    )
                {
                    return Err(Error::Configuration);
                }
            }
            deployments.extend(
                fields
                    .bindings
                    .values()
                    .map(|b| b.server().deployment_id().to_string()),
            );
            runtime
                .register_graph(&operator, &compiled.executable_bytes, &[])
                .await?;
            roles.insert(
                name.clone(),
                AdmittedRole {
                    fingerprint: compiled.fingerprint,
                    document: compiled.canonical_document,
                },
            );
        }
        if roles.is_empty()
            || !config.required_deployments.is_subset(&deployments)
            || deployments
                .iter()
                .any(|id| !config.endpoints.contains_key(id))
        {
            return Err(Error::Configuration);
        }
        for job in restored {
            grant(&policy, &roles, job.clone(), &limits)?;
        }
        let engine = ExecutionEngine::launch(
            runtime.clone(),
            ExecutionConfig {
                workers: config.workers,
                max_runs: config.max_runs,
                ..ExecutionConfig::default()
            },
        )?;
        Ok(Self {
            runtime,
            adapter,
            engine,
            store,
            policy,
            roles,
            limits,
            stopping: AtomicBool::new(false),
        })
    }

    /// Install a currently authorized job grant before starting any role operation.
    ///
    /// # Errors
    /// Rejects unknown roles, changed identity/authority for an existing job and shutdown.
    pub fn grant_job(&self, identity: JobGrant) -> Result<()> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        grant(&self.policy, &self.roles, identity, &self.limits)
    }

    /// Revoke future dispatch/reads immediately; cancellation uses the saved handle.
    ///
    /// # Errors
    /// Rejects unknown jobs and unavailable authorization state.
    pub fn revoke_job(&self, job: JobId) -> Result<()> {
        self.policy
            .jobs
            .write()
            .map_err(|_| Error::Unauthorized)?
            .get_mut(&job)
            .ok_or(Error::Unauthorized)?
            .active = false;
        Ok(())
    }

    /// Return the immutable selected workflow identity for durable job admission.
    ///
    /// # Errors
    /// Rejects roles absent from operator configuration.
    pub fn fingerprint(&self, role: &str) -> Result<String> {
        Ok(self
            .roles
            .get(role)
            .ok_or(Error::Configuration)?
            .fingerprint
            .to_string())
    }

    /// Check exact live descriptors for trusted selected roles before job admission.
    ///
    /// This uses existing HTLK discovery and compares complete descriptor digests.
    /// It does not make model calls or promise that a provider account has quota.
    ///
    /// # Errors
    /// Rejects missing roles, identity/schema drift and unavailable deployments.
    pub async fn ready(&self, roles: &[String]) -> Result<()> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        let context = McpRunContext {
            run_id: RunId::new(format!("readiness:{}", JobId::new()), &self.limits)
                .map_err(|_| Error::Configuration)?,
            host: HostContext::new("context-operator".into(), &self.limits)?,
            fence: 1,
            cancellation: Arc::new(McpCancellation::new()),
        };
        let result = async {
            for role in roles {
                for binding in self
                    .roles
                    .get(role)
                    .ok_or(Error::Configuration)?
                    .document
                    .fields()
                    .bindings
                    .values()
                {
                    let descriptor = self
                        .adapter
                        .descriptor(binding, &context, &self.limits)
                        .await
                        .map_err(|e| Error::Initialization(Box::new(e)))?;
                    if descriptor.digest() != binding.descriptor() {
                        return Err(Error::Configuration);
                    }
                }
            }
            Ok(())
        }
        .await;
        context.cancellation.cancel();
        result
    }

    /// Admit a role run under an authenticated job's fixed provenance/authority.
    ///
    /// Persist the returned handle in the application job record. Reusing the same
    /// key/request targets the same HTLK admission even across process restart.
    ///
    /// # Errors
    /// Rejects revoked jobs, unauthorized roles, nonobject requests and invalid inputs.
    pub async fn start(
        &self,
        job: JobId,
        role: &str,
        request: &serde_json::Value,
        key: &str,
    ) -> Result<Handle> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        if !request.is_object() {
            return Err(Error::Configuration);
        }
        {
            let jobs = self.policy.jobs.read().map_err(|_| Error::Unauthorized)?;
            if !jobs
                .get(&job)
                .is_some_and(|grant| grant.active && grant.identity.roles.contains(role))
            {
                return Err(Error::Unauthorized);
            }
        }
        let role = self.roles.get(role).ok_or(Error::Configuration)?;
        let document = JsonDocument::new(
            &serde_json::to_vec(request).map_err(|_| Error::Configuration)?,
            &self.limits,
        )
        .map_err(|_| Error::Configuration)?;
        let inputs = [(
            "request".parse().map_err(|_| Error::Configuration)?,
            InputValue::Inline(document.value().clone()),
        )]
        .into();
        let started = self
            .runtime
            .start_run(
                &job_context(job, &self.limits)?,
                role.fingerprint,
                &inputs,
                key,
                None,
            )
            .await?;
        Ok(Handle {
            job,
            fingerprint: role.fingerprint.to_string(),
            run_id: started.run_id.to_string(),
        })
    }

    /// Observe a saved handle and read only its authorized accepted result.
    ///
    /// # Errors
    /// Denies cross-job handles, invalid fingerprints, inconsistent output contracts,
    /// or transient fenced observations (callers may boundedly retry E_FENCED).
    pub async fn inspect(&self, handle: &Handle) -> Result<State> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        let context = job_context(handle.job, &self.limits)?;
        let run =
            RunId::new(handle.run_id.clone(), &self.limits).map_err(|_| Error::Configuration)?;
        let view = self.runtime.inspect_run(&context, &run).await?;
        if view.header.fingerprint.to_string() != handle.fingerprint {
            return Err(Error::Unauthorized);
        }
        match view.header.status {
            RunStatus::Running => Ok(State::Running),
            RunStatus::Failed => Ok(State::Failed(
                view.failure.map_or(FailureCode::Dependency, |e| e.code()),
            )),
            RunStatus::Cancelled => Ok(State::Cancelled),
            RunStatus::Succeeded => {
                let Value::Map(outputs) = view.outputs.ok_or(Error::Configuration)? else {
                    return Err(Error::Configuration);
                };
                let Some(Value::Map(entry)) = outputs.get("result") else {
                    return Err(Error::Configuration);
                };
                let Some(Value::Text(id)) = entry.get("artifact_id") else {
                    return Err(Error::Configuration);
                };
                let value = self
                    .runtime
                    .read_artifact(&context, id.parse().map_err(|_| Error::Configuration)?)
                    .await?;
                let value = JsonDocument::from_value(&value.value, &self.limits)
                    .map_err(|_| Error::Configuration)?;
                let value: serde_json::Value =
                    serde_json::from_slice(value.as_bytes()).map_err(|_| Error::Configuration)?;
                if !value.is_object() {
                    return Err(Error::Configuration);
                }
                Ok(State::Succeeded(value))
            }
        }
    }

    /// Cancel an owned run with stable idempotency; saved terminal outcomes win.
    ///
    /// # Errors
    /// Denies cross-job handles and returns existing HTLK cancellation failures.
    pub async fn cancel(&self, handle: &Handle, key: &str) -> Result<()> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        let context = job_context(handle.job, &self.limits)?;
        let run =
            RunId::new(handle.run_id.clone(), &self.limits).map_err(|_| Error::Configuration)?;
        let view = self.runtime.inspect_run(&context, &run).await?;
        if view.header.fingerprint.to_string() != handle.fingerprint {
            return Err(Error::Unauthorized);
        }
        self.runtime
            .cancel_run(&context, &run, "application requested cancellation", key)
            .await?;
        Ok(())
    }

    /// Stop admission/workers and release native sessions, preserving durable records.
    ///
    /// The application drains request/worker ownership before invoking shutdown.
    ///
    /// # Errors
    /// Returns shutdown/connection-owner errors from the existing public interfaces.
    pub async fn shutdown(&self) -> Result<()> {
        self.stopping.store(true, Ordering::Release);
        self.engine.shutdown().await?;
        self.adapter
            .shutdown()
            .await
            .map_err(|e| Error::Initialization(Box::new(e)))?;
        self.store
            .clone()
            .close()
            .await
            .map_err(|e| Error::Initialization(Box::new(e)))?;
        Ok(())
    }
}

fn grant(
    policy: &Policy,
    roles: &BTreeMap<String, AdmittedRole>,
    identity: JobGrant,
    limits: &Limits,
) -> Result<()> {
    if identity.roles.is_empty() {
        return Err(Error::Unauthorized);
    }
    let mut graphs = BTreeSet::new();
    let mut bindings = BTreeSet::new();
    for role in &identity.roles {
        let role = roles.get(role).ok_or(Error::Configuration)?;
        graphs.insert(role.fingerprint);
        for binding in role.document.fields().bindings.values() {
            bindings.insert(binding.digest(limits).map_err(|_| Error::Configuration)?);
        }
    }
    let mut jobs = policy.jobs.write().map_err(|_| Error::Unauthorized)?;
    match jobs.get(&identity.job) {
        Some(existing) if existing.identity == identity && existing.active => Ok(()),
        Some(_) => Err(Error::Unauthorized),
        None => {
            jobs.insert(
                identity.job,
                AuthorizedJob {
                    identity,
                    graphs,
                    bindings,
                    active: true,
                },
            );
            Ok(())
        }
    }
}

fn bounded_file(path: &Path) -> crate::Result<Vec<u8>> {
    if std::fs::metadata(path)?.len() > 16 * 1024 * 1024 {
        return Err("operator file exceeds bound".into());
    }
    let bytes = std::fs::read(path)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("operator file exceeds bound".into());
    }
    Ok(bytes)
}

fn production_compiler(limits: &Limits) -> crate::Result<(Compiler, NativeRegistry)> {
    let policy = PolicyDocument::new(
        PolicyFields {
            cost_unit: "credits".into(),
            defaults: ExecutionLimits::new()
                .with_timeout_ms(30 * 60 * 1000)?
                .with_attempt_timeout_ms(120_000)?
                .with_max_concurrency(4)?,
            evaluator_limits: EvaluatorLimits {
                max_expression_depth: 64,
                max_value_bytes: 4 * 1024 * 1024,
                max_collection_visits: 100_000,
                max_regex_bytes: 4096,
                max_regex_compiled_bytes: 65_536,
                max_output_bytes: 4 * 1024 * 1024,
                max_steps: 64_000_000,
            },
            maximum_scope_depth: 32,
            maximum_expanded_nodes: 10_000,
        },
        limits,
    )?;
    let mut registry = NativeRegistry::new(limits)?;
    let profile = registry.link_policy(&policy)?;
    Ok((
        Compiler::new(CompilerConfig {
            profile,
            policy,
            libraries: BTreeMap::new(),
            limits: limits.clone(),
            frontend: FrontendLimits::default(),
            schema_options: analyzer::NativeSchemaOptions::default(),
            capabilities: CompilerCapabilities::default(),
        })?,
        registry,
    ))
}
