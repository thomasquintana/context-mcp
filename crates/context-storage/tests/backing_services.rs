//! Actual S3-compatible and Typesense protocol acceptance; no fixture server.

use bytes::Bytes;
use context_core::{
    content::Origin,
    ids::{ContentId, RevisionId, WorkspaceId},
};
use context_storage::{
    Error,
    blobs::{BlobConfig, Blobs},
    search::{Search, SearchDocument},
};
use object_store::{
    aws::AmazonS3Builder,
    path::Path,
    signer::{Method, Signer},
};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires CONTEXT_TEST_S3_ENDPOINT and isolated S3 credentials"]
async fn s3_signed_transfers_integrity_limits_and_idempotent_purge() {
    let endpoint = std::env::var("CONTEXT_TEST_S3_ENDPOINT").expect("isolated S3 endpoint");
    let access = std::env::var("CONTEXT_TEST_S3_ACCESS_KEY").expect("isolated S3 access key");
    let secret = std::env::var("CONTEXT_TEST_S3_SECRET_KEY").expect("isolated S3 secret key");
    let bucket = format!("context-test-{}", Uuid::now_v7().simple());
    let admin = AmazonS3Builder::new()
        .with_endpoint(&endpoint)
        .with_bucket_name(&bucket)
        .with_region("us-east-1")
        .with_access_key_id(&access)
        .with_secret_access_key(&secret)
        .with_allow_http(true)
        .with_virtual_hosted_style_request(false)
        .build()
        .unwrap();
    let root = Path::from("");
    let client = reqwest::Client::new();
    // Operator fixture provisioning via standard signed S3 CreateBucket.
    let url = admin
        .signed_url(Method::PUT, &root, Duration::from_secs(60))
        .await
        .unwrap();
    let response = client
        .put(url)
        .send()
        .await
        .expect("bucket provisioning transport");
    assert!(
        response.status().is_success(),
        "S3 test bucket provisioning failed"
    );
    let blobs = Blobs::new(BlobConfig {
        endpoint,
        bucket,
        region: "us-east-1".into(),
        access_key: access,
        secret_key: secret,
        allow_http: true,
    })
    .unwrap();
    let bytes = Bytes::from_static(b"independent source fixture");
    blobs
        .put("source/original", bytes.clone(), 1024)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "test object write failed: {:?}",
                std::error::Error::source(&error)
            )
        });
    assert_eq!(blobs.get("source/original", 1024).await.unwrap(), bytes);
    assert_eq!(
        blobs.size("source/original").await.unwrap(),
        bytes.len() as u64
    );
    assert!(matches!(
        blobs.get("source/original", 1).await,
        Err(Error::Integrity)
    ));
    assert!(
        blobs
            .put("source/original", Bytes::from_static(b"replacement"), 1024)
            .await
            .is_err()
    );
    assert_eq!(blobs.get("source/original", 1024).await.unwrap(), bytes);
    let dir = tempfile::tempdir().unwrap();
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let path = dir.path().join("source");
    blobs
        .download_verified("source/original", &path, bytes.len() as u64, &digest, 1024)
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(&path).await.unwrap(), bytes);
    let invalid = dir.path().join("invalid");
    assert!(matches!(
        blobs
            .download_verified(
                "source/original",
                &invalid,
                bytes.len() as u64,
                &[1; 32],
                1024
            )
            .await,
        Err(Error::Integrity)
    ));
    assert!(!invalid.exists(), "invalid content must not remain staged");
    assert!(matches!(
        blobs.upload_url("", Duration::from_secs(60)).await,
        Err(Error::Invalid)
    ));
    let url = blobs
        .upload_url("uploads/session", Duration::from_secs(60))
        .await
        .unwrap();
    assert!(
        client
            .put(url)
            .body("uploaded fixture")
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    let url = blobs
        .download_url("uploads/session", Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(
        client.get(url).send().await.unwrap().bytes().await.unwrap(),
        "uploaded fixture"
    );
    blobs.delete("uploads/session").await.unwrap();
    blobs.delete("source/original").await.unwrap();
    blobs.delete("source/original").await.unwrap();
    assert!(blobs.get("source/original", 1024).await.is_err());
    let url = admin
        .signed_url(Method::DELETE, &root, Duration::from_secs(60))
        .await
        .unwrap();
    assert!(
        client
            .delete(url)
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
}

#[tokio::test]
#[ignore = "requires CONTEXT_TEST_TYPESENSE_ENDPOINT and isolated Typesense key"]
async fn real_hybrid_search_filters_unauthorized_ids_and_rejects_wrong_vector_spaces() {
    let endpoint =
        std::env::var("CONTEXT_TEST_TYPESENSE_ENDPOINT").expect("isolated Typesense endpoint");
    let key = std::env::var("CONTEXT_TEST_TYPESENSE_KEY").expect("isolated Typesense key");
    let collection = format!("context_test_{}", Uuid::now_v7().simple());
    let search = Search::new(&endpoint, &key, &collection, 3).unwrap();
    search.ensure_collection().await.unwrap();
    search.ensure_collection().await.unwrap();
    let allowed = ContentId::new();
    let secret = ContentId::new();
    let workspace = WorkspaceId::new();
    for (id, text) in [
        (allowed, "blue ocean source"),
        (secret, "secret blue ocean source"),
    ] {
        search
            .upsert(&SearchDocument {
                id,
                workspace,
                revision: Some(RevisionId::new()),
                text: text.into(),
                origin: Origin::OriginalEvidence,
                embedding: vec![1.0, 0.0, 0.0],
            })
            .await
            .unwrap();
    }
    assert_eq!(
        search
            .hybrid("ocean", &[1.0, 0.0, 0.0], &[allowed], 10)
            .await
            .unwrap(),
        [allowed]
    );
    assert!(
        search
            .hybrid("ocean", &[1.0, 0.0, 0.0], &[], 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        search.hybrid("ocean", &[1.0, 0.0], &[allowed], 10).await,
        Err(Error::Invalid)
    ));
    assert!(
        Search::new(&endpoint, &key, &collection, 2)
            .unwrap()
            .ensure_collection()
            .await
            .is_err()
    );
    search.delete(allowed).await.unwrap();
    search.delete(allowed).await.unwrap();
    assert!(
        search
            .hybrid("ocean", &[1.0, 0.0, 0.0], &[allowed], 10)
            .await
            .unwrap()
            .is_empty()
    );
    search.delete(secret).await.unwrap();
    let response = reqwest::Client::new()
        .delete(format!("{endpoint}/collections/{collection}"))
        .header("X-TYPESENSE-API-KEY", key)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
}
