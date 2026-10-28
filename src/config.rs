//! Operator configuration and strict startup-file parsing.

use context_engine::Config as ApplicationConfig;
use serde::Deserialize;
use std::{net::SocketAddr, path::Path};

/// Operator-only server configuration. All credentials are environment references.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Maximum concurrently executing HTTP requests (including authentication).
    #[serde(default = "default_concurrency")]
    pub max_concurrent_requests: usize,
    /// Core application, authority and workflow configuration.
    pub application: ApplicationConfig,
    /// Local HTTP listen address.
    pub bind: SocketAddr,
    /// Public MCP resource URL advertised in authorization metadata.
    pub public_url: String,
    /// Explicit hosts accepted by the standard SDK transport.
    pub allowed_hosts: Vec<String>,
    /// Explicit browser origins; absent Origin remains valid for native clients.
    pub allowed_origins: Vec<String>,
    /// Environment variable containing the service credential for stdio.
    pub stdio_token_env: String,
    /// Upper bound on JSON-RPC frame/request bodies.
    pub max_request_bytes: usize,
    /// Finite request processing deadline, including incomplete HTTP bodies.
    #[serde(default = "default_timeout")]
    pub request_timeout_ms: u64,
}

fn default_timeout() -> u64 {
    180_000
}
fn default_concurrency() -> usize {
    128
}

impl Config {
    /// Read a trusted, size-bounded JSON file through HTLK's strict JSON parser.
    ///
    /// # Errors
    /// Rejects duplicate members, unknown configuration fields and invalid limits/URLs.
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 1024 * 1024 {
            return Err("configuration exceeds bound".into());
        }
        let json = context_workflows::host_json(&bytes)?;
        let config: Self = serde_json::from_slice(&json)?;
        let url = axum::http::Uri::try_from(config.public_url.as_str())?;
        if !matches!(url.scheme_str(), Some("http" | "https"))
            || url.authority().is_none()
            || url.path() != "/mcp"
            || url.query().is_some()
            || url
                .authority()
                .is_some_and(|authority| authority.as_str().contains('@'))
            || config.allowed_hosts.is_empty()
            || !(16_384..=4 * 1024 * 1024).contains(&config.max_request_bytes)
            || !(1000..=600_000).contains(&config.request_timeout_ms)
            || !(1..=1024).contains(&config.max_concurrent_requests)
        {
            return Err("invalid server configuration".into());
        }
        for origin in &config.allowed_origins {
            if origin == "null" {
                continue;
            }
            let origin = url::Url::parse(origin)?;
            if !matches!(origin.scheme(), "https" | "http")
                || origin.host().is_none()
                || !origin.username().is_empty()
                || origin.password().is_some()
                || origin.query().is_some()
                || origin.fragment().is_some()
                || origin.path() != "/"
            {
                return Err("invalid browser origin".into());
            }
        }
        Ok(config)
    }
}
