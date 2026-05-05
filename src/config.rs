use figment::{providers::Env, Figment};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub database_url: String,
    #[serde(default = "default_db_max")]
    pub database_max_connections: u32,
    #[serde(default = "default_bind")]
    pub bind_address: String,
    pub jwt_secret: String,
    #[serde(default = "default_jwt_ttl")]
    pub jwt_ttl_secs: i64,
    #[serde(default = "default_jwt_iss")]
    pub jwt_issuer: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default)]
    pub cors_allowed_origins: String,
    #[serde(default = "default_reset_ttl")]
    pub password_reset_ttl_secs: i64,
    #[serde(default = "default_operator_name")]
    pub operator_org_name: String,
    #[serde(default = "default_audit_capacity")]
    pub audit_channel_capacity: usize,
    #[serde(default = "default_audit_retries")]
    pub audit_max_retries: u32,
    #[serde(default = "default_audit_backoff")]
    pub audit_retry_backoff_ms_initial: u64,
}

fn default_db_max() -> u32 {
    10
}
fn default_bind() -> String {
    "0.0.0.0:8080".into()
}
fn default_jwt_ttl() -> i64 {
    3600
}
fn default_jwt_iss() -> String {
    "egras".into()
}
fn default_log_level() -> String {
    "info".into()
}
fn default_log_format() -> String {
    "json".into()
}
fn default_reset_ttl() -> i64 {
    3600
}
fn default_operator_name() -> String {
    "operator".into()
}
fn default_audit_capacity() -> usize {
    4096
}
fn default_audit_retries() -> u32 {
    3
}
fn default_audit_backoff() -> u64 {
    100
}

#[cfg(any(test, feature = "testing"))]
impl AppConfig {
    pub fn default_for_tests() -> Self {
        Self {
            database_url: std::env::var("TEST_DATABASE_URL")
                .unwrap_or_else(|_| "postgres://egras:egras@127.0.0.1:15432/postgres".into()),
            database_max_connections: 5,
            bind_address: "127.0.0.1:0".into(),
            jwt_secret: "x".repeat(32),
            jwt_ttl_secs: 3600,
            jwt_issuer: "egras-test".into(),
            log_level: "info".into(),
            log_format: "json".into(),
            cors_allowed_origins: "http://localhost:3000".into(),
            password_reset_ttl_secs: 3600,
            operator_org_name: "operator".into(),
            audit_channel_capacity: 128,
            audit_max_retries: 0,
            audit_retry_backoff_ms_initial: 10,
        }
    }
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let cfg: AppConfig = Figment::new()
            .merge(Env::prefixed("EGRAS_"))
            .extract()
            .map_err(|e| anyhow::anyhow!("failed to load config: {e}"))?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.jwt_secret.len() < 32 {
            anyhow::bail!(
                "EGRAS_JWT_SECRET must be at least 32 bytes (got {})",
                self.jwt_secret.len()
            );
        }
        if !["json", "pretty"].contains(&self.log_format.as_str()) {
            anyhow::bail!(
                "EGRAS_LOG_FORMAT must be 'json' or 'pretty' (got {})",
                self.log_format
            );
        }
        // EGRAS_CORS_ALLOWED_ORIGINS is only required when the HTTP server starts;
        // CLI subcommands like `seed-admin` don't need it. Validation is enforced
        // lazily in `lib::build_cors`.
        Ok(())
    }
}
