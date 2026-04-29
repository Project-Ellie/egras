use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, Params, Version,
};

const M_COST: u32 = 19_456;
const T_COST: u32 = 2;
const P_COST: u32 = 1;

const EXPECTED_PARAMS_FRAGMENT: &str = "m=19456,t=2,p=1";

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let params = Params::new(M_COST, T_COST, P_COST, None)
        .map_err(|e| anyhow::anyhow!("argon2 params: {e}"))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params);
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash_password: {e}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    let parsed = PasswordHash::new(hash).map_err(|e| anyhow::anyhow!("parse hash: {e}"))?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(e) => Err(anyhow::anyhow!("verify_password: {e}")),
    }
}

pub fn needs_rehash(hash: &str) -> bool {
    !hash.contains(EXPECTED_PARAMS_FRAGMENT)
}
