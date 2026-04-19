use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub org: Uuid,
    pub iat: i64,
    pub exp: i64,
    pub jti: Uuid,
    pub iss: String,
    pub typ: String,
}

pub fn encode_access_token(
    secret: &str,
    issuer: &str,
    user_id: Uuid,
    org_id: Uuid,
    ttl_secs: i64,
) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        org: org_id,
        iat: now,
        exp: now + ttl_secs,
        jti: Uuid::now_v7(),
        iss: issuer.to_string(),
        typ: "access".to_string(),
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn decode_access_token(
    secret: &str,
    expected_issuer: &str,
    token: &str,
) -> anyhow::Result<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 0;
    validation.set_issuer(&[expected_issuer]);
    validation.set_required_spec_claims(&["exp", "iss", "sub"]);
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;
    if data.claims.typ != "access" {
        anyhow::bail!("token typ is not 'access'");
    }
    Ok(data.claims)
}
