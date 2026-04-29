use clap::{Parser, Subcommand};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use egras::config::AppConfig;

#[derive(Parser)]
#[command(name = "egras", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the HTTP server (default).
    Serve,
    /// Seed the first operator admin user.
    SeedAdmin {
        #[arg(long)]
        email: String,
        #[arg(long)]
        username: String,
        #[arg(long)]
        password: String,
        #[arg(long, default_value = "operator_admin")]
        role: String,
    },
    /// Dump OpenAPI 3.1 JSON to stdout.
    DumpOpenapi,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    let cfg = AppConfig::from_env()?;
    init_tracing(&cfg);

    match cli.command.unwrap_or(Commands::Serve) {
        Commands::Serve => run_serve(cfg).await,
        Commands::SeedAdmin { .. } => {
            eprintln!("seed-admin: not implemented yet (Plan 3)");
            std::process::exit(2);
        }
        Commands::DumpOpenapi => {
            eprintln!("dump-openapi: not implemented yet (Plan 3)");
            std::process::exit(2);
        }
    }
}

fn init_tracing(cfg: &AppConfig) {
    let filter = EnvFilter::try_new(&cfg.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(filter);
    if cfg.log_format == "json" {
        registry.with(fmt::layer().json()).init();
    } else {
        registry.with(fmt::layer().pretty()).init();
    }
}

async fn run_serve(cfg: AppConfig) -> anyhow::Result<()> {
    let pool = egras::db::build_pool(&cfg).await?;
    egras::db::run_migrations(&pool).await?;

    let (router, audit_handle) = egras::build_app(pool.clone(), cfg.clone()).await?;

    let listener = tokio::net::TcpListener::bind(&cfg.bind_address).await?;
    tracing::info!(bind = %cfg.bind_address, "egras listening");

    let shutdown = async {
        let ctrl_c = async {
            tokio::signal::ctrl_c().await.ok();
        };
        #[cfg(unix)]
        let term = async {
            let mut s = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
            s.recv().await;
        };
        #[cfg(not(unix))]
        let term = std::future::pending::<()>();

        tokio::select! { _ = ctrl_c => {}, _ = term => {} }
        tracing::info!("shutdown signal received");
    };

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await?;

    audit_handle.shutdown().await;
    pool.close().await;
    Ok(())
}
