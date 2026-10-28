//! Actual PostgreSQL/MinIO/Typesense plus real HTLK engine application-flow checks.
//! Processing roles here are explicit pure fixtures, not external-provider proof.

use context_auth::Authenticator;
use context_core::{
    access::{Authentication, Permission},
    ids::{JobId, PrincipalId},
};
use context_engine::{Application, Config, bootstrap};
use context_storage::postgres::Database;
use object_store::{
    ObjectStoreExt,
    aws::AmazonS3Builder,
    path::Path,
    signer::{Method, Signer},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use uuid::Uuid;

fn write(path: &std::path::Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn operation_error(error: context_engine::Error) -> ! {
    match error {
        context_engine::Error::Storage(context_storage::Error::Database(error)) => {
            if let Some(error) = error.as_database_error() {
                panic!(
                    "fixture SQL failure: code={:?}, message={}",
                    error.code(),
                    error.message()
                );
            }
            panic!("fixture database transport failure");
        }
        context_engine::Error::Storage(context_storage::Error::Http(error)) => panic!(
            "fixture HTTP failure: status={:?}, transport={}",
            error.status(),
            error.is_connect()
        ),
        context_engine::Error::Storage(context_storage::Error::Objects(error)) => {
            if let object_store::Error::Generic { store, source } = error {
                panic!("fixture S3 SDK failure: {store}: {source}");
            }
            let kind = match error {
                object_store::Error::NotFound { .. } => "not_found",
                object_store::Error::Precondition { .. } => "precondition",
                object_store::Error::PermissionDenied { .. } => "permission",
                object_store::Error::Unauthenticated { .. } => "authentication",
                object_store::Error::Generic { .. } => "generic",
                _ => "other",
            };
            panic!("fixture S3 failure class: {kind}");
        }
        context_engine::Error::Storage(error) => panic!("fixture storage failure: {error:?}"),
        error => panic!("fixture operation failed: {error}"),
    }
}

struct Fixture {
    app: Arc<Application>,
    actor: Authentication,
    collection: context_core::ids::CollectionId,
    tenant: context_core::ids::TenantId,
    workspace: context_core::ids::WorkspaceId,
    db: Database,
    admin: object_store::aws::AmazonS3,
    http: reqwest::Client,
    typesense: String,
    index: String,
    index_key: String,
    _dir: tempfile::TempDir,
    principal: PrincipalId,
    config: Config,
}

impl Fixture {
    async fn open() -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("context_engine=warn")
            .try_init();
        let dir = tempfile::tempdir().unwrap();
        let db = Database::connect(
            &std::env::var("CONTEXT_TEST_DATABASE_URL").expect("isolated PG"),
            8,
        )
        .await
        .unwrap();
        let principal = db
            .identity("ingestion-test", &Uuid::now_v7().to_string(), "service")
            .await
            .unwrap();
        let initial = bootstrap(&db, principal, "ingestion fixture")
            .await
            .unwrap();
        let auth = Authenticator::open(db.clone(), None).await.unwrap();
        let issued = auth
            .issue_service(
                principal,
                &BTreeSet::from([Permission::Read, Permission::Ingest]),
                Duration::from_secs(600),
            )
            .await
            .unwrap();
        let actor = auth.authenticate(issued.secret()).await.unwrap();
        let endpoint = std::env::var("CONTEXT_TEST_S3_ENDPOINT").expect("isolated S3");
        let bucket = format!("ingestion-{}", Uuid::now_v7().simple());
        let admin = AmazonS3Builder::new()
            .with_endpoint(&endpoint)
            .with_bucket_name(&bucket)
            .with_region("us-east-1")
            .with_access_key_id(std::env::var("CONTEXT_TEST_S3_ACCESS_KEY").unwrap())
            .with_secret_access_key(std::env::var("CONTEXT_TEST_S3_SECRET_KEY").unwrap())
            .with_allow_http(true)
            .with_virtual_hosted_style_request(false)
            .build()
            .unwrap();
        let http = reqwest::Client::new();
        let url = admin
            .signed_url(Method::PUT, &Path::from(""), Duration::from_secs(60))
            .await
            .unwrap();
        assert!(http.put(url).send().await.unwrap().status().is_success());
        for (role, result) in [
            (
                "extract",
                r#"{ document = { encoding = "json", value = { media_type = "text/plain", segments = [{ kind = "text", text = "fixture content", location = { kind = "asset" } }] } } }"#,
            ),
            (
                "broken",
                r#"{ document = { encoding = "json", value = { media_type = "text/plain", segments = [] } } }"#,
            ),
            (
                "enrich",
                r#"{ document = { encoding = "json", value = { summary = "Fixture summary", entities = [] } } }"#,
            ),
            ("embed", r#"{ embedding = [1,0,0], space = "test" }"#),
            ("cleanup", r#"{ done = true }"#),
            ("bad_cleanup", r#"{ done = false }"#),
        ] {
            std::fs::write(
                dir.path().join(format!("{role}.htlk")),
                format!(
                    r#"
ir_version = "0.1"
graph fixture.{role} {{
    inputs = {{ request = json }}
    outputs = {{ result = json }}
    nodes {{ result = eval(json, {result}) }}
    edges {{ edge result {{ from = result.outputs.value to = outputs.result }} }}
}}
"#
                ),
            )
            .unwrap();
        }
        write(
            &dir.path().join("catalogs.json"),
            &json!({
                "tools":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{},"schema_documents":{},"tools":[]},
                "resources":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{},"schema_documents":{},"resources":[],"templates":[]},
                "prompts":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{},"schema_documents":{},"prompts":[]},
            }),
        );
        let mut roles = serde_json::Map::new();
        for role in [
            "extract",
            "broken",
            "enrich",
            "embed",
            "cleanup",
            "bad_cleanup",
        ] {
            roles.insert(
                role.into(),
                json!({"source":format!("{role}.htlk"),"catalogs":"catalogs.json","checkpoints":["result"]}),
            );
        }
        let profile = json!({"media_types":["text/plain"],"extract_role":"extract","enrich_role":"enrich","space":"test","chunk_bytes":128,"cleanup_role":"cleanup","cleanup_node":"result"});
        let mut broken = profile.clone();
        broken["extract_role"] = json!("broken");
        let mut unclean = broken.clone();
        unclean["cleanup_role"] = json!("bad_cleanup");
        let typesense = std::env::var("CONTEXT_TEST_TYPESENSE_ENDPOINT").unwrap();
        let index_key = std::env::var("CONTEXT_TEST_TYPESENSE_KEY").unwrap();
        let index = format!("ingestion_{}", Uuid::now_v7().simple());
        let config:Config=serde_json::from_value(json!({"database_url_env":"CONTEXT_TEST_DATABASE_URL","database_connections":8,"oidc":null,
            "workflows":{"database":"runtime.sqlite","roles":roles,"endpoints":{},"required_deployments":[],"workers":2,"max_runs":16},
            "data":{"s3_endpoint":endpoint,"s3_bucket":bucket,"s3_region":"us-east-1","s3_access_key_env":"CONTEXT_TEST_S3_ACCESS_KEY",
                "s3_secret_key_env":"CONTEXT_TEST_S3_SECRET_KEY","s3_allow_http":true,"typesense_endpoint":typesense,"typesense_key_env":"CONTEXT_TEST_TYPESENSE_KEY",
                "spaces":{"test":{"collection":index,"dimensions":3,"document_role":"embed","query_role":"embed"}},"default_space":"test",
                "profiles":{"default":profile,"broken":broken,"unclean":unclean},"default_profile":"default","staging_directory":"staging",
                "max_upload_bytes":1048576,"upload_lifetime_seconds":60,"max_pending_jobs":1024,"workers":2,"max_segments":100}}
        )).unwrap();
        let app = Arc::new(Application::open(&config, dir.path()).await.unwrap());
        Self {
            app,
            actor,
            collection: initial.collection,
            tenant: initial.tenant,
            workspace: initial.workspace,
            db,
            admin,
            http,
            typesense,
            index,
            index_key,
            _dir: dir,
            principal,
            config,
        }
    }

    async fn wait(&self, job: JobId) -> Value {
        for _ in 0..500 {
            let status = self.app.job_status(&self.actor, job).await.unwrap();
            if matches!(
                status["state"].as_str(),
                Some("succeeded" | "failed" | "cancelled")
            ) && status["cleanup_pending"] == false
            {
                return status;
            }
            actix::clock::sleep(Duration::from_millis(30)).await;
        }
        panic!("bounded ingestion deadline exceeded")
    }

    async fn close(self) {
        self.app.shutdown().await.unwrap();
        let keys:Vec<String>=sqlx::query_scalar("SELECT o.object_key FROM context.objects o JOIN context.content c ON c.id=o.content WHERE c.workspace=$1")
            .bind(Uuid::from(self.workspace)).fetch_all(self.db.pool()).await.unwrap();
        for key in keys {
            self.admin.delete(&Path::from(key)).await.unwrap();
        }
        sqlx::query("DELETE FROM context.objects WHERE content IN(SELECT id FROM context.content WHERE workspace=$1)")
            .bind(Uuid::from(self.workspace)).execute(self.db.pool()).await.unwrap();
        sqlx::query("DELETE FROM context.resources WHERE id=$1")
            .bind(Uuid::from(self.tenant))
            .execute(self.db.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM context.receipts WHERE principal=$1")
            .bind(Uuid::from(self.principal))
            .execute(self.db.pool())
            .await
            .unwrap();
        let url = self
            .admin
            .signed_url(Method::DELETE, &Path::from(""), Duration::from_secs(60))
            .await
            .unwrap();
        assert!(
            self.http
                .delete(url)
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        );
        assert!(
            self.http
                .delete(format!("{}/collections/{}", self.typesense, self.index))
                .header("X-TYPESENSE-API-KEY", self.index_key)
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        );
        self.db.close().await;
    }
}

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; pure processing fixtures"]
fn durable_admission_publication_failure_cancel_and_retry_use_real_backing_services() {
    actix::System::new().block_on(async {
        let fixture=Fixture::open().await;
        fixture.app.start_workers().await.unwrap();
        let receipt=fixture.app.ingest_text(&fixture.actor,fixture.collection,"fixture.txt","fixture content",None,"text-1").await.unwrap_or_else(|error|operation_error(error));
        let job:JobId=receipt["job"].as_str().unwrap().parse().unwrap();
        assert_eq!(fixture.wait(job).await["state"],"succeeded");
        assert_eq!(fixture.app.ingest_text(&fixture.actor,fixture.collection,"fixture.txt","fixture content",None,"text-1").await.unwrap(),receipt);
        let asset=receipt["asset"].as_str().unwrap().parse().unwrap();
        let hash=Sha256::digest(b"fixture content").iter().map(|v|format!("{v:02x}")).collect::<String>();
        fixture.app.create_upload(&fixture.actor,context_engine::uploads::CreateUpload {
            collection:fixture.collection,asset:None,name:"fixture.txt".into(),media_type:"text/plain".into(),size_bytes:15,sha256:hash.clone(),
            profile:None,idempotency_key:"crossed:upload".into(),
        }).await.unwrap();
        let original_upload:Value=sqlx::query_scalar("SELECT receipt FROM context.receipts WHERE principal=$1 AND operation='create_upload' AND idempotency_key='text-1:upload'")
            .bind(Uuid::from(fixture.principal)).fetch_one(fixture.db.pool()).await.unwrap();
        fixture.app.finalize_upload(&fixture.actor,original_upload["upload"].as_str().unwrap().parse().unwrap(),"crossed:finalize").await.unwrap();
        assert!(matches!(fixture.app.ingest_text(&fixture.actor,fixture.collection,"fixture.txt","fixture content",None,"crossed").await,
            Err(context_engine::Error::Storage(context_storage::Error::Conflict))));
        let transfer=fixture.app.create_upload(&fixture.actor,context_engine::uploads::CreateUpload {
            collection:fixture.collection,asset:Some(asset),name:"replacement.txt".into(),media_type:"text/plain".into(),size_bytes:15,sha256:hash,
            profile:Some("broken".into()),idempotency_key:"replacement".into(),
        }).await.unwrap();
        let mut put=fixture.http.put(transfer["url"].as_str().unwrap()).body("fixture content");
        for (name,value) in transfer["headers"].as_object().unwrap(){put=put.header(name,value.as_str().unwrap());}
        assert!(put.send().await.unwrap().status().is_success());
        let replacement=fixture.app.finalize_upload(&fixture.actor,transfer["upload"].as_str().unwrap().parse().unwrap(),"replacement-final").await.unwrap();
        let status=fixture.wait(replacement["job"].as_str().unwrap().parse().unwrap()).await;
        assert_eq!(status["state"],"failed");
        let current:Uuid=sqlx::query_scalar("SELECT current_revision FROM context.assets WHERE id=$1")
            .bind(Uuid::parse_str(receipt["asset"].as_str().unwrap()).unwrap()).fetch_one(fixture.db.pool()).await.unwrap();
        assert_eq!(current.to_string(),receipt["revision"]);
        let count=sqlx::query("SELECT count(*) AS count FROM context.content WHERE asset=$1 AND published AND kind='chunk'")
            .bind(Uuid::parse_str(receipt["asset"].as_str().unwrap()).unwrap()).fetch_one(fixture.db.pool()).await.unwrap().get::<i64,_>("count");
        assert_eq!(count,1);
        fixture.close().await;

        let fixture=Fixture::open().await; // worker-free admission makes cancellation deterministic
        let receipt=fixture.app.ingest_text(&fixture.actor,fixture.collection,"cancel.txt","fixture content",None,"cancel-1").await.unwrap();
        let job:JobId=receipt["job"].as_str().unwrap().parse().unwrap();
        fixture.app.cancel_job(&fixture.actor,job,"cancel-key").await.unwrap();
        // Stale cleanup may remove only its own unpublished bodies, even when
        // another job has staged derivatives of the same immutable revision.
        let foreign_job=Uuid::now_v7();
        sqlx::query("INSERT INTO context.jobs(id,principal,asset,revision,generation,kind,state,workflow,scope,auth_scopes) SELECT $2,principal,asset,revision,generation,'reprocess','failed',workflow,scope,auth_scopes FROM context.jobs WHERE id=$1")
            .bind(Uuid::from(job)).bind(foreign_job).execute(fixture.db.pool()).await.unwrap();
        let own=Uuid::now_v7();let foreign=Uuid::now_v7();
        for (content,producer) in [(own,Uuid::from(job)),(foreign,foreign_job)] {
            sqlx::query("INSERT INTO context.content(id,workspace,asset,revision,kind,origin,body,embedding_space,producer_job) VALUES($1,$2,$3,$4,'chunk','original_evidence','{}','test',$5)")
                .bind(content).bind(Uuid::from(fixture.workspace)).bind(Uuid::parse_str(receipt["asset"].as_str().unwrap()).unwrap())
                .bind(Uuid::parse_str(receipt["revision"].as_str().unwrap()).unwrap()).bind(producer).execute(fixture.db.pool()).await.unwrap();
        }
        fixture.app.start_workers().await.unwrap();
        assert_eq!(fixture.wait(job).await["state"],"cancelled");
        let ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM context.content WHERE id=ANY($1)")
            .bind(vec![own,foreign]).fetch_all(fixture.db.pool()).await.unwrap();
        assert_eq!(ids,vec![foreign],"cleanup crossed the job ownership boundary");
        sqlx::query("DELETE FROM context.content WHERE id=$1").bind(foreign).execute(fixture.db.pool()).await.unwrap();
        let retry=fixture.app.retry_job(&fixture.actor,job,"retry-key").await.unwrap();
        assert_ne!(retry["job"],receipt["job"]);
        assert_eq!(fixture.app.retry_job(&fixture.actor,job,"retry-key").await.unwrap(),retry);
        assert!(fixture.app.retry_job(&fixture.actor,job,"competing-retry").await.is_err());
        assert_eq!(fixture.wait(retry["job"].as_str().unwrap().parse().unwrap()).await["state"],"succeeded");
        fixture.close().await;

        let mut fixture=Fixture::open().await;
        let receipt=fixture.app.ingest_text(&fixture.actor,fixture.collection,"recover.txt","fixture content",None,"recover-1").await.unwrap();
        let job:JobId=receipt["job"].as_str().unwrap().parse().unwrap();
        fixture.app.shutdown().await.unwrap();
        // Simulate the cross-operation acknowledgement gap and later transfer-row
        // expiration. Child receipts must recover the existing accepted job.
        sqlx::query("DELETE FROM context.receipts WHERE principal=$1 AND operation='ingest_text' AND idempotency_key='recover-1'")
            .bind(Uuid::from(fixture.principal)).execute(fixture.db.pool()).await.unwrap();
        sqlx::query("DELETE FROM context.uploads WHERE job=$1").bind(Uuid::from(job)).execute(fixture.db.pool()).await.unwrap();
        fixture.app=Arc::new(Application::open(&fixture.config,fixture._dir.path()).await.unwrap());
        assert_eq!(fixture.app.ingest_text(&fixture.actor,fixture.collection,"recover.txt","fixture content",None,"recover-1").await.unwrap(),receipt);
        fixture.app.start_workers().await.unwrap();
        assert_eq!(fixture.wait(job).await["state"],"succeeded");
        fixture.close().await;
    });
}

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; pure processing fixtures"]
fn failed_job_reopens_fenced_and_recovers_missing_admission_handles_before_cleanup() {
    actix::System::new().block_on(async {
        let mut fixture=Fixture::open().await;
        let receipt=fixture.app.ingest_text(&fixture.actor,fixture.collection,"gap.txt","fixture content",None,"gap").await.unwrap();
        let job:JobId=receipt["job"].as_str().unwrap().parse().unwrap();
        let request=json!({});
        let fingerprint=fixture.app.workflows().fingerprint("extract").unwrap();
        let hash=Sha256::digest(context_workflows::host_json(&serde_json::to_vec(&request).unwrap()).unwrap());
        // Stage intent is durable before admission, but the application crashes
        // before saving the returned handle. Recover the exact stable admission.
        sqlx::query("INSERT INTO context.workflow_stages(job,stage,role,fingerprint,request_digest,request) VALUES($1,'extract:0','extract',$2,$3,$4)")
            .bind(Uuid::from(job)).bind(fingerprint).bind(hash.as_slice()).bind(&request).execute(fixture.db.pool()).await.unwrap();
        let handle=fixture.app.workflows().start(job,"extract",&request,&format!("{job}:extract:0")).await.unwrap();
        for _ in 0..250 {
            if matches!(fixture.app.workflows().inspect(&handle).await,Ok(context_workflows::host::State::Succeeded(_))){break;}
            actix::clock::sleep(Duration::from_millis(20)).await;
        }
        assert!(fixture.app.workflows().checkpoint(&handle,"result").await.unwrap().is_some());
        fixture.app.shutdown().await.unwrap();
        sqlx::query("UPDATE context.jobs SET state='failed',detail='{\"cleanup_pending\":true}' WHERE id=$1")
            .bind(Uuid::from(job)).execute(fixture.db.pool()).await.unwrap();
        // This second intent was never admitted. Recovery must safely materialize
        // and settle it under the fence as well as finding the admitted run above.
        let request=json!({"text":"never dispatched"});
        let hash=Sha256::digest(context_workflows::host_json(&serde_json::to_vec(&request).unwrap()).unwrap());
        sqlx::query("INSERT INTO context.workflow_stages(job,stage,role,fingerprint,request_digest,request) VALUES($1,'embed:99','embed',$2,$3,$4)")
            .bind(Uuid::from(job)).bind(fixture.app.workflows().fingerprint("embed").unwrap()).bind(hash.as_slice()).bind(request).execute(fixture.db.pool()).await.unwrap();
        fixture.app=Arc::new(Application::open(&fixture.config,fixture._dir.path()).await.unwrap());
        let fenced:bool=sqlx::query_scalar("SELECT (authority->>'fenced')::boolean FROM context.workflow_authority WHERE job=$1")
            .bind(Uuid::from(job)).fetch_one(fixture.db.pool()).await.unwrap();
        assert!(fenced,"terminal authority must be fenced before engine launch");
        fixture.app.start_workers().await.unwrap();
        assert_eq!(fixture.wait(job).await["state"],"failed");
        let row=sqlx::query("SELECT run_id,request FROM context.workflow_stages WHERE job=$1 AND stage='extract:0'")
            .bind(Uuid::from(job)).fetch_one(fixture.db.pool()).await.unwrap();
        assert_eq!(row.get::<String,_>("run_id"),handle.run_id);
        assert!(row.get::<Option<Value>,_>("request").is_none());
        let cleanup:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.workflow_stages WHERE job=$1 AND stage='cleanup:extract:0' AND run_id IS NOT NULL)")
            .bind(Uuid::from(job)).fetch_one(fixture.db.pool()).await.unwrap();
        assert!(cleanup,"accepted checkpoint must be cleaned after handle recovery");
        fixture.close().await;
    });
}

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; pure processing fixtures"]
fn retry_receipts_serialize_and_respect_capacity_before_dispatch() {
    actix::System::new().block_on(async {
        let mut fixture = Fixture::open().await;
        fixture.app.start_workers().await.unwrap();
        let mut previous = Vec::new();
        for key in ["capacity-a", "capacity-b"] {
            let receipt = fixture
                .app
                .ingest_text(
                    &fixture.actor,
                    fixture.collection,
                    "broken.txt",
                    "fixture content",
                    Some("broken".into()),
                    key,
                )
                .await
                .unwrap();
            let job: JobId = receipt["job"].as_str().unwrap().parse().unwrap();
            assert_eq!(fixture.wait(job).await["state"], "failed");
            previous.push(job);
        }
        fixture.app.shutdown().await.unwrap();
        fixture.config.data.as_mut().unwrap().max_pending_jobs = 1;
        fixture.app = Arc::new(
            Application::open(&fixture.config, fixture._dir.path())
                .await
                .unwrap(),
        );
        let (left, right) = tokio::join!(
            fixture
                .app
                .retry_job(&fixture.actor, previous[0], "same-retry"),
            fixture
                .app
                .retry_job(&fixture.actor, previous[0], "same-retry"),
        );
        let admitted = left.unwrap();
        assert_eq!(admitted, right.unwrap());
        assert!(matches!(
            fixture
                .app
                .retry_job(&fixture.actor, previous[1], "full")
                .await,
            Err(context_engine::Error::Busy)
        ));
        assert!(
            fixture
                .app
                .retry_job(&fixture.actor, previous[0], "competing")
                .await
                .is_err()
        );
        assert_eq!(
            fixture
                .app
                .retry_job(&fixture.actor, previous[0], "same-retry")
                .await
                .unwrap(),
            admitted
        );
        fixture.app.start_workers().await.unwrap();
        assert_eq!(
            fixture
                .wait(admitted["job"].as_str().unwrap().parse().unwrap())
                .await["state"],
            "failed"
        );
        fixture.close().await;
    });
}

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; pure processing fixtures"]
fn unconfirmed_provider_cleanup_keeps_authority_fenced_and_blocks_retry() {
    actix::System::new().block_on(async {
        let fixture=Fixture::open().await;
        fixture.app.start_workers().await.unwrap();
        let receipt=fixture.app.ingest_text(&fixture.actor,fixture.collection,"unclean.txt","fixture content",Some("unclean".into()),"unclean").await.unwrap();
        let job:JobId=receipt["job"].as_str().unwrap().parse().unwrap();
        let mut observed=false;
        for _ in 0..250 {
            let failed:bool=sqlx::query_scalar("SELECT detail->>'cleanup_error' IS NOT NULL FROM context.jobs WHERE id=$1")
                .bind(Uuid::from(job)).fetch_one(fixture.db.pool()).await.unwrap();
            if failed {observed=true;break;}
            actix::clock::sleep(Duration::from_millis(20)).await;
        }
        assert!(observed,"cleanup failure was incorrectly marked settled");
        let status=fixture.app.job_status(&fixture.actor,job).await.unwrap();
        assert_eq!(status["state"],"failed");assert_eq!(status["cleanup_pending"],true);
        assert!(fixture.app.retry_job(&fixture.actor,job,"unsafe-retry").await.is_err());
        let row=sqlx::query("SELECT active,(authority->>'fenced')::boolean AS fenced FROM context.workflow_authority WHERE job=$1")
            .bind(Uuid::from(job)).fetch_one(fixture.db.pool()).await.unwrap();
        assert!(row.get::<bool,_>("active") &&row.get::<bool,_>("fenced"));
        fixture.close().await;
    });
}

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; pure processing fixtures"]
fn missing_handles_outside_receipt_retention_are_not_replayed_or_marked_clean() {
    actix::System::new().block_on(async {
        let mut fixture=Fixture::open().await;
        let receipt=fixture.app.ingest_text(&fixture.actor,fixture.collection,"old.txt","fixture content",None,"old-gap").await.unwrap();
        let job:JobId=receipt["job"].as_str().unwrap().parse().unwrap();
        let hash=Sha256::digest(context_workflows::host_json(b"{}").unwrap());
        sqlx::query("INSERT INTO context.workflow_stages(job,stage,role,fingerprint,request_digest,request) VALUES($1,'extract:0','extract',$2,$3,'{}')")
            .bind(Uuid::from(job)).bind(fixture.app.workflows().fingerprint("extract").unwrap()).bind(hash.as_slice()).execute(fixture.db.pool()).await.unwrap();
        fixture.app.shutdown().await.unwrap();
        sqlx::query("UPDATE context.jobs SET created_at=now()-interval '31 days',state='failed',detail='{\"cleanup_pending\":true}' WHERE id=$1")
            .bind(Uuid::from(job)).execute(fixture.db.pool()).await.unwrap();
        fixture.app=Arc::new(Application::open(&fixture.config,fixture._dir.path()).await.unwrap());
        fixture.app.start_workers().await.unwrap();
        let mut observed=false;
        for _ in 0..250 {
            let failed:bool=sqlx::query_scalar("SELECT detail->>'cleanup_error' IS NOT NULL FROM context.jobs WHERE id=$1")
                .bind(Uuid::from(job)).fetch_one(fixture.db.pool()).await.unwrap();
            if failed {observed=true;break;}
            actix::clock::sleep(Duration::from_millis(20)).await;
        }
        assert!(observed,"old unresolved admission was silently considered clean");
        let pending=fixture.app.job_status(&fixture.actor,job).await.unwrap();assert_eq!(pending["cleanup_pending"],true);
        let absent:bool=sqlx::query_scalar("SELECT run_id IS NULL FROM context.workflow_stages WHERE job=$1 AND stage='extract:0'")
            .bind(Uuid::from(job)).fetch_one(fixture.db.pool()).await.unwrap();
        assert!(absent,"application replayed an intent beyond its safe receipt window");
        fixture.close().await;
    });
}
