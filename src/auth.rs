use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use sha2::{Digest, Sha256};

use crate::error::AppError;

pub const SESSION_COOKIE: &str = "__Host-sonde-session";
pub const DEVELOPMENT_SESSION_COOKIE: &str = "sonde-session";

pub fn hash_password(password: &str, pepper: &[u8]) -> Result<String, AppError> {
    validate_password(password)?;
    hash_password_unchecked(password, pepper)
}

pub fn hash_password_unchecked(password: &str, pepper: &[u8]) -> Result<String, AppError> {
    password_hasher(pepper)?
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|error| AppError::internal("hash password", error))
}

pub fn validate_identity(email: &str, username: &str) -> Result<(), AppError> {
    let trimmed_email = email.trim();
    if trimmed_email != email
        || !trimmed_email.contains('@')
        || trimmed_email.len() > 254
        || trimmed_email.is_empty()
    {
        return Err(AppError::Validation("valid email is required".into()));
    }

    let trimmed_username = username.trim();
    let username_len = trimmed_username.chars().count();
    if trimmed_username != username
        || !(2..=64).contains(&username_len)
        || trimmed_username.contains('@')
    {
        return Err(AppError::Validation(
            "username must be 2..64 characters, contain no @, and have no surrounding whitespace"
                .into(),
        ));
    }
    Ok(())
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
    .map_err(|error| AppError::internal("initialize password hasher", error))
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

/// Constant-time equality for short secrets (setup token, CSRF token).
///
/// The length check short-circuits, which only reveals whether two secrets have the same
/// length; byte comparison always runs over the full overlap.
#[must_use]
pub fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0_u8, |accumulator, (left, right)| {
            accumulator | (left ^ right)
        })
        == 0
}

/// Unambiguous Crockford-style alphanumeric character set (excluding 0, O, 1, I).
const SETUP_CODE_ALPHABET: &[u8; 32] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

/// Generates a human-friendly one-time setup code (e.g. `8F4K-9W2M`).
///
/// 8 characters from Crockford Base32 gives 40 bits of entropy (over 1 trillion possibilities),
/// mathematically immune to brute-force under pre-installation rate limiting while being
/// easy to read and type on physical consoles or mobile screens.
#[must_use]
pub fn random_setup_code() -> String {
    let bytes = random_bytes(8);
    let mut code = String::with_capacity(9);
    for (i, b) in bytes.into_iter().enumerate() {
        if i == 4 {
            code.push('-');
        }
        let idx = (b as usize) % SETUP_CODE_ALPHABET.len();
        code.push(SETUP_CODE_ALPHABET[idx] as char);
    }
    code
}

/// Constant-time comparison for setup verification codes.
///
/// Supports exact match as well as case-insensitive, hyphen-insensitive match for
/// human-entered setup verification codes.
#[must_use]
pub fn constant_time_eq_setup_code(presented: &str, expected: &str) -> bool {
    if constant_time_eq(presented, expected) {
        return true;
    }
    let norm_p: String = presented
        .chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect();
    let norm_e: String = expected
        .chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect();
    if norm_p.is_empty() || norm_e.is_empty() {
        return false;
    }
    constant_time_eq(&norm_p, &norm_e)
}

#[cfg(test)]
mod tests {
    use super::{
        constant_time_eq, constant_time_eq_setup_code, hash_password, random_setup_code,
        validate_identity, validate_password, verify_password,
    };

    #[test]
    fn setup_code_generation_and_matching() {
        let code = random_setup_code();
        assert_eq!(code.len(), 9);
        assert_eq!(&code[4..5], "-");
        // Exact match
        assert!(constant_time_eq_setup_code(&code, &code));
        // Lowercase match
        assert!(constant_time_eq_setup_code(&code.to_lowercase(), &code));
        // Stripped hyphen match
        let stripped = code.replace('-', "");
        assert!(constant_time_eq_setup_code(&stripped, &code));
        // Lowercase without hyphen
        assert!(constant_time_eq_setup_code(&stripped.to_lowercase(), &code));
        // With spaces
        let spaced = format!("{} {}", &code[..4], &code[5..]);
        assert!(constant_time_eq_setup_code(&spaced, &code));
        // Incorrect code rejected
        assert!(!constant_time_eq_setup_code("WRONGCOD", &code));
    }

    #[test]
    fn constant_time_eq_requires_exact_match() {
        assert!(constant_time_eq(
            "sonde-setup-token-value",
            "sonde-setup-token-value"
        ));
        assert!(!constant_time_eq(
            "sonde-setup-token-value",
            "sonde-setup-token-valux"
        ));
        assert!(!constant_time_eq("short", "sonde-setup-token-value"));
        assert!(constant_time_eq("", ""));
    }

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
    fn account_identity_rejects_ambiguous_usernames_and_surrounding_whitespace() {
        assert!(validate_identity("alice@example.com", "Alice User").is_ok());
        assert!(validate_identity("alice@example.com", "alice@example.com").is_err());
        assert!(validate_identity(" alice@example.com", "Alice").is_err());
        assert!(validate_identity("alice@example.com", " Alice").is_err());
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
