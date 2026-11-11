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
    /// Discover exact initial-provider descriptors through unchanged HTLK interfaces.
    DiscoverProviders {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Provision the explicitly configured unversioned object bucket as a trusted operator.
    ProvisionData {
        #[arg(long)]
        config: PathBuf,
    },
    /// Register an exact issuer/subject identity without implicitly granting access.
    RegisterIdentity {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        issuer: String,
        #[arg(long)]
        subject: String,
        #[arg(long, default_value = "human")]
        kind: String,
    },
    /// Add/remove authoritative tenant/workspace membership; grants remain separate.
    Membership {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        principal: PrincipalId,
        #[arg(long)]
        resource: uuid::Uuid,
        #[arg(long)]
        remove: bool,
    },
    /// Revoke one owner-bound service credential as a trusted local operator.
    RevokeCredential {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        principal: PrincipalId,
        #[arg(long)]
        credential: context_core::ids::CredentialId,
    },
    /// Invoke any application tool through HTLK using JSON from a file or stdin.
    Call {
        #[arg(long, default_value = "http://localhost:8089/mcp")]
        endpoint: String,
        #[arg(long, default_value = "CONTEXT_TOKEN")]
        token_env: String,
        #[arg(long)]
        allow_http: bool,
        #[arg(long)]
        tool: String,
        #[arg(long, default_value = "-")]
        arguments: PathBuf,
    },
    /// Stream a local source through authenticated verified upload/finalization contracts.
    Upload {
        #[arg(long, default_value = "http://localhost:8089/mcp")]
        endpoint: String,
        #[arg(long, default_value = "CONTEXT_TOKEN")]
        token_env: String,
        #[arg(long)]
        allow_http: bool,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        collection: context_core::ids::CollectionId,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        media_type: Option<String>,
        #[arg(long)]
        idempotency_key: String,
        #[arg(long)]
        wait: bool,
    },
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
        Command::DiscoverProviders { config, output } => {
            let file = std::fs::File::open(config)?;
            use std::io::Read;
            let mut bytes = Vec::new();
            file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 16 * 1024 * 1024 {
                return Err("discovery config exceeds bound".into());
            }
            let configuration =
                serde_json::from_slice::<context_workflows::DiscoveryConfig>(&bytes);
            let configuration = match configuration {
                Ok(value) => value,
                Err(_) => return Err("invalid discovery configuration".into()),
            };
            let catalogs = context_workflows::discover(&configuration)
                .await
                .map_err(|_| "provider discovery failed")?;
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(output)?;
            file.write_all(&serde_json::to_vec_pretty(&catalogs)?)?;
            file.sync_all()?;
            println!("Exact provider catalogs written");
        }
        Command::ProvisionData { config } => {
            let config = Config::load(&config)?;
            let data = config
                .application
                .data
                .ok_or("data configuration is required")?;
            context_storage::blobs::Blobs::new(context_storage::blobs::BlobConfig {
                endpoint: data.s3_endpoint,
                public_endpoint: data.s3_public_endpoint,
                bucket: data.s3_bucket,
                region: data.s3_region,
                access_key: std::env::var(data.s3_access_key_env)
                    .map_err(|_| "S3 access-key reference is required")?,
                secret_key: std::env::var(data.s3_secret_key_env)
                    .map_err(|_| "S3 secret reference is required")?,
                allow_http: data.s3_allow_http,
            })?
            .provision()
            .await?;
            println!("Unversioned data bucket ready");
        }
        Command::RegisterIdentity {
            config,
            issuer,
            subject,
            kind,
        } => {
            let config = Config::load(&config)?;
            let db = database(&config).await?;
            let principal = db.identity(&issuer, &subject, &kind).await?;
            println!("{}", serde_json::json!({"principal":principal}));
            db.close().await;
        }
        Command::Membership {
            config,
            principal,
            resource,
            remove,
        } => {
            let config = Config::load(&config)?;
            let db = database(&config).await?;
            let mut tx = db.transaction().await?;
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context.resources WHERE id=$1 AND kind IN ('tenant','workspace') AND NOT purging)").bind(resource).fetch_one(&mut *tx).await.map_err(|_|"membership lookup failed")?;
            if !valid {
                return Err("membership target is unavailable".into());
            }
            if remove {
                sqlx::query("DELETE FROM context.memberships WHERE principal=$1 AND resource=$2")
                    .bind(uuid::Uuid::from(principal))
                    .bind(resource)
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| "membership mutation failed")?;
            } else {
                sqlx::query("INSERT INTO context.memberships VALUES($1,$2) ON CONFLICT DO NOTHING")
                    .bind(uuid::Uuid::from(principal))
                    .bind(resource)
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| "membership mutation failed")?;
            }
            tx.commit()
                .await
                .map_err(|_| "membership mutation failed")?;
            db.close().await;
            println!("Membership updated; resource grants remain independent");
        }
        Command::RevokeCredential {
            config,
            principal,
            credential,
        } => {
            let config = Config::load(&config)?;
            let db = database(&config).await?;
            context_auth::Authenticator::open(db.clone(), None)
                .await?
                .revoke_service(principal, credential)
                .await?;
            db.close().await;
            println!("Credential revoked");
        }
        Command::Call {
            endpoint,
            token_env,
            allow_http,
            tool,
            arguments,
        } => {
            let args = context_mcp::client::arguments(&arguments)?;
            let client =
                context_mcp::client::Client::open(&endpoint, &token_env, allow_http).await?;
            let result = client.call(&tool, &args).await;
            client.shutdown().await?;
            println!("{}", serde_json::to_string_pretty(&result?)?);
        }
        Command::Upload {
            endpoint,
            token_env,
            allow_http,
            file,
            collection,
            profile,
            media_type,
            idempotency_key,
            wait,
        } => {
            let client =
                context_mcp::client::Client::open(&endpoint, &token_env, allow_http).await?;
            let result = client
                .upload(
                    &file,
                    collection,
                    profile,
                    media_type,
                    &idempotency_key,
                    wait,
                )
                .await;
            client.shutdown().await?;
            println!("{}", serde_json::to_string_pretty(&result?)?);
        }
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
