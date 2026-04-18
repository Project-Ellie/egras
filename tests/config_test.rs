use egras::config::AppConfig;

fn set_required_env() {
    std::env::set_var("EGRAS_DATABASE_URL", "postgres://e:e@localhost/e");
    std::env::set_var("EGRAS_JWT_SECRET", "a".repeat(64));
}

#[test]
fn loads_with_defaults() {
    set_required_env();
    let cfg = AppConfig::from_env().expect("config loads");
    assert_eq!(cfg.bind_address, "0.0.0.0:8080");
    assert_eq!(cfg.jwt_ttl_secs, 3600);
    assert_eq!(cfg.audit_channel_capacity, 4096);
    assert_eq!(cfg.audit_max_retries, 3);
}

#[test]
fn rejects_short_jwt_secret() {
    std::env::set_var("EGRAS_DATABASE_URL", "postgres://e:e@localhost/e");
    std::env::set_var("EGRAS_JWT_SECRET", "short");
    let err = AppConfig::from_env().expect_err("must reject short secret");
    assert!(format!("{err:#}").contains("EGRAS_JWT_SECRET"));
}
