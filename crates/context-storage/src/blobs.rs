//! Bounded S3-compatible payload storage and expiring transfer URLs.

use crate::{Error, Result};
use bytes::Bytes;
use futures_util::TryStreamExt;
use object_store::{
    ClientOptions, ObjectStore, ObjectStoreExt, PutMode, PutOptions, RetryConfig,
    aws::{AmazonS3, AmazonS3Builder, S3ConditionalPut},
    path::Path,
    signer::{Method, Signer},
};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// Operator-owned credentials/configuration. Intentionally has no Debug/Serialize.
pub struct BlobConfig {
    /// S3-compatible endpoint URL; no credentials, query or fragment.
    pub endpoint: String,
    /// Existing operator-provisioned bucket.
    pub bucket: String,
    /// Signing region.
    pub region: String,
    /// Access-key identifier, retained only in process configuration.
    pub access_key: String,
    /// Secret access key, retained only in process configuration.
    pub secret_key: String,
    /// Permit plain HTTP for explicitly configured local infrastructure.
    pub allow_http: bool,
}

/// S3 client. Keys are generated/authorized by the application, never capabilities.
#[derive(Clone)]
pub struct Blobs {
    store: AmazonS3,
}

fn managed_path(key: &str) -> Result<Path> {
    if key.is_empty() || key.starts_with('/') {
        return Err(Error::Invalid);
    }
    Path::parse(key).map_err(|_| Error::Invalid)
}

impl Blobs {
    /// Construct a path-style S3 client with bounded requests and explicit credentials.
    ///
    /// # Errors
    /// Rejects malformed endpoints, implicit HTTP and invalid S3 settings.
    pub fn new(config: BlobConfig) -> Result<Self> {
        let endpoint = reqwest::Url::parse(&config.endpoint).map_err(|_| Error::Invalid)?;
        if config.bucket.is_empty()
            || config.region.is_empty()
            || config.access_key.is_empty()
            || config.secret_key.is_empty()
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !(endpoint.scheme() == "https" || (config.allow_http && endpoint.scheme() == "http"))
        {
            return Err(Error::Invalid);
        }
        let store = AmazonS3Builder::new()
            .with_endpoint(config.endpoint)
            .with_bucket_name(config.bucket)
            .with_region(config.region)
            .with_access_key_id(config.access_key)
            .with_secret_access_key(config.secret_key)
            .with_allow_http(config.allow_http)
            .with_virtual_hosted_style_request(false)
            .with_conditional_put(S3ConditionalPut::ETagMatch)
            .with_client_options(
                ClientOptions::new()
                    .with_allow_http(config.allow_http)
                    .with_timeout(Duration::from_secs(30)),
            )
            .with_retry(RetryConfig {
                max_retries: 0,
                ..RetryConfig::default()
            })
            .build()?;
        Ok(Self { store })
    }

    /// Store a bounded small payload using create-only semantics.
    ///
    /// # Errors
    /// Rejects oversize bodies, malformed keys and overwrite attempts.
    pub async fn put(&self, key: &str, bytes: Bytes, max_bytes: usize) -> Result<()> {
        if bytes.len() > max_bytes {
            return Err(Error::Integrity);
        }
        self.store
            .put_opts(
                &managed_path(key)?,
                bytes.into(),
                PutOptions {
                    mode: PutMode::Create,
                    ..PutOptions::default()
                },
            )
            .await?;
        Ok(())
    }

    /// Read a small body with both advertised and actual streaming bounds.
    ///
    /// # Errors
    /// Rejects oversize metadata/streams and missing/corrupt objects.
    pub async fn get(&self, key: &str, max_bytes: usize) -> Result<Bytes> {
        let result = self.store.get(&managed_path(key)?).await?;
        if result.meta.size > max_bytes as u64 {
            return Err(Error::Integrity);
        }
        let mut stream = result.into_stream();
        let mut data = Vec::new();
        while let Some(bytes) = stream.try_next().await? {
            let size = data
                .len()
                .checked_add(bytes.len())
                .ok_or(Error::Integrity)?;
            if size > max_bytes {
                return Err(Error::Integrity);
            }
            data.extend_from_slice(&bytes);
        }
        Ok(data.into())
    }

    /// Verify the advertised object length before finalizing an upload session.
    ///
    /// # Errors
    /// Returns storage/path failures; content digest still needs streaming verification.
    pub async fn size(&self, key: &str) -> Result<u64> {
        Ok(self.store.head(&managed_path(key)?).await?.size)
    }

    /// Stream a source to a newly created private staging file and verify its bytes.
    ///
    /// The caller owns the parent directory and must not allow untrusted symlinks.
    /// Partial/mismatched files are removed before returning a failure.
    ///
    /// # Errors
    /// Rejects existing destinations, wrong sizes/digests and storage/file failures.
    pub async fn download_verified(
        &self,
        key: &str,
        destination: &std::path::Path,
        expected_size: u64,
        expected_sha256: &[u8; 32],
        maximum_size: u64,
    ) -> Result<()> {
        if expected_size > maximum_size {
            return Err(Error::Integrity);
        }
        let object = self.store.get(&managed_path(key)?).await?;
        if object.meta.size != expected_size {
            return Err(Error::Integrity);
        }
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(destination).await?;
        let result = async {
            let mut stream = object.into_stream();
            let mut size = 0_u64;
            let mut digest = Sha256::new();
            while let Some(bytes) = stream.try_next().await? {
                size = size
                    .checked_add(bytes.len() as u64)
                    .ok_or(Error::Integrity)?;
                if size > expected_size || size > maximum_size {
                    return Err(Error::Integrity);
                }
                digest.update(&bytes);
                file.write_all(&bytes).await?;
            }
            if size != expected_size || digest.finalize().as_slice() != expected_sha256 {
                return Err(Error::Integrity);
            }
            file.sync_all().await?;
            Ok(())
        }
        .await;
        drop(file);
        if result.is_err() {
            tokio::fs::remove_file(destination).await?;
        }
        result
    }

    /// Delete a managed key idempotently. Versioned buckets require operator policy.
    ///
    /// # Errors
    /// Returns a path or storage failure; absence is already a successful deletion.
    pub async fn delete(&self, key: &str) -> Result<()> {
        match self.store.delete(&managed_path(key)?).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Issue a short-lived upload URL after admission of a tracked upload session.
    ///
    /// # Errors
    /// Rejects zero/excessive lifetime; credentials never appear in normal diagnostics.
    pub async fn upload_url(&self, key: &str, lifetime: Duration) -> Result<String> {
        self.signed_url(Method::PUT, key, lifetime).await
    }

    /// Issue a short-lived source URL only after current resource authorization.
    ///
    /// # Errors
    /// Rejects zero/excessive lifetime and malformed keys.
    pub async fn download_url(&self, key: &str, lifetime: Duration) -> Result<String> {
        self.signed_url(Method::GET, key, lifetime).await
    }

    async fn signed_url(&self, method: Method, key: &str, lifetime: Duration) -> Result<String> {
        if lifetime.is_zero() || lifetime > Duration::from_secs(900) {
            return Err(Error::Invalid);
        }
        Ok(self
            .store
            .signed_url(method, &managed_path(key)?, lifetime)
            .await?
            .into())
    }
}
