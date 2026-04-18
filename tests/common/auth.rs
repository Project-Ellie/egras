use egras::testing::mint_jwt;
use uuid::Uuid;

pub fn bearer(secret: &str, issuer: &str, user: Uuid, org: Uuid) -> String {
    format!("Bearer {}", mint_jwt(secret, issuer, user, org, 3600))
}
