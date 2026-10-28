//! Explicit operator-run catalog discovery and compiled-graph execution.

use context_workflows::{DiscoveryConfig, Result, RunConfig};
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: provider_spike <discover|run> <config.json> <output.json>".into());
    }
    let mode = args[0].to_str().ok_or("invalid mode")?;
    let path = Path::new(&args[1]);
    let bytes = std::fs::read(path)?;
    let output = actix::System::new().block_on(async {
        match mode {
            "discover" => {
                let config: DiscoveryConfig = serde_json::from_slice(&bytes)?;
                context_workflows::discover(&config).await
            }
            "run" => {
                let config: RunConfig = serde_json::from_slice(&bytes)?;
                let report =
                    context_workflows::run(&config, path.parent().unwrap_or(Path::new(".")))
                        .await?;
                Ok(serde_json::to_value(report)?)
            }
            _ => Err("expected discover or run".into()),
        }
    })?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(&output)?)?;
    println!("{mode} completed; wrote explicit operator output");
    Ok(())
}
