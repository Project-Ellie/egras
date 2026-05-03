//! Generation, hashing, and parsing for the API-key wire format
//! `egras_<env>_<prefix8>_<secret_b64>`.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;

const ENV_LIVE: &str = "live";

#[derive(Debug, Clone)]
pub struct GeneratedKey {
    /// 8 hex chars; matches the `prefix` UNIQUE column on `api_keys`.
    pub prefix: String,
    /// Full token value as the client will receive it.
    pub plaintext: String,
    /// Bare secret (without the `egras_<env>_<prefix>_` framing).
    /// Hash this with `hash_secret` before persisting.
    pub secret: String,
}

pub fn generate() -> anyhow::Result<GeneratedKey> {
    let mut prefix_bytes = [0u8; 4];
    let mut secret_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut prefix_bytes);
    rand::thread_rng().fill_bytes(&mut secret_bytes);
    let prefix = hex::encode(prefix_bytes);
    let secret = URL_SAFE_NO_PAD.encode(secret_bytes);
    let plaintext = format!("egras_{ENV_LIVE}_{prefix}_{secret}");
    Ok(GeneratedKey {
        prefix,
        plaintext,
        secret,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct ParsedKey<'a> {
    pub env: &'a str,
    pub prefix: &'a str,
    pub secret: &'a str,
}

/// Parse `egras_<env>_<prefix8>_<secret>`. Returns None on any malformed shape.
pub fn parse(s: &str) -> Option<ParsedKey<'_>> {
    let body = s.strip_prefix("egras_")?;
    let parts: Vec<&str> = body.splitn(3, '_').collect();
    if parts.len() != 3 {
        return None;
    }
    let env = parts[0];
    let prefix = parts[1];
    let secret = parts[2];
    if prefix.len() != 8 || !prefix.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    if secret.is_empty() {
        return None;
    }
    Some(ParsedKey {
        env,
        prefix,
        secret,
    })
}

/// Hash a secret using the same Argon2 config as passwords.
pub fn hash_secret(secret: &str) -> anyhow::Result<String> {
    crate::security::service::password_hash::hash_password(secret)
}

/// Verify a secret against a stored argon2 hash.
pub fn verify_secret(secret: &str, hash: &str) -> anyhow::Result<bool> {
    crate::security::service::password_hash::verify_password(secret, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let k = generate().unwrap();
        assert_eq!(k.prefix.len(), 8);
        let parsed = parse(&k.plaintext).expect("parse");
        assert_eq!(parsed.env, "live");
        assert_eq!(parsed.prefix, k.prefix);
        assert_eq!(parsed.secret, k.secret);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse("egras_live_zzzz_xxx").is_none()); // non-hex prefix
        assert!(parse("egras_live_aaaaaaaa_").is_none()); // empty secret
        assert!(parse("notakey").is_none());
        assert!(parse("egras_live").is_none());
        assert!(parse("egras_live_aaaa_xx").is_none()); // prefix too short
    }

    #[test]
    fn hash_then_verify_round_trip() {
        let g = generate().unwrap();
        let h = hash_secret(&g.secret).unwrap();
        assert!(verify_secret(&g.secret, &h).unwrap());
        assert!(!verify_secret("wrong", &h).unwrap());
    }
}
