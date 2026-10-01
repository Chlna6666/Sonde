use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{Error, Result};

const DEVICE_ID_MIN_BYTES: usize = 4;
const DEVICE_ID_MAX_BYTES: usize = 128;
const DEFAULT_MACHINE_SALT: &str = "sonde-machine-id-v1";

/// Generate a high-entropy pseudonymous installation/device identifier.
///
/// The identifier is a random UUID v4 and therefore contains no hardware, account, or creation-time
/// information.
pub fn generate_device_id() -> String {
    Uuid::new_v4().to_string()
}

/// Derive a stable, tamper-resistant pseudonymous device identifier from the host machine.
///
/// The identifier is derived in-memory by computing a salted SHA-256 hash of the platform's
/// hardware or OS installation identifier (e.g. Windows MachineGuid, Linux /etc/machine-id, or macOS
/// platform UUID). It does NOT write any files to disk, preventing users or malicious scripts
/// from resetting, duplicating, or spoofing identities by modifying or deleting local files.
pub fn machine_device_id(salt: Option<&str>) -> Result<String> {
    let raw = platform_machine_code()
        .ok_or_else(|| Error::InvalidConfiguration("failed to detect platform machine identifier".into()))?;
    let salt = salt.unwrap_or(DEFAULT_MACHINE_SALT);
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(b":");
    hasher.update(raw.as_bytes());
    let hash = hex::encode(hasher.finalize());
    Ok(hash)
}

#[cfg(target_os = "windows")]
fn platform_machine_code() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey_with_flags(
            "SOFTWARE\\Microsoft\\Cryptography",
            KEY_READ | KEY_WOW64_64KEY,
        )
        .ok()?;
    let guid: String = key.get_value("MachineGuid").ok()?;
    let guid = guid.trim().to_string();
    if guid.is_empty() {
        None
    } else {
        Some(format!("win:{}", guid))
    }
}

#[cfg(target_os = "linux")]
fn platform_machine_code() -> Option<String> {
    if let Ok(id) = std::fs::read_to_string("/etc/machine-id") {
        let id = id.trim();
        if !id.is_empty() {
            return Some(format!("linux:{}", id));
        }
    }
    if let Ok(id) = std::fs::read_to_string("/var/lib/dbus/machine-id") {
        let id = id.trim();
        if !id.is_empty() {
            return Some(format!("linux:{}", id));
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn platform_machine_code() -> Option<String> {
    let output = std::process::Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.contains("IOPlatformUUID") {
                if let Some(pos) = line.find('=') {
                    let uuid = line[pos + 1..].trim().trim_matches('"').trim();
                    if !uuid.is_empty() {
                        return Some(format!("macos:{}", uuid));
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn platform_machine_code() -> Option<String> {
    None
}

pub(crate) fn validate_device_id(value: &str) -> std::result::Result<&str, &'static str> {
    let value = value.trim();
    if value.len() < DEVICE_ID_MIN_BYTES
        || value.len() > DEVICE_ID_MAX_BYTES
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':')
        })
    {
        return Err("device ID must be 4..128 bytes using letters, digits, '-', '_', '.', or ':'");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{generate_device_id, machine_device_id, validate_device_id};

    #[test]
    fn generated_device_id_matches_server_contract() {
        let value = generate_device_id();
        assert_eq!(validate_device_id(&value), Ok(value.as_str()));
    }

    #[test]
    fn machine_device_id_is_stable_and_valid() {
        let id1 = machine_device_id(None);
        if let Ok(id1) = id1 {
            assert_eq!(id1.len(), 64);
            assert_eq!(validate_device_id(&id1), Ok(id1.as_str()));

            let id2 = machine_device_id(None).unwrap();
            assert_eq!(id1, id2, "machine_device_id must be stable across multiple calls");

            let id_custom_salt = machine_device_id(Some("custom-salt")).unwrap();
            assert_ne!(id1, id_custom_salt, "different salts must produce different IDs");
        }
    }
}
