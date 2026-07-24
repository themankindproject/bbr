//! Filesystem paths and credential/config file parsing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{BitbucketError, Result};

/// Application / config directory name.
pub const APP_NAME: &str = "bbr";

/// Filename for stored credentials.
pub const CREDENTIALS_FILE: &str = "credentials.toml";

/// Filename for general config (v0.3+; reserved now).
pub const CONFIG_FILE: &str = "config.toml";

/// Returns the platform-appropriate config directory for `bbr`.
///
/// - Linux: `$XDG_CONFIG_HOME/bbr` or `$HOME/.config/bbr`
/// - macOS: `~/Library/Application Support/bbr`
/// - Windows: `%APPDATA%\bbr`
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    if let Ok(xdg) = xdg::BaseDirectories::with_prefix(APP_NAME) {
        return Some(xdg.get_config_home());
    }
    #[cfg(not(unix))]
    if let Ok(val) = std::env::var("XDG_CONFIG_HOME") {
        let p = PathBuf::from(val);
        if p.is_absolute() {
            return Some(p.join(APP_NAME));
        }
    }
    dirs::config_dir().map(|d| d.join(APP_NAME))
}

/// Full path to the credentials file, if a config dir is resolvable.
pub fn credentials_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(CREDENTIALS_FILE))
}

/// Full path to the general config file, if a config dir is resolvable.
pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(CONFIG_FILE))
}

/// On-disk shape of `credentials.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CredentialsFile {
    #[serde(default)]
    pub default: CredentialProfile,
}

/// A single credential profile (only `default` is used in v0.1).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CredentialProfile {
    pub username: String,
    /// Atlassian API token (from id.atlassian.com).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Optional workspace override; otherwise inferred from git remote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

impl CredentialProfile {
    pub fn secret(&self) -> Option<&str> {
        self.token
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    }
}

/// Read and parse the credentials file. Returns `Ok(None)` if no file exists.
pub fn load_credentials() -> Result<Option<CredentialsFile>> {
    let path = match credentials_path() {
        Some(p) => p,
        None => return Ok(None),
    };
    if !path.exists() {
        return Ok(None);
    }
    let creds = read_credentials_file(&path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&path)?;
        let mode = meta.permissions().mode();
        if mode & 0o077 != 0 {
            eprintln!(
                "bbr: warning: {} has overly permissive file mode {:o}; fixing to 0600",
                path.display(),
                mode & 0o777
            );
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            if let Err(e) = fs::set_permissions(&path, perms) {
                eprintln!(
                    "bbr: warning: failed to fix permissions on {}: {e}",
                    path.display()
                );
            }
        }
    }

    Ok(Some(creds))
}

fn read_credentials_file(path: &Path) -> Result<CredentialsFile> {
    let raw = fs::read_to_string(path)
        .map_err(|e| BitbucketError::Config(format!("reading {}: {e}", path.display())))?;
    let parsed: CredentialsFile = toml::from_str(&raw)
        .map_err(|e| BitbucketError::Config(format!("parsing {}: {e}", path.display())))?;
    Ok(parsed)
}

/// Write the credentials file with mode 0600 on unix. Creates the parent dir.
pub fn save_credentials(creds: &CredentialsFile) -> Result<PathBuf> {
    let path = credentials_path()
        .ok_or_else(|| BitbucketError::Config("no writable config directory".into()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| BitbucketError::Config(format!("creating {}: {e}", parent.display())))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    let serialized = toml::to_string_pretty(creds)
        .map_err(|e| BitbucketError::Config(format!("serializing credentials: {e}")))?;

    write_private(&path, &serialized)
        .map_err(|e| BitbucketError::Config(format!("writing {}: {e}", path.display())))?;

    Ok(path)
}

/// Write `contents` to `path` with mode 0600 on Unix (atomically created so
/// the file is never readable by others even for a moment).
fn write_private(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
            .write_all(contents.as_bytes())?;
        Ok(())
    }
    #[cfg(not(unix))]
    fs::write(path, contents)
}

/// Delete the credentials file, if present. Returns `true` if a file was removed.
pub fn delete_credentials() -> Result<bool> {
    let Some(path) = credentials_path() else {
        return Ok(false);
    };
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(&path)
        .map_err(|e| BitbucketError::Config(format!("removing {}: {e}", path.display())))?;
    Ok(true)
}

/// On-disk shape of `config.toml` (contexts, active context, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_context: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub contexts: HashMap<String, ContextEntry>,
}

/// A named workspace/slug profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEntry {
    pub workspace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
}

/// Load `config.toml`. Returns a default (empty) config if the file does not exist.
pub fn load_config() -> Result<ConfigFile> {
    let path = match config_path() {
        Some(p) => p,
        None => return Ok(ConfigFile::default()),
    };
    if !path.exists() {
        return Ok(ConfigFile::default());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| BitbucketError::Config(format!("reading {}: {e}", path.display())))?;
    let parsed: ConfigFile = toml::from_str(&raw)
        .map_err(|e| BitbucketError::Config(format!("parsing {}: {e}", path.display())))?;
    Ok(parsed)
}

/// Write `config.toml`. Creates the parent directory if needed.
pub fn save_config(cfg: &ConfigFile) -> Result<PathBuf> {
    let path = config_path()
        .ok_or_else(|| BitbucketError::Config("no writable config directory".into()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| BitbucketError::Config(format!("creating {}: {e}", parent.display())))?;
    }
    let serialized = toml::to_string_pretty(cfg)
        .map_err(|e| BitbucketError::Config(format!("serializing config: {e}")))?;
    fs::write(&path, serialized)
        .map_err(|e| BitbucketError::Config(format!("writing {}: {e}", path.display())))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    #[test]
    fn save_credentials_uses_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        let creds = CredentialsFile {
            default: CredentialProfile {
                username: "u".into(),
                token: Some("t".into()),
                workspace: None,
            },
        };
        let path = save_credentials(&creds).unwrap();
        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn parses_token_profile() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempdir().unwrap();
        let bb_dir = tmp.path().join(APP_NAME);
        fs::create_dir_all(&bb_dir).unwrap();
        let f = bb_dir.join(CREDENTIALS_FILE);
        let mut fh = fs::File::create(&f).unwrap();
        writeln!(fh, "[default]").unwrap();
        writeln!(fh, r#"username = "u""#).unwrap();
        writeln!(fh, r#"token = "t""#).unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        let creds = load_credentials().unwrap().unwrap();
        assert_eq!(creds.default.username, "u");
        assert_eq!(creds.default.secret(), Some("t"));
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn config_file_roundtrip() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());

        let mut cfg = ConfigFile {
            active_context: Some("work".into()),
            contexts: HashMap::new(),
        };
        cfg.contexts.insert(
            "work".into(),
            ContextEntry {
                workspace: "mycompany".into(),
                slug: Some("main-repo".into()),
            },
        );
        cfg.contexts.insert(
            "personal".into(),
            ContextEntry {
                workspace: "myuser".into(),
                slug: None,
            },
        );

        save_config(&cfg).unwrap();
        let loaded = load_config().unwrap();
        assert_eq!(loaded.active_context, Some("work".into()));
        assert_eq!(loaded.contexts.len(), 2);
        assert_eq!(loaded.contexts["work"].workspace, "mycompany");
        assert_eq!(loaded.contexts["work"].slug, Some("main-repo".into()));
        assert_eq!(loaded.contexts["personal"].workspace, "myuser");
        assert_eq!(loaded.contexts["personal"].slug, None);

        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn load_config_returns_default_when_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());

        let cfg = load_config().unwrap();
        assert!(cfg.active_context.is_none());
        assert!(cfg.contexts.is_empty());

        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn config_file_parses_toml_with_contexts() {
        let toml_str = r#"
active_context = "dev"

[contexts.dev]
workspace = "devteam"
slug = "api"

[contexts.staging]
workspace = "devteam"
"#;
        let cfg: ConfigFile = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.active_context, Some("dev".into()));
        assert_eq!(cfg.contexts.len(), 2);
        assert_eq!(cfg.contexts["dev"].slug, Some("api".into()));
        assert_eq!(cfg.contexts["staging"].slug, None);
    }
}
