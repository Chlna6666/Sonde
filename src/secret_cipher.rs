//! Authenticated encryption for secrets that must be recoverable (TOTP shared secrets).
//!
//! The AEAD key is derived from the installation master key with HMAC-SHA256 over a domain
//! label. The master key is always uniformly random (32 bytes from the OS CSPRNG), so this is
//! equivalent to HKDF with the extract step already satisfied. Stored values use the versioned
//! envelope `v1:<base64url nonce>:<base64url ciphertext>`; values without the prefix are legacy
//! plaintext and keep working until the secret is rewritten.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use hmac::{Hmac, KeyInit as HmacKeyInit, Mac};
use rand::Rng;
use sha2::Sha256;

use crate::error::AppError;

const ENVELOPE_PREFIX: &str = "v1:";
const NONCE_BYTES: usize = 24;
const DERIVE_INFO: &[u8] = b"sonde/secret-encryption/v1";

#[derive(Clone)]
pub struct SecretCipher {
    cipher: XChaCha20Poly1305,
}

impl SecretCipher {
    /// Derives the AEAD key from the installation master key.
    pub fn new(master_key: &[u8; 32]) -> Result<Self, AppError> {
        let mut mac = <Hmac<Sha256> as HmacKeyInit>::new_from_slice(master_key)
            .map_err(|error| AppError::internal("initialize secret key derivation", error))?;
        mac.update(DERIVE_INFO);
        let derived: [u8; 32] = mac.finalize().into_bytes().into();
        Ok(Self {
            cipher: XChaCha20Poly1305::new(&derived.into()),
        })
    }

    /// Whether `stored` is an encrypted envelope (as opposed to a legacy plaintext secret).
    #[must_use]
    pub fn is_envelope(stored: &str) -> bool {
        stored.starts_with(ENVELOPE_PREFIX)
    }

    /// Encrypts `plaintext` into a versioned envelope with a fresh random nonce.
    pub fn seal(&self, plaintext: &str) -> Result<String, AppError> {
        let mut nonce_bytes = [0_u8; NONCE_BYTES];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = XNonce::from(nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|error| AppError::internal("seal stored secret", error))?;
        Ok(format!(
            "{ENVELOPE_PREFIX}{}:{}",
            URL_SAFE_NO_PAD.encode(nonce_bytes),
            URL_SAFE_NO_PAD.encode(ciphertext)
        ))
    }

    /// Decrypts an envelope produced by [`SecretCipher::seal`].
    pub fn open(&self, stored: &str) -> Result<String, AppError> {
        let body = stored
            .strip_prefix(ENVELOPE_PREFIX)
            .ok_or_else(|| AppError::internal("stored secret is not an encrypted envelope", ""))?;
        let (nonce_part, ciphertext_part) = body
            .split_once(':')
            .ok_or_else(|| AppError::internal("stored secret envelope is malformed", ""))?;
        let nonce_bytes: [u8; NONCE_BYTES] = URL_SAFE_NO_PAD
            .decode(nonce_part)
            .map_err(|error| AppError::internal("stored secret nonce is malformed", error))?
            .try_into()
            .map_err(|_| AppError::internal("stored secret nonce has an invalid length", ""))?;
        let ciphertext = URL_SAFE_NO_PAD
            .decode(ciphertext_part)
            .map_err(|error| AppError::internal("stored secret ciphertext is malformed", error))?;
        let plaintext = self
            .cipher
            .decrypt(&XNonce::from(nonce_bytes), ciphertext.as_ref())
            .map_err(|error| AppError::internal("stored secret failed authentication", error))?;
        String::from_utf8(plaintext)
            .map_err(|error| AppError::internal("stored secret is not valid UTF-8", error))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{ENVELOPE_PREFIX, SecretCipher};

    const KEY_A: [u8; 32] = [7_u8; 32];
    const KEY_B: [u8; 32] = [11_u8; 32];

    #[test]
    fn seal_open_round_trip_with_random_nonces() {
        let cipher = SecretCipher::new(&KEY_A).unwrap();
        let envelope = cipher.seal("JBSWY3DPEHPK3PXP").unwrap();
        assert!(SecretCipher::is_envelope(&envelope));
        assert!(envelope.starts_with(ENVELOPE_PREFIX));
        assert_eq!(cipher.open(&envelope).unwrap(), "JBSWY3DPEHPK3PXP");

        // Two seals of the same plaintext must differ (fresh nonce each time).
        let again = cipher.seal("JBSWY3DPEHPK3PXP").unwrap();
        assert_ne!(envelope, again);
        assert_eq!(cipher.open(&again).unwrap(), "JBSWY3DPEHPK3PXP");
    }

    #[test]
    fn wrong_master_key_cannot_open_the_envelope() {
        let sealed = SecretCipher::new(&KEY_A).unwrap().seal("secret").unwrap();
        let other = SecretCipher::new(&KEY_B).unwrap();
        assert!(other.open(&sealed).is_err());
    }

    #[test]
    fn tampered_envelopes_are_rejected() {
        let cipher = SecretCipher::new(&KEY_A).unwrap();
        let envelope = cipher.seal("secret").unwrap();
        let flipped = format!("{}x", &envelope[..envelope.len() - 2]);
        assert!(cipher.open(&flipped).is_err());
        assert!(cipher.open("v1:not-a-valid-envelope").is_err());
        assert!(cipher.open("v1:onlynonce").is_err());
    }

    #[test]
    fn legacy_plaintext_is_not_treated_as_an_envelope() {
        assert!(!SecretCipher::is_envelope("JBSWY3DPEHPK3PXP"));
    }
}
