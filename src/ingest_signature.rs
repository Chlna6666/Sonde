use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::error::AppError;

const SIGNATURE_CONTEXT: &[u8] = b"sonde-hmac-sha256-v2\n";
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
type HmacSha256 = Hmac<Sha256>;

pub const SIGNATURE_VERSION: &str = "sonde-hmac-sha256-v2";

pub struct VerifyRequest<'a> {
    pub signing_key: &'a str,
    pub timestamp_ms: i64,
    pub nonce: &'a str,
    pub method: &'a str,
    pub path: &'a str,
    pub body: &'a [u8],
    pub signature_hex: &'a str,
}

#[must_use = "HMAC verification failure must be handled"]
pub fn verify(request: VerifyRequest<'_>) -> Result<(), AppError> {
    verify_at(request, unix_millis())
}

fn unix_millis() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(elapsed) => i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

#[must_use = "HMAC verification failure must be handled"]
pub fn verify_at(request: VerifyRequest<'_>, now_ms: i64) -> Result<(), AppError> {
    if (now_ms - request.timestamp_ms).abs() > 60_000 {
        return Err(AppError::Validation(
            "request timestamp drift too large (allowed +/- 60s)".into(),
        ));
    }

    let provided = decode_sha256_hex(request.signature_hex).ok_or(AppError::Forbidden)?;
    let mut mac = HmacSha256::new_from_slice(request.signing_key.as_bytes())
        .map_err(|error| AppError::internal("initialize request signature verifier", error))?;
    update_canonical_request(
        &mut mac,
        request.timestamp_ms,
        request.nonce,
        request.method,
        request.path,
        request.body,
    );
    mac.verify_slice(&provided).map_err(|_| AppError::Forbidden)
}

fn update_canonical_request(
    mac: &mut HmacSha256,
    timestamp_ms: i64,
    nonce: &str,
    method: &str,
    path: &str,
    body: &[u8],
) {
    let mut timestamp_digits = [0_u8; 20];
    let mut body_hash_hex = [0_u8; 64];
    encode_sha256_hex(&Sha256::digest(body), &mut body_hash_hex);
    mac.update(SIGNATURE_CONTEXT);
    mac.update(write_i64(timestamp_ms, &mut timestamp_digits));
    mac.update(b"\n");
    mac.update(nonce.as_bytes());
    mac.update(b"\n");
    write_ascii_uppercase(mac, method);
    mac.update(b"\n");
    mac.update(path.as_bytes());
    mac.update(b"\n");
    mac.update(&body_hash_hex);
}

fn write_ascii_uppercase(mac: &mut HmacSha256, method: &str) {
    if method
        .as_bytes()
        .iter()
        .all(|byte| byte.is_ascii_uppercase())
    {
        mac.update(method.as_bytes());
        return;
    }
    let mut chunk = [0_u8; 16];
    for bytes in method.as_bytes().chunks(chunk.len()) {
        for (index, byte) in bytes.iter().enumerate() {
            chunk[index] = byte.to_ascii_uppercase();
        }
        mac.update(&chunk[..bytes.len()]);
    }
}

fn write_i64(value: i64, buffer: &mut [u8; 20]) -> &[u8] {
    let mut index = buffer.len();
    let mut remaining = value.unsigned_abs();
    if remaining == 0 {
        index -= 1;
        buffer[index] = b'0';
    } else {
        while remaining > 0 {
            index -= 1;
            buffer[index] = b'0' + (remaining % 10) as u8;
            remaining /= 10;
        }
    }
    if value < 0 {
        index -= 1;
        buffer[index] = b'-';
    }
    &buffer[index..]
}

fn encode_sha256_hex(digest: &[u8], output: &mut [u8; 64]) {
    for (index, byte) in digest.iter().take(32).enumerate() {
        output[index * 2] = HEX_DIGITS[usize::from(byte >> 4)];
        output[index * 2 + 1] = HEX_DIGITS[usize::from(byte & 0x0f)];
    }
}

fn decode_sha256_hex(value: &str) -> Option<[u8; 32]> {
    let bytes = value.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        output[index] = (decode_hex_nibble(pair[0])? << 4) | decode_hex_nibble(pair[1])?;
    }
    Some(output)
}

const fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
fn canonical_request(
    timestamp_ms: i64,
    nonce: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> Vec<u8> {
    let mut timestamp_digits = [0_u8; 20];
    let mut body_hash_hex = [0_u8; 64];
    encode_sha256_hex(&Sha256::digest(body), &mut body_hash_hex);
    let timestamp = write_i64(timestamp_ms, &mut timestamp_digits);
    let mut canonical = Vec::with_capacity(
        SIGNATURE_CONTEXT.len()
            + timestamp.len()
            + nonce.len()
            + method.len()
            + path.len()
            + body_hash_hex.len()
            + 4,
    );
    canonical.extend_from_slice(SIGNATURE_CONTEXT);
    canonical.extend_from_slice(timestamp);
    canonical.push(b'\n');
    canonical.extend_from_slice(nonce.as_bytes());
    canonical.push(b'\n');
    canonical.extend(
        method
            .as_bytes()
            .iter()
            .copied()
            .map(|byte| byte.to_ascii_uppercase()),
    );
    canonical.push(b'\n');
    canonical.extend_from_slice(path.as_bytes());
    canonical.push(b'\n');
    canonical.extend_from_slice(&body_hash_hex);
    canonical
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    use super::{VerifyRequest, canonical_request};

    fn request<'a>(
        key: &'a str,
        timestamp: i64,
        nonce: &'a str,
        method: &'a str,
        path: &'a str,
        body: &'a [u8],
        signature: &'a str,
    ) -> VerifyRequest<'a> {
        VerifyRequest {
            signing_key: key,
            timestamp_ms: timestamp,
            nonce,
            method,
            path,
            body,
            signature_hex: signature,
        }
    }

    #[test]
    fn canonical_request_formats_negative_and_zero_timestamps() {
        let zero = canonical_request(0, "n", "post", "/p", b"");
        assert!(std::str::from_utf8(&zero).unwrap().contains("\n0\n"));
        let negative = canonical_request(-12, "n", "post", "/p", b"");
        assert!(std::str::from_utf8(&negative).unwrap().contains("\n-12\n"));
    }

    #[test]
    fn decode_sha256_hex_accepts_mixed_case() {
        let mut digest = [0_u8; 32];
        digest[0] = 0xab;
        digest[1] = 0xcd;
        digest[2] = 0xef;
        let mut hex = [0_u8; 64];
        super::encode_sha256_hex(&digest, &mut hex);
        hex[0] = b'A';
        hex[1] = b'B';
        let decoded = super::decode_sha256_hex(std::str::from_utf8(&hex).unwrap()).unwrap();
        assert_eq!(&decoded[..3], &[0xab, 0xcd, 0xef]);
        assert!(super::decode_sha256_hex("zz").is_none());
        assert!(super::decode_sha256_hex(&"0".repeat(63)).is_none());
    }

    #[test]
    fn signature_is_bound_to_method_path_and_body() {
        let key = "sec_test";
        let timestamp = chrono::Utc::now().timestamp_millis();
        let nonce = "nonce-1";
        let body = br#"{"items":[]}"#;
        let canonical = canonical_request(timestamp, nonce, "POST", "/api/v1/ingest/events", body);
        let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
        mac.update(&canonical);
        let signature = hex::encode(mac.finalize().into_bytes());

        assert!(
            super::verify_at(
                request(
                    key,
                    timestamp,
                    nonce,
                    "POST",
                    "/api/v1/ingest/events",
                    body,
                    &signature,
                ),
                timestamp,
            )
            .is_ok()
        );
        assert!(
            super::verify_at(
                request(
                    key,
                    timestamp,
                    nonce,
                    "POST",
                    "/api/v1/ingest/errors",
                    body,
                    &signature,
                ),
                timestamp,
            )
            .is_err()
        );
        assert!(
            super::verify_at(
                request(
                    key,
                    timestamp,
                    nonce,
                    "PUT",
                    "/api/v1/ingest/events",
                    body,
                    &signature,
                ),
                timestamp,
            )
            .is_err()
        );
        assert!(
            super::verify_at(
                request(
                    key,
                    timestamp,
                    nonce,
                    "POST",
                    "/api/v1/ingest/events",
                    b"tampered",
                    &signature,
                ),
                timestamp,
            )
            .is_err()
        );
        assert!(
            super::verify_at(
                request(
                    key,
                    timestamp,
                    nonce,
                    "POST",
                    "/api/v1/ingest/events",
                    body,
                    &signature,
                ),
                timestamp + 60_001,
            )
            .is_err()
        );
    }
}
