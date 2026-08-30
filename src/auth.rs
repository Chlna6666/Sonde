use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::error::AppError;

pub const SESSION_COOKIE: &str = "__Host-sonde-session";
pub const DEVELOPMENT_SESSION_COOKIE: &str = "sonde-session";

pub fn hash_password(password: &str, pepper: &[u8]) -> Result<String, AppError> {
    validate_password(password)?;
    hash_password_unchecked(password, pepper)
}

pub fn hash_password_unchecked(password: &str, pepper: &[u8]) -> Result<String, AppError> {
    let mut salt_bytes = [0_u8; 16];
    rand::rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| AppError::Internal)?;
    password_hasher(pepper)?
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AppError::Internal)
}

pub fn validate_password(password: &str) -> Result<(), AppError> {
    if password.chars().count() < 15 || password.chars().count() > 128 {
        return Err(AppError::PasswordLength);
    }
    let normalized = password.trim().to_lowercase();
    const BLOCKED: &[&str] = &[
        "passwordpassword",
        "123456789012345",
        "qwertyuiopasdfgh",
        "correct horse battery staple",
    ];
    if BLOCKED.contains(&normalized.as_str()) {
        return Err(AppError::PasswordBlocked);
    }
    Ok(())
}

pub fn verify_password(password: &str, encoded: &str, pepper: &[u8]) -> bool {
    let Ok(hash) = PasswordHash::new(encoded) else {
        return false;
    };
    password_hasher(pepper)
        .is_ok_and(|hasher| hasher.verify_password(password.as_bytes(), &hash).is_ok())
}

pub fn verify_legacy_password(password: &str, encoded: &str) -> bool {
    let Ok(hash) = PasswordHash::new(encoded) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &hash)
        .is_ok()
}

fn password_hasher(pepper: &[u8]) -> Result<Argon2<'_>, AppError> {
    Argon2::new_with_secret(
        pepper,
        Algorithm::Argon2id,
        Version::V0x13,
        Params::default(),
    )
    .map_err(|_| AppError::Internal)
}

pub fn random_token(byte_length: usize) -> String {
    URL_SAFE_NO_PAD.encode(random_bytes(byte_length))
}

pub fn random_bytes(byte_length: usize) -> Vec<u8> {
    let mut bytes = vec![0_u8; byte_length];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{hash_password, validate_password, verify_password};

    #[test]
    fn password_hash_requires_the_installation_pepper() -> Result<(), crate::error::AppError> {
        let encoded = hash_password("a long and uncommon passphrase", b"installation-pepper")?;
        assert!(verify_password(
            "a long and uncommon passphrase",
            &encoded,
            b"installation-pepper"
        ));
        assert!(!verify_password(
            "a long and uncommon passphrase",
            &encoded,
            b"different-pepper"
        ));
        Ok(())
    }

    #[test]
    fn password_may_contain_account_words() {
        assert!(validate_password("sonde-owner-2026-secure").is_ok());
    }

    #[test]
    fn known_blocked_password_is_rejected() {
        assert!(matches!(
            validate_password("correct horse battery staple"),
            Err(crate::error::AppError::PasswordBlocked)
        ));
    }
}
