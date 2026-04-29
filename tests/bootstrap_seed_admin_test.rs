use egras::security::service::bootstrap_seed_admin::{
    bootstrap_seed_admin, SeedAdminError, SeedAdminInput,
};
use egras::testing::TestPool;

fn input(email: &str, username: &str) -> SeedAdminInput {
    SeedAdminInput {
        email: email.into(),
        username: username.into(),
        password: "hunter2hunter2".into(),
        role_code: "operator_admin".into(),
        operator_org_name: "operator".into(),
    }
}

#[tokio::test]
async fn seed_admin_happy_path_creates_user_and_audit() {
    let pool = TestPool::fresh().await.pool;

    let out = bootstrap_seed_admin(&pool, input("admin@example.com", "admin"))
        .await
        .expect("seed admin");

    // user row exists
    let username: String =
        sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
            .bind(out.user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(username, "admin");

    // membership row exists in the operator org
    let is_member: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM user_organisation_roles uor
           JOIN organisations o ON o.id = uor.organisation_id
           WHERE uor.user_id = $1 AND o.name = 'operator'
         )",
    )
    .bind(out.user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(is_member, "user should be in the operator org");

    // audit row exists
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE event_type = 'user.registered'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);

    let target_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT target_id FROM audit_events WHERE event_type = 'user.registered' LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(target_id, Some(out.user_id));
}

#[tokio::test]
async fn seed_admin_refuses_duplicate_email() {
    let pool = TestPool::fresh().await.pool;

    bootstrap_seed_admin(&pool, input("dup@example.com", "first"))
        .await
        .expect("first seed");

    let err = bootstrap_seed_admin(&pool, input("dup@example.com", "second"))
        .await
        .unwrap_err();

    assert!(matches!(err, SeedAdminError::UserAlreadyExists(_)));
}

#[tokio::test]
async fn seed_admin_fails_when_operator_org_absent() {
    let pool = TestPool::fresh().await.pool;

    let err = bootstrap_seed_admin(
        &pool,
        SeedAdminInput {
            operator_org_name: "nonexistent_org".into(),
            ..input("x@example.com", "x")
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, SeedAdminError::OperatorOrgNotFound(_)));
}

#[tokio::test]
async fn seed_admin_duplicate_username_returns_internal_error() {
    let pool = TestPool::fresh().await.pool;

    // First seed succeeds.
    bootstrap_seed_admin(&pool, input("first@example.com", "adminuser"))
        .await
        .expect("first seed");

    // Second seed with same username but different email hits the DB unique constraint.
    let err = bootstrap_seed_admin(&pool, input("second@example.com", "adminuser"))
        .await
        .unwrap_err();

    // The service surfaces this as Internal (DB constraint violation), not a dedicated variant.
    assert!(matches!(err, SeedAdminError::Internal(_)));
}
