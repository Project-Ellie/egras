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
        Commands::SeedAdmin {
            email,
            username,
            password,
            role,
        } => run_seed_admin(cfg, email, username, password, role).await,
        Commands::DumpOpenapi => run_dump_openapi(),
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

    let handles = egras::build_app(pool.clone(), cfg.clone()).await?;
    let egras::AppHandles {
        router,
        audit: audit_handle,
        jobs: jobs_handle,
    } = handles;

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

    jobs_handle.shutdown().await;
    audit_handle.shutdown().await;
    pool.close().await;
    Ok(())
}

async fn run_seed_admin(
    cfg: AppConfig,
    email: String,
    username: String,
    password: String,
    role: String,
) -> anyhow::Result<()> {
    use egras::security::service::bootstrap_seed_admin::{
        bootstrap_seed_admin, SeedAdminError, SeedAdminInput,
    };

    let pool = egras::db::build_pool(&cfg).await?;
    egras::db::run_migrations(&pool).await?;

    match bootstrap_seed_admin(
        &pool,
        SeedAdminInput {
            email,
            username,
            password,
            role_code: role,
            operator_org_name: cfg.operator_org_name.clone(),
        },
    )
    .await
    {
        Ok(out) => {
            println!("{}", out.user_id);
            pool.close().await;
            Ok(())
        }
        Err(SeedAdminError::OperatorOrgNotFound(name)) => {
            eprintln!("error: operator organisation '{name}' not found — did you run migrations?");
            pool.close().await;
            std::process::exit(1);
        }
        Err(SeedAdminError::UserAlreadyExists(email)) => {
            eprintln!("error: user with email '{email}' already exists — skipping");
            pool.close().await;
            std::process::exit(1);
        }
        Err(SeedAdminError::Internal(e)) => {
            pool.close().await;
            Err(e)
        }
    }
}

fn run_dump_openapi() -> anyhow::Result<()> {
    use utoipa::OpenApi;
    let json = egras::openapi::ApiDoc::openapi()
        .to_pretty_json()
        .map_err(|e| anyhow::anyhow!("failed to serialise OpenAPI: {e}"))?;
    println!("{json}");
    Ok(())
}
