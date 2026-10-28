//! Independent acceptance checks through unchanged HTLK public interfaces.

use context_workflows::{RunConfig, run};
use serde_json::json;

fn fixture() -> (tempfile::TempDir, RunConfig) {
    let dir = tempfile::tempdir().expect("isolated execution directory");
    std::fs::write(
        dir.path().join("workflow.htlk"),
        r#"
ir_version = "0.1"
graph acceptance.compare {
    inputs = { value = integer }
    outputs = { result = boolean }
    nodes {
        compare = eval(boolean, inputs.value > 20) { inputs = { value = integer } }
    }
    edges {
        edge input { from = inputs.value to = compare.inputs.value }
        edge output { from = compare.outputs.value to = outputs.result }
    }
}
"#,
    )
    .unwrap();
    let common = json!({
        "catalog_version": "0.1", "mcp_protocol_version": "2025-11-25",
        "servers": {}, "schema_documents": {},
    });
    let mut tools = common.clone();
    tools["tools"] = json!([]);
    let mut resources = common.clone();
    resources["resources"] = json!([]);
    resources["templates"] = json!([]);
    let mut prompts = common;
    prompts["prompts"] = json!([]);
    std::fs::write(
        dir.path().join("catalogs.json"),
        serde_json::to_vec(&json!({
            "tools": tools, "resources": resources, "prompts": prompts,
        }))
        .unwrap(),
    )
    .unwrap();
    let config: RunConfig = serde_json::from_value(json!({
        "source": "workflow.htlk", "catalogs": "catalogs.json",
        "inputs": { "value": 21 }, "endpoints": {},
        "database": "runtime.sqlite", "idempotency_key": "independent-compare",
    }))
    .unwrap();
    (dir, config)
}

#[test]
fn real_compiler_runtime_artifact_read_and_durable_admission_are_consistent() {
    let (dir, config) = fixture();
    actix::System::new().block_on(async {
        let first = run(&config, dir.path()).await.unwrap();
        assert_eq!(first.outputs, [("result".to_string(), json!(true))].into());
        // Reopen the durable store with a fresh runtime: the admission key returns
        // the same run and accepted value, rather than allocating duplicate work.
        let reopened = run(&config, dir.path()).await.unwrap();
        assert_eq!(reopened.run_id, first.run_id);
        assert_eq!(reopened.fingerprint, first.fingerprint);
        assert_eq!(reopened.outputs, first.outputs);
    });
}

#[test]
fn input_schema_rejection_does_not_produce_a_success_report() {
    let (dir, mut config) = fixture();
    config
        .inputs
        .insert("value".into(), json!("a string cannot become an integer"));
    actix::System::new().block_on(async {
        let Err(error) = run(&config, dir.path()).await else {
            panic!("invalid input must not produce a successful run report");
        };
        let error = error
            .downcast_ref::<htlk::runtime::RuntimeError>()
            .expect("rejected by runtime input admission, not compilation or I/O");
        assert_eq!(error.code(), htlk::runtime::failure::FailureCode::Input);
    });
}

#[test]
fn unknown_operator_configuration_is_rejected() {
    assert!(
        serde_json::from_value::<RunConfig>(json!({
            "source": "a", "catalogs": "b", "inputs": {}, "endpoints": {},
            "idempotency_key": "k", "unrecognized_auth_bypass": true,
        }))
        .is_err()
    );
}
