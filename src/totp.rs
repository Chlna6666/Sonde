use hmac::{Hmac, Mac};
use rand::RngCore;
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn generate_totp_secret() -> String {
    let mut bytes = [0u8; 20];
    rand::rng().fill_bytes(&mut bytes);
    base32_encode(&bytes)
}

pub fn base32_encode(data: &[u8]) -> String {
    let mut result = String::new();
    let mut buffer: u32 = 0;
    let mut bits_in_buffer = 0;

    for &byte in data {
        buffer = (buffer << 8) | (byte as u32);
        bits_in_buffer += 8;

        while bits_in_buffer >= 5 {
            bits_in_buffer -= 5;
            let index = ((buffer >> bits_in_buffer) & 0x1F) as usize;
            result.push(BASE32_ALPHABET[index] as char);
            buffer &= (1 << bits_in_buffer) - 1;
        }
    }

    if bits_in_buffer > 0 {
        let index = ((buffer << (5 - bits_in_buffer)) & 0x1F) as usize;
        result.push(BASE32_ALPHABET[index] as char);
    }

    result
}

pub fn base32_decode(input: &str) -> Option<Vec<u8>> {
    let mut buffer: u32 = 0;
    let mut bits_in_buffer = 0;
    let mut output = Vec::new();

    for c in input.chars() {
        if c.is_whitespace() || c == '-' || c == '=' {
            continue;
        }
        let upper = c.to_ascii_uppercase();
        let val = match upper {
            'A'..='Z' => (upper as u8 - b'A') as u32,
            '2'..='7' => (upper as u8 - b'2' + 26) as u32,
            _ => return None,
        };

        buffer = (buffer << 5) | val;
        bits_in_buffer += 5;

        if bits_in_buffer >= 8 {
            bits_in_buffer -= 8;
            output.push(((buffer >> bits_in_buffer) & 0xFF) as u8);
            buffer &= (1 << bits_in_buffer) - 1;
        }
    }

    Some(output)
}

pub fn compute_totp(secret: &str, time_step: u64) -> Option<String> {
    let secret_bytes = base32_decode(secret)?;
    let mut mac = HmacSha1::new_from_slice(&secret_bytes).ok()?;
    mac.update(&time_step.to_be_bytes());
    let result = mac.finalize().into_bytes();

    let offset = (result[result.len() - 1] & 0x0F) as usize;
    let binary_code = (((result[offset] & 0x7F) as u32) << 24)
        | ((result[offset + 1] as u32) << 16)
        | ((result[offset + 2] as u32) << 8)
        | (result[offset + 3] as u32);

    let code = binary_code % 1_000_000;
    Some(format!("{:06}", code))
}

pub fn verify_totp_step(secret: &str, user_code: &str) -> Option<u64> {
    let user_code = user_code.trim();
    if user_code.len() != 6 || !user_code.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let now_sec = chrono::Utc::now().timestamp() as u64;
    let current_step = now_sec / 30;

    // Check step -2, step -1, step, step + 1, step + 2 (+/- 60s tolerance for clock drift)
    for step_offset in [-2i64, -1i64, 0i64, 1i64, 2i64] {
        let step = (current_step as i64 + step_offset) as u64;
        if let Some(valid_code) = compute_totp(secret, step) {
            if subtle_eq(&valid_code, user_code) {
                return Some(step);
            }
        }
    }
    None
}

pub fn verify_totp(secret: &str, user_code: &str) -> bool {
    verify_totp_step(secret, user_code).is_some()
}

pub fn build_otpauth_uri(account_name: &str, secret: &str) -> String {
    let issuer = "Sonde";
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits=6&period=30",
        urlencoding(issuer),
        urlencoding(account_name),
        secret,
        urlencoding(issuer)
    )
}

fn urlencoding(input: &str) -> String {
    let mut out = String::new();
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn subtle_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_base32_rfc4648_test_vectors() {
        assert_eq!(base32_encode(b""), "");
        assert_eq!(base32_encode(b"f"), "MY");
        assert_eq!(base32_encode(b"fo"), "MZXQ");
        assert_eq!(base32_encode(b"foo"), "MZXW6");
        assert_eq!(base32_encode(b"foob"), "MZXW6YQ");
        assert_eq!(base32_encode(b"fooba"), "MZXW6YTB");
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");

        assert_eq!(
            base32_decode("MY").expect("decode"),
            b"f"
        );
        assert_eq!(
            base32_decode("MZXQ").expect("decode"),
            b"fo"
        );
        assert_eq!(
            base32_decode("MZXW6").expect("decode"),
            b"foo"
        );
        assert_eq!(
            base32_decode("MZXW6YQ").expect("decode"),
            b"foob"
        );
        assert_eq!(
            base32_decode("MZXW6YTB").expect("decode"),
            b"fooba"
        );
        assert_eq!(
            base32_decode("MZXW6YTBOI").expect("decode"),
            b"foobar"
        );
    }

    #[test]
    fn test_base32_20_byte_secret_roundtrip() {
        let secret_ascii = b"12345678901234567890";
        let encoded = base32_encode(secret_ascii);
        assert_eq!(encoded, "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");

        let decoded = base32_decode(&encoded).expect("should decode");
        assert_eq!(decoded.as_slice(), secret_ascii);
    }

    #[test]
    fn test_totp_rfc6238_vector() {
        // RFC 6238 Test vector: Secret is ASCII "12345678901234567890" -> Base32 "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
        let secret_b32 = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

        // T=59s (step = 1) -> 287082
        let code = compute_totp(secret_b32, 1).expect("compute");
        assert_eq!(code, "287082");

        // T=1111111109s (step = 37037036) -> 081804
        let code = compute_totp(secret_b32, 37037036).expect("compute");
        assert_eq!(code, "081804");

        // T=1111111111s (step = 37037037) -> 050471
        let code = compute_totp(secret_b32, 37037037).expect("compute");
        assert_eq!(code, "050471");

        // T=1234567890s (step = 41152263) -> 005924
        let code = compute_totp(secret_b32, 41152263).expect("compute");
        assert_eq!(code, "005924");

        // T=2000000000s (step = 66666666) -> 279037
        let code = compute_totp(secret_b32, 66666666).expect("compute");
        assert_eq!(code, "279037");
    }

    #[test]
    fn test_totp_verify_valid_and_invalid() {
        let secret = generate_totp_secret();
        let now_step = (chrono::Utc::now().timestamp() as u64) / 30;
        let current_code = compute_totp(&secret, now_step).expect("compute");

        assert!(verify_totp(&secret, &current_code));
        assert!(!verify_totp(&secret, "000000"));
        assert!(!verify_totp(&secret, "abc"));
    }
}
