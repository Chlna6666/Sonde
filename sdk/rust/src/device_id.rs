use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use uuid::Uuid;

use crate::error::{Error, Result};

const DEVICE_ID_MIN_BYTES: usize = 4;
const DEVICE_ID_MAX_BYTES: usize = 128;
const CREATE_RACE_READ_ATTEMPTS: usize = 20;
const CREATE_RACE_READ_DELAY: Duration = Duration::from_millis(5);

/// Generate a high-entropy pseudonymous installation/device identifier.
///
/// The identifier contains no hardware or account information. Persist it and reuse it for the
/// lifetime of the installation.
pub fn generate_device_id() -> String {
    Uuid::now_v7().to_string()
}

/// Load a stable installation/device identifier from `path`, creating it on first launch.
///
/// Creation uses `create_new` so concurrent processes cannot overwrite an identifier that another
/// process has already claimed. A malformed existing file is reported instead of silently rotating
/// identity, because replacing it would make Sonde count the same installation as a new device.
pub fn load_or_create_device_id(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    if let Some(device_id) = read_device_id(path, false)? {
        return Ok(device_id);
    }

    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|source| storage_error(parent, source))?;
    }

    let generated = generate_device_id();
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(generated.as_bytes())
                .map_err(|source| storage_error(path, source))?;
            file.sync_all()
                .map_err(|source| storage_error(path, source))?;
            Ok(generated)
        }
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            read_after_create_race(path)
        }
        Err(source) => Err(storage_error(path, source)),
    }
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

fn read_after_create_race(path: &Path) -> Result<String> {
    for _ in 0..CREATE_RACE_READ_ATTEMPTS {
        if let Some(device_id) = read_device_id(path, true)? {
            return Ok(device_id);
        }
        thread::sleep(CREATE_RACE_READ_DELAY);
    }

    read_device_id(path, false)?.ok_or_else(|| Error::InvalidStoredDeviceId {
        path: path.to_path_buf(),
        reason: "device ID file disappeared during concurrent creation",
    })
}

fn read_device_id(path: &Path, tolerate_empty: bool) -> Result<Option<String>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(storage_error(path, source)),
    };
    let device_id = contents.trim();
    if tolerate_empty && device_id.is_empty() {
        return Ok(None);
    }
    validate_device_id(device_id).map_err(|reason| Error::InvalidStoredDeviceId {
        path: path.to_path_buf(),
        reason,
    })?;
    Ok(Some(device_id.to_owned()))
}

fn storage_error(path: &Path, source: std::io::Error) -> Error {
    Error::DeviceIdStorage {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{generate_device_id, load_or_create_device_id, validate_device_id};

    #[test]
    fn generated_device_id_matches_server_contract() {
        let value = generate_device_id();
        assert_eq!(validate_device_id(&value), Ok(value.as_str()));
    }

    #[test]
    fn persists_and_reuses_device_id() -> crate::Result<()> {
        let root = std::env::temp_dir().join(format!("sonde-sdk-test-{}", generate_device_id()));
        let path = root.join("device-id");
        let first = load_or_create_device_id(&path)?;
        let second = load_or_create_device_id(&path)?;
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(root);
        Ok(())
    }
}
