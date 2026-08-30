use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::error::AppError;

const SIGNATURE_CONTEXT: &[u8] = b"sonde-hmac-sha256-v2\n";
type HmacSha256 = Hmac<Sha256>;

pub const SIGNATURE_VERSION: &str = "sonde-hmac-sha256-v2";

pub fn verify(
    signing_key: &str,
    timestamp_ms: i64,
    nonce: &str,
    method: &str,
    path: &str,
    body: &[u8],
    provided_signature_hex: &str,
) -> Result<(), AppError> {
    let now = chrono::Utc::now().timestamp_millis();
    if (now - timestamp_ms).abs() > 60_000 {
        return Err(AppError::Validation(
            "request timestamp drift too large (allowed +/- 60s)".into(),
        ));
    }

    let provided = hex::decode(provided_signature_hex).map_err(|_| AppError::Forbidden)?;
    let canonical = canonical_request(timestamp_ms, nonce, method, path, body);
    let mut mac = HmacSha256::new_from_slice(signing_key.as_bytes())
        .map_err(|error| AppError::internal("initialize request signature verifier", error))?;
    mac.update(&canonical);
    mac.verify_slice(&provided).map_err(|_| AppError::Forbidden)
}

fn canonical_request(
    timestamp_ms: i64,
    nonce: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> Vec<u8> {
    let body_hash = Sha256::digest(body);
    let body_hash_hex = hex::encode(body_hash);
    let method = method.to_ascii_uppercase();

    let mut canonical = Vec::with_capacity(
        SIGNATURE_CONTEXT.len()
            + method.len()
            + path.len()
            + nonce.len()
            + body_hash_hex.len()
            + 48,
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
    canonical.extend_from_slice(body_hash_hex.as_bytes());
    canonical
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    use super::canonical_request;

    #[test]
    fn signature_is_bound_to_method_path_and_body() {
        let key = "sec_test";
        let timestamp = chrono::Utc::now().timestamp_millis();
        let nonce = "nonce-1";
        let body = br#"{"items":[]}"#;
        let canonical = canonical_request(
            timestamp,
            nonce,
            "POST",
            "/api/v1/ingest/events",
            body,
        );
        let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
        mac.update(&canonical);
        let signature = hex::encode(mac.finalize().into_bytes());

        assert!(super::verify(
            key,
            timestamp,
            nonce,
            "POST",
            "/api/v1/ingest/events",
            body,
            &signature,
        )
        .is_ok());
        assert!(super::verify(
            key,
            timestamp,
            nonce,
            "POST",
            "/api/v1/ingest/errors",
            body,
            &signature,
        )
        .is_err());
        assert!(super::verify(
            key,
            timestamp,
            nonce,
            "PUT",
            "/api/v1/ingest/events",
            body,
            &signature,
        )
        .is_err());
        assert!(super::verify(
            key,
            timestamp,
            nonce,
            "POST",
            "/api/v1/ingest/events",
            b"tampered",
            &signature,
        )
        .is_err());
    }
}
