//! Actual PostgreSQL and Keycloak acceptance; tokens/secrets are never printed.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use context_auth::{Authenticator, Error, OidcConfig, Verification};
use context_core::access::Permission;
use context_storage::postgres::Database;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use uuid::Uuid;

async fn database() -> Database {
    Database::connect(
        &std::env::var("CONTEXT_TEST_DATABASE_URL").expect("isolated test database"),
        5,
    )
    .await
    .unwrap()
}

fn config(issuer: String) -> OidcConfig {
    OidcConfig {
        issuer,
        audience: "http://localhost:8089/mcp".into(),
        scope_map: BTreeMap::from([
            ("context:read".into(), Permission::Read),
            ("context:ingest".into(), Permission::Ingest),
            ("context:memory:write".into(), Permission::WriteMemory),
            ("context:synthesize".into(), Permission::Synthesize),
            ("context:share".into(), Permission::Share),
            ("context:purge".into(), Permission::Purge),
        ]),
        required_claims: BTreeMap::from([("typ".into(), json!("Bearer"))]),
        verification: Verification::Jwks,
        allow_http: true,
    }
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn scoped_credentials_are_hashed_owned_revocable_and_not_automatic_resource_grants() {
    let db = database().await;
    let auth = Authenticator::open(db.clone(), None).await.unwrap();
    let principal = db
        .identity("context-service", &Uuid::now_v7().to_string(), "service")
        .await
        .unwrap();
    let issued = auth
        .issue_service(
            principal,
            &BTreeSet::from([Permission::Read]),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
    let accepted = auth.authenticate(issued.secret()).await.unwrap();
    assert_eq!(accepted.principal, principal);
    assert_eq!(accepted.credential, Some(issued.id));
    assert_eq!(accepted.scopes, BTreeSet::from([Permission::Read]));
    assert!(
        db.access_context(principal, accepted.scopes.clone())
            .await
            .unwrap()
            .tenants
            .is_empty()
    );
    let stored: Vec<u8> = sqlx::query_scalar("SELECT digest FROM context.credentials WHERE id=$1")
        .bind(Uuid::from(issued.id))
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(stored, Sha256::digest(issued.secret().as_bytes()).to_vec());
    let stranger = db
        .identity("context-service", &Uuid::now_v7().to_string(), "service")
        .await
        .unwrap();
    auth.revoke_service(stranger, issued.id).await.unwrap();
    assert!(auth.authenticate(issued.secret()).await.is_ok());
    auth.revoke_service(principal, issued.id).await.unwrap();
    assert!(matches!(
        auth.authenticate(issued.secret()).await,
        Err(Error::Unauthorized)
    ));
    let human = db
        .identity("test-issuer", &Uuid::now_v7().to_string(), "human")
        .await
        .unwrap();
    assert!(matches!(
        auth.issue_service(
            human,
            &BTreeSet::from([Permission::Read]),
            Duration::from_secs(60)
        )
        .await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        auth.authenticate("invalid.jwt.value").await,
        Err(Error::Unauthorized)
    ));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires actual development Keycloak issuer and isolated PostgreSQL"]
async fn real_oidc_jwks_and_introspection_reject_id_tokens_wrong_audience_and_tampering() {
    let db = database().await;
    let issuer = std::env::var("CONTEXT_TEST_OIDC_ISSUER").expect("actual Keycloak issuer");
    let secret =
        std::env::var("CONTEXT_TEST_OIDC_CLIENT_SECRET").expect("public development client secret");
    let response = reqwest::Client::new()
        .post(format!("{issuer}/protocol/openid-connect/token"))
        .form(&[
            ("grant_type", "password"),
            ("client_id", "context-development-client"),
            ("client_secret", secret.as_str()),
            ("username", "developer"),
            ("password", "context-development-password"),
            ("scope", "openid context:read"),
        ])
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "development issuer token request failed"
    );
    let tokens: Value = response.json().await.unwrap();
    let token = tokens["access_token"].as_str().expect("access token");
    let id_token = tokens["id_token"].as_str().expect("ID token");
    let auth = Authenticator::open(db.clone(), Some(config(issuer.clone())))
        .await
        .unwrap();
    let first = auth.authenticate(token).await.unwrap_or_else(|error| {
        let parts:Vec<_>=token.split('.').collect();
        let claims:Value=serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        panic!("actual issuer authentication failed: {error}; nonsecret token metadata={}", json!({
            "header":jsonwebtoken::decode_header(token).map(|header|json!({"alg":header.alg,"typ":header.typ})).ok(),
            "iss":claims.get("iss"),"aud":claims.get("aud"),"scope":claims.get("scope"),
            "typ":claims.get("typ"),"token_use":claims.get("token_use"),"exp":claims.get("exp"),
            "subject_present":claims.get("sub").and_then(Value::as_str).is_some(),
        }));
    });
    assert!(first.scopes.contains(&Permission::Read));
    assert!(first.credential.is_none());
    assert_eq!(
        auth.authenticate(token).await.unwrap().principal,
        first.principal
    );
    assert!(
        db.access_context(first.principal, first.scopes.clone())
            .await
            .unwrap()
            .tenants
            .is_empty()
    );
    assert!(matches!(
        auth.authenticate(id_token).await,
        Err(Error::Unauthorized)
    ));
    let mut wrong = config(issuer.clone());
    wrong.audience = "different-api".into();
    assert!(matches!(
        Authenticator::open(db.clone(), Some(wrong))
            .await
            .unwrap()
            .authenticate(token)
            .await,
        Err(Error::Unauthorized)
    ));
    let parts: Vec<_> = token.split('.').collect();
    let mut payload: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    payload["sub"] = json!("forged-subject");
    let tampered = format!(
        "{}.{}.{}",
        parts[0],
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap()),
        parts[2]
    );
    assert!(matches!(
        auth.authenticate(&tampered).await,
        Err(Error::Unauthorized)
    ));
    let mut introspection = config(issuer.clone());
    // RFC7662 active/audience/expiry/scope checks establish access-token purpose;
    // its response need not repeat Keycloak's JWT-specific `typ` claim.
    introspection.required_claims.clear();
    introspection.verification = Verification::Introspection {
        client_id: "context-development-client".into(),
        client_secret_env: "CONTEXT_TEST_OIDC_CLIENT_SECRET".into(),
    };
    let introspection = Authenticator::open(db.clone(), Some(introspection))
        .await
        .unwrap();
    let actual = introspection.authenticate(token).await;
    if actual.is_err() {
        let claims: Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(token.split('.').nth(1).unwrap())
                .unwrap(),
        )
        .unwrap();
        let value: Value = reqwest::Client::new()
            .post(format!("{issuer}/protocol/openid-connect/token/introspect"))
            .basic_auth("context-development-client", Some(secret))
            .form(&[("token", token), ("token_type_hint", "access_token")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        panic!(
            "actual introspection rejected; nonsecret metadata={}",
            json!({
                "active":value.get("active"),"aud":value.get("aud"),"iss":value.get("iss"),
                "scope":value.get("scope"),"typ":value.get("typ"),"token_type":value.get("token_type"),
                "subject_present":value.get("sub").and_then(Value::as_str).is_some(),"exp":value.get("exp"),
                "token_claim_names":claims.as_object().map(|object|object.keys().collect::<Vec<_>>()),
                "authorized_party":claims.get("azp"),
            })
        );
    }
    assert_eq!(actual.unwrap().principal, first.principal);
    assert!(matches!(
        introspection.authenticate("not-an-active-token").await,
        Err(Error::Unauthorized)
    ));
    // Rotate a key in this disposable issuer, rather than claiming key-rotation
    // compatibility solely from a fixture or a successful initial signature.
    let base = issuer
        .strip_suffix("/realms/context-dev")
        .expect("development realm issuer");
    let client = reqwest::Client::new();
    let admin: Value = client
        .post(format!(
            "{base}/realms/master/protocol/openid-connect/token"
        ))
        .form(&[
            ("grant_type", "password"),
            ("client_id", "admin-cli"),
            ("username", "context-admin"),
            ("password", "context-development-admin"),
        ])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let admin = admin["access_token"].as_str().unwrap();
    let realm: Value = client
        .get(format!("{base}/admin/realms/context-dev"))
        .bearer_auth(admin)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let response=client.post(format!("{base}/admin/realms/context-dev/components")).bearer_auth(admin)
        .json(&json!({"name":format!("rotation-{}",Uuid::now_v7()),"providerId":"rsa-generated",
            "providerType":"org.keycloak.keys.KeyProvider","parentId":realm["id"],
            "config":{"priority":["200"],"enabled":["true"],"active":["true"],"algorithm":["RS256"],"keySize":["2048"]}}))
        .send().await.unwrap().error_for_status().unwrap();
    let component = response.headers()["location"].to_str().unwrap().to_string();
    let rotated: Value = client
        .post(format!("{issuer}/protocol/openid-connect/token"))
        .form(&[
            ("grant_type", "password"),
            ("client_id", "context-development-client"),
            ("client_secret", secret.as_str()),
            ("username", "developer"),
            ("password", "context-development-password"),
            ("scope", "openid context:read"),
        ])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let rotated = rotated["access_token"].as_str().unwrap();
    assert_ne!(
        jsonwebtoken::decode_header(rotated).unwrap().kid,
        jsonwebtoken::decode_header(token).unwrap().kid
    );
    tokio::time::sleep(Duration::from_secs(11)).await;
    let rotated_identity = auth.authenticate(rotated).await;
    let removed = client
        .delete(component)
        .bearer_auth(admin)
        .send()
        .await
        .unwrap();
    assert!(removed.status().is_success());
    assert_eq!(rotated_identity.unwrap().principal, first.principal);
    db.close().await;
}
