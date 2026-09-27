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
        return Ok(PasswordPepper(Arc::new(read_secret_file(
            path,
            "password pepper",
        )?)));
    }
    let mut value = [0_u8; 32];
    rand::rng().fill_bytes(&mut value);
    match write_new_secret_file(path, &value) {
        Ok(()) => Ok(PasswordPepper(Arc::new(value))),
        // A concurrent start created the file between `exists()` and `create_new`, so the bytes
        // we generated were never written. The persisted value is the only one every process can
        // agree on; reading it back keeps this process from sealing data under a key that exists
        // nowhere else.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(PasswordPepper(Arc::new(
            read_secret_file(path, "password pepper")?,
        ))),
        Err(error) => Err(error),
    }
}

/// Reads the 32-byte master key at `path`, creating it from the OS CSPRNG on first start.
fn load_or_create_master_key_file(path: &Path) -> io::Result<MasterKey> {
    if path.exists() {
        return Ok(MasterKey(Arc::new(read_secret_file(path, "master key")?)));
    }
    let mut value = [0_u8; 32];
    rand::rng().fill_bytes(&mut value);
    match write_new_secret_file(path, &value) {
        Ok(()) => Ok(MasterKey(Arc::new(value))),
        // See `load_or_create_pepper`: the file on disk wins so all processes share one key.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            Ok(MasterKey(Arc::new(read_secret_file(path, "master key")?)))
        }
        Err(error) => Err(error),
    }
}

fn write_new_secret_file(path: &Path, value: &[u8; 32]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    // On Unix the restrictive mode is part of the atomic create itself. This avoids a window
    // where a secret exists with permissions derived only from the process umask, and keeps the
    // file private even when a later best-effort chmod cannot be completed.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    use io::Write;
    file.write_all(value)?;
    file.sync_all()?;
    // Windows ACL tightening can fail for service accounts without USERNAME. Creation must still
    // be durable; Unix is already owner-only at create time and this also repairs unusual modes.
    if let Err(error) = restrict_secret_permissions(path) {
        tracing::warn!(
            error = %error,
            "could not restrict permissions on a freshly written secret file"
        );
    }
    Ok(())
}

/// Reads a 32-byte secret, failing loudly when the file is not exactly that size.
///
/// A truncated or partially written secret is never silently replaced: doing so would invalidate
/// every password hash or TOTP secret already sealed with the previous value, which is exactly
/// the kind of unrecoverable, silent damage this guard exists to prevent.
fn read_secret_file(path: &Path, label: &str) -> io::Result<[u8; 32]> {
    let bytes = fs::read(path)?;
    bytes.try_into().map_err(|actual: Vec<u8>| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{label} at {} must be exactly 32 bytes but holds {}; \
                 restore it from a backup, or delete it only if you accept that passwords and \
                 TOTP secrets sealed with the previous value become unrecoverable",
                path.display(),
                actual.len()
            ),
        )
    })
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
        use io::Write;

        let parent = path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        // A unique same-directory tempfile prevents a stale/symlinked fixed .tmp path from being
        // followed. tempfile creates Unix tempfiles as 0600, so database credentials are private
        // from the first write rather than only after a chmod.
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(
            &serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
        )?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        // Same reasoning as `InstallationConfig::read`: Windows ACL tightening may be
        // unavailable for service accounts. The temporary file remains atomic and, on Unix,
        // was already created owner-only.
        if let Err(error) = restrict_secret_permissions(temporary.path()) {
            tracing::warn!(
                error = %error,
                "could not restrict installation config permissions; the file carries database credentials"
            );
        }
        temporary.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}
