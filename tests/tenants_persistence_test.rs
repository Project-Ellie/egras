use egras::tenants::model::OrganisationCursor;
use egras::tenants::persistence::{
    OrganisationRepository, OrganisationRepositoryPg, RepoError, RoleRepository, RoleRepositoryPg,
};
use egras::testing::TestPool;
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_user(pool: &PgPool, username: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, 'x')")
        .bind(id)
        .bind(username)
        .bind(format!("{username}@test"))
        .execute(pool)
        .await
        .expect("seed user");
    id
}

#[tokio::test]
async fn create_returns_organisation_with_non_operator_flag() {
    let pool = TestPool::fresh().await.pool;
    let repo = OrganisationRepositoryPg::new(pool);

    let org = repo.create("acme", "retail").await.unwrap();
    assert_eq!(org.name, "acme");
    assert_eq!(org.business, "retail");
    assert!(!org.is_operator);
}

#[tokio::test]
async fn create_duplicate_name_maps_to_duplicate_name_error() {
    let pool = TestPool::fresh().await.pool;
    let repo = OrganisationRepositoryPg::new(pool);
    repo.create("acme", "retail").await.unwrap();

    let err = repo.create("acme", "media").await.unwrap_err();
    assert!(matches!(err, RepoError::DuplicateName(n) if n == "acme"));
}

#[tokio::test]
async fn create_with_initial_owner_assigns_role_in_one_tx() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let orgs = OrganisationRepositoryPg::new(pool.clone());

    let org = orgs
        .create_with_initial_owner("acme", "retail", user, "org_owner")
        .await
        .unwrap();

    let members = orgs.list_members(org.id, None, 50).await.unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].user_id, user);
    assert_eq!(members[0].role_codes, vec!["org_owner"]);
}

#[tokio::test]
async fn create_with_initial_owner_rolls_back_on_unknown_role() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let orgs = OrganisationRepositoryPg::new(pool.clone());

    let err = orgs
        .create_with_initial_owner("acme", "retail", user, "no_such_role")
        .await
        .unwrap_err();
    assert!(matches!(err, RepoError::UnknownRoleCode(_)));

    // Nothing landed.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organisations WHERE name = 'acme'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn list_for_user_is_scoped_and_paginated() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let other = seed_user(&pool, "bob").await;
    let orgs = OrganisationRepositoryPg::new(pool.clone());

    for name in ["a", "b", "c"] {
        orgs.create_with_initial_owner(name, "retail", user, "org_owner")
            .await
            .unwrap();
    }
    orgs.create_with_initial_owner("hidden", "retail", other, "org_owner")
        .await
        .unwrap();

    let page1 = orgs.list_for_user(user, None, 2).await.unwrap();
    assert_eq!(page1.len(), 2);

    let last = page1.last().unwrap();
    let cursor = OrganisationCursor {
        created_at: last.created_at,
        id: last.id,
    };

    let page2 = orgs.list_for_user(user, Some(cursor), 2).await.unwrap();
    assert_eq!(page2.len(), 1);
    assert!(!page2.iter().any(|o| o.name == "hidden"));
}

#[tokio::test]
async fn roles_find_by_code_returns_builtin() {
    let pool = TestPool::fresh().await.pool;
    let repo = RoleRepositoryPg::new(pool);

    let r = repo.find_by_code("org_owner").await.unwrap().unwrap();
    assert_eq!(r.code, "org_owner");

    assert!(repo.find_by_code("no_such").await.unwrap().is_none());
}

#[tokio::test]
async fn roles_assign_is_idempotent() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let orgs = OrganisationRepositoryPg::new(pool.clone());
    let roles = RoleRepositoryPg::new(pool.clone());

    let org = orgs.create("acme", "retail").await.unwrap();
    let role = roles.find_by_code("org_member").await.unwrap().unwrap();

    roles.assign(user, org.id, role.id).await.unwrap();
    // second call must succeed (ON CONFLICT DO NOTHING)
    roles.assign(user, org.id, role.id).await.unwrap();

    let _members = orgs.list_members(org.id, None, 10).await.unwrap();
    // Exactly one member, one role code.
    assert!(_members.iter().all(|m| m.role_codes == vec!["org_member"]));
    assert_eq!(_members.len(), 1);
}

#[tokio::test]
async fn is_member_true_only_for_actual_members() {
    let pool = TestPool::fresh().await.pool;
    let user = seed_user(&pool, "alice").await;
    let orgs = OrganisationRepositoryPg::new(pool.clone());

    let org = orgs
        .create_with_initial_owner("acme", "retail", user, "org_owner")
        .await
        .unwrap();
    assert!(orgs.is_member(user, org.id).await.unwrap());
    let stranger = seed_user(&pool, "mallory").await;
    assert!(!orgs.is_member(stranger, org.id).await.unwrap());
}
