//! Fixture-only protocol regressions through the real public native HTLK host.

use context_core::ids::{JobId, PrincipalId, TenantId};
use context_workflows::host::{Config, Error, Handle, JobGrant, State, WorkflowHost};
use htlk::runtime::failure::FailureCode;
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path, time::Duration};

fn fixture() -> (tempfile::TempDir, Config, JobGrant) {
    let directory = tempfile::tempdir().unwrap();
    let server = json!({"deployment_id":"fixture","transport":"stdio",
        "implementation_name":"envelope-fixture","implementation_version":"1"});
    let mut tools = Vec::new();
    for name in [
        "inspect",
        "unstructured",
        "sandbox_create",
        "sandbox_exec",
        "sandbox_destroy",
    ] {
        let mut descriptor = json!({"name":name,"inputSchema":{"type":"object"}});
        if name != "unstructured" {
            descriptor["outputSchema"] = if name == "inspect" {
                json!({"type":"object","required":["marker"],"properties":{"marker":{"type":"string"}}})
            } else {
                json!({"type":"object"})
            };
        }
        tools.push(json!({"server":"sandbox","descriptor":descriptor}));
    }
    let catalogs = json!({
        "tools":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25",
            "servers":{"sandbox":server},"schema_documents":{},"tools":tools},
        "resources":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25",
            "servers":{},"schema_documents":{},"resources":[],"templates":[]},
        "prompts":{"catalog_version":"0.1","mcp_protocol_version":"2025-11-25",
            "servers":{},"schema_documents":{},"prompts":[]},
    });
    let catalog = directory.path().join("catalogs.json");
    std::fs::write(&catalog, serde_json::to_vec(&catalogs).unwrap()).unwrap();
    let mut roles = serde_json::Map::new();
    for name in ["inspect", "unstructured"] {
        std::fs::write(
            directory.path().join(format!("{name}.htlk")),
            format!(
                r#"
ir_version = "0.1"
graph fixture.{name} {{
    inputs = {{ request = json }}
    outputs = {{ result = json }}
    nodes {{
        operation = call(mcp.tool("sandbox", "{name}"))
        result = eval(json, inputs.response.data) {{ inputs = {{ response = McpToolResult }} }}
    }}
    edges {{
        edge request {{ from = inputs.request to = operation.inputs.arguments }}
        edge data {{ from = operation.outputs.value to = result.inputs.response }}
        edge result {{ from = result.outputs.value to = outputs.result }}
    }}
    limits {{ max_mcp_calls = 1 }}
}}
"#
            ),
        )
        .unwrap();
        roles.insert(
            name.into(),
            json!({"source":format!("{name}.htlk"),"catalogs":"catalogs.json"}),
        );
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in ["extract", "cleanup"] {
        roles.insert(name.into(), json!({"source":root.join(format!("examples/operator/{name}.htlk")),
            "catalogs":"catalogs.json","checkpoints":if name=="extract" {vec!["create"]} else {vec![]}}));
    }
    let config = serde_json::from_value(json!({
        "database":"runtime.sqlite","roles":roles,"required_deployments":["fixture"],"workers":2,"max_runs":8,
        "endpoints":{"fixture":{"transport":"stdio","program":"python3","arguments":[
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/envelope_peer.py"),
            catalog,directory.path().join("dispatch.jsonl")],"environment":{}}},
    })).unwrap();
    let grant = JobGrant {
        job: JobId::new(),
        principal: PrincipalId::new(),
        tenant: TenantId::new(),
        assets: BTreeSet::new(),
        roles: roles.keys().cloned().collect(),
        fingerprints: Default::default(),
        fenced: false,
    };
    (directory, config, grant)
}

async fn settled(host: &WorkflowHost, handle: &Handle) -> State {
    for _ in 0..1000 {
        match host.inspect(handle).await {
            Ok(State::Running) => {}
            Ok(state) => return state,
            Err(Error::Runtime(error)) if error.code() == FailureCode::Fenced => {}
            Err(error) => panic!("unexpected host failure: {error}"),
        }
        actix::clock::sleep(Duration::from_millis(20)).await;
    }
    panic!("fixture operation exceeded deadline")
}

#[test]
fn required_data_errors_large_results_and_offline_reopen_preserve_dispatch_facts() {
    let (directory, config, grant) = fixture();
    actix::System::new().block_on(async {
        let host = WorkflowHost::open(&config, directory.path(), &[]).await.unwrap();
        host.grant_job(grant.clone()).unwrap();
        // More sequential readiness lifetimes than the native client's concurrent
        // request capacity: retired sessions must not exhaust active capacity.
        for _ in 0..70 { host.ready(&["inspect".into()]).await.unwrap(); }
        assert!(!directory.path().join("dispatch.jsonl").exists());
        let mut saved = Vec::new();
        for (role, case, succeeds) in [
            ("inspect", "success", true), ("inspect", "large", true),
            ("inspect", "invalid", false), ("inspect", "text_only", false),
            ("inspect", "error", false), ("unstructured", "text_only", true),
            ("unstructured", "missing", false),
        ] {
            let handle = host.start(grant.job, role, &json!({"case":case}), &format!("{role}:{case}")).await.unwrap();
            match settled(&host, &handle).await {
                State::Succeeded(value) if succeeds => {
                    assert_eq!(value["marker"], "independent literal evidence");
                    assert!(value.get("protocol_result").is_none());
                    if case == "large" { assert_eq!(value["payload"], "Z".repeat(70_000)); }
                },
                State::Failed(_) if !succeeds => {},
                _ => panic!("unexpected {role}/{case} outcome"),
            }
            saved.push((handle, succeeds));
        }
        host.shutdown().await.unwrap();
        let before = std::fs::read(directory.path().join("dispatch.jsonl")).unwrap();
        assert_eq!(before.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()).count(), 7);
        // Remove routing so a reopened runtime cannot contact the peer at all.
        let mut offline = config;
        for endpoint in offline.endpoints.values_mut() {
            *endpoint = serde_json::from_value(json!({"transport":"stdio","program":"/absent-fixture-program","arguments":[],"environment":{}})).unwrap();
        }
        let host = WorkflowHost::open(&offline, directory.path(), &[grant]).await.unwrap();
        for (handle, succeeds) in saved {
            assert_eq!(matches!(settled(&host, &handle).await, State::Succeeded(_)), succeeds);
        }
        host.shutdown().await.unwrap();
        assert_eq!(std::fs::read(directory.path().join("dispatch.jsonl")).unwrap(), before);
    });
}

#[test]
fn shipped_extraction_checkpoint_stays_json_and_cleanup_remains_fenced() {
    let (directory, config, grant) = fixture();
    actix::System::new().block_on(async {
        let host = WorkflowHost::open(&config, directory.path(), &[]).await.unwrap();
        host.grant_job(grant.clone()).unwrap();
        let handle = host.start(grant.job, "extract", &json!({"directory":"fixture-only","job":"fixture-job"}), "extract").await.unwrap();
        // Literal peer exit=7 must fail only after normal destroy executes.
        assert!(matches!(settled(&host, &handle).await, State::Failed(FailureCode::Postconditions)));
        let checkpoint = host.checkpoint(&handle, "create").await.unwrap().unwrap();
        assert_eq!(checkpoint, json!({"id":"opaque-fixture-handle"}));
        host.fence_job(grant.job).unwrap();
        host.allow_cleanup(grant.job, &BTreeSet::from(["cleanup".into()])).unwrap();
        let cleanup = host.start(grant.job, "cleanup", &json!({"checkpoint":checkpoint}), "cleanup").await.unwrap();
        assert!(matches!(settled(&host, &cleanup).await, State::Succeeded(value) if value==json!({"done":true})));
        host.revoke_job(grant.job).unwrap();
        assert!(host.checkpoint(&handle, "create").await.is_err());
        host.shutdown().await.unwrap();
        let facts = std::fs::read_to_string(directory.path().join("dispatch.jsonl")).unwrap();
        let names: Vec<Value> = facts.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()["name"].clone()).collect();
        assert_eq!(names, vec![json!("sandbox_create"),json!("sandbox_exec"),json!("sandbox_destroy"),json!("sandbox_destroy")]);
    });
}
