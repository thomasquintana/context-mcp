//! Authenticated Streamable HTTP and explicitly identified stdio lifecycles.

use crate::{config::Config, mcp::McpServer};
use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use context_engine::Application;
use rmcp::{
    ServiceExt,
    transport::{
        async_rw::AsyncRwTransport,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};
use secrecy::SecretString;
use serde_json::json;
use std::{path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;

/// Server lifecycle error; diagnostics omit credential values and application data.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct WebState {
    application: Arc<Application>,
    metadata_url: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Finalize {
    idempotency_key: String,
}

fn application_response(result: context_engine::Result<serde_json::Value>) -> Response {
    match result {
        Ok(value) => (StatusCode::OK, axum::Json(value)).into_response(),
        Err(error) => {
            let (status, code) = match error {
                context_engine::Error::Invalid
                | context_engine::Error::Storage(context_storage::Error::Invalid) => {
                    (StatusCode::BAD_REQUEST, "invalid_input")
                }
                context_engine::Error::Storage(context_storage::Error::Unavailable) => {
                    (StatusCode::NOT_FOUND, "unavailable")
                }
                context_engine::Error::Storage(context_storage::Error::Conflict) => {
                    (StatusCode::CONFLICT, "conflict")
                }
                context_engine::Error::MissingCapability => {
                    (StatusCode::SERVICE_UNAVAILABLE, "missing_capability")
                }
                context_engine::Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "overloaded"),
                _ => (StatusCode::SERVICE_UNAVAILABLE, "operation_failed"),
            };
            (
                status,
                axum::Json(json!({"error":{"code":code,"message":error.to_string()}})),
            )
                .into_response()
        }
    }
}

async fn authenticate(
    State(state): State<Arc<WebState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(kind, token)| kind.eq_ignore_ascii_case("bearer") && !token.is_empty())
        .map(|(_, token)| token);
    let result = match token {
        Some(token) => state.application.authentication().authenticate(token).await,
        None => Err(context_auth::Error::Unauthorized),
    };
    match result {
        Ok(actor) => {
            request.extensions_mut().insert(actor);
            next.run(request).await
        }
        Err(error) => {
            let status = if matches!(
                error,
                context_auth::Error::Unavailable | context_auth::Error::Storage(_)
            ) {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::UNAUTHORIZED
            };
            let mut response = (
                status,
                axum::Json(json!({"error":"authentication_required"})),
            )
                .into_response();
            if let Ok(value) = HeaderValue::from_str(&format!(
                "Bearer resource_metadata=\"{}\"",
                state.metadata_url
            )) {
                response
                    .headers_mut()
                    .insert(header::WWW_AUTHENTICATE, value);
            }
            response
        }
    }
}

/// Run one application instance until transport closure or an OS termination signal.
///
/// # Errors
/// Returns configuration, authentication, binding or transport failures.
pub async fn serve(config: Config, base: &Path, stdio: bool) -> Result<()> {
    let application = Arc::new(Application::open(&config.application, base).await?);
    application.start_workers().await?;
    let shutdown = CancellationToken::new();
    let signal = shutdown.clone();
    let signals = tokio::spawn(async move {
        #[cfg(unix)]
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
            signal.cancel();
            return;
        }
        let _ = tokio::signal::ctrl_c().await;
        signal.cancel();
    });
    let result = if stdio {
        serve_stdio(&config, application.clone(), shutdown.clone()).await
    } else {
        serve_http(&config, application.clone(), shutdown.clone()).await
    };
    signals.abort();
    let closed = application.shutdown().await;
    closed?;
    result
}

async fn serve_stdio(
    config: &Config,
    application: Arc<Application>,
    shutdown: CancellationToken,
) -> Result<()> {
    let token = std::env::var(&config.stdio_token_env)
        .map_err(|_| "explicit stdio service token is required")?;
    let token = SecretString::from(token);
    use secrecy::ExposeSecret;
    application
        .authentication()
        .authenticate(token.expose_secret())
        .await?;
    let handler = McpServer::new(application, Some(token));
    let transport = AsyncRwTransport::new_server(tokio::io::stdin(), tokio::io::stdout())
        .with_max_line_length(config.max_request_bytes);
    let service = tokio::select! {
        service=handler.serve(transport)=>service.map_err(|_|"MCP initialization failed")?,
        _=shutdown.cancelled()=>return Ok(()),
    };
    let cancel = service.cancellation_token();
    let waiting = service.waiting();
    tokio::pin!(waiting);
    tokio::select! {
        result=&mut waiting=>{result.map_err(|_|"MCP stdio transport failed")?;},
        _=shutdown.cancelled()=>{cancel.cancel();waiting.await.map_err(|_|"MCP stdio shutdown failed")?;},
    }
    Ok(())
}

async fn serve_http(
    config: &Config,
    application: Arc<Application>,
    shutdown: CancellationToken,
) -> Result<()> {
    let sdk_origins = config
        .allowed_origins
        .iter()
        .map(|origin| -> Result<String> {
            if origin == "null" {
                return Ok(origin.clone());
            }
            let origin = url::Url::parse(origin)?;
            let host = origin.host_str().ok_or("missing origin host")?;
            let host = if host.contains(':') && !host.starts_with('[') {
                format!("[{host}]")
            } else {
                host.to_string()
            };
            Ok(format!(
                "{}://{}:{}",
                origin.scheme(),
                host,
                origin
                    .port_or_known_default()
                    .ok_or("missing origin port")?
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut http = StreamableHttpServerConfig::default()
        .with_allowed_hosts(config.allowed_hosts.clone())
        .with_allowed_origins(sdk_origins)
        .enforce_origin_validation();
    http.legacy_session_mode = false;
    http.json_response = true;
    http.max_request_body_bytes = config.max_request_bytes;
    http.cancellation_token = shutdown.clone();
    http.sse_keep_alive = None;
    http.sse_retry = None;
    let factory = application.clone();
    let service = StreamableHttpService::new(
        move || Ok(McpServer::new(factory.clone(), None)),
        Arc::new(LocalSessionManager::default()),
        http,
    );
    let public = axum::http::Uri::try_from(config.public_url.as_str())?;
    let metadata_url = format!(
        "{}://{}/.well-known/oauth-protected-resource/mcp",
        public.scheme_str().ok_or("missing public scheme")?,
        public.authority().ok_or("missing public authority")?
    );
    let state = Arc::new(WebState {
        application: application.clone(),
        metadata_url,
    });
    let uploads = application.clone();
    let finalize = application.clone();
    let jobs = application.clone();
    let protected = Router::new()
        .nest_service("/mcp", service)
        .route("/uploads",post(move|axum::Extension(actor):axum::Extension<context_core::access::Authentication>,axum::Json(args):axum::Json<context_engine::uploads::CreateUpload>|{
            let application=uploads.clone();async move{application_response(application.create_upload(&actor,args).await)}
        }))
        .route("/uploads/{id}/complete",post(move|axum::Extension(actor):axum::Extension<context_core::access::Authentication>,axum::extract::Path(id):axum::extract::Path<uuid::Uuid>,axum::Json(args):axum::Json<Finalize>|{
            let application=finalize.clone();async move{application_response(application.finalize_upload(&actor,id,&args.idempotency_key).await)}
        }))
        .route("/jobs/{id}",get(move|axum::Extension(actor):axum::Extension<context_core::access::Authentication>,axum::extract::Path(id):axum::extract::Path<context_core::ids::JobId>|{
            let application=jobs.clone();async move{application_response(application.job_status(&actor,id).await)}
        }))
        .layer(axum::extract::DefaultBodyLimit::max(config.max_request_bytes))
        .route_layer(middleware::from_fn_with_state(state, authenticate))
        .layer(tower::limit::ConcurrencyLimitLayer::new(config.max_concurrent_requests));
    let issuer = config
        .application
        .oidc
        .as_ref()
        .map(|issuer| issuer.issuer.clone());
    let scopes = config
        .application
        .oidc
        .as_ref()
        .map(|issuer| issuer.scope_map.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let metadata = json!({"resource":config.public_url,"authorization_servers":issuer.into_iter().collect::<Vec<_>>(),"scopes_supported":scopes,"bearer_methods_supported":["header"],"resource_name":"context-mcp"});
    let ready = shutdown.clone();
    let origins = config
        .allowed_origins
        .iter()
        .map(|origin| -> Result<HeaderValue> {
            let origin = if origin == "null" {
                origin.clone()
            } else {
                url::Url::parse(origin)?.origin().ascii_serialization()
            };
            Ok(HeaderValue::from_str(&origin)?)
        })
        .collect::<Result<Vec<_>>>()?;
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::list(origins))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            axum::http::HeaderName::from_static("mcp-protocol-version"),
            axum::http::HeaderName::from_static("mcp-session-id"),
            axum::http::HeaderName::from_static("last-event-id"),
        ])
        .expose_headers([
            header::WWW_AUTHENTICATE,
            axum::http::HeaderName::from_static("mcp-session-id"),
        ]);
    let router = protected
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(move || {
                let metadata = metadata.clone();
                async move { axum::Json(metadata) }
            }),
        )
        .route("/health/live", get(|| async { StatusCode::OK }))
        .route(
            "/health/ready",
            get(move || {
                let ready = ready.clone();
                async move {
                    if ready.is_cancelled() {
                        StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        StatusCode::OK
                    }
                }
            }),
        )
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            std::time::Duration::from_millis(config.request_timeout_ms),
        ))
        .layer(cors);
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(address=%listener.local_addr()?,"context-mcp HTTP ready");
    let stopping = shutdown.clone();
    let server = async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(stopping.cancelled_owned())
            .await
    };
    tokio::pin!(server);
    tokio::select! {
        result=&mut server=>{result?;},
        _=shutdown.cancelled()=>{
            match tokio::time::timeout(std::time::Duration::from_secs(15),&mut server).await {
                Ok(result)=>{result?;},
                Err(_)=>return Err("HTTP shutdown deadline exceeded".into()),
            }
        }
    }
    Ok(())
}
