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
        let dir = match std::env::var("CONTEXT_TEST_WORKDIR") {
            Ok(path) => tempfile::Builder::new()
                .prefix("application-")
                .tempdir_in(path)
                .unwrap(),
            Err(_) => tempfile::tempdir().unwrap(),
        };
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
                &BTreeSet::from([
                    Permission::Read,
                    Permission::Ingest,
                    Permission::WriteMemory,
                    Permission::Synthesize,
                    Permission::Share,
                    Permission::Purge,
                ]),
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
                r#"{ document = { encoding = "json", value = { summary = "Fixture summary", entities = [{name = "Ocean", entity_type = "place"},{name = "Blue",entity_type = "color"}], relationships = [{from = "Ocean",to = "Blue",relationship = "has_color"}] } } }"#,
            ),
            ("embed", r#"{ embedding = [1,0,0], space = "test" }"#),
            ("cleanup", r#"{ done = true }"#),
            ("bad_cleanup", r#"{ done = false }"#),
            (
                "reconcile",
                r#"{ document = {encoding = "json",value = {relation = "complementary",rationale = "Explicit pure fixture classification"}} }"#,
            ),
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
        let rendered_hash = Sha256::digest(b"derived fixture bytes")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        std::fs::write(dir.path().join("render.htlk"),format!(r#"
ir_version = "0.1"
graph fixture.render {{
    inputs = {{ request = json }}
    outputs = {{ result = json }}
    nodes {{ result = eval(json, {{ document = {{encoding = "json",value = {{file = "selection.bin",media_type = "application/octet-stream",sha256 = "{rendered_hash}"}}}} }}) }}
    edges {{ edge result {{from = result.outputs.value to = outputs.result}} }}
}}
"#)).unwrap();
        for (role, citation) in [
            ("synthesize", "inputs.request.evidence[0]"),
            ("bad_synthesize", "\"00000000-0000-0000-0000-000000000000\""),
        ] {
            std::fs::write(dir.path().join(format!("{role}.htlk")),format!(r#"
ir_version = "0.1"
graph fixture.{role} {{
    inputs = {{request = json}}
    outputs = {{result = json}}
    nodes {{result = eval(json, {{document = {{encoding = "json",value = {{text = "The ocean is blue.",citations = [{citation}]}}}}}}) {{inputs = {{request = json}}}} }}
    edges {{edge request {{from = inputs.request to = result.inputs.request}} edge result {{from = result.outputs.value to = outputs.result}}}}
}}
"#)).unwrap();
        }
        let mut roles = serde_json::Map::new();
        for role in [
            "extract",
            "broken",
            "enrich",
            "embed",
            "cleanup",
            "bad_cleanup",
            "render",
            "reconcile",
            "synthesize",
            "bad_synthesize",
        ] {
            roles.insert(
                role.into(),
                json!({"source":format!("{role}.htlk"),"catalogs":"catalogs.json","checkpoints":["result"]}),
            );
        }
        let profile = json!({"media_types":["text/plain"],"extract_role":"extract","media_role":"render","enrich_role":"enrich","space":"test","chunk_bytes":128,"cleanup_role":"cleanup","cleanup_node":"result"});
        let mut broken = profile.clone();
        broken["extract_role"] = json!("broken");
        let mut unclean = broken.clone();
        unclean["cleanup_role"] = json!("bad_cleanup");
        let typesense = std::env::var("CONTEXT_TEST_TYPESENSE_ENDPOINT").unwrap();
        let index_key = std::env::var("CONTEXT_TEST_TYPESENSE_KEY").unwrap();
        let index = format!("ingestion_{}", Uuid::now_v7().simple());
        let config:Config=serde_json::from_value(json!({"database_url_env":"CONTEXT_TEST_DATABASE_URL","database_connections":8,"oidc":null,
            "workflows":{"database":"runtime.sqlite","roles":roles,"endpoints":{},"required_deployments":[],"workers":2,"max_runs":16},
            "data":{"reconcile_role":"reconcile","synthesis_role":"synthesize","s3_endpoint":endpoint,"s3_bucket":bucket,"s3_region":"us-east-1","s3_access_key_env":"CONTEXT_TEST_S3_ACCESS_KEY",
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

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; exact HTTP source and pure roles"]
fn exact_url_acquisition_is_bounded_allowlisted_and_recovers_child_receipts() {
    actix::System::new().block_on(async {
        use tokio::io::{AsyncReadExt,AsyncWriteExt};
        let mut fixture=Fixture::open().await;
        fixture.app.shutdown().await.unwrap();
        fixture.config.data.as_mut().unwrap().url_hosts.insert("127.0.0.1".into());
        fixture.config.data.as_mut().unwrap().url_allow_http=true;
        fixture.app=Arc::new(Application::open(&fixture.config,fixture._dir.path()).await.unwrap());
        fixture.app.start_workers().await.unwrap();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();
        let hits=Arc::new(std::sync::atomic::AtomicUsize::new(0));let count=hits.clone();
        let server=tokio::spawn(async move {
            loop {let (mut stream,_)=listener.accept().await.unwrap();let mut request=[0;4096];let size=stream.read(&mut request).await.unwrap();
                count.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
                let redirect=std::str::from_utf8(&request[..size]).unwrap().contains("GET /redirect ");
                let response=if redirect {"HTTP/1.1 302 Found\r\nLocation: http://invalid.example/never\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"}
                    else{"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 15\r\nConnection: close\r\n\r\nfixture content"};
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let args=context_engine::urls::IngestUrl {collection:fixture.collection,asset:None,name:"url.txt".into(),url:format!("http://{address}/exact"),media_type:"text/plain".into(),profile:None,idempotency_key:"url-1".into()};
        let admitted=fixture.app.ingest_url(&fixture.actor,args.clone()).await.unwrap();
        let job:JobId=admitted["job"].as_str().unwrap().parse().unwrap();assert_eq!(fixture.wait(job).await["state"],"succeeded");
        assert_eq!(fixture.app.ingest_url(&fixture.actor,args.clone()).await.unwrap(),admitted);
        sqlx::query("DELETE FROM context.receipts WHERE principal=$1 AND operation='ingest_url' AND idempotency_key='url-1'")
            .bind(Uuid::from(fixture.principal)).execute(fixture.db.pool()).await.unwrap();
        sqlx::query("DELETE FROM context.uploads WHERE job=$1").bind(Uuid::from(job)).execute(fixture.db.pool()).await.unwrap();
        assert_eq!(fixture.app.ingest_url(&fixture.actor,args.clone()).await.unwrap(),admitted);
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst),1,"receipt replay fetched mutable URL again");
        let mut denied=args.clone();denied.url=format!("http://localhost:{}/exact",address.port());denied.idempotency_key="denied".into();
        assert!(fixture.app.ingest_url(&fixture.actor,denied).await.is_err());
        let mut redirect=args;redirect.url=format!("http://{address}/redirect");redirect.idempotency_key="redirect".into();
        assert!(fixture.app.ingest_url(&fixture.actor,redirect).await.is_err());
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst),2,"redirect followed to an unrequested resource");
        server.abort();fixture.close().await;
    });
}

#[test]
#[ignore = "requires actual model/sandbox binaries, processing image/profile, model credential and backing services; paid calls"]
fn actual_video_model_workflows_publish_independently_checked_evidence() {
    actix::System::new().block_on(async {
        let mut fixture=Fixture::open().await;fixture.app.shutdown().await.unwrap();
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
        for role in ["extract","cleanup","enrich","embed","describe","transcribe"] {
            fixture.config.workflows.roles.insert(role.into(),context_workflows::host::Role {
                source:root.join(format!("examples/operator/{role}.htlk")),catalogs:root.join("target/providers/catalogs.json"),
                checkpoints:if role=="extract"{BTreeSet::from(["create".into()])}else{BTreeSet::new()},
            });
        }
        fixture.config.workflows.endpoints=serde_json::from_value(json!({
            "initial-model-provider":{"transport":"stdio","program":root.join("../llm-mcp/target/debug/llm-mcp"),"arguments":["serve"],"environment":{"LLM_MCP_AUTH":"api_key","LLM_MCP_TRANSPORT":"stdio"},"inherit_environment":["OPENAI_API_KEY"]},
            "initial-sandbox-provider":{"transport":"stdio","program":root.join("target/providers/mcp-sandbox"),"arguments":[],"environment":{},"inherit_environment":["ALLOW_DIRS","SANDBOX_PROFILES_DIR","DOCKER_HOST"]}
        })).unwrap();
        fixture.config.workflows.required_deployments=BTreeSet::from(["initial-model-provider".into(),"initial-sandbox-provider".into()]);
        let data=fixture.config.data.as_mut().unwrap();let mut space=data.spaces.remove("test").unwrap();space.dimensions=256;
        let name="text-embedding-3-small:256:v1";data.spaces.insert(name.into(),space);data.default_space=name.into();
        for profile in data.profiles.values_mut(){profile.space=name.into();}
        let profile=data.profiles.get_mut("default").unwrap();profile.cleanup_node=Some("create".into());profile.describe_role=Some("describe".into());profile.transcribe_role=Some("transcribe".into());profile.media_types.insert("video/mp4".into());
        assert!(fixture.http.delete(format!("{}/collections/{}",fixture.typesense,fixture.index)).header("X-TYPESENSE-API-KEY",&fixture.index_key).send().await.unwrap().status().is_success());
        fixture.app=Arc::new(Application::open(&fixture.config,fixture._dir.path()).await.unwrap());fixture.app.start_workers().await.unwrap();
        let bytes=std::fs::read(std::env::var("CONTEXT_TEST_VIDEO_FIXTURE").expect("synthetic blue video with known speech")).unwrap();
        let hash=Sha256::digest(&bytes).iter().map(|b|format!("{b:02x}")).collect::<String>();
        let transfer=fixture.app.create_upload(&fixture.actor,context_engine::uploads::CreateUpload {collection:fixture.collection,asset:None,name:"blue-with-speech.mp4".into(),media_type:"video/mp4".into(),size_bytes:bytes.len() as u64,sha256:hash,profile:None,idempotency_key:"real-video".into()}).await.unwrap();
        let mut put=fixture.http.put(transfer["url"].as_str().unwrap()).body(bytes);for(k,v)in transfer["headers"].as_object().unwrap(){put=put.header(k,v.as_str().unwrap());}assert!(put.send().await.unwrap().status().is_success());
        let admitted=fixture.app.finalize_upload(&fixture.actor,transfer["upload"].as_str().unwrap().parse().unwrap(),"real-video-final").await.unwrap();
        let job:JobId=admitted["job"].as_str().unwrap().parse().unwrap();let mut success=false;
        for _ in 0..1200 {
            let status=fixture.app.job_status(&fixture.actor,job).await.unwrap();
            match status["state"].as_str(){Some("succeeded")=>{success=true;break;},Some("failed"|"cancelled")=>panic!("actual processing failed: {status}"),_=>{}}
            actix::clock::sleep(Duration::from_millis(100)).await;
        }
        assert!(success,"actual processing exceeded deadline");
        let rows:Vec<Value>=sqlx::query_scalar("SELECT body FROM context.content WHERE producer_job=$1 AND published AND kind='chunk'").bind(Uuid::from(job)).fetch_all(fixture.db.pool()).await.unwrap();
        assert!(rows.len()>=2);let text=rows.iter().map(|v|v["text"].as_str().unwrap()).collect::<Vec<_>>().join(" ").to_lowercase();
        assert!(text.contains("blue") &&text.contains("ocean"),"model evidence disagrees with independent blue-video/known-speech fixture");
        for row in &rows {let vector=row["embedding"].as_array().unwrap();assert_eq!(vector.len(),256);assert!(vector.iter().all(|v|v.as_f64().unwrap().is_finite()));}
        let indexed:Value=fixture.http.get(format!("{}/collections/{}",fixture.typesense,fixture.index)).header("X-TYPESENSE-API-KEY",&fixture.index_key).send().await.unwrap().json().await.unwrap();
        assert_eq!(indexed["num_documents"].as_u64(),Some(rows.len() as u64));
        fixture.close().await;
    });
}

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; real hybrid index and pure embedding roles"]
fn context_budgets_navigation_and_revocation_use_authoritative_evidence() {
    actix::System::new().block_on(async {
        use context_core::{access::Resource, context::Pointer};
        let fixture = Fixture::open().await;
        fixture.app.start_workers().await.unwrap();
        let admitted = fixture
            .app
            .ingest_text(
                &fixture.actor,
                fixture.collection,
                "context.txt",
                "fixture content",
                None,
                "context",
            )
            .await
            .unwrap();
        let job: JobId = admitted["job"].as_str().unwrap().parse().unwrap();
        assert_eq!(fixture.wait(job).await["state"], "succeeded");
        let args = || context_engine::retrieval::Query {
            scope: Resource::Workspace(fixture.workspace),
            query: "fixture content".into(),
            limit: 10,
            token_budget: None,
            historical: false,
            space: None,
        };
        let map = fixture.app.query(&fixture.actor, args()).await.unwrap();
        assert_eq!(map.evidence.len(), 1);
        assert_eq!(map.evidence[0].text, "fixture content");
        assert_eq!(map.pointers.len(), 2);
        let pointer = Pointer::Content {
            content: map.evidence[0].id,
        };
        let expanded = fixture
            .app
            .expand(&fixture.actor, pointer.clone())
            .await
            .unwrap();
        assert_eq!(expanded["body"]["text"], "fixture content");
        assert!(expanded["body"].get("embedding").is_none());
        let entity = fixture
            .app
            .expand(&fixture.actor, map.pointers[0].clone())
            .await
            .unwrap();
        assert_eq!(entity["origin"], "generated");
        assert_eq!(entity["body"]["relationships"].as_array().unwrap().len(), 1);
        let mut bounded = args();
        bounded.token_budget = std::num::NonZeroU32::new(1);
        let map = fixture.app.query(&fixture.actor, bounded).await.unwrap();
        assert!(map.tokens.used <= 1);
        assert!(map.truncated);
        assert!(!map.evidence[0].text.is_empty());
        let asset = Uuid::parse_str(admitted["asset"].as_str().unwrap()).unwrap();
        sqlx::query(
            "UPDATE context.resources SET inherit=false,acl_version=acl_version+1 WHERE id=$1",
        )
        .bind(asset)
        .execute(fixture.db.pool())
        .await
        .unwrap();
        assert!(
            fixture
                .app
                .query(&fixture.actor, args())
                .await
                .unwrap()
                .evidence
                .is_empty()
        );
        assert!(fixture.app.expand(&fixture.actor, pointer).await.is_err());
        fixture.close().await;
    });
}

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; pure render role verifies durable materialization"]
fn selected_media_publishes_derived_bytes_without_replacing_the_source() {
    actix::System::new().block_on(async {
        let mut fixture = Fixture::open().await;
        fixture.app.start_workers().await.unwrap();
        let original = fixture
            .app
            .ingest_text(
                &fixture.actor,
                fixture.collection,
                "source.txt",
                "fixture content",
                None,
                "source-for-render",
            )
            .await
            .unwrap();
        assert_eq!(
            fixture
                .wait(original["job"].as_str().unwrap().parse().unwrap())
                .await["state"],
            "succeeded"
        );
        fixture.app.shutdown().await.unwrap();
        fixture.app = Arc::new(
            Application::open(&fixture.config, fixture._dir.path())
                .await
                .unwrap(),
        );
        let asset = original["asset"].as_str().unwrap().parse().unwrap();
        let revision = original["revision"].as_str().unwrap().parse().unwrap();
        let receipt = fixture
            .app
            .extract_media(
                &fixture.actor,
                context_engine::media::ExtractMedia {
                    asset,
                    revision,
                    location: context_core::content::SourceLocation::Pages {
                        first: 1.try_into().unwrap(),
                        last: 1.try_into().unwrap(),
                    },
                    profile: None,
                    idempotency_key: "render-1".into(),
                },
            )
            .await
            .unwrap();
        let job: JobId = receipt["job"].as_str().unwrap().parse().unwrap();
        let directory = fixture._dir.path().join("staging").join(job.to_string());
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("selection.bin"), b"derived fixture bytes").unwrap();
        fixture.app.start_workers().await.unwrap();
        let status = fixture.wait(job).await;
        assert_eq!(status["state"], "succeeded");
        let content = status["result"]["content"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let expanded = fixture
            .app
            .expand(
                &fixture.actor,
                context_core::context::Pointer::Content { content },
            )
            .await
            .unwrap();
        let bytes = fixture
            .http
            .get(expanded["url"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"derived fixture bytes");
        let current: Uuid =
            sqlx::query_scalar("SELECT current_revision FROM context.assets WHERE id=$1")
                .bind(Uuid::from(asset))
                .fetch_one(fixture.db.pool())
                .await
                .unwrap();
        assert_eq!(current, Uuid::from(revision));
        assert!(!directory.exists());
        fixture.close().await;
    });
}

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; pure processing roles"]
fn explicit_reprocess_preserves_history_and_live_cross_tenant_sharing() {
    actix::System::new().block_on(async {
        use context_core::{
            access::{Grant, Recipient, Resource},
            context::Pointer,
        };
        let fixture = Fixture::open().await;
        fixture.app.start_workers().await.unwrap();
        let original = fixture
            .app
            .ingest_text(
                &fixture.actor,
                fixture.collection,
                "versioned.txt",
                "fixture content",
                None,
                "history-original",
            )
            .await
            .unwrap();
        assert_eq!(
            fixture
                .wait(original["job"].as_str().unwrap().parse().unwrap())
                .await["state"],
            "succeeded"
        );
        let asset = original["asset"].as_str().unwrap().parse().unwrap();
        let revision = original["revision"].as_str().unwrap().parse().unwrap();
        let changed = fixture
            .app
            .reprocess(&fixture.actor, asset, revision, None, "reprocess")
            .await
            .unwrap();
        assert_ne!(changed["revision"], original["revision"]);
        assert_eq!(
            fixture
                .wait(changed["job"].as_str().unwrap().parse().unwrap())
                .await["state"],
            "succeeded"
        );
        assert_eq!(
            fixture
                .app
                .reprocess(&fixture.actor, asset, revision, None, "reprocess")
                .await
                .unwrap(),
            changed
        );
        assert!(
            fixture
                .app
                .reprocess(&fixture.actor, asset, revision, None, "stale-reprocess")
                .await
                .is_err()
        );
        let history = fixture
            .app
            .asset_history(&fixture.actor, asset, None, 20)
            .await
            .unwrap();
        assert_eq!(history["revisions"].as_array().unwrap().len(), 2);
        assert_eq!(
            fixture
                .app
                .asset_revision(&fixture.actor, asset, revision)
                .await
                .unwrap()["current"],
            false
        );
        let guest = fixture
            .db
            .identity("sharing-test", &Uuid::now_v7().to_string(), "service")
            .await
            .unwrap();
        let foreign = bootstrap(&fixture.db, guest, "foreign reader")
            .await
            .unwrap();
        let auth = Authenticator::open(fixture.db.clone(), None).await.unwrap();
        let token = auth
            .issue_service(
                guest,
                &BTreeSet::from([Permission::Read]),
                Duration::from_secs(600),
            )
            .await
            .unwrap();
        let actor = auth.authenticate(token.secret()).await.unwrap();
        let pointer = Pointer::Asset { asset, revision };
        assert!(fixture.app.expand(&actor, pointer.clone()).await.is_err());
        let shared = fixture
            .app
            .set_access(
                &fixture.actor,
                context_engine::sharing::SetAccess {
                    resource: Resource::Asset(asset),
                    expected_acl_version: 1,
                    inherit: true,
                    grants: vec![Grant {
                        recipient: Recipient::Tenant(foreign.tenant),
                        permissions: BTreeSet::from([Permission::Read]),
                    }],
                    idempotency_key: "share".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(shared["acl_version"], 2);
        assert!(fixture.app.expand(&actor, pointer.clone()).await.is_ok());
        fixture
            .app
            .set_access(
                &fixture.actor,
                context_engine::sharing::SetAccess {
                    resource: Resource::Asset(asset),
                    expected_acl_version: 2,
                    inherit: true,
                    grants: vec![],
                    idempotency_key: "revoke".into(),
                },
            )
            .await
            .unwrap();
        assert!(fixture.app.expand(&actor, pointer).await.is_err());
        sqlx::query("DELETE FROM context.resources WHERE id=$1")
            .bind(Uuid::from(foreign.tenant))
            .execute(fixture.db.pool())
            .await
            .unwrap();
        fixture.close().await;
    });
}

fn assertion(
    workspace: context_core::ids::WorkspaceId,
    subject: &str,
    value: context_core::memory::MemoryValue,
    version: i64,
    key: &str,
) -> context_engine::memories::WriteMemory {
    context_engine::memories::WriteMemory {
        workspace,
        memory: None,
        subject: subject.into(),
        description: "Agent assertion".into(),
        value,
        expected_version: version,
        derived_from: Vec::new(),
        citations: Vec::new(),
        derived_content: BTreeSet::new(),
        supersedes: BTreeSet::new(),
        idempotency_key: key.into(),
    }
}

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; pure reconciliation/model roles"]
fn memory_history_equivalence_complement_conflicts_and_concurrent_versions_are_preserved() {
    actix::System::new().block_on(async {
        use context_core::memory::MemoryValue;
        let fixture = Fixture::open().await;
        fixture.app.start_workers().await.unwrap();
        let numeric = |n| MemoryValue::Typed {
            value_type: "number".into(),
            value: json!(n),
        };
        let first = fixture
            .app
            .write_memory(
                &fixture.actor,
                assertion(
                    fixture.workspace,
                    "Office temperature",
                    numeric(20),
                    0,
                    "memory-1",
                ),
            )
            .await
            .unwrap();
        let memory = first["memory"].as_str().unwrap().parse().unwrap();
        let second = fixture
            .app
            .write_memory(
                &fixture.actor,
                assertion(
                    fixture.workspace,
                    "Office temperature",
                    numeric(21),
                    1,
                    "memory-2",
                ),
            )
            .await
            .unwrap();
        assert_eq!(second["view"]["state"], "conflicted");
        assert_eq!(second["basis"], "unresolved_conflict");
        let mut resolve = assertion(
            fixture.workspace,
            "Office temperature",
            numeric(22),
            2,
            "memory-3",
        );
        resolve.supersedes = second["view"]["revisions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().parse().unwrap())
            .collect();
        let resolved = fixture
            .app
            .write_memory(&fixture.actor, resolve)
            .await
            .unwrap();
        assert_eq!(resolved["view"]["state"], "current");
        assert_eq!(resolved["basis"], "explicit_supersession");
        let history = fixture
            .app
            .read_memory(&fixture.actor, memory, true, None, 20)
            .await
            .unwrap();
        assert_eq!(history["revisions"].as_array().unwrap().len(), 3);
        let (left, right) = tokio::join!(
            fixture.app.write_memory(
                &fixture.actor,
                assertion(
                    fixture.workspace,
                    "Office temperature",
                    numeric(23),
                    3,
                    "race-left"
                )
            ),
            fixture.app.write_memory(
                &fixture.actor,
                assertion(
                    fixture.workspace,
                    "Office temperature",
                    numeric(24),
                    3,
                    "race-right"
                )
            ),
        );
        assert_ne!(
            left.is_ok(),
            right.is_ok(),
            "two stale-version writes both published"
        );
        let text = |statement: &str| MemoryValue::Text {
            statement: statement.into(),
        };
        fixture
            .app
            .write_memory(
                &fixture.actor,
                assertion(
                    fixture.workspace,
                    "Ocean facts",
                    text("The ocean is blue."),
                    0,
                    "ocean-1",
                ),
            )
            .await
            .unwrap();
        let equivalent = fixture
            .app
            .write_memory(
                &fixture.actor,
                assertion(
                    fixture.workspace,
                    "Ocean facts",
                    text("The ocean is blue."),
                    1,
                    "ocean-2",
                ),
            )
            .await
            .unwrap();
        assert_eq!(equivalent["basis"], "equivalent");
        let complement = fixture
            .app
            .write_memory(
                &fixture.actor,
                assertion(
                    fixture.workspace,
                    "Ocean facts",
                    text("The ocean is deep."),
                    2,
                    "ocean-3",
                ),
            )
            .await
            .unwrap();
        assert_eq!(complement["basis"], "complementary");
        let current = fixture
            .app
            .memory_subject(&fixture.actor, fixture.workspace, " ocean   FACTS ")
            .await
            .unwrap();
        let statement = current["revisions"][0]["value"]["statement"]
            .as_str()
            .unwrap();
        assert!(statement.contains("blue") && statement.contains("deep"));
        assert_eq!(current["revisions"][0]["origin"], "generated");
        fixture.close().await;
    });
}

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; pure model roles, actual authority/grants"]
fn configured_authority_is_host_owned_and_synthesis_is_cited_and_transient_until_saved() {
    actix::System::new().block_on(async {
        use context_core::{access::{Resource,Recipient,Grant},memory::MemoryValue};
        let mut fixture=Fixture::open().await;fixture.app.shutdown().await.unwrap();fixture.config.data.as_mut().unwrap().memory_authority.insert(fixture.principal,10);
        fixture.app=Arc::new(Application::open(&fixture.config,fixture._dir.path()).await.unwrap());fixture.app.start_workers().await.unwrap();
        let guest=fixture.db.identity("memory-guest",&Uuid::now_v7().to_string(),"service").await.unwrap();let auth=Authenticator::open(fixture.db.clone(),None).await.unwrap();let token=auth.issue_service(guest,&BTreeSet::from([Permission::Read,Permission::WriteMemory]),Duration::from_secs(600)).await.unwrap();let actor=auth.authenticate(token.secret()).await.unwrap();
        fixture.app.set_access(&fixture.actor,context_engine::sharing::SetAccess {resource:Resource::Workspace(fixture.workspace),expected_acl_version:1,inherit:true,grants:vec![Grant{recipient:Recipient::Principal(guest),permissions:BTreeSet::from([Permission::Read,Permission::WriteMemory])}],idempotency_key:"guest-memory".into()}).await.unwrap();
        let first=fixture.app.write_memory(&fixture.actor,assertion(fixture.workspace,"Approved policy",MemoryValue::Typed{value_type:"boolean".into(),value:json!(true)},0,"policy-owner")).await.unwrap();
        let second=fixture.app.write_memory(&actor,assertion(fixture.workspace,"Approved policy",MemoryValue::Typed{value_type:"boolean".into(),value:json!(false)},1,"policy-guest")).await.unwrap();assert_eq!(second["basis"],"authority_rule");assert_eq!(second["view"]["revision"],first["revision"]);
        let evidence=first["content"].as_str().unwrap().parse().unwrap();
        let request=|save|context_engine::synthesis::Synthesize{workspace:fixture.workspace,question:"What is supported?".into(),evidence:BTreeSet::from([evidence]),token_budget:None,save,name:None,idempotency_key:if save{Some("save-answer".into())}else{None}};
        let transient=fixture.app.synthesize(&fixture.actor,request(false)).await.unwrap();assert_eq!(transient["saved"],false);assert_eq!(transient["citations"][0],first["content"]);
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM context.content WHERE workspace=$1 AND kind='summary'").bind(Uuid::from(fixture.workspace)).fetch_one(fixture.db.pool()).await.unwrap();assert_eq!(count,0);
        let saved=fixture.app.synthesize(&fixture.actor,request(true)).await.unwrap();assert_eq!(saved["saved"],true);assert_eq!(fixture.app.synthesize(&fixture.actor,request(true)).await.unwrap()["content"],saved["content"]);
        let dependency:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.dependencies WHERE source=$1 AND dependent=$2 AND relationship='derivation')").bind(Uuid::parse_str(first["content"].as_str().unwrap()).unwrap()).bind(Uuid::parse_str(saved["content"].as_str().unwrap()).unwrap()).fetch_one(fixture.db.pool()).await.unwrap();assert!(dependency);
        fixture.app.shutdown().await.unwrap();fixture.config.data.as_mut().unwrap().synthesis_role=Some("bad_synthesize".into());fixture.app=Arc::new(Application::open(&fixture.config,fixture._dir.path()).await.unwrap());fixture.app.start_workers().await.unwrap();
        assert!(matches!(fixture.app.synthesize(&fixture.actor,context_engine::synthesis::Synthesize{workspace:fixture.workspace,question:"Invented citation".into(),evidence:BTreeSet::from([evidence]),token_budget:None,save:false,name:None,idempotency_key:None}).await,Err(context_engine::Error::Contract)));
        fixture.close().await;
    });
}

#[test]
#[ignore = "requires isolated PostgreSQL, MinIO and Typesense; source-protected knowledge with pure model roles"]
fn source_revocation_hides_derivatives_but_preserves_independent_citation_only_assertions() {
    actix::System::new().block_on(async {
        use context_core::{
            access::Resource,
            content::{SourceLocation, SourceReference},
            memory::MemoryValue,
        };
        let fixture = Fixture::open().await;
        fixture.app.start_workers().await.unwrap();
        let source = fixture
            .app
            .ingest_text(
                &fixture.actor,
                fixture.collection,
                "support.txt",
                "fixture content",
                None,
                "support",
            )
            .await
            .unwrap();
        assert_eq!(
            fixture
                .wait(source["job"].as_str().unwrap().parse().unwrap())
                .await["state"],
            "succeeded"
        );
        let asset = source["asset"].as_str().unwrap().parse().unwrap();
        let reference = SourceReference {
            asset,
            revision: source["revision"].as_str().unwrap().parse().unwrap(),
            location: SourceLocation::Asset,
            available: true,
        };
        let mut derived = assertion(
            fixture.workspace,
            "Derived fact",
            MemoryValue::Text {
                statement: "Source-derived statement".into(),
            },
            0,
            "derived",
        );
        derived.derived_from = vec![reference.clone()];
        let derived = fixture
            .app
            .write_memory(&fixture.actor, derived)
            .await
            .unwrap();
        let mut independent = assertion(
            fixture.workspace,
            "Independent fact",
            MemoryValue::Text {
                statement: "Independently authored statement".into(),
            },
            0,
            "independent",
        );
        independent.citations = vec![reference];
        let independent = fixture
            .app
            .write_memory(&fixture.actor, independent)
            .await
            .unwrap();
        let summary = fixture
            .app
            .synthesize(
                &fixture.actor,
                context_engine::synthesis::Synthesize {
                    workspace: fixture.workspace,
                    question: "What follows?".into(),
                    evidence: BTreeSet::from([derived["content"]
                        .as_str()
                        .unwrap()
                        .parse()
                        .unwrap()]),
                    token_budget: None,
                    save: true,
                    name: None,
                    idempotency_key: Some("protected-summary".into()),
                },
            )
            .await
            .unwrap();
        fixture
            .app
            .set_access(
                &fixture.actor,
                context_engine::sharing::SetAccess {
                    resource: Resource::Asset(asset),
                    expected_acl_version: 1,
                    inherit: false,
                    grants: Vec::new(),
                    idempotency_key: "remove-source-read".into(),
                },
            )
            .await
            .unwrap();
        assert!(
            fixture
                .app
                .read_memory(
                    &fixture.actor,
                    derived["memory"].as_str().unwrap().parse().unwrap(),
                    false,
                    None,
                    20
                )
                .await
                .is_err()
        );
        assert!(
            fixture
                .app
                .expand(
                    &fixture.actor,
                    context_core::context::Pointer::Content {
                        content: summary["content"].as_str().unwrap().parse().unwrap()
                    }
                )
                .await
                .is_err()
        );
        let retained = fixture
            .app
            .read_memory(
                &fixture.actor,
                independent["memory"].as_str().unwrap().parse().unwrap(),
                false,
                None,
                20,
            )
            .await
            .unwrap();
        assert_eq!(retained["revisions"][0]["citations"][0]["available"], false);
        assert_eq!(retained["revisions"][0]["origin"], "agent_assertion");
        let map = fixture
            .app
            .query(
                &fixture.actor,
                context_engine::retrieval::Query {
                    scope: Resource::Workspace(fixture.workspace),
                    query: "statement".into(),
                    limit: 20,
                    token_budget: None,
                    historical: false,
                    space: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(map.evidence.len(), 1);
        assert_eq!(
            map.evidence[0].id.to_string(),
            independent["content"].as_str().unwrap()
        );
        fixture.close().await;
    });
}
