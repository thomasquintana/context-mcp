//! Long-lived host isolation, authority, recovery and existing native dispatch.

use context_core::ids::{JobId, PrincipalId, TenantId};
use context_workflows::host::{Config, Error, Handle, JobGrant, State, WorkflowHost};
use htlk::runtime::failure::FailureCode;
use serde_json::json;
use std::{collections::BTreeSet, time::Duration};

fn fixture() -> (tempfile::TempDir, Config) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("role.htlk"), r#"
ir_version = "0.1"
graph acceptance.role {
    inputs = { request = json }
    outputs = { result = json }
    nodes { result = eval(json, { marker = inputs.request.marker }) { inputs = { request = json } } }
    edges {
        edge request { from = inputs.request to = result.inputs.request }
        edge result { from = result.outputs.value to = outputs.result }
    }
}
"#).unwrap();
    let catalogs = json!({
        "tools":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{},"schema_documents":{},"tools":[]},
        "resources":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{},"schema_documents":{},"resources":[],"templates":[]},
        "prompts":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25","servers":{},"schema_documents":{},"prompts":[]},
    });
    std::fs::write(
        dir.path().join("catalogs.json"),
        serde_json::to_vec(&catalogs).unwrap(),
    )
    .unwrap();
    let config = serde_json::from_value(json!({
        "database":"runtime.sqlite", "workers":2, "max_runs":8,
        "roles":{"echo":{"source":"role.htlk","catalogs":"catalogs.json"},"other":{"source":"role.htlk","catalogs":"catalogs.json"}},
        "endpoints":{},"required_deployments":[],
    })).unwrap();
    (dir, config)
}

fn identity(role: &str) -> JobGrant {
    JobGrant {
        job: JobId::new(),
        principal: PrincipalId::new(),
        tenant: TenantId::new(),
        assets: BTreeSet::new(),
        roles: BTreeSet::from([role.into()]),
    }
}

async fn result(host: &WorkflowHost, handle: &Handle) -> serde_json::Value {
    for _ in 0..250 {
        match host.inspect(handle).await {
            Ok(State::Running) => {}
            Ok(State::Succeeded(value)) => return value,
            Err(Error::Runtime(error)) if error.code() == FailureCode::Fenced => {}
            _ => panic!("unexpected workflow outcome"),
        }
        actix::clock::sleep(Duration::from_millis(20)).await;
    }
    panic!("bounded workflow deadline exceeded")
}

#[test]
fn shared_host_isolates_jobs_and_rejects_forged_identity_and_ungranted_role() {
    let (dir, config) = fixture();
    actix::System::new().block_on(async {
        let host = WorkflowHost::open(&config, dir.path(), &[]).await.unwrap();
        let a = identity("echo");
        let b = identity("echo");
        host.grant_job(a.clone()).unwrap();
        host.grant_job(b.clone()).unwrap();
        let left = host.start(a.job, "echo", &json!({"marker":"left","tenant":b.tenant,"principal":b.principal}), "left").await.unwrap();
        let right = host.start(b.job, "echo", &json!({"marker":"right"}), "right").await.unwrap();
        assert_eq!(result(&host, &left).await, json!({"marker":"left"}));
        assert_eq!(result(&host, &right).await, json!({"marker":"right"}));
        let forged = Handle { job:b.job, ..left.clone() };
        assert!(matches!(host.inspect(&forged).await, Err(Error::Runtime(error)) if error.code()==FailureCode::Policy));
        assert!(matches!(host.start(a.job, "other", &json!({"marker":"bad"}), "bad-role").await, Err(Error::Unauthorized)));
        host.revoke_job(a.job).unwrap();
        assert!(matches!(host.inspect(&left).await, Err(Error::Runtime(error)) if error.code()==FailureCode::Policy));
        assert!(matches!(host.start(a.job,"echo",&json!({"marker":"revoked"}),"revoked").await,Err(Error::Unauthorized)));
        assert!(matches!(host.grant_job(a), Err(Error::Unauthorized)));
        host.shutdown().await.unwrap();
    });
}

#[test]
fn durable_host_reopens_terminal_run_without_duplicate_admission() {
    let (dir, config) = fixture();
    actix::System::new().block_on(async {
        let a = identity("echo");
        let host = WorkflowHost::open(&config, dir.path(), std::slice::from_ref(&a))
            .await
            .unwrap();
        let handle = host
            .start(a.job, "echo", &json!({"marker":"durable"}), "same-key")
            .await
            .unwrap();
        assert_eq!(result(&host, &handle).await, json!({"marker":"durable"}));
        host.shutdown().await.unwrap();
        let host = WorkflowHost::open(&config, dir.path(), std::slice::from_ref(&a))
            .await
            .unwrap();
        let replay = host
            .start(a.job, "echo", &json!({"marker":"durable"}), "same-key")
            .await
            .unwrap();
        assert_eq!(replay.run_id, handle.run_id);
        assert_eq!(result(&host, &replay).await, json!({"marker":"durable"}));
        host.shutdown().await.unwrap();
    });
}

#[test]
fn invalid_role_ports_are_rejected_before_execution() {
    let (dir, config) = fixture();
    std::fs::write(
        dir.path().join("role.htlk"),
        r#"
ir_version = "0.1"
graph bad.role {
    inputs = { request = string }
    outputs = { result = string }
    nodes { result = eval(string, inputs.request) { inputs = { request = string } } }
    edges {
        edge request { from = inputs.request to = result.inputs.request }
        edge result { from = result.outputs.value to = outputs.result }
    }
}
"#,
    )
    .unwrap();
    actix::System::new().block_on(async {
        assert!(matches!(
            WorkflowHost::open(&config, dir.path(), &[]).await,
            Err(Error::Configuration)
        ));
    });
}

#[test]
fn required_providers_must_be_referenced_by_admitted_roles() {
    let (dir, mut config) = fixture();
    config
        .required_deployments
        .insert("missing-deployment".into());
    actix::System::new().block_on(async {
        assert!(matches!(
            WorkflowHost::open(&config, dir.path(), &[]).await,
            Err(Error::Configuration)
        ));
    });
}

#[test]
#[ignore = "requires actual sandbox binary and exact catalog snapshot environment"]
fn actual_sandbox_metadata_executes_under_the_production_host_policy() {
    let (dir, mut config) = fixture();
    let program = std::env::var("CONTEXT_TEST_SANDBOX_BINARY").expect("actual sandbox binary");
    let catalogs = std::env::var("CONTEXT_TEST_MCP_CATALOGS").expect("actual exact catalogs");
    std::fs::write(
        dir.path().join("role.htlk"),
        r#"
ir_version = "0.1"
graph acceptance.metadata {
    inputs = { request = json }
    outputs = { result = json }
    nodes { list = call(mcp.tool("sandbox","sandbox_list")) }
    edges {
        edge request { from = inputs.request to = list.inputs.arguments }
        edge result { from = list.outputs.value to = outputs.result }
    }
    limits { max_mcp_calls = 1 }
}
"#,
    )
    .unwrap();
    config.roles.get_mut("echo").unwrap().catalogs = catalogs.clone().into();
    config.roles.get_mut("other").unwrap().catalogs = catalogs.into();
    config.endpoints.insert(
        "initial-sandbox-provider".into(),
        serde_json::from_value(json!({
            "transport":"stdio","program":program,"arguments":[],
            "environment":{"ALLOW_DIRS":dir.path(),"SANDBOX_PROFILES_DIR":dir.path()},
        }))
        .unwrap(),
    );
    config
        .required_deployments
        .insert("initial-sandbox-provider".into());
    actix::System::new().block_on(async {
        let host = WorkflowHost::open(&config, dir.path(), &[]).await.unwrap();
        host.ready(&["echo".into()]).await.unwrap();
        let job = identity("echo");
        host.grant_job(job.clone()).unwrap();
        let handle = host
            .start(job.job, "echo", &json!({}), "real-metadata")
            .await
            .unwrap();
        assert_eq!(result(&host, &handle).await, json!({"sessions":[]}));
        host.shutdown().await.unwrap();
    });
}

#[test]
fn scalar_role_results_are_rejected_before_application_publication() {
    let (dir, config) = fixture();
    std::fs::write(
        dir.path().join("role.htlk"),
        r#"
ir_version = "0.1"
graph bad.scalar {
    inputs = { request = json }
    outputs = { result = json }
    nodes { result = eval(json, "not an object") }
    edges { edge result { from = result.outputs.value to = outputs.result } }
}
"#,
    )
    .unwrap();
    actix::System::new().block_on(async {
        let job = identity("echo");
        let host = WorkflowHost::open(&config, dir.path(), std::slice::from_ref(&job))
            .await
            .unwrap();
        let handle = host
            .start(job.job, "echo", &json!({}), "scalar")
            .await
            .unwrap();
        let mut rejected = false;
        for _ in 0..250 {
            match host.inspect(&handle).await {
                Ok(State::Running) => {}
                Err(Error::Configuration) => {
                    rejected = true;
                    break;
                }
                Err(Error::Runtime(error)) if error.code() == FailureCode::Fenced => {}
                _ => panic!("unexpected scalar-role result"),
            }
            actix::clock::sleep(Duration::from_millis(20)).await;
        }
        assert!(rejected, "scalar role result was not rejected");
        host.shutdown().await.unwrap();
    });
}
