//! context-mcp server and explicit operator commands.

use clap::{Parser, Subcommand, ValueEnum};
use context_core::{access::Permission, ids::PrincipalId};
use context_mcp::{config::Config, transport::Result};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Parser)]
#[command(version, about = "HTLK-orchestrated multi-tenant context engine")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Serve authenticated MCP over HTTP or explicitly scoped stdio.
    Serve {
        #[arg(long)]
        config: PathBuf,
        #[arg(long, value_enum, default_value = "http")]
        transport: Transport,
    },
    /// Export exact HTLK tool catalogs without starting any service.
    Catalog {
        #[arg(long, value_enum, default_value = "http")]
        transport: Transport,
    },
    /// Apply application database migrations.
    Migrate {
        #[arg(long)]
        config: PathBuf,
    },
    /// Explicit local-operator creation of an initial hierarchy and principal grant.
    Bootstrap {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        issuer: String,
        #[arg(long)]
        subject: String,
        #[arg(long, default_value = "service")]
        kind: String,
        #[arg(long)]
        label: String,
    },
    /// Issue a scoped agent credential and write it once into a private new file.
    IssueCredential {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        principal: PrincipalId,
        #[arg(long, required = true)]
        scope: Vec<String>,
        #[arg(long, default_value_t = 86400)]
        lifetime_seconds: u64,
        #[arg(long)]
        output: PathBuf,
    },
}
#[derive(Clone, Copy, ValueEnum)]
enum Transport {
    Http,
    Stdio,
}

async fn database(config: &Config) -> Result<context_storage::postgres::Database> {
    let url = std::env::var(&config.application.database_url_env)
        .map_err(|_| "database URL environment reference is required")?;
    Ok(
        context_storage::postgres::Database::connect(&url, config.application.database_connections)
            .await?,
    )
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Catalog { transport } => println!(
            "{}",
            serde_json::to_string_pretty(&context_mcp::mcp::catalog(match transport {
                Transport::Http => "streamable_http",
                Transport::Stdio => "stdio",
            }))?
        ),
        Command::Serve { config, transport } => {
            let configuration = Config::load(&config)?;
            context_mcp::transport::serve(
                configuration,
                config.parent().unwrap_or(Path::new(".")),
                matches!(transport, Transport::Stdio),
            )
            .await?;
        }
        Command::Migrate { config } => {
            let config = Config::load(&config)?;
            let db = database(&config).await?;
            db.close().await;
            println!("Application migrations completed");
        }
        Command::Bootstrap {
            config,
            issuer,
            subject,
            kind,
            label,
        } => {
            let config = Config::load(&config)?;
            let db = database(&config).await?;
            let principal = db.identity(&issuer, &subject, &kind).await?;
            let created = context_engine::bootstrap(&db, principal, &label).await?;
            println!("{}", serde_json::to_string_pretty(&created)?);
            db.close().await;
        }
        Command::IssueCredential {
            config,
            principal,
            scope,
            lifetime_seconds,
            output,
        } => {
            let scopes = scope
                .into_iter()
                .map(|scope| serde_json::from_value(serde_json::Value::String(scope)))
                .collect::<std::result::Result<BTreeSet<Permission>, _>>()?;
            let config = Config::load(&config)?;
            let db = database(&config).await?;
            let auth = context_auth::Authenticator::open(db.clone(), None).await?;
            let issued = auth
                .issue_service(principal, &scopes, Duration::from_secs(lifetime_seconds))
                .await?;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = match options.open(&output) {
                Ok(file) => file,
                Err(error) => {
                    auth.revoke_service(principal, issued.id).await?;
                    return Err(error.into());
                }
            };
            use std::io::Write;
            if let Err(error) = file
                .write_all(issued.secret().as_bytes())
                .and_then(|_| file.sync_all())
            {
                auth.revoke_service(principal, issued.id).await?;
                drop(file);
                std::fs::remove_file(&output)?;
                return Err(error.into());
            }
            println!(
                "Credential {} written to its private output file",
                issued.id
            );
            db.close().await;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    actix::System::new().block_on(run(Cli::parse()))
}
