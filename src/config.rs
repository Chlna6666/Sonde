use std::{
    env, fs, io,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct PasswordPepper(Arc<[u8; 32]>);

impl PasswordPepper {
    pub fn new(value: [u8; 32]) -> Self {
        Self(Arc::new(value))
    }

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
    pub trusted_proxies: Vec<IpAddr>,
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
            trusted_proxies: parse_trusted_proxies(
                env::var("SONDE_TRUSTED_PROXIES").ok().as_deref(),
            )?,
        })
    }

    #[must_use]
    pub fn bind_is_loopback(&self) -> bool {
        self.bind.starts_with("127.0.0.1:")
            || self.bind.starts_with("[::1]:")
            || self.bind.starts_with("localhost:")
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

fn parse_trusted_proxies(raw: Option<&str>) -> io::Result<Vec<IpAddr>> {
    let Some(raw) = raw.filter(|value| !value.trim().is_empty()) else {
        return Ok(Vec::new());
    };
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse::<IpAddr>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid SONDE_TRUSTED_PROXIES entry '{value}': {error}"),
                )
            })
        })
        .collect()
}

#[cfg(unix)]
fn restrict_secret_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
fn restrict_secret_permissions(path: &Path) -> io::Result<()> {
    restrict_windows_secret(path)
}

#[cfg(not(any(unix, windows)))]
fn restrict_secret_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn restrict_windows_secret(path: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let user = env::var("USERNAME").map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "USERNAME is required to restrict password pepper ACL",
        )
    })?;
    let status = Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r", &format!("{user}:(F)")])
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            "failed to restrict password pepper ACL to the current user",
        ))
    }
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
