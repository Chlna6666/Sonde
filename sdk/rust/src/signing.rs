use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

pub(crate) const SIGNATURE_VERSION: &str = "sonde-hmac-sha256-v2";
const SIGNATURE_CONTEXT: &[u8] = b"sonde-hmac-sha256-v2\n";
type HmacSha256 = Hmac<Sha256>;

pub(crate) fn sign(
    signing_key: &str,
    timestamp_ms: i64,
    nonce: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<String> {
    let canonical = canonical_request(timestamp_ms, nonce, method, path, body);
    let mut mac = HmacSha256::new_from_slice(signing_key.as_bytes()).map_err(|_| Error::SigningKey)?;
    mac.update(&canonical);
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn canonical_request(
    timestamp_ms: i64,
    nonce: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> Vec<u8> {
    let body_hash = hex::encode(Sha256::digest(body));
    let method = method.to_ascii_uppercase();
    let mut canonical = Vec::with_capacity(
        SIGNATURE_CONTEXT.len() + method.len() + path.len() + nonce.len() + body_hash.len() + 48,
    );
    canonical.extend_from_slice(SIGNATURE_CONTEXT);
    canonical.extend_from_slice(timestamp_ms.to_string().as_bytes());
    canonical.push(b'\n');
    canonical.extend_from_slice(nonce.as_bytes());
    canonical.push(b'\n');
    canonical.extend_from_slice(method.as_bytes());
    canonical.push(b'\n');
    canonical.extend_from_slice(path.as_bytes());
    canonical.push(b'\n');
    canonical.extend_from_slice(body_hash.as_bytes());
    canonical
}

#[cfg(test)]
mod tests {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    use super::{canonical_request, sign};

    #[test]
    fn sdk_signature_matches_protocol_canonicalization() {
        let key = "sec_test";
        let timestamp = 1_725_000_000_123_i64;
        let nonce = "018f47f2-2d4b-7d6c-9f30-111111111111";
        let body = br#"{"items":[{"name":"app_startup"}]}"#;
        let canonical = canonical_request(
            timestamp,
            nonce,
            "POST",
            "/api/v1/ingest/events",
            body,
        );
        let mut mac = match Hmac::<Sha256>::new_from_slice(key.as_bytes()) {
            Ok(mac) => mac,
            Err(_) => return,
        };
        mac.update(&canonical);
        let expected = hex::encode(mac.finalize().into_bytes());
        assert_eq!(
            sign(
                key,
                timestamp,
                nonce,
                "POST",
                "/api/v1/ingest/events",
                body,
            )
            .ok(),
            Some(expected),
        );
    }
}
