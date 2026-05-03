use egras::security::service::api_key_secret;
use egras::security::service::create_api_key::{create_api_key, CreateApiKeyError, CreateApiKeyInput};
use egras::security::service::create_service_account::{
    create_service_account, CreateServiceAccountInput,
};
use egras::security::service::list_api_keys::{list_api_keys, ListApiKeysError, ListApiKeysInput};
use egras::security::service::revoke_api_key::{revoke_api_key, RevokeApiKeyError, RevokeApiKeyInput};
use egras::security::service::rotate_api_key::{rotate_api_key, RotateApiKeyError, RotateApiKeyInput};
use egras::testing::{MockAppStateBuilder, TestPool};
use uuid::Uuid;

use crate::common::seed::{seed_org, seed_user};

fn state(pool: sqlx::PgPool) -> egras::app_state::AppState {
    MockAppStateBuilder::new(pool)
        .with_blocking_audit()
        .with_pg_tenants_repos()
        .with_pg_channels_repo()
        .with_pg_security_repos()
        .with_pg_service_account_repos()
        .build()
}

async fn make_sa(
    st: &egras::app_state::AppState,
    actor: Uuid,
    org: Uuid,
    name: &str,
) -> Uuid {
    create_service_account(
        st,
        CreateServiceAccountInput {
            organisation_id: org,
            name: name.into(),
            description: None,
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap()
    .user_id
}

#[tokio::test]
async fn create_returns_plaintext_once_storage_only_has_hash() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let st = state(pool.clone());
    let sa = make_sa(&st, actor, org, "bot").await;

    let mat = create_api_key(
        &st,
        CreateApiKeyInput {
            organisation_id: org,
            sa_user_id: sa,
            name: "primary".into(),
            scopes: None,
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap();

    let parsed = api_key_secret::parse(&mat.plaintext).expect("parse");
    assert_eq!(parsed.prefix, mat.key.prefix);

    let secret_hash: String =
        sqlx::query_scalar("SELECT secret_hash FROM api_keys WHERE id = $1")
            .bind(mat.key.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_ne!(secret_hash, parsed.secret, "secret must be hashed at rest");
    assert!(secret_hash.starts_with("$argon2"));
}

#[tokio::test]
async fn create_empty_scopes_rejected() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let st = state(pool);
    let sa = make_sa(&st, actor, org, "bot").await;

    let err = create_api_key(
        &st,
        CreateApiKeyInput {
            organisation_id: org,
            sa_user_id: sa,
            name: "k".into(),
            scopes: Some(vec![]),
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, CreateApiKeyError::EmptyScopes));
}

#[tokio::test]
async fn create_for_sa_in_wrong_org_returns_not_found() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org_a = seed_org(&pool, "acme-a", "retail").await;
    let org_b = seed_org(&pool, "acme-b", "retail").await;
    let st = state(pool);
    let sa = make_sa(&st, actor, org_a, "bot").await;

    let err = create_api_key(
        &st,
        CreateApiKeyInput {
            organisation_id: org_b,
            sa_user_id: sa,
            name: "k".into(),
            scopes: None,
            actor_user_id: actor,
            actor_org_id: org_b,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, CreateApiKeyError::NotFound));
}

#[tokio::test]
async fn list_returns_keys_for_sa() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let st = state(pool);
    let sa = make_sa(&st, actor, org, "bot").await;

    for n in 0..3 {
        create_api_key(
            &st,
            CreateApiKeyInput {
                organisation_id: org,
                sa_user_id: sa,
                name: format!("k{n}"),
                scopes: None,
                actor_user_id: actor,
                actor_org_id: org,
            },
        )
        .await
        .unwrap();
    }

    let keys = list_api_keys(
        &st,
        ListApiKeysInput {
            organisation_id: org,
            sa_user_id: sa,
        },
    )
    .await
    .unwrap();
    assert_eq!(keys.len(), 3);
    // No `secret_hash` field on ApiKey — that's the point of the metadata-only DTO.
}

#[tokio::test]
async fn list_for_unknown_sa_returns_not_found() {
    let pool = TestPool::fresh().await.pool;
    let _actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let st = state(pool);

    let err = list_api_keys(
        &st,
        ListApiKeysInput {
            organisation_id: org,
            sa_user_id: Uuid::now_v7(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ListApiKeysError::NotFound));
}

#[tokio::test]
async fn revoke_emits_audit_and_subsequent_revoke_returns_not_found() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let st = state(pool.clone());
    let sa = make_sa(&st, actor, org, "bot").await;

    let mat = create_api_key(
        &st,
        CreateApiKeyInput {
            organisation_id: org,
            sa_user_id: sa,
            name: "k".into(),
            scopes: None,
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap();

    revoke_api_key(
        &st,
        RevokeApiKeyInput {
            organisation_id: org,
            sa_user_id: sa,
            key_id: mat.key.id,
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap();

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE event_type = 'api_key.revoked'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);

    let err = revoke_api_key(
        &st,
        RevokeApiKeyInput {
            organisation_id: org,
            sa_user_id: sa,
            key_id: mat.key.id,
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, RevokeApiKeyError::KeyNotFound));
}

#[tokio::test]
async fn rotate_creates_new_revokes_old_emits_audit() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let st = state(pool.clone());
    let sa = make_sa(&st, actor, org, "bot").await;

    let original = create_api_key(
        &st,
        CreateApiKeyInput {
            organisation_id: org,
            sa_user_id: sa,
            name: "k".into(),
            scopes: Some(vec!["service_accounts.read".into()]),
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap();

    let rotated = rotate_api_key(
        &st,
        RotateApiKeyInput {
            organisation_id: org,
            sa_user_id: sa,
            old_key_id: original.key.id,
            name: None,
            scopes: None,
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap();

    assert_ne!(rotated.key.id, original.key.id);
    assert_eq!(rotated.key.scopes, original.key.scopes);

    let revoked: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT revoked_at FROM api_keys WHERE id = $1")
            .bind(original.key.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(revoked.is_some());

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE event_type = 'api_key.rotated'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn rotate_for_unknown_key_returns_key_not_found() {
    let pool = TestPool::fresh().await.pool;
    let actor = seed_user(&pool, "alice").await;
    let org = seed_org(&pool, "acme", "retail").await;
    let st = state(pool);
    let sa = make_sa(&st, actor, org, "bot").await;

    let err = rotate_api_key(
        &st,
        RotateApiKeyInput {
            organisation_id: org,
            sa_user_id: sa,
            old_key_id: Uuid::now_v7(),
            name: None,
            scopes: None,
            actor_user_id: actor,
            actor_org_id: org,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, RotateApiKeyError::KeyNotFound));
}
