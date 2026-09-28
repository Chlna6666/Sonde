use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use chrono::{NaiveDate, Utc};
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

const LOG_FILE_PREFIX: &str = "sonde.";
const LOG_FILE_SUFFIX: &str = ".log";

/// A thread-safe writer that outputs to stdout and automatically rolls log files by day.
///
/// Log files are created at `<log_dir>/sonde.YYYY-MM-DD.log`. When the date rolls over,
/// the current file is closed and a new date-stamped file is created.
#[derive(Clone)]
pub struct RollingDailyAppender {
    inner: Arc<Mutex<RollingDailyInner>>,
}

struct RollingDailyInner {
    log_dir: PathBuf,
    current_date: String,
    current_file: Option<File>,
    write_to_stdout: bool,
}

impl RollingDailyAppender {
    #[must_use]
    pub fn new(log_dir: PathBuf, write_to_stdout: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RollingDailyInner {
                log_dir,
                current_date: String::new(),
                current_file: None,
                write_to_stdout,
            })),
        }
    }
}

impl Write for RollingDailyAppender {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("rolling log appender mutex poisoned"))?;

        if inner.write_to_stdout {
            let _ = io::stdout().write_all(buf);
        }

        let today = Utc::now().format("%Y-%m-%d").to_string();
        if inner.current_file.is_none() || inner.current_date != today {
            fs::create_dir_all(&inner.log_dir)?;
            let file_path = inner
                .log_dir
                .join(format!("{LOG_FILE_PREFIX}{today}{LOG_FILE_SUFFIX}"));
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path)?;
            inner.current_file = Some(file);
            inner.current_date = today;
        }

        if let Some(file) = inner.current_file.as_mut() {
            file.write_all(buf)?;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("rolling log appender mutex poisoned"))?;

        if inner.write_to_stdout {
            let _ = io::stdout().flush();
        }
        if let Some(file) = inner.current_file.as_mut() {
            file.flush()?;
        }
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for RollingDailyAppender {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Initializes the global tracing subscriber with daily rolling log files and terminal output.
///
/// Uses `try_init()` so tests or repeat invocations do not panic if tracing was already initialized.
pub fn init(log_dir: &Path) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("sonde=info,actix_web=info"));
    let appender = RollingDailyAppender::new(log_dir.to_path_buf(), true);

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(appender)
        .with_ansi(false)
        .try_init();
}

/// Prunes log files in `log_dir` older than `retention_days`.
///
/// Looks for files following the `sonde.YYYY-MM-DD.log` pattern and deletes files whose
/// date is strictly older than `retention_days` relative to today (UTC).
///
/// Returns the number of pruned log files.
pub fn prune_archived_logs(log_dir: &Path, retention_days: u32) -> io::Result<usize> {
    if !log_dir.exists() {
        return Ok(0);
    }

    let today = Utc::now().date_naive();
    let mut pruned = 0;

    for entry in fs::read_dir(log_dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }

        let file_name = entry.file_name();
        let Some(name_str) = file_name.to_str() else {
            continue;
        };

        let Some(date_part) = name_str
            .strip_prefix(LOG_FILE_PREFIX)
            .and_then(|name| name.strip_suffix(LOG_FILE_SUFFIX))
        else {
            continue;
        };

        if let Ok(file_date) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d") {
            let age_days = (today - file_date).num_days();
            if age_days > i64::from(retention_days) {
                fs::remove_file(entry.path())?;
                pruned += 1;
            }
        }
    }

    Ok(pruned)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn rolling_appender_writes_to_date_stamped_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let log_dir = temp.path().join("logs");

        let mut appender = RollingDailyAppender::new(log_dir.clone(), false);
        writeln!(appender, "test log message 1").expect("write");
        writeln!(appender, "test log message 2").expect("write");
        appender.flush().expect("flush");

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let expected_file = log_dir.join(format!("sonde.{today}.log"));
        assert!(expected_file.exists());

        let content = fs::read_to_string(&expected_file).expect("read");
        assert!(content.contains("test log message 1"));
        assert!(content.contains("test log message 2"));
    }

    #[test]
    fn prune_archived_logs_deletes_only_expired_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let log_dir = temp.path().join("logs");
        fs::create_dir_all(&log_dir).expect("mkdir");

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let old_file = log_dir.join("sonde.2020-01-01.log");
        let recent_file = log_dir.join(format!("sonde.{today}.log"));
        let other_file = log_dir.join("other.log");

        fs::write(&old_file, "old logs").expect("write");
        fs::write(&recent_file, "recent logs").expect("write");
        fs::write(&other_file, "unrelated").expect("write");

        let pruned = prune_archived_logs(&log_dir, 14).expect("prune");
        assert_eq!(pruned, 1);
        assert!(!old_file.exists());
        assert!(recent_file.exists());
        assert!(other_file.exists());
    }
}
