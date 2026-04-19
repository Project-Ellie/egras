use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::config::AppConfig;

pub async fn build_pool(cfg: &AppConfig) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(cfg.database_max_connections)
        .connect(&cfg.database_url)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
