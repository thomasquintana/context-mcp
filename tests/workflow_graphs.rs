//! Compile every shipped graph against explicit fixture-only provider catalogs.
//!
//! These checks establish compiler/graph compatibility, not provider semantics.

use htlk::compiler::{MCPCatalogInputs, executable::cbor::Limits};
use serde_json::json;

#[test]
fn shipped_graphs_compile_with_uniform_tool_results() {
    let limits = Limits::default();
    let mut catalog = context_mcp::mcp::catalog("stdio");
    for (alias, names) in [
        ("llm", vec!["generate", "embed", "transcribe"]),
        (
            "sandbox",
            vec![
                "sandbox_create",
                "sandbox_exec",
                "sandbox_destroy",
                "sandbox_list",
                "profile_list",
            ],
        ),
    ] {
        catalog["tools"]["servers"][alias] = json!({
            "deployment_id": format!("fixture-{alias}"), "transport":"stdio",
            "implementation_name":format!("fixture-{alias}"), "implementation_version":"1",
        });
        for name in names {
            catalog["tools"]["tools"].as_array_mut().unwrap().push(
                json!({"server":alias,"descriptor":{
                    "name":name,"inputSchema":{"type":"object"},
                    "outputSchema":{"type":"object"},
                }}),
            );
        }
    }
    let catalogs = MCPCatalogInputs::from_json(&serde_json::to_vec(&catalog).unwrap(), &limits)
        .expect("explicit fixture catalogs");
    let (compiler, _) = context_workflows::compiler(&limits).unwrap();
    for directory in ["examples/operator", "examples/integration"] {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(directory);
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "htlk")
            {
                compiler
                    .compile_source(&std::fs::read_to_string(&path).unwrap(), &catalogs)
                    .unwrap_or_else(|errors| panic!("{}: {errors:?}", path.display()));
            }
        }
    }
}
