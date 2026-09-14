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
    pub allow_insecure_cookies: bool,
    /// Operator-supplied one-time setup token. When absent, a random token is generated at
    /// startup and logged once, so the pre-installation API is never anonymously usable.
    pub setup_token: Option<String>,
    /// Master key for AEAD encryption of stored secrets (see `secret_cipher`).
    pub master_key: MasterKey,
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

        let setup_token = match env::var("SONDE_SETUP_TOKEN") {
            Ok(value) => {
                let token = value.trim().to_owned();
                if token.chars().count() < 16 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "SONDE_SETUP_TOKEN must be at least 16 characters",
                    ));
                }
                Some(token)
            }
            Err(_) => None,
        };
        let master_key = load_master_key(&data_dir)?;

        Ok(Self {
            bind: env::var("SONDE_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into()),
            config_path,
            data_dir,
            database_url_override: env::var("SONDE_DATABASE_URL").ok(),
            password_pepper,
            trusted_proxies: parse_trusted_proxies(
                env::var("SONDE_TRUSTED_PROXIES").ok().as_deref(),
            )?,
            allow_insecure_cookies: env_flag("SONDE_ALLOW_INSECURE_COOKIES"),
            setup_token,
            master_key,
        })
    }

    #[must_use]
    pub fn bind_is_loopback(&self) -> bool {
        self.bind.starts_with("127.0.0.1:")
            || self.bind.starts_with("[::1]:")
            || self.bind.starts_with("localhost:")
    }

    /// Whether session cookies must stay marked `Secure`.
    ///
    /// A server reachable from other hosts has to assume TLS termination in front of it, so the
    /// secure cookie mode is enforced server-side. `SONDE_ALLOW_INSECURE_COOKIES` is the explicit
    /// opt-out for plain-HTTP deployments that cannot be moved behind TLS.
    #[must_use]
    pub fn requires_secure_cookies(&self) -> bool {
        !self.bind_is_loopback() && !self.allow_insecure_cookies
    }
}

fn env_flag(name: &str) -> bool {
    env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn load_or_create_pepper(path: &Path) -> io::Result<PasswordPepper> {
    if path.exists() {
        return read_pepper(path);
    }
    let mut value = [0_u8; 32];
    rand::rng().fill_bytes(&mut value);
    write_new_secret_file(path, &value)?;
    Ok(PasswordPepper(Arc::new(value)))
}

/// Reads the 32-byte secret at `path`, creating it from the OS CSPRNG on first start.
fn load_or_create_master_key_file(path: &Path) -> io::Result<MasterKey> {
    if path.exists() {
        let bytes = fs::read(path)?;
        let value: [u8; 32] = bytes.try_into().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "master key must contain exactly 32 bytes",
            )
        })?;
        return Ok(MasterKey(Arc::new(value)));
    }
    let mut value = [0_u8; 32];
    rand::rng().fill_bytes(&mut value);
    write_new_secret_file(path, &value)?;
    Ok(MasterKey(Arc::new(value)))
}

fn write_new_secret_file(path: &Path, value: &[u8; 32]) -> io::Result<()> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            use io::Write;
            file.write_all(value)?;
            file.sync_all()?;
            restrict_secret_permissions(path)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            // A concurrent start created the file first; load that one.
            Ok(())
        }
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

/// 32-byte installation master key backing the AEAD encryption of stored secrets
/// (currently TOTP shared secrets). Losing it makes existing TOTP enrollments unusable,
/// so it lives next to the password pepper with the same restricted permissions.
#[derive(Clone)]
pub struct MasterKey(Arc<[u8; 32]>);

impl MasterKey {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Builds a key from explicit bytes (used by tests and tooling).
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Arc::new(bytes))
    }
}

fn load_master_key(data_dir: &Path) -> io::Result<MasterKey> {
    if let Some(raw) = env::var("SONDE_MASTER_KEY").ok().as_deref() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SONDE_MASTER_KEY is set but empty",
            ));
        }
        let mut value = [0_u8; 32];
        hex::decode_to_slice(trimmed, &mut value).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("SONDE_MASTER_KEY must be 64 hex characters: {error}"),
            )
        })?;
        return Ok(MasterKey(Arc::new(value)));
    }
    load_or_create_master_key_file(&data_dir.join("sonde.master-key"))
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
        let config: Self = serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)?;
        // The config carries the database URL (often including credentials), so re-tighten the
        // file mode on every start: older installations may still hold a world-readable file.
        if let Err(error) = restrict_secret_permissions(path) {
            tracing::warn!(
                error = %error,
                "could not restrict installation config permissions; the file carries database credentials"
            );
        }
        Ok(config)
    }

    pub fn write_atomic(&self, path: &Path) -> io::Result<()> {
        let temporary = path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
        )?;
        restrict_secret_permissions(&temporary)?;
        fs::rename(temporary, path)
    }
}
