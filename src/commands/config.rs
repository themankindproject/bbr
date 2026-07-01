//! `bbr config` — view and manage configuration.

use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::config;
use crate::error::{BitbucketError, Result};
use crate::output::Formatter;

#[derive(Debug, Serialize)]
pub struct ConfigOut {
    pub config_path: Option<String>,
    pub credentials_path: Option<String>,
    pub workspace: Option<String>,
    pub username: Option<String>,
    pub has_token: bool,
}

pub fn run_path(g: &GlobalArgs) -> Result<()> {
    let cfg_path = config::config_path();
    let creds_path = config::credentials_path();
    let out = ConfigOut {
        config_path: cfg_path.as_ref().map(|p| p.display().to_string()),
        credentials_path: creds_path.as_ref().map(|p| p.display().to_string()),
        workspace: None,
        username: None,
        has_token: false,
    };
    let human = format!(
        "config:      {}\ncredentials: {}",
        out.config_path.as_deref().unwrap_or("(unavailable)"),
        out.credentials_path.as_deref().unwrap_or("(unavailable)"),
    );
    Formatter::from_json_flag(g.json).print(&out, &human)
}

pub fn run_show(g: &GlobalArgs) -> Result<()> {
    let creds = config::load_credentials()?;
    let cfg_path = config::config_path();
    let creds_path = config::credentials_path();

    let out = ConfigOut {
        config_path: cfg_path.as_ref().map(|p| p.display().to_string()),
        credentials_path: creds_path.as_ref().map(|p| p.display().to_string()),
        username: creds.as_ref().map(|c| c.default.username.clone()),
        workspace: creds.as_ref().and_then(|c| c.default.workspace.clone()),
        has_token: creds.as_ref().and_then(|c| c.default.secret()).is_some(),
    };

    let human = format!(
        "config_path:      {}\n\
         credentials_path: {}\n\
         workspace:        {}\n\
         username:         {}\n\
         has_token:        {}",
        out.config_path.as_deref().unwrap_or("(unavailable)"),
        out.credentials_path.as_deref().unwrap_or("(unavailable)"),
        out.workspace.as_deref().unwrap_or("(not set)"),
        out.username.as_deref().unwrap_or("(not set)"),
        out.has_token,
    );
    Formatter::from_json_flag(g.json).print(&out, &human)
}

pub fn run_set(g: &GlobalArgs, key: &str, value: &str) -> Result<()> {
    match key {
        "workspace" => {
            let mut creds = config::load_credentials()?.unwrap_or_default();
            creds.default.workspace = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            };
            let path = config::save_credentials(&creds)?;
            let human = format!("Set workspace = \"{value}\" in {}", path.display());
            let out = serde_json::json!({ "key": key, "value": value, "path": path.display().to_string() });
            Formatter::from_json_flag(g.json).print(&out, &human)
        }
        _ => Err(BitbucketError::Other(format!(
            "unknown config key: {key} (valid: workspace)"
        ))),
    }
}
