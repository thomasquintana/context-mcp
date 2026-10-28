//! Actual application HTTP/stdio MCP acceptance. Every MCP call is owned by HTLK.

use context_auth::Authenticator;
use context_core::{access::Permission, ids::JobId};
use context_engine::bootstrap;
use context_storage::postgres::Database;
use context_workflows::host::{Config, Handle, JobGrant, State, WorkflowHost};
use serde_json::{Value, json};
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
#[ignore = "requires isolated PostgreSQL; launches the actual context-mcp binary"]
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
        let socket=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address=socket.local_addr().unwrap();drop(socket);
        let public=format!("http://{address}/mcp");
        let config_path=dir.path().join("server.json");
        write(&config_path,&json!({
            "bind":address.to_string(),"public_url":public,"allowed_hosts":["127.0.0.1","localhost"],"allowed_origins":[],
            "stdio_token_env":"CONTEXT_STDIO_TOKEN","max_request_bytes":1048576,
            "application":{"database_url_env":"CONTEXT_TEST_DATABASE_URL","database_connections":5,"oidc":null,
                "workflows":{"database":"server.sqlite","roles":{"fixture":{"source":"operator.htlk","catalogs":"operator.catalogs.json"}},
                    "endpoints":{},"required_deployments":[],"workers":2,"max_runs":8}}
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
        let grant=JobGrant {job:JobId::new(),principal,tenant:initial.tenant,assets:BTreeSet::new(),fingerprints:Default::default(),
            roles:BTreeSet::from(["identity".into(),"resource".into(),"create".into()])};
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

        let stdio_config=client_config(dir.path(),"client-stdio",json!({"transport":"stdio","program":binary,
            "arguments":["serve","--config",config_path,"--transport","stdio"],
            "environment":{"CONTEXT_TEST_DATABASE_URL":database_url,"CONTEXT_STDIO_TOKEN":full.secret(),"RUST_LOG":"warn"}}),"stdio");
        let host=WorkflowHost::open(&stdio_config,dir.path(),&[]).await.unwrap();
        host.grant_job(grant.clone()).unwrap();
        assert_eq!(succeeded(call(&host,grant.job,"identity",json!({})).await)["principal"],principal.to_string());
        host.shutdown().await.unwrap();
        auth.revoke_service(principal,full.id).await.unwrap();
        db.close().await;
    });
}
