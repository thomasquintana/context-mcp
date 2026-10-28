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
use futures_util::StreamExt;
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
        public_endpoint: None,
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
    let expected: [u8; 32] = Sha256::digest(b"uploaded fixture").into();
    blobs
        .promote_verified("uploads/session", "immutable/source", 16, &expected, 1024)
        .await
        .unwrap();
    let upload = blobs
        .upload_url("uploads/session", Duration::from_secs(60))
        .await
        .unwrap();
    assert!(
        client
            .put(upload)
            .body("changed upload!!")
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert_eq!(
        blobs.get("immutable/source", 1024).await.unwrap(),
        "uploaded fixture"
    );
    blobs
        .promote_verified("uploads/session", "immutable/source", 16, &expected, 1024)
        .await
        .unwrap();
    assert!(matches!(
        blobs
            .promote_verified("uploads/session", "immutable/rejected", 16, &expected, 1024)
            .await,
        Err(Error::Integrity)
    ));
    assert!(blobs.get("immutable/rejected", 1024).await.is_err());
    blobs.ready().await.unwrap();
    let (url, headers) = blobs
        .bounded_upload_url("uploads/inflight", 16, &expected, Duration::from_secs(60))
        .await
        .unwrap();
    let (started, observed) = tokio::sync::oneshot::channel();
    let (release, proceed) = tokio::sync::oneshot::channel();
    let body = futures_util::stream::once(async move {
        let _ = started.send(());
        Ok::<_, std::io::Error>(Bytes::from_static(b"upl"))
    })
    .chain(futures_util::stream::once(async move {
        proceed.await.map_err(std::io::Error::other)?;
        Ok::<_, std::io::Error>(Bytes::from_static(b"oaded fixture"))
    }));
    let uploading = tokio::spawn({
        let client = client.clone();
        let url = url.clone();
        let headers = headers.clone();
        async move {
            let mut request = client.put(url).body(reqwest::Body::wrap_stream(body));
            for (k, v) in headers {
                request = request.header(k, v);
            }
            request.send().await.unwrap()
        }
    });
    observed.await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let fenced = tokio::spawn({
        let blobs = blobs.clone();
        async move { blobs.fence_upload("uploads/inflight").await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let fence_won = fenced.is_finished();
    let _ = release.send(());
    fenced.await.unwrap().unwrap();
    let completed = uploading.await.unwrap();
    if fence_won {
        assert_eq!(completed.status(), reqwest::StatusCode::PRECONDITION_FAILED);
    } else {
        assert!(
            completed.status().is_success()
                || completed.status() == reqwest::StatusCode::PRECONDITION_FAILED
        );
    }
    assert_eq!(
        blobs.size("uploads/inflight").await.unwrap(),
        0,
        "late PUT recreated purged payload"
    );
    let mut repeat = client.put(url).body("uploaded fixture");
    for (k, v) in headers {
        repeat = repeat.header(k, v);
    }
    assert_eq!(
        repeat.send().await.unwrap().status(),
        reqwest::StatusCode::PRECONDITION_FAILED
    );
    // Disposable bucket teardown, not production fence retirement.
    blobs.delete("uploads/inflight").await.unwrap();
    blobs.delete("immutable/source").await.unwrap();
    blobs.delete("uploads/session").await.unwrap();
    blobs.delete("source/original").await.unwrap();
    blobs.delete("source/original").await.unwrap();
    assert!(blobs.get("source/original", 1024).await.is_err());
    let versioning = admin
        .signed_url_opts(
            Method::PUT,
            &root,
            Duration::from_secs(60),
            &object_store::signer::SignedUrlOptions::new().with_query([("versioning", "")]),
        )
        .await
        .unwrap();
    assert!(client.put(versioning).body("<VersioningConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Status>Enabled</Status></VersioningConfiguration>").send().await.unwrap().status().is_success());
    assert!(
        blobs.ready().await.is_err(),
        "versioned bucket silently accepted a permanent-purge contract"
    );
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
