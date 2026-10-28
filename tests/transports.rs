//! Actual application HTTP/stdio MCP acceptance. Every MCP call is owned by HTLK.

use context_auth::Authenticator;
use context_core::{access::Permission, ids::JobId};
use context_engine::bootstrap;
use context_storage::postgres::Database;
use context_workflows::host::{Config, Handle, JobGrant, State, WorkflowHost};
use object_store::{
    ObjectStoreExt,
    aws::AmazonS3Builder,
    path::Path,
    signer::{Method, Signer},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{
    collections::BTreeSet,
    process::{Child, Command, Stdio},
    time::Duration,
};
use uuid::Uuid;

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn result(host: &WorkflowHost, handle: &Handle) -> State {
    for _ in 0..500 {
        match host.inspect(handle).await {
            Ok(State::Running) => {}
            Ok(state) => return state,
            Err(context_workflows::host::Error::Runtime(error))
                if error.code() == htlk::runtime::failure::FailureCode::Fenced => {}
            _ => panic!("unexpected transport workflow failure"),
        }
        actix::clock::sleep(Duration::from_millis(20)).await;
    }
    panic!("transport workflow deadline exceeded")
}

async fn call(host: &WorkflowHost, job: JobId, role: &str, request: Value) -> State {
    let handle = host
        .start(job, role, &request, &Uuid::now_v7().to_string())
        .await
        .unwrap();
    result(host, &handle).await
}

fn succeeded(state: State) -> Value {
    match state {
        State::Succeeded(value) => value,
        _ => panic!("expected successful application operation"),
    }
}

fn write(path: &std::path::Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn client_config(dir: &std::path::Path, name: &str, endpoint: Value, transport: &str) -> Config {
    let mut roles = serde_json::Map::new();
    for (role, tool) in [
        ("identity", "context_identity"),
        ("resource", "context_resource"),
        ("create", "context_create_collection"),
        ("upload", "context_create_upload"),
        ("finalize", "context_finalize_upload"),
        ("text", "context_ingest_text"),
        ("job", "context_job_status"),
        ("cancel", "context_cancel_job"),
        ("retry", "context_retry_job"),
        ("query", "context_query"),
        ("expand", "context_expand"),
        ("history", "context_asset_history"),
    ] {
        let file = dir.join(format!("client-{role}.htlk"));
        std::fs::write(
            &file,
            format!(
                r#"
ir_version = "0.1"
graph acceptance.client_{role} {{
    inputs = {{ request = json }}
    outputs = {{ result = json }}
    nodes {{ operation = call(mcp.tool("context","{tool}")) }}
    edges {{
        edge request {{ from = inputs.request to = operation.inputs.arguments }}
        edge result {{ from = operation.outputs.value to = outputs.result }}
    }}
    limits {{ max_mcp_calls = 1 }}
}}
"#
            ),
        )
        .unwrap();
        roles.insert(
            role.into(),
            json!({"source":file,"catalogs":dir.join(format!("context-{transport}.json"))}),
        );
    }
    write(
        &dir.join(format!("context-{transport}.json")),
        &context_mcp::mcp::catalog(transport),
    );
    serde_json::from_value(json!({"database":format!("{name}.sqlite"),"roles":roles,
        "endpoints":{"context-mcp":endpoint},"required_deployments":["context-mcp"],"workers":2,"max_runs":8})).unwrap()
}

#[test]
#[ignore = "requires isolated PostgreSQL/MinIO/Typesense; actual binary with pure processing fixtures"]
fn real_http_and_stdio_auth_resource_isolation_and_idempotency_run_through_htlk() {
    actix::System::new().block_on(async {
        let database_url=std::env::var("CONTEXT_TEST_DATABASE_URL").expect("isolated database");
        let db=Database::connect(&database_url,5).await.unwrap();
        let principal=db.identity("application-test",&Uuid::now_v7().to_string(),"service").await.unwrap();
        let initial=bootstrap(&db,principal,"transport owner").await.unwrap();
        let stranger=db.identity("application-test",&Uuid::now_v7().to_string(),"service").await.unwrap();
        let foreign=bootstrap(&db,stranger,"foreign resource").await.unwrap();
        let auth=Authenticator::open(db.clone(),None).await.unwrap();
        let scopes=BTreeSet::from([Permission::Read,Permission::Share,Permission::Ingest]);
        let full=auth.issue_service(principal,&scopes,Duration::from_secs(600)).await.unwrap();
        let read=auth.issue_service(principal,&BTreeSet::from([Permission::Read]),Duration::from_secs(600)).await.unwrap();
        let dir=tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("operator.htlk"),r#"
ir_version = "0.1"
graph acceptance.operator {
    inputs = { request = json }
    outputs = { result = json }
    nodes { value = eval(json, {}) }
    edges { edge result { from = value.outputs.value to = outputs.result } }
}
"#).unwrap();
        write(&dir.path().join("operator.catalogs.json"),&json!({
            "tools":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{},"schema_documents":{},"tools":[]},
            "resources":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{},"schema_documents":{},"resources":[],"templates":[]},
            "prompts":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{},"schema_documents":{},"prompts":[]},
        }));
        let mut operator_roles=json!({"fixture":{"source":"operator.htlk","catalogs":"operator.catalogs.json"}});
        for (role,value) in [
            ("extract",r#"{ document = { encoding = "json", value = { media_type = "text/plain", segments = [{kind = "text",text = "transport fixture",location = {kind = "asset"}}] } } }"#),
            ("enrich",r#"{ document = {encoding = "json",value = {summary = "Transport fixture",entities = []}} }"#),
            ("embed",r#"{ embedding = [1,0,0],space = "test" }"#),
        ] {
            std::fs::write(dir.path().join(format!("{role}.htlk")),format!(r#"
ir_version = "0.1"
graph fixture.{role} {{
    inputs = {{ request = json }}
    outputs = {{ result = json }}
    nodes {{ result = eval(json, {value}) }}
    edges {{ edge result {{ from = result.outputs.value to = outputs.result }} }}
}}
"#)).unwrap();
            operator_roles[role]=json!({"source":format!("{role}.htlk"),"catalogs":"operator.catalogs.json"});
        }
        let bucket=format!("transport-{}",Uuid::now_v7().simple());
        let s3_endpoint=std::env::var("CONTEXT_TEST_S3_ENDPOINT").expect("isolated MinIO");
        let objects=AmazonS3Builder::new().with_endpoint(&s3_endpoint).with_bucket_name(&bucket).with_region("us-east-1")
            .with_access_key_id(std::env::var("CONTEXT_TEST_S3_ACCESS_KEY").unwrap())
            .with_secret_access_key(std::env::var("CONTEXT_TEST_S3_SECRET_KEY").unwrap())
            .with_allow_http(true).with_virtual_hosted_style_request(false).build().unwrap();
        let setup=reqwest::Client::new();
        let create_bucket=objects.signed_url(Method::PUT,&Path::from(""),Duration::from_secs(60)).await.unwrap();
        assert!(setup.put(create_bucket).send().await.unwrap().status().is_success());
        let typesense=std::env::var("CONTEXT_TEST_TYPESENSE_ENDPOINT").expect("isolated Typesense");
        let index_key=std::env::var("CONTEXT_TEST_TYPESENSE_KEY").unwrap();
        let index=format!("transport_{}",Uuid::now_v7().simple());
        let socket=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address=socket.local_addr().unwrap();drop(socket);
        let public=format!("http://{address}/mcp");
        let config_path=dir.path().join("server.json");
        write(&config_path,&json!({
            "bind":address.to_string(),"public_url":public,"allowed_hosts":["127.0.0.1","localhost"],"allowed_origins":[],
            "stdio_token_env":"CONTEXT_STDIO_TOKEN","max_request_bytes":1048576,
            "application":{"database_url_env":"CONTEXT_TEST_DATABASE_URL","database_connections":5,"oidc":null,
                "workflows":{"database":"server.sqlite","roles":operator_roles,
                    "endpoints":{},"required_deployments":[],"workers":2,"max_runs":8},
                "data":{"s3_endpoint":s3_endpoint,"s3_bucket":bucket,"s3_region":"us-east-1","s3_access_key_env":"CONTEXT_TEST_S3_ACCESS_KEY",
                    "s3_secret_key_env":"CONTEXT_TEST_S3_SECRET_KEY","s3_allow_http":true,"typesense_endpoint":typesense,"typesense_key_env":"CONTEXT_TEST_TYPESENSE_KEY",
                    "spaces":{"test":{"collection":index,"dimensions":3,"document_role":"embed","query_role":"embed"}},"default_space":"test",
                    "profiles":{"default":{"media_types":["text/plain"],"extract_role":"extract","enrich_role":"enrich","space":"test","chunk_bytes":128}},"default_profile":"default",
                    "staging_directory":"staging","max_upload_bytes":1048576,"upload_lifetime_seconds":60,"max_pending_jobs":1024,"workers":2,"max_segments":100}}
        }));
        let binary=env!("CARGO_BIN_EXE_context-mcp");
        let mut process=Process(Command::new(binary).args(["serve","--config"]).arg(&config_path)
            .args(["--transport","http"]).env("CONTEXT_TEST_DATABASE_URL",&database_url)
            .env("RUST_LOG","warn").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap());
        let http=reqwest::Client::builder().timeout(Duration::from_secs(1)).build().unwrap();
        let base=format!("http://{address}");
        let mut ready=false;
        for _ in 0..100 {
            if http.get(format!("{base}/health/ready")).send().await.is_ok_and(|r|r.status().is_success()){ready=true;break;}
            assert!(process.0.try_wait().unwrap().is_none(),"actual server exited during startup");
            actix::clock::sleep(Duration::from_millis(100)).await;
        }
        assert!(ready,"actual server readiness deadline");
        let unauthorized=http.get(&public).send().await.unwrap();
        assert_eq!(unauthorized.status(),reqwest::StatusCode::UNAUTHORIZED);
        assert!(unauthorized.headers().contains_key("www-authenticate"));
        let metadata:Value=http.get(format!("{base}/.well-known/oauth-protected-resource/mcp")).send().await.unwrap().json().await.unwrap();
        assert_eq!(metadata["resource"],public);

        let config=client_config(dir.path(),"client-full",json!({"transport":"streamable_http","url":public,
            "headers":{"Authorization":format!("Bearer {}",full.secret())}}),"streamable_http");
        let host=WorkflowHost::open(&config,dir.path(),&[]).await.unwrap();
        let grant=JobGrant {job:JobId::new(),principal,tenant:initial.tenant,assets:BTreeSet::new(),fingerprints:Default::default(),fenced:false,
            roles:config.roles.keys().cloned().collect()};
        host.grant_job(grant.clone()).unwrap();
        let identity=succeeded(call(&host,grant.job,"identity",json!({})).await);
        assert_eq!(identity["principal"],principal.to_string());
        let resource=succeeded(call(&host,grant.job,"resource",json!({"resource":{"kind":"workspace","id":initial.workspace}})).await);
        assert_eq!(resource["owner"],initial.tenant.to_string());
        assert!(matches!(call(&host,grant.job,"resource",json!({"resource":{"kind":"workspace","id":foreign.workspace}})).await,State::Failed(_)));
        let create=json!({"workspace":initial.workspace,"label":"idempotent collection","idempotency_key":"one"});
        let first=succeeded(call(&host,grant.job,"create",create.clone()).await);
        let second=succeeded(call(&host,grant.job,"create",create).await);
        assert_eq!(first,second);
        let parallel=json!({"workspace":initial.workspace,"label":"parallel collection","idempotency_key":"parallel"});
        let a=host.start(grant.job,"create",&parallel,"parallel-a").await.unwrap();
        let b=host.start(grant.job,"create",&parallel,"parallel-b").await.unwrap();
        assert_eq!(succeeded(result(&host,&a).await),succeeded(result(&host,&b).await));
        let count=sqlx::query("SELECT count(*) AS count FROM context.resources WHERE parent=$1 AND label='parallel collection'")
            .bind(Uuid::from(initial.workspace)).fetch_one(db.pool()).await.unwrap().get::<i64,_>("count");
        assert_eq!(count,1);
        let hash=Sha256::digest(b"transport fixture").iter().map(|v|format!("{v:02x}")).collect::<String>();
        let upload_request=json!({"collection":initial.collection,"name":"transport.txt","media_type":"text/plain","size_bytes":17,"sha256":hash,"idempotency_key":"upload-http"});
        let transfer=succeeded(call(&host,grant.job,"upload",upload_request.clone()).await);
        let rest_upload:Value=setup.post(format!("{base}/uploads")).bearer_auth(full.secret()).json(&upload_request)
            .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        assert_eq!(rest_upload["upload"],transfer["upload"]);
        let mut put=setup.put(transfer["url"].as_str().unwrap()).body("transport fixture");
        for (name,value) in transfer["headers"].as_object().unwrap(){put=put.header(name,value.as_str().unwrap());}
        assert!(put.send().await.unwrap().status().is_success());
        // Exercise both REST upload endpoints and MCP finalization against the same receipt.
        let finalized:Value=setup.post(format!("{base}/uploads/{}/complete",transfer["upload"].as_str().unwrap())).bearer_auth(full.secret())
            .json(&json!({"idempotency_key":"finalize-http"})).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        assert_eq!(succeeded(call(&host,grant.job,"finalize",json!({"upload":transfer["upload"],"idempotency_key":"finalize-http"})).await),finalized);
        wait_job(&host,grant.job,&finalized["job"]).await;
        let context=succeeded(call(&host,grant.job,"query",json!({"scope":{"kind":"workspace","id":initial.workspace},"query":"transport fixture","token_budget":1})).await);
        assert_eq!(context["evidence"].as_array().unwrap().len(),1);assert!(context["tokens"]["used"].as_u64().unwrap()<=1);
        let expanded=succeeded(call(&host,grant.job,"expand",json!({"pointer":{"kind":"content","content":context["evidence"][0]["id"]}})).await);assert_eq!(expanded["body"]["text"],"transport fixture");
        assert_eq!(succeeded(call(&host,grant.job,"history",json!({"asset":finalized["asset"]})).await)["revisions"].as_array().unwrap().len(),1);
        let rest:Value=setup.get(format!("{base}/jobs/{}",finalized["job"].as_str().unwrap())).bearer_auth(full.secret())
            .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
        assert_eq!(rest["state"],"succeeded");
        assert_eq!(setup.get(format!("{base}/jobs/{}",finalized["job"].as_str().unwrap())).send().await.unwrap().status(),reqwest::StatusCode::UNAUTHORIZED);
        // A persisted cancellation fence precedes engine startup. Missing admission
        // handles can be replayed without invoking a new external mutation.
        let mut fenced=grant.clone();fenced.job=JobId::new();fenced.fenced=true;
        host.grant_job(fenced.clone()).unwrap();
        let denied=json!({"workspace":initial.workspace,"label":"fenced mutation","idempotency_key":"fenced"});
        let blocked=host.start(fenced.job,"create",&denied,"lost-handle").await.unwrap();
        match result(&host,&blocked).await {
            State::Failed(code)=>assert_eq!(code,htlk::runtime::failure::FailureCode::Dependency,"HTLK propagates the denied call as graph dependency failure"),
            _=>panic!("fenced run did not fail"),
        }
        host.shutdown().await.unwrap();
        let host=WorkflowHost::open(&config,dir.path(),std::slice::from_ref(&fenced)).await.unwrap();
        let replay=host.start(fenced.job,"create",&denied,"lost-handle").await.unwrap();
        assert_eq!(replay.run_id,blocked.run_id);
        host.cancel(&replay,"settle").await.unwrap();
        let mut stale=fenced.clone();stale.fenced=false;host.grant_job(stale).unwrap();
        assert!(host.allow_cleanup(fenced.job,&BTreeSet::from(["ungranted".into()])).is_err());
        host.allow_cleanup(fenced.job,&BTreeSet::from(["identity".into()])).unwrap();
        assert_eq!(succeeded(call(&host,fenced.job,"identity",json!({})).await)["principal"],principal.to_string());
        assert!(matches!(call(&host,fenced.job,"create",denied).await,State::Failed(htlk::runtime::failure::FailureCode::Dependency)));
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM context.resources WHERE parent=$1 AND label='fenced mutation'")
            .bind(Uuid::from(initial.workspace)).fetch_one(db.pool()).await.unwrap();
        assert_eq!(count,0,"fenced stable-key replay or normal dispatch caused an external effect");
        host.shutdown().await.unwrap();

        let read_config=client_config(dir.path(),"client-read",json!({"transport":"streamable_http","url":public,
            "headers":{"Authorization":format!("Bearer {}",read.secret())}}),"streamable_http");
        let read_host=WorkflowHost::open(&read_config,dir.path(),&[]).await.unwrap();
        read_host.grant_job(grant.clone()).unwrap();
        assert!(matches!(call(&read_host,grant.job,"create",json!({"workspace":initial.workspace,"label":"forbidden","idempotency_key":"denied"})).await,State::Failed(_)));
        auth.revoke_service(principal,read.id).await.unwrap();
        assert!(matches!(call(&read_host,grant.job,"identity",json!({})).await,State::Failed(_)));
        read_host.shutdown().await.unwrap();
        process.0.kill().unwrap();process.0.wait().unwrap();

        let mut stdio_config=client_config(dir.path(),"client-stdio",json!({"transport":"stdio","program":binary,
            "arguments":["serve","--config",config_path,"--transport","stdio"],
            "environment":{"CONTEXT_TEST_DATABASE_URL":database_url,"CONTEXT_STDIO_TOKEN":full.secret(),"RUST_LOG":"warn"}}),"stdio");
        std::fs::write(dir.path().join("stdio-roundtrip.htlk"),r#"
ir_version = "0.1"
graph acceptance.stdio_roundtrip {
    inputs = { request = json }
    outputs = { result = json }
    nodes {
        arguments = eval(json, {collection = inputs.request.collection,name = inputs.request.name,text = inputs.request.text,idempotency_key = inputs.request.idempotency_key}) {inputs = {request = json}}
        ingest = call(mcp.tool("context","context_ingest_text"))
        duplicate_arguments = eval(json, {collection = inputs.request.collection,name = inputs.request.name,text = inputs.request.text,idempotency_key = inputs.request.idempotency_key}) {inputs = {request = json,first = json}}
        duplicate = call(mcp.tool("context","context_ingest_text"))
        poll = loop {
            inputs = {receipt = json}
            outputs = {progress = json}
            carried = {receipt = inputs.receipt}
            body {
                nodes {
                    arguments = eval(json, {job = inputs.receipt.job}) {inputs = {receipt = json}}
                    inspect = call(mcp.tool("context","context_job_status"))
                }
                edges {
                    edge receipt {from = carried.receipt to = arguments.inputs.receipt}
                    edge arguments {from = arguments.outputs.value to = inspect.inputs.arguments}
                    edge progress {from = inspect.outputs.value to = outputs.progress}
                    edge next {from = carried.receipt to = next.receipt}
                }
            }
            until = outputs.progress.state == "succeeded" or outputs.progress.state == "failed" or outputs.progress.state == "cancelled"
            max_iterations = 100
        }
        query_arguments = eval(json, {scope = {kind = "workspace",id = inputs.request.workspace},query = "transport fixture"}) {inputs = {request = json,progress = json}}
        query = call(mcp.tool("context","context_query"))
        result = eval(json, {admitted = inputs.admitted,duplicate = inputs.duplicate,progress = inputs.progress,context = inputs.context}) {inputs = {admitted = json,duplicate = json,progress = json,context = json}}
    }
    edges {
        edge request {from = inputs.request to = arguments.inputs.request}
        edge arguments {from = arguments.outputs.value to = ingest.inputs.arguments}
        edge request_again {from = inputs.request to = duplicate_arguments.inputs.request}
        edge first {from = ingest.outputs.value to = duplicate_arguments.inputs.first}
        edge duplicate_arguments {from = duplicate_arguments.outputs.value to = duplicate.inputs.arguments}
        edge receipt {from = duplicate.outputs.value to = poll.inputs.receipt}
        edge query_request {from = inputs.request to = query_arguments.inputs.request}
        edge progress {from = poll.outputs.progress to = query_arguments.inputs.progress}
        edge query_arguments {from = query_arguments.outputs.value to = query.inputs.arguments}
        edge admitted {from = ingest.outputs.value to = result.inputs.admitted}
        edge duplicate {from = duplicate.outputs.value to = result.inputs.duplicate}
        edge result_progress {from = poll.outputs.progress to = result.inputs.progress}
        edge context {from = query.outputs.value to = result.inputs.context}
        edge result {from = result.outputs.value to = outputs.result}
    }
    postconditions = outputs.result.progress.state == "succeeded"
    limits {max_mcp_calls = 103}
}
"#).unwrap();
        stdio_config.roles.insert("roundtrip".into(),serde_json::from_value(json!({"source":"stdio-roundtrip.htlk","catalogs":dir.path().join("context-stdio.json")})).unwrap());
        let host=WorkflowHost::open(&stdio_config,dir.path(),&[]).await.unwrap();
        let mut stdio_grant=grant.clone();stdio_grant.roles.insert("roundtrip".into());host.grant_job(stdio_grant).unwrap();
        assert_eq!(succeeded(call(&host,grant.job,"identity",json!({})).await)["principal"],principal.to_string());
        let roundtrip=succeeded(call(&host,grant.job,"roundtrip",json!({"collection":initial.collection,"workspace":initial.workspace,"name":"stdio.txt","text":"transport fixture","idempotency_key":"text-stdio"})).await);
        assert_eq!(roundtrip["admitted"],roundtrip["duplicate"]);assert_eq!(roundtrip["context"]["evidence"].as_array().unwrap().len(),1,"duplicate text is selected once across transports");
        host.shutdown().await.unwrap();
        auth.revoke_service(principal,full.id).await.unwrap();
        let keys:Vec<String>=sqlx::query_scalar("SELECT o.object_key FROM context.objects o JOIN context.content c ON c.id=o.content WHERE c.workspace=$1")
            .bind(Uuid::from(initial.workspace)).fetch_all(db.pool()).await.unwrap();
        for key in keys {objects.delete(&Path::from(key)).await.unwrap();}
        sqlx::query("DELETE FROM context.objects WHERE content IN(SELECT id FROM context.content WHERE workspace=$1)")
            .bind(Uuid::from(initial.workspace)).execute(db.pool()).await.unwrap();
        sqlx::query("DELETE FROM context.resources WHERE id=ANY($1)").bind(vec![Uuid::from(initial.tenant),Uuid::from(foreign.tenant)]).execute(db.pool()).await.unwrap();
        let delete_bucket=objects.signed_url(Method::DELETE,&Path::from(""),Duration::from_secs(60)).await.unwrap();
        assert!(setup.delete(delete_bucket).send().await.unwrap().status().is_success());
        assert!(setup.delete(format!("{typesense}/collections/{index}")).header("X-TYPESENSE-API-KEY",index_key).send().await.unwrap().status().is_success());
        sqlx::query("DELETE FROM context.index_bindings WHERE collection=$1").bind(&index).execute(db.pool()).await.unwrap();
        db.close().await;
    });
}

async fn wait_job(host: &WorkflowHost, job: JobId, id: &Value) {
    for _ in 0..100 {
        let status = succeeded(call(host, job, "job", json!({"job":id})).await);
        match status["state"].as_str() {
            Some("succeeded") => return,
            Some("failed" | "cancelled") => panic!("transport ingestion did not succeed"),
            _ => actix::clock::sleep(Duration::from_millis(30)).await,
        }
    }
    panic!("transport ingestion exceeded deadline");
}
