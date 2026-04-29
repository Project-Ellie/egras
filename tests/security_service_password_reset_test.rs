#[path = "common/mod.rs"]
mod common;

use egras::security::service::password_reset_confirm::{
    password_reset_confirm, PasswordResetConfirmError, PasswordResetConfirmInput,
};
use egras::security::service::password_reset_request::{
    password_reset_request, PasswordResetRequestInput,
};
use egras::testing::{MockAppStateBuilder, TestPool};

#[tokio::test]
async fn password_reset_unknown_email_returns_ok() {
    let pool = TestPool::fresh().await.pool;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    password_reset_request(
        &state,
        PasswordResetRequestInput {
            email: "nobody@example.com".into(),
            base_url: "http://localhost:3000".into(),
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn password_reset_invalid_token_returns_error() {
    let pool = TestPool::fresh().await.pool;
    let state = MockAppStateBuilder::new(pool.clone())
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_security_repos()
        .build();

    let err = password_reset_confirm(
        &state,
        PasswordResetConfirmInput {
            raw_token: hex::encode([0u8; 32]),
            new_password: "newpass123".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, PasswordResetConfirmError::InvalidToken));
}
