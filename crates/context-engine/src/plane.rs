//! Managed payload/index clients and single-instance work coordination.

use crate::{
    Error, Result,
    data::{Config, Pipeline},
};
use context_core::ids::{JobId, TenantId};
use context_storage::{
    blobs::{BlobConfig, Blobs},
    search::Search,
};
use context_workflows::host::WorkflowHost;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};
use tokio::sync::{Mutex, Notify, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;

pub(crate) struct Control {
    pub cancel: CancellationToken,
    pub finished: Arc<Notify>,
}

pub(crate) struct Plane {
    pub config: Config,
    pub blobs: Blobs,
    pub search: BTreeMap<String, Search>,
    pub staging: PathBuf,
    pub publications: Mutex<()>,
    pub tokenizer: tiktoken_rs::CoreBPE,
    pub controls: Mutex<BTreeMap<JobId, Arc<Control>>>,
    pub stop: CancellationToken,
    pub tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    pub calls: tokio::sync::mpsc::Sender<crate::operations::Invocation>,
    pub receiver: Mutex<tokio::sync::mpsc::Receiver<crate::operations::Invocation>>,
    pub requests: Mutex<BTreeMap<JobId, crate::operations::Pending>>,
    mutations: Mutex<BTreeMap<TenantId, Weak<Mutex<()>>>>,
}

impl Plane {
    pub async fn open(config: &Config, base: &Path, host: &WorkflowHost) -> Result<Self> {
        if config.max_upload_bytes == 0
            || config.max_upload_bytes > 5_000_000_000
            || !(1..=900).contains(&config.upload_lifetime_seconds)
            || !(1..=4096).contains(&config.max_pending_jobs)
            || !(1..=16).contains(&config.workers)
            || !(1..=10_000).contains(&config.max_segments)
            || config.spaces.is_empty()
            || config.spaces.len() > 16
            || !config.profiles.contains_key(&config.default_profile)
            || !config.spaces.contains_key(&config.default_space)
        {
            return Err(Error::Configuration);
        }
        let blobs = Blobs::new(BlobConfig {
            endpoint: config.s3_endpoint.clone(),
            bucket: config.s3_bucket.clone(),
            region: config.s3_region.clone(),
            access_key: std::env::var(&config.s3_access_key_env)
                .map_err(|_| Error::Configuration)?,
            secret_key: std::env::var(&config.s3_secret_key_env)
                .map_err(|_| Error::Configuration)?,
            allow_http: config.s3_allow_http,
        })?;
        blobs.ready().await?;
        let key = std::env::var(&config.typesense_key_env).map_err(|_| Error::Configuration)?;
        let mut search = BTreeMap::new();
        for (name, space) in &config.spaces {
            host.fingerprint(&space.document_role)?;
            host.fingerprint(&space.query_role)?;
            let index = Search::new(
                &config.typesense_endpoint,
                &key,
                &space.collection,
                space.dimensions,
            )?;
            index.ensure_collection().await?;
            search.insert(name.clone(), index);
        }
        let staging = base.join(&config.staging_directory);
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&staging).map_err(|_| Error::Configuration)?;
        let staging = staging.canonicalize().map_err(|_| Error::Configuration)?;
        let (calls, receiver) = tokio::sync::mpsc::channel(config.workers * 4);
        let plane = Self {
            publications: Mutex::new(()),
            tokenizer: match config.token_encoding.as_str() {
                "cl100k_base" => tiktoken_rs::cl100k_base(),
                "o200k_base" => tiktoken_rs::o200k_base(),
                _ => return Err(Error::Configuration),
            }
            .map_err(|_| Error::Configuration)?,
            calls,
            receiver: Mutex::new(receiver),
            requests: Mutex::new(BTreeMap::new()),
            config: config.clone(),
            blobs,
            search,
            staging,
            controls: Mutex::new(BTreeMap::new()),
            stop: CancellationToken::new(),
            tasks: Mutex::new(Vec::new()),
            mutations: Mutex::new(BTreeMap::new()),
        };
        for profile in plane.config.profiles.keys() {
            plane.pipeline(host, Some(profile), None)?;
        }
        for role in plane
            .config
            .reconcile_role
            .iter()
            .chain(plane.config.synthesis_role.iter())
        {
            host.fingerprint(role)?;
        }
        if plane.config.memory_authority.len() > 1024
            || plane.config.memory_types.len() > 128
            || plane.config.memory_types.iter().any(|(name, kind)| {
                name.len() > 128
                    || !matches!(
                        kind.as_str(),
                        "string" | "number" | "integer" | "boolean" | "object" | "array" | "null"
                    )
            })
        {
            return Err(Error::Configuration);
        }
        Ok(plane)
    }

    pub fn pipeline(
        &self,
        host: &WorkflowHost,
        name: Option<&str>,
        media: Option<&str>,
    ) -> Result<Pipeline> {
        let profile = self
            .config
            .profiles
            .get(name.unwrap_or(&self.config.default_profile))
            .ok_or(Error::MissingCapability)?
            .clone();
        if profile.media_types.is_empty() || !(128..=8192).contains(&profile.chunk_bytes) {
            return Err(Error::Configuration);
        }
        if let Some(media) = media {
            let category = media.split_once('/').ok_or(Error::Invalid)?.0;
            if !profile.media_types.contains(media)
                && !profile.media_types.contains(&format!("{category}/*"))
            {
                return Err(Error::MissingCapability);
            }
        }
        let space = self
            .config
            .spaces
            .get(&profile.space)
            .ok_or(Error::Configuration)?
            .clone();
        let mut names = vec![
            &profile.extract_role,
            &profile.enrich_role,
            &space.document_role,
        ];
        names.extend(profile.describe_role.iter());
        names.extend(profile.transcribe_role.iter());
        names.extend(profile.artifact_role.iter());
        names.extend(profile.media_role.iter());
        names.extend(profile.cleanup_role.iter());
        if let Some(node) = &profile.cleanup_node
            && !host.has_checkpoint(&profile.extract_role, node)
        {
            return Err(Error::Configuration);
        }
        if profile.cleanup_node.is_some() != profile.cleanup_role.is_some() {
            return Err(Error::Configuration);
        }
        let fingerprints = names
            .into_iter()
            .map(|name| Ok((name.clone(), host.fingerprint(name)?)))
            .collect::<Result<_>>()?;
        Ok(Pipeline {
            profile,
            space,
            fingerprints,
        })
    }

    pub async fn tenant_lock(&self, tenant: TenantId) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.mutations.lock().await;
            if locks.len() > 1024 {
                locks.retain(|_, lock| lock.strong_count() > 0);
            }
            match locks.get(&tenant).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(Mutex::new(()));
                    locks.insert(tenant, Arc::downgrade(&lock));
                    lock
                }
            }
        };
        lock.lock_owned().await
    }

    pub fn directory(&self, job: JobId) -> PathBuf {
        self.staging.join(job.to_string())
    }
}
