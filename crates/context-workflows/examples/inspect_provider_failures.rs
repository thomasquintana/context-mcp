//! Operator-only bounded failure diagnostics; prints codes/status, never raw values.

use context_workflows::Result;
use htlk::{
    compiler::executable::{
        JsonDocument,
        cbor::{Limits, Value},
    },
    runtime::{
        identity::RunId,
        store::{RecordFamily, SqliteStore, StoreConfig},
    },
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: inspect_provider_failures <runtime.sqlite> <run-id>".into());
    }
    actix::System::new().block_on(async {
        let limits = Limits::default();
        let store = SqliteStore::open(&args[0], StoreConfig::default()).await?;
        let run = RunId::new(args[1].clone(), &limits)?;
        let mut after = None;
        loop {
            let page = store.records(&run, RecordFamily::Response, after.as_deref(), 16).await?;
            if page.is_empty() { break; }
            for record in &page {
                let Value::Map(payload) = &record.payload else { continue; };
                if let Some(Value::Text(code)) = payload.get("failure_code") {
                    println!("runtime_failure={code}");
                }
                let Some(Value::Bytes(raw)) = payload.get("raw") else { continue; };
                let response = JsonDocument::new(raw, &limits)?;
                let json: serde_json::Value = serde_json::from_slice(response.as_bytes())?;
                if json.get("isError").and_then(|v| v.as_bool()) == Some(true) {
                    let error = &json["structuredContent"]["error"];
                    // Explicit metadata allowlist: provider error messages may contain
                    // submitted content, so never print them or the response object.
                    println!("provider_failure={}", serde_json::json!({
                        "code": error.get("code"), "status": error.get("status"),
                        "retryable": error.get("retryable"), "request_id": error.get("request_id"),
                    }));
                } else {
                    println!("provider_response=received");
                }
            }
            after = page.last().map(|r| r.key.id.clone());
        }
        Ok(())
    })
}
