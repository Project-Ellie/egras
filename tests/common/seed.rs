#![allow(dead_code)]

use sqlx::PgPool;
use uuid::Uuid;

/// Insert a user with fixed password hash (tests never log in as this user
/// unless they also bypass auth via minted JWTs).
pub async fn seed_user(pool: &PgPool, username: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, 'test')",
    )
    .bind(id)
    .bind(username)
    .bind(format!("{username}@test"))
    .execute(pool)
    .await
    .expect("seed user");
    id
}

/// Insert a non-operator organisation and return its id.
pub async fn seed_org(pool: &PgPool, name: &str, business: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO organisations (id, name, business, is_operator) VALUES ($1, $2, $3, FALSE)",
    )
    .bind(id)
    .bind(name)
    .bind(business)
    .execute(pool)
    .await
    .expect("seed org");
    id
}

/// Insert a user with a real argon2id hash so login tests can authenticate.
pub async fn seed_user_with_password(pool: &PgPool, username: &str, password: &str) -> Uuid {
    let id = Uuid::now_v7();
    let hash =
        egras::security::service::password_hash::hash_password(password).expect("hash password");
    sqlx::query("INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(username)
        .bind(format!("{username}@test"))
        .bind(hash)
        .execute(pool)
        .await
        .expect("seed user with password");
    id
}

/// Assign a role to `(user, org)` by role code. Panics if the role does not
/// exist (tests should only use codes from migration 0005: operator_admin,
/// org_owner, org_admin, org_member).
pub async fn grant_role(pool: &PgPool, user: Uuid, org: Uuid, role_code: &str) {
    let role_id: Uuid = sqlx::query_scalar("SELECT id FROM roles WHERE code = $1")
        .bind(role_code)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|_| panic!("role {role_code} not seeded"));
    sqlx::query(
        "INSERT INTO user_organisation_roles (user_id, organisation_id, role_id) \
         VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(user)
    .bind(org)
    .bind(role_id)
    .execute(pool)
    .await
    .expect("grant role");
}
