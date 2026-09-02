use std::path::{Path, PathBuf};
use std::{env, fmt};

/// Storage limits and retention knobs that aren't filesystem roots.
#[derive(Debug, Clone, Copy)]
pub struct StorageLimits {
    pub max_artifact_upload_bytes: u64,
    pub output_preview_length: usize,
    pub temp_file_retention_seconds: u64,
}

impl Default for StorageLimits {
    fn default() -> Self {
        StorageLimits {
            max_artifact_upload_bytes: 512 * 1024 * 1024,
            output_preview_length: 2048,
            temp_file_retention_seconds: 24 * 60 * 60,
        }
    }
}

pub const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:3000";

#[derive(Debug, Clone)]
pub struct Config {
    /// Socket address the HTTP server binds (env `LISTEN_ADDR`, default
    /// `0.0.0.0:3000`).
    pub listen_addr: std::net::SocketAddr,
    pub database_url: String,
    /// Base directory the storage roots default beneath when their own
    /// override env var is unset. Not itself used for any file operation.
    pub data_root: PathBuf,
    pub artifact_root: PathBuf,
    pub model_root: PathBuf,
    /// Directories beneath which `POST /api/v1/models/register` may
    /// register `.pte` files in place (env `MODEL_REGISTER_ROOTS`,
    /// colon-separated; default: just the model root). Paths outside every
    /// listed root are rejected, so an unauthenticated client cannot probe
    /// or hash arbitrary server files. Only the HTTP route is confined; the
    /// importer and Rust callers register wherever the operator points them.
    pub model_register_roots: Vec<PathBuf>,
    pub temporary_dir: PathBuf,
    pub trash_dir: PathBuf,
    /// Built dashboard output directory to serve at `/` (env
    /// `DASHBOARD_DIST`). `None` (unset) leaves the site root unserved.
    pub dashboard_dist: Option<PathBuf>,
    pub limits: StorageLimits,
    /// Seconds between keep-alive comments on the live event stream (env
    /// `EVENTS_KEEP_ALIVE_SECONDS`, default 15; the contract promises at
    /// most 30).
    pub events_keep_alive_seconds: u64,
}

#[derive(Debug)]
pub struct ConfigError(String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ConfigError {}

fn required_var(name: &str) -> Result<String, ConfigError> {
    let value = env::var(name)
        .map_err(|_| ConfigError(format!("{name} environment variable is not set")))?;
    if value.trim().is_empty() {
        return Err(ConfigError(format!("{name} environment variable is empty")));
    }
    Ok(value)
}

/// Reads `name`, falling back to `default` beneath `data_root` when unset.
/// An explicitly set but empty value is still an error.
fn path_var_or_default(
    name: &str,
    data_root: &std::path::Path,
    default_leaf: &str,
) -> Result<PathBuf, ConfigError> {
    match env::var(name) {
        Ok(value) if value.trim().is_empty() => {
            Err(ConfigError(format!("{name} environment variable is empty")))
        }
        Ok(value) => Ok(PathBuf::from(value)),
        Err(env::VarError::NotPresent) => Ok(data_root.join(default_leaf)),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigError(format!(
            "{name} environment variable is not valid UTF-8"
        ))),
    }
}

/// Reads an optional path: unset → `None`; set but empty → error.
fn optional_path_var(name: &str) -> Result<Option<PathBuf>, ConfigError> {
    match env::var(name) {
        Ok(value) if value.trim().is_empty() => {
            Err(ConfigError(format!("{name} environment variable is empty")))
        }
        Ok(value) => Ok(Some(PathBuf::from(value))),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigError(format!(
            "{name} environment variable is not valid UTF-8"
        ))),
    }
}

fn parse_u64_var(name: &str, default: u64) -> Result<u64, ConfigError> {
    match env::var(name) {
        Ok(value) if value.trim().is_empty() => {
            Err(ConfigError(format!("{name} environment variable is empty")))
        }
        Ok(value) => value.trim().parse::<u64>().map_err(|err| {
            ConfigError(format!("{name} is not a valid non-negative integer: {err}"))
        }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigError(format!(
            "{name} environment variable is not valid UTF-8"
        ))),
    }
}

fn parse_usize_var(name: &str, default: usize) -> Result<usize, ConfigError> {
    match env::var(name) {
        Ok(value) if value.trim().is_empty() => {
            Err(ConfigError(format!("{name} environment variable is empty")))
        }
        Ok(value) => value.trim().parse::<usize>().map_err(|err| {
            ConfigError(format!("{name} is not a valid non-negative integer: {err}"))
        }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigError(format!(
            "{name} environment variable is not valid UTF-8"
        ))),
    }
}

/// Reads a colon-separated list of directories from `name`, defaulting to
/// `[default]` when unset. An explicitly set but empty value, or an entry
/// that is empty after trimming, is an error rather than a silent skip.
fn path_list_var(name: &str, default: &Path) -> Result<Vec<PathBuf>, ConfigError> {
    match env::var(name) {
        Err(_) => Ok(vec![default.to_path_buf()]),
        Ok(value) if value.trim().is_empty() => Err(ConfigError(format!(
            "{name} environment variable is empty"
        ))),
        Ok(value) => value
            .split(':')
            .map(|entry| {
                let entry = entry.trim();
                if entry.is_empty() {
                    Err(ConfigError(format!("{name} contains an empty entry")))
                } else {
                    Ok(PathBuf::from(entry))
                }
            })
            .collect(),
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let listen_addr = match env::var("LISTEN_ADDR") {
            Ok(value) if value.trim().is_empty() => {
                return Err(ConfigError(
                    "LISTEN_ADDR environment variable is empty".to_string(),
                ));
            }
            Ok(value) => value.trim().parse().map_err(|err| {
                ConfigError(format!("LISTEN_ADDR is not a valid socket address: {err}"))
            })?,
            Err(_) => DEFAULT_LISTEN_ADDR
                .parse()
                .expect("default listen address is valid"),
        };
        let database_url = required_var("DATABASE_URL")?;

        let data_root = PathBuf::from(env::var("DATA_ROOT").unwrap_or_else(|_| "data".to_string()));

        let artifact_root = path_var_or_default("ARTIFACT_ROOT", &data_root, "artifacts")?;
        let model_root = path_var_or_default("MODEL_ROOT", &data_root, "models")?;
        let temporary_dir = path_var_or_default("TEMPORARY_DIR", &data_root, "temporary")?;
        let trash_dir = path_var_or_default("TRASH_DIR", &data_root, "trash")?;
        let model_register_roots = path_list_var("MODEL_REGISTER_ROOTS", &model_root)?;
        let dashboard_dist = optional_path_var("DASHBOARD_DIST")?;

        let defaults = StorageLimits::default();
        let limits = StorageLimits {
            max_artifact_upload_bytes: parse_u64_var(
                "MAX_ARTIFACT_UPLOAD_BYTES",
                defaults.max_artifact_upload_bytes,
            )?,
            output_preview_length: parse_usize_var(
                "OUTPUT_PREVIEW_LENGTH",
                defaults.output_preview_length,
            )?,
            temp_file_retention_seconds: parse_u64_var(
                "TEMP_FILE_RETENTION_SECONDS",
                defaults.temp_file_retention_seconds,
            )?,
        };

        Ok(Config {
            listen_addr,
            database_url,
            data_root,
            artifact_root,
            model_root,
            model_register_roots,
            temporary_dir,
            trash_dir,
            dashboard_dist,
            limits,
            events_keep_alive_seconds: parse_u64_var("EVENTS_KEEP_ALIVE_SECONDS", 15)?,
        })
    }

    /// Checks that a configured `DASHBOARD_DIST` is a readable directory
    /// containing an `index.html`, so a misconfigured path fails startup
    /// loudly instead of silently serving nothing. A `None` passes.
    pub fn validate_dashboard_dist(&self) -> Result<(), ConfigError> {
        let Some(dir) = &self.dashboard_dist else {
            return Ok(());
        };
        if !dir.is_dir() {
            return Err(ConfigError(format!(
                "dashboard directory ({}) is not a readable directory",
                dir.display()
            )));
        }
        let index = dir.join("index.html");
        if !index.is_file() {
            return Err(ConfigError(format!(
                "dashboard directory ({}) does not contain index.html",
                dir.display()
            )));
        }
        Ok(())
    }

    /// Creates every storage root (idempotent), returning a
    /// `ConfigError` naming the specific root that could not be created or
    /// is otherwise unusable. Never falls back to a different location.
    pub fn prepare_storage_roots(&self) -> Result<(), ConfigError> {
        for (label, path) in [
            ("artifact root", &self.artifact_root),
            ("model root", &self.model_root),
            ("temporary directory", &self.temporary_dir),
            ("trash directory", &self.trash_dir),
        ] {
            std::fs::create_dir_all(path).map_err(|err| {
                ConfigError(format!("{label} ({}) is unusable: {err}", path.display()))
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const ALL_VARS: &[&str] = &[
        "LISTEN_ADDR",
        "DATABASE_URL",
        "DATA_ROOT",
        "ARTIFACT_ROOT",
        "MODEL_ROOT",
        "TEMPORARY_DIR",
        "TRASH_DIR",
        "MODEL_REGISTER_ROOTS",
        "DASHBOARD_DIST",
        "MAX_ARTIFACT_UPLOAD_BYTES",
        "OUTPUT_PREVIEW_LENGTH",
        "TEMP_FILE_RETENTION_SECONDS",
    ];

    fn clear_env() {
        for var in ALL_VARS {
            unsafe {
                env::remove_var(var);
            }
        }
    }

    #[test]
    fn loads_database_url_and_artifact_root_when_present() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("ARTIFACT_ROOT", "data/artifacts");
        }
        let config = Config::from_env().expect("should load config");
        assert_eq!(config.database_url, "sqlite://data/benchmarks.sqlite3");
        assert_eq!(config.artifact_root, PathBuf::from("data/artifacts"));
        clear_env();
    }

    #[test]
    fn errors_when_database_url_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("ARTIFACT_ROOT", "data/artifacts");
        }
        let err = Config::from_env().expect_err("should fail without DATABASE_URL");
        assert!(err.to_string().contains("DATABASE_URL"));
        clear_env();
    }

    #[test]
    fn errors_when_database_url_empty() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "");
            env::set_var("ARTIFACT_ROOT", "data/artifacts");
        }
        let err = Config::from_env().expect_err("should fail with empty DATABASE_URL");
        assert!(err.to_string().contains("DATABASE_URL"));
        clear_env();
    }

    #[test]
    fn model_register_roots_default_to_the_model_root() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("MODEL_ROOT", "/srv/models");
        }
        let config = Config::from_env().expect("should load config");
        assert_eq!(config.model_register_roots, vec![PathBuf::from("/srv/models")]);
        clear_env();
    }

    #[test]
    fn model_register_roots_parse_a_colon_separated_list() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("MODEL_REGISTER_ROOTS", "/mnt/share/models: /srv/models ");
        }
        let config = Config::from_env().expect("should load config");
        assert_eq!(
            config.model_register_roots,
            vec![PathBuf::from("/mnt/share/models"), PathBuf::from("/srv/models")]
        );
        clear_env();
    }

    #[test]
    fn empty_model_register_roots_are_rejected() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("MODEL_REGISTER_ROOTS", "/srv/models::/other");
        }
        let err = Config::from_env().expect_err("an empty entry should fail");
        assert!(err.to_string().contains("MODEL_REGISTER_ROOTS"));
        clear_env();
    }

    #[test]
    fn storage_roots_default_beneath_data_root_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("DATA_ROOT", "benchmark-data");
        }
        let config = Config::from_env().expect("should load config with defaults");
        assert_eq!(
            config.artifact_root,
            PathBuf::from("benchmark-data/artifacts")
        );
        assert_eq!(config.model_root, PathBuf::from("benchmark-data/models"));
        assert_eq!(
            config.temporary_dir,
            PathBuf::from("benchmark-data/temporary")
        );
        assert_eq!(config.trash_dir, PathBuf::from("benchmark-data/trash"));
        clear_env();
    }

    #[test]
    fn each_storage_root_can_be_overridden_independently() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("DATA_ROOT", "benchmark-data");
            env::set_var("MODEL_ROOT", "/mnt/models");
        }
        let config = Config::from_env().expect("should load config");
        assert_eq!(
            config.artifact_root,
            PathBuf::from("benchmark-data/artifacts")
        );
        assert_eq!(config.model_root, PathBuf::from("/mnt/models"));
        clear_env();
    }

    #[test]
    fn errors_when_an_overridden_storage_root_is_empty() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("MODEL_ROOT", "");
        }
        let err = Config::from_env().expect_err("should fail with empty MODEL_ROOT");
        assert!(err.to_string().contains("MODEL_ROOT"));
        clear_env();
    }

    #[test]
    fn storage_limits_default_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
        }
        let config = Config::from_env().expect("should load config with default limits");
        let defaults = StorageLimits::default();
        assert_eq!(
            config.limits.max_artifact_upload_bytes,
            defaults.max_artifact_upload_bytes
        );
        assert_eq!(
            config.limits.output_preview_length,
            defaults.output_preview_length
        );
        assert_eq!(
            config.limits.temp_file_retention_seconds,
            defaults.temp_file_retention_seconds
        );
        clear_env();
    }

    #[test]
    fn storage_limits_can_be_overridden() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("MAX_ARTIFACT_UPLOAD_BYTES", "1024");
            env::set_var("OUTPUT_PREVIEW_LENGTH", "64");
            env::set_var("TEMP_FILE_RETENTION_SECONDS", "60");
        }
        let config = Config::from_env().expect("should load overridden limits");
        assert_eq!(config.limits.max_artifact_upload_bytes, 1024);
        assert_eq!(config.limits.output_preview_length, 64);
        assert_eq!(config.limits.temp_file_retention_seconds, 60);
        clear_env();
    }

    #[test]
    fn errors_when_a_storage_limit_is_not_a_valid_integer() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("MAX_ARTIFACT_UPLOAD_BYTES", "not-a-number");
        }
        let err = Config::from_env().expect_err("should fail with invalid integer");
        assert!(err.to_string().contains("MAX_ARTIFACT_UPLOAD_BYTES"));
        clear_env();
    }

    #[test]
    fn prepare_storage_roots_creates_every_configured_directory() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("DATA_ROOT", dir.path().to_str().unwrap());
        }
        let config = Config::from_env().expect("should load config");
        config
            .prepare_storage_roots()
            .expect("should create storage roots");
        assert!(config.artifact_root.is_dir());
        assert!(config.model_root.is_dir());
        assert!(config.temporary_dir.is_dir());
        assert!(config.trash_dir.is_dir());
        clear_env();
    }

    #[test]
    fn prepare_storage_roots_names_the_unusable_root_in_its_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        // A path with a regular file as a path component can never be
        // created as a directory, so `create_dir_all` fails deterministically.
        let blocking_file = tempfile::NamedTempFile::new().expect("failed to create temp file");
        let blocked_model_root = blocking_file.path().join("models");
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("ARTIFACT_ROOT", "data/artifacts");
            env::set_var("MODEL_ROOT", blocked_model_root.to_str().unwrap());
            env::set_var("TEMPORARY_DIR", "data/temporary");
            env::set_var("TRASH_DIR", "data/trash");
        }
        let config = Config::from_env().expect("should load config");
        let err = config
            .prepare_storage_roots()
            .expect_err("should fail to create the blocked model root");
        assert!(err.to_string().contains("model root"));
        clear_env();
    }

    #[test]
    fn dashboard_dist_is_none_when_unset_and_validates_trivially() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
        }
        let config = Config::from_env().expect("should load config");
        assert_eq!(config.dashboard_dist, None);
        config
            .validate_dashboard_dist()
            .expect("unset dashboard dir is valid");
        clear_env();
    }

    #[test]
    fn dashboard_dist_accepts_a_directory_containing_index_html() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("DASHBOARD_DIST", dir.path().to_str().unwrap());
        }
        let config = Config::from_env().expect("should load config");
        assert_eq!(config.dashboard_dist.as_deref(), Some(dir.path()));
        config.validate_dashboard_dist().expect("valid dashboard dir");
        clear_env();
    }

    #[test]
    fn dashboard_dist_rejects_a_missing_directory_and_a_directory_without_index() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("DASHBOARD_DIST", dir.path().join("missing").to_str().unwrap());
        }
        let err = Config::from_env()
            .unwrap()
            .validate_dashboard_dist()
            .expect_err("missing dir should fail");
        assert!(err.to_string().contains("dashboard directory"));
        assert!(err.to_string().contains("missing"));

        unsafe {
            env::set_var("DASHBOARD_DIST", dir.path().to_str().unwrap());
        }
        let err = Config::from_env()
            .unwrap()
            .validate_dashboard_dist()
            .expect_err("dir without index.html should fail");
        assert!(err.to_string().contains("index.html"));
        clear_env();
    }

    #[test]
    fn dashboard_dist_rejects_an_empty_value() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
            env::set_var("DASHBOARD_DIST", "");
        }
        let err = Config::from_env().expect_err("empty DASHBOARD_DIST should fail");
        assert!(err.to_string().contains("DASHBOARD_DIST"));
        clear_env();
    }

    #[test]
    fn listen_addr_defaults_and_can_be_overridden_or_rejected() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("DATABASE_URL", "sqlite://data/benchmarks.sqlite3");
        }
        let config = Config::from_env().unwrap();
        assert_eq!(config.listen_addr.to_string(), DEFAULT_LISTEN_ADDR);

        unsafe {
            env::set_var("LISTEN_ADDR", "127.0.0.1:3100");
        }
        let config = Config::from_env().unwrap();
        assert_eq!(config.listen_addr.to_string(), "127.0.0.1:3100");

        unsafe {
            env::set_var("LISTEN_ADDR", "not-an-address");
        }
        let err = Config::from_env().expect_err("invalid LISTEN_ADDR should fail");
        assert!(err.to_string().contains("LISTEN_ADDR"));
        clear_env();
    }
}
