//! Filesystem paths and credential/config file parsing.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{BitbucketError, Result};

/// Application / config directory name.
pub const APP_NAME: &str = "bb";

/// Filename for stored credentials.
pub const CREDENTIALS_FILE: &str = "credentials.toml";

/// Filename for general config (v0.3+; reserved now).
pub const CONFIG_FILE: &str = "config.toml";

/// Returns the platform-appropriate config directory for `bb`.
///
/// Order: `$XDG_CONFIG_HOME/bb` -> `$HOME/.config/bb` (Linux),
/// `~/Library/Application Support/bb` (macOS), `%APPDATA%\bb` (Windows).
pub fn config_dir() -> Option<PathBuf> {
    if let Ok(xdg) = xdg::BaseDirectories::with_prefix(APP_NAME) {
        return Some(xdg.get_config_home());
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
    /// Personal Access Token (preferred).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Legacy app password (deprecated by Bitbucket; kept for transition).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_password: Option<String>,
    /// Optional workspace override; otherwise inferred from git remote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

impl CredentialProfile {
    pub fn secret(&self) -> Option<&str> {
        self.token
            .as_deref()
            .or(self.app_password.as_deref())
            .filter(|s| !s.is_empty())
    }

    pub fn is_pat(&self) -> bool {
        self.token.is_some()
    }

    /// Detect if this looks like an Atlassian API token (starts with "ATATT").
    pub fn is_atlassian_api_token(&self) -> bool {
        self.token
            .as_deref()
            .map(|t| t.starts_with("ATATT"))
            .unwrap_or(false)
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
    read_credentials_file(&path).map(Some)
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
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
            .write_all(contents.as_bytes())?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
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
                app_password: None,
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
        // xdg prepends the prefix ("bb") to XDG_CONFIG_HOME, so write inside
        // a `bb` subdirectory.
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
        assert!(creds.default.is_pat());
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}
