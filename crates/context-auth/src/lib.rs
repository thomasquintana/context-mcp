//! Provider-neutral API authentication with scoped, revocable service credentials.
//!
//! Human access tokens use trusted OIDC discovery with JWT/JWKS verification or
//! RFC7662 introspection. Resource grants and tenant memberships stay in PostgreSQL.
//! A token's tenant-like claims never create resource authority.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use context_core::{
    access::{Authentication, Permission},
    ids::{CredentialId, PrincipalId},
};
use context_storage::postgres::Database;
use futures_util::TryStreamExt;
use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{Jwk, JwkSet, KeyOperations, PublicKeyUse},
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Stable authentication failures, without token, header or upstream-body exposure.
#[derive(thiserror::Error)]
pub enum Error {
    /// Missing, malformed, expired, revoked or otherwise invalid credential.
    #[error("invalid or unavailable credentials")]
    Unauthorized,
    /// Trusted authentication configuration is invalid.
    #[error("invalid authentication configuration")]
    Configuration,
    /// Trusted identity infrastructure is temporarily unavailable.
    #[error("identity provider is unavailable")]
    Unavailable,
    /// Application identity persistence failed.
    #[error("identity persistence failed")]
    Storage(#[from] context_storage::Error),
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AuthenticationError({self})")
    }
}

/// Authentication result.
pub type Result<T> = std::result::Result<T, Error>;

/// Standard resource-token verification mode.
#[derive(Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Verification {
    /// Verify asymmetric signatures using the configured issuer's discovered JWKS.
    Jwks,
    /// Verify opaque or JWT tokens using the issuer's RFC7662 endpoint.
    Introspection {
        /// Registered resource-server client ID.
        client_id: String,
        /// Environment variable containing its client secret.
        client_secret_env: String,
    },
}

/// Trusted issuer configuration; values come from operators, never token headers.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OidcConfig {
    /// Exact trusted issuer identifier, including its configured path.
    pub issuer: String,
    /// Audience/resource identifier dedicated to this API, not a browser client ID.
    pub audience: String,
    /// Map issuer access scopes to effective application capability ceilings.
    pub scope_map: BTreeMap<String, Permission>,
    /// Additional exact access-token markers, e.g. `typ=Bearer` or `token_use=access`.
    #[serde(default)]
    pub required_claims: BTreeMap<String, Value>,
    /// JWT/JWKS or standard introspection.
    pub verification: Verification,
    /// Explicit plain-HTTP opt-in for the local development/test issuer.
    #[serde(default)]
    pub allow_http: bool,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: Option<String>,
    introspection_endpoint: Option<String>,
}

struct Cache {
    keys: JwkSet,
    fetched: Instant,
    attempted: Instant,
}

struct Oidc {
    config: OidcConfig,
    discovery: Discovery,
    client: reqwest::Client,
    cache: Mutex<Cache>,
    introspection_secret: Option<SecretString>,
}

/// A generated credential. The secret can be deliberately returned once, never logged.
pub struct IssuedCredential {
    /// Stable nonsecret credential identifier used for revocation.
    pub id: CredentialId,
    /// Service account identity.
    pub principal: PrincipalId,
    secret: SecretString,
}

impl IssuedCredential {
    /// Expose once to an authenticated operator/client over its intended secret channel.
    pub fn secret(&self) -> &str {
        self.secret.expose_secret()
    }
}

/// Cloneable authenticated-identity boundary shared across all server requests.
#[derive(Clone)]
pub struct Authenticator {
    database: Database,
    oidc: Option<Arc<Oidc>>,
}

impl Authenticator {
    /// Initialize trusted discovery/JWKS or introspection, with bounded HTTP and no redirects.
    ///
    /// `None` permits only service credentials (e.g. stdio); it does not disable
    /// authentication or create a default administrator.
    ///
    /// # Errors
    /// Rejects malformed issuers/metadata, unavailable keys and missing secret references.
    pub async fn open(database: Database, config: Option<OidcConfig>) -> Result<Self> {
        let oidc = match config {
            None => None,
            Some(config) => {
                validate_url(&config.issuer, config.allow_http)?;
                if config.audience.is_empty() || config.scope_map.is_empty() {
                    return Err(Error::Configuration);
                }
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(10))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|_| Error::Configuration)?;
                let discovery: Discovery = bounded_json(
                    client
                        .get(format!(
                            "{}/.well-known/openid-configuration",
                            config.issuer.trim_end_matches('/')
                        ))
                        .send()
                        .await
                        .map_err(|_| Error::Unavailable)?,
                )
                .await?;
                if discovery.issuer != config.issuer {
                    return Err(Error::Configuration);
                }
                let (keys, secret) = match &config.verification {
                    Verification::Jwks => {
                        let uri = discovery.jwks_uri.as_deref().ok_or(Error::Configuration)?;
                        validate_url(uri, config.allow_http)?;
                        let keys = bounded_json(
                            client
                                .get(uri)
                                .send()
                                .await
                                .map_err(|_| Error::Unavailable)?,
                        )
                        .await?;
                        (keys, None)
                    }
                    Verification::Introspection {
                        client_id,
                        client_secret_env,
                    } => {
                        if client_id.is_empty() {
                            return Err(Error::Configuration);
                        }
                        validate_url(
                            discovery
                                .introspection_endpoint
                                .as_deref()
                                .ok_or(Error::Configuration)?,
                            config.allow_http,
                        )?;
                        let secret =
                            std::env::var(client_secret_env).map_err(|_| Error::Configuration)?;
                        if secret.is_empty() {
                            return Err(Error::Configuration);
                        }
                        (
                            JwkSet { keys: Vec::new() },
                            Some(SecretString::from(secret)),
                        )
                    }
                };
                Some(Arc::new(Oidc {
                    config,
                    discovery,
                    client,
                    cache: Mutex::new(Cache {
                        keys,
                        fetched: Instant::now(),
                        attempted: Instant::now(),
                    }),
                    introspection_secret: secret,
                }))
            }
        };
        Ok(Self { database, oidc })
    }

    /// Authenticate one request's bearer value without accepting tenant claims as grants.
    ///
    /// # Errors
    /// Rejects malformed, expired, revoked, wrong-issuer/audience or scope-less tokens.
    pub async fn authenticate(&self, bearer: &str) -> Result<Authentication> {
        if bearer.is_empty() || bearer.len() > 16_384 {
            return Err(Error::Unauthorized);
        }
        if bearer.starts_with("ctx_") {
            return self.service(bearer).await;
        }
        let oidc = self.oidc.as_ref().ok_or(Error::Unauthorized)?;
        let claims = match &oidc.config.verification {
            Verification::Jwks => oidc.jwt(bearer).await?,
            Verification::Introspection { client_id, .. } => {
                let endpoint = oidc
                    .discovery
                    .introspection_endpoint
                    .as_deref()
                    .ok_or(Error::Configuration)?;
                let secret = oidc
                    .introspection_secret
                    .as_ref()
                    .ok_or(Error::Configuration)?;
                let response = oidc
                    .client
                    .post(endpoint)
                    .basic_auth(client_id, Some(secret.expose_secret()))
                    .form(&[("token", bearer), ("token_type_hint", "access_token")])
                    .send()
                    .await
                    .map_err(|_| Error::Unavailable)?;
                let claims: Value = bounded_json(response).await?;
                if claims["active"] != true
                    || !audience_matches(&claims["aud"], &oidc.config.audience)
                {
                    return Err(Error::Unauthorized);
                }
                if claims
                    .get("iss")
                    .is_some_and(|issuer| issuer != &Value::String(oidc.config.issuer.clone()))
                    || claims["exp"]
                        .as_u64()
                        .is_none_or(|expires| expires <= now_seconds().unwrap_or(u64::MAX))
                {
                    return Err(Error::Unauthorized);
                }
                claims
            }
        };
        let scopes = access_scopes(&claims, &oidc.config)?;
        let subject = claims["sub"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 1024)
            .ok_or(Error::Unauthorized)?;
        let principal = self
            .database
            .identity(&oidc.config.issuer, subject, "human")
            .await?;
        Ok(Authentication {
            principal,
            scopes,
            credential: None,
        })
    }

    async fn service(&self, bearer: &str) -> Result<Authentication> {
        let encoded = bearer.strip_prefix("ctx_").ok_or(Error::Unauthorized)?;
        if encoded.len() != 43
            || URL_SAFE_NO_PAD
                .decode(encoded)
                .map_err(|_| Error::Unauthorized)?
                .len()
                != 32
        {
            return Err(Error::Unauthorized);
        }
        let digest = Sha256::digest(bearer.as_bytes());
        let row=sqlx::query("SELECT c.id,c.principal,c.scopes FROM context.credentials c JOIN context.principals p ON p.id=c.principal WHERE c.digest=$1 AND NOT c.revoked AND c.expires_at>now() AND p.kind='service'")
            .bind(digest.as_slice()).fetch_optional(self.database.pool()).await.map_err(context_storage::Error::from)?
            .ok_or(Error::Unauthorized)?;
        let scopes = row
            .try_get::<Vec<String>, _>("scopes")
            .map_err(context_storage::Error::from)?
            .into_iter()
            .map(|scope| {
                serde_json::from_value(Value::String(scope)).map_err(|_| Error::Unauthorized)
            })
            .collect::<Result<BTreeSet<Permission>>>()?;
        if scopes.is_empty() {
            return Err(Error::Unauthorized);
        }
        Ok(Authentication {
            principal: row
                .try_get::<Uuid, _>("principal")
                .map_err(context_storage::Error::from)?
                .into(),
            credential: Some(
                row.try_get::<Uuid, _>("id")
                    .map_err(context_storage::Error::from)?
                    .into(),
            ),
            scopes,
        })
    }

    /// Issue a scoped, finite-lived credential for an existing service account.
    ///
    /// This is an operator/application command, not an unauthenticated issuance API.
    /// Only a SHA-256 digest is stored; resource grants remain independently required.
    ///
    /// # Errors
    /// Rejects human principals, empty scopes, excessive lifetimes and entropy/storage failures.
    pub async fn issue_service(
        &self,
        principal: PrincipalId,
        scopes: &BTreeSet<Permission>,
        lifetime: Duration,
    ) -> Result<IssuedCredential> {
        if scopes.is_empty()
            || lifetime.as_secs() == 0
            || lifetime > Duration::from_secs(90 * 24 * 3600)
        {
            return Err(Error::Configuration);
        }
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(|_| Error::Unavailable)?;
        let secret = SecretString::from(format!("ctx_{}", URL_SAFE_NO_PAD.encode(random)));
        let digest = Sha256::digest(secret.expose_secret().as_bytes());
        let id = CredentialId::new();
        let scopes = scopes
            .iter()
            .map(|scope| {
                serde_json::to_value(scope)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .ok_or(Error::Configuration)
            })
            .collect::<Result<Vec<_>>>()?;
        let result=sqlx::query("INSERT INTO context.credentials(id,principal,digest,scopes,expires_at) SELECT $1,id,$3,$4,now()+($5::bigint*interval '1 second') FROM context.principals WHERE id=$2 AND kind='service'")
            .bind(Uuid::from(id)).bind(Uuid::from(principal)).bind(digest.as_slice()).bind(scopes)
            .bind(lifetime.as_secs() as i64).execute(self.database.pool()).await.map_err(context_storage::Error::from)?;
        if result.rows_affected() != 1 {
            return Err(Error::Unauthorized);
        }
        Ok(IssuedCredential {
            id,
            principal,
            secret,
        })
    }

    /// Revoke an owned service credential; future requests and protected reads recheck it.
    ///
    /// # Errors
    /// Returns a persistence failure. Missing/nonowned IDs do not disclose existence.
    pub async fn revoke_service(&self, principal: PrincipalId, id: CredentialId) -> Result<()> {
        sqlx::query("UPDATE context.credentials SET revoked=true WHERE principal=$1 AND id=$2")
            .bind(Uuid::from(principal))
            .bind(Uuid::from(id))
            .execute(self.database.pool())
            .await
            .map_err(context_storage::Error::from)?;
        Ok(())
    }
}

impl Oidc {
    async fn jwt(&self, token: &str) -> Result<Value> {
        let header = decode_header(token).map_err(|_| Error::Unauthorized)?;
        if !matches!(
            header.alg,
            Algorithm::RS256 | Algorithm::ES256 | Algorithm::EdDSA
        ) || header.typ.as_ref().is_some_and(|typ| {
            !matches!(
                typ.to_ascii_lowercase().as_str(),
                "jwt" | "at+jwt" | "application/at+jwt"
            )
        }) || header.kid.as_ref().is_some_and(|kid| kid.len() > 256)
        {
            return Err(Error::Unauthorized);
        }
        let mut cache = self.cache.lock().await;
        let missing = find_key(&cache.keys, header.kid.as_deref()).is_none();
        if (cache.fetched.elapsed() > Duration::from_secs(300) || missing)
            && cache.attempted.elapsed() > Duration::from_secs(10)
        {
            cache.attempted = Instant::now();
            let uri = self
                .discovery
                .jwks_uri
                .as_deref()
                .ok_or(Error::Configuration)?;
            cache.keys = bounded_json(
                self.client
                    .get(uri)
                    .send()
                    .await
                    .map_err(|_| Error::Unavailable)?,
            )
            .await?;
            cache.fetched = Instant::now();
        }
        if cache.fetched.elapsed() > Duration::from_secs(300) {
            return Err(Error::Unavailable);
        }
        let key = find_key(&cache.keys, header.kid.as_deref()).ok_or(Error::Unauthorized)?;
        if key
            .common
            .public_key_use
            .as_ref()
            .is_some_and(|purpose| purpose != &PublicKeyUse::Signature)
            || key
                .common
                .key_operations
                .as_ref()
                .is_some_and(|operations| !operations.contains(&KeyOperations::Verify))
            || key
                .common
                .key_algorithm
                .is_some_and(|algorithm| Algorithm::try_from(algorithm).ok() != Some(header.alg))
        {
            return Err(Error::Unauthorized);
        }
        let key = DecodingKey::from_jwk(key).map_err(|_| Error::Unauthorized)?;
        let mut validation = Validation::new(header.alg);
        validation.set_audience(&[&self.config.audience]);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_required_spec_claims(&["iss", "sub", "aud", "exp"]);
        validation.validate_nbf = true;
        validation.leeway = 30;
        Ok(decode::<Value>(token, &key, &validation)
            .map_err(|_| Error::Unauthorized)?
            .claims)
    }
}

fn find_key<'a>(keys: &'a JwkSet, kid: Option<&str>) -> Option<&'a Jwk> {
    match kid {
        Some(kid) => {
            let mut matching = keys
                .keys
                .iter()
                .filter(|key| key.common.key_id.as_deref() == Some(kid));
            let key = matching.next()?;
            if matching.next().is_some() {
                None
            } else {
                Some(key)
            }
        }
        None if keys.keys.len() == 1 => keys.keys.first(),
        None => None,
    }
}

fn access_scopes(claims: &Value, config: &OidcConfig) -> Result<BTreeSet<Permission>> {
    if claims
        .get("token_use")
        .is_some_and(|value| value != "access")
        || claims
            .get("typ")
            .is_some_and(|value| value != "Bearer" && value != "access")
        || config
            .required_claims
            .iter()
            .any(|(name, value)| claims.get(name) != Some(value))
    {
        return Err(Error::Unauthorized);
    }
    let mut names = BTreeSet::new();
    if let Some(scope) = claims.get("scope").and_then(Value::as_str) {
        names.extend(scope.split_whitespace().map(String::from));
    }
    if let Some(scopes) = claims.get("scp").and_then(Value::as_array) {
        for scope in scopes {
            names.insert(scope.as_str().ok_or(Error::Unauthorized)?.into());
        }
    }
    let scopes = names
        .iter()
        .filter_map(|name| config.scope_map.get(name).copied())
        .collect::<BTreeSet<_>>();
    if scopes.is_empty() {
        return Err(Error::Unauthorized);
    }
    Ok(scopes)
}

fn audience_matches(actual: &Value, expected: &str) -> bool {
    actual.as_str() == Some(expected)
        || actual
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(expected)))
}

fn now_seconds() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Unavailable)?
        .as_secs())
}

fn validate_url(value: &str, allow_http: bool) -> Result<()> {
    let value = reqwest::Url::parse(value).map_err(|_| Error::Configuration)?;
    if value.host_str().is_none()
        || !value.username().is_empty()
        || value.password().is_some()
        || value.query().is_some()
        || value.fragment().is_some()
        || !(value.scheme() == "https" || (allow_http && value.scheme() == "http"))
    {
        return Err(Error::Configuration);
    }
    Ok(())
}

async fn bounded_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > 1024 * 1024)
    {
        return Err(Error::Unavailable);
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.try_next().await.map_err(|_| Error::Unavailable)? {
        if bytes
            .len()
            .checked_add(chunk.len())
            .is_none_or(|size| size > 1024 * 1024)
        {
            return Err(Error::Unavailable);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> OidcConfig {
        OidcConfig {
            issuer: "https://issuer.example".into(),
            audience: "https://context.example/mcp".into(),
            scope_map: BTreeMap::from([("context:read".into(), Permission::Read)]),
            required_claims: BTreeMap::new(),
            verification: Verification::Jwks,
            allow_http: false,
        }
    }

    #[test]
    fn id_refresh_and_unmapped_scopes_do_not_authenticate_as_access_tokens() {
        let config = config();
        assert!(matches!(
            access_scopes(&json!({"token_use":"id","scope":"context:read"}), &config),
            Err(Error::Unauthorized)
        ));
        assert!(matches!(
            access_scopes(&json!({"typ":"Refresh","scope":"context:read"}), &config),
            Err(Error::Unauthorized)
        ));
        assert!(matches!(
            access_scopes(&json!({"scope":"openid profile"}), &config),
            Err(Error::Unauthorized)
        ));
        assert_eq!(
            access_scopes(
                &json!({"scp":["context:read"],"tenant":"untrusted"}),
                &config
            )
            .unwrap(),
            BTreeSet::from([Permission::Read])
        );
    }

    #[test]
    fn insecure_or_credential_bearing_issuer_urls_fail_configuration() {
        assert!(validate_url("http://issuer.example", false).is_err());
        assert!(validate_url("https://user:secret@issuer.example", false).is_err());
        assert!(validate_url("https://issuer.example?secret=value", false).is_err());
        assert!(validate_url("http://127.0.0.1:8080/realms/test", true).is_ok());
        assert!(audience_matches(&json!(["other", "context"]), "context"));
        assert!(!audience_matches(&json!("other"), "context"));
    }
}
