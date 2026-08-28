use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct PasswordPepper(Arc<[u8; 32]>);

impl PasswordPepper {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[derive(Clone)]
pub struct RuntimeConfig {
    pub bind: String,
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
    pub database_url_override: Option<String>,
    pub password_pepper: PasswordPepper,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InstallationConfig {
    pub database_url: String,
    pub locale: String,
    pub timezone: String,
    pub secure_cookie: bool,
}

impl RuntimeConfig {
    pub fn from_environment() -> io::Result<Self> {
        let data_dir = env::var_os("SONDE_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data"));
        fs::create_dir_all(&data_dir)?;

        let config_path = env::var_os("SONDE_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("sonde.json"));
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let pepper_path = env::var_os("SONDE_PEPPER_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("sonde.password-pepper"));
        if let Some(parent) = pepper_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let password_pepper = load_or_create_pepper(&pepper_path)?;

        Ok(Self {
            bind: env::var("SONDE_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into()),
            config_path,
            data_dir,
            database_url_override: env::var("SONDE_DATABASE_URL").ok(),
            password_pepper,
        })
    }
}

fn load_or_create_pepper(path: &Path) -> io::Result<PasswordPepper> {
    if path.exists() {
        return read_pepper(path);
    }
    let mut value = [0_u8; 32];
    rand::rng().fill_bytes(&mut value);
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            use io::Write;
            file.write_all(&value)?;
            file.sync_all()?;
            restrict_secret_permissions(path)?;
            Ok(PasswordPepper(Arc::new(value)))
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => read_pepper(path),
        Err(error) => Err(error),
    }
}

fn read_pepper(path: &Path) -> io::Result<PasswordPepper> {
    let bytes = fs::read(path)?;
    let value: [u8; 32] = bytes.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "password pepper must contain exactly 32 bytes",
        )
    })?;
    Ok(PasswordPepper(Arc::new(value)))
}

#[cfg(unix)]
fn restrict_secret_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_secret_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

impl InstallationConfig {
    pub fn read(path: &Path) -> io::Result<Self> {
        serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)
    }

    pub fn write_atomic(&self, path: &Path) -> io::Result<()> {
        let temporary = path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
        )?;
        fs::rename(temporary, path)
    }
}
