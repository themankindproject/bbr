//! `bb auth` — setup / status / logout.

use std::io::{self, BufRead, Write};

use serde::Serialize;

use crate::auth::{self, CredentialKind};
use crate::cli::GlobalArgs;
use crate::commands::client;
use crate::config::{self, CredentialProfile, CredentialsFile};
use crate::error::{BitbucketError, Result};
use crate::output::Formatter;

const PAT_HELP_URL: &str = "https://id.atlassian.com/manage-profile/security/api-tokens";

#[derive(Debug, Serialize)]
pub struct AuthStatusOut {
    pub authenticated: bool,
    pub username: String,
    pub credential_kind: Option<String>,
    pub display_name: Option<String>,
    pub account_id: Option<String>,
    pub source: &'static str,
}

/// Interactive credential setup.
pub fn setup() -> Result<()> {
    println!("bb auth setup");
    println!("  Need a Personal Access Token? {PAT_HELP_URL}");
    println!("  Required scopes: account:read, repository:read, repository:write,");
    println!("                   pullrequest:read, pullrequest:write, pipeline:read");
    println!();

    let username = prompt("Bitbucket username (email): ")?;
    if username.trim().is_empty() {
        return Err(BitbucketError::Other("username is required".into()));
    }

    println!("  Credential type:");
    println!("    1) Atlassian API Token (recommended, from id.atlassian.com)");
    println!("    2) Personal Access Token (from bitbucket.org)");
    println!("    3) App password (legacy)");
    let choice = prompt("Choose [1]: ")?;
    let kind = match choice.trim() {
        "2" => CredentialKind::Pat,
        "3" => CredentialKind::AppPassword,
        _ => CredentialKind::ApiToken,
    };

    let secret = prompt_secret("Secret: ")?;
    if secret.is_empty() {
        return Err(BitbucketError::Other("secret is required".into()));
    }

    let profile = CredentialProfile {
        username: username.trim().to_string(),
        token: (kind == CredentialKind::Pat || kind == CredentialKind::ApiToken)
            .then_some(secret.clone()),
        app_password: (kind == CredentialKind::AppPassword).then_some(secret),
        workspace: None,
    };

    let creds = CredentialsFile {
        default: profile.clone(),
    };
    let path = config::save_credentials(&creds)?;
    println!("  Stored credentials in: {}", path.display());
    println!("  Run `bb auth status` to verify.");
    Ok(())
}

/// Verify auth works by calling `GET /user`.
pub async fn status(g: &GlobalArgs) -> Result<()> {
    let creds = auth::resolve();
    let (username, kind) = match creds {
        Ok(c) => (c.username, Some(c.kind)),
        Err(_) => (String::new(), None),
    };

    let source = if std::env::var(auth::ENV_TOKEN).is_ok()
        || std::env::var(auth::ENV_APP_PASSWORD).is_ok()
    {
        "environment"
    } else if config::credentials_path()
        .map(|p| p.exists())
        .unwrap_or(false)
    {
        "config-file"
    } else {
        "none"
    };

    let client = client(g);
    let (authenticated, display_name, account_id, error_msg) = match client {
        Ok(c) => match c.current_user().await {
            Ok(u) => (true, Some(u.display_name), u.uuid, None),
            Err(e) => (false, None, None, Some(e.to_string())),
        },
        Err(e) => (false, None, None, Some(e.to_string())),
    };

    let out = AuthStatusOut {
        authenticated,
        username,
        credential_kind: kind.map(|k| match k {
            CredentialKind::Pat => "pat".into(),
            CredentialKind::AppPassword => "app_password".into(),
            CredentialKind::ApiToken => "atlassian_api_token".into(),
        }),
        display_name,
        account_id,
        source,
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = if out.authenticated {
        format!(
            "Authenticated as {} ({}) via {}",
            out.display_name.as_deref().unwrap_or(&out.username),
            out.username,
            out.source
        )
    } else {
        let mut msg = "Not authenticated.".to_string();
        if let Some(err) = &error_msg {
            msg.push_str(&format!(" {err}"));
        }
        msg.push_str(" Run `bb auth setup`.");
        msg
    };
    fmt.print(&out, &human)
}

/// Remove stored credentials.
pub fn logout(g: &GlobalArgs) -> Result<()> {
    let removed = config::delete_credentials()?;
    let out = serde_json::json!({ "removed": removed });
    let human = if removed {
        "Removed stored credentials.".to_string()
    } else {
        "No stored credentials to remove.".to_string()
    };
    Formatter::from_json_flag(g.json).print(&out, &human)
}

// ---- prompt helpers -------------------------------------------------------

fn prompt(msg: &str) -> Result<String> {
    let mut out = io::stdout().lock();
    out.write_all(msg.as_bytes()).map_err(BitbucketError::Io)?;
    out.flush().map_err(BitbucketError::Io)?;
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(BitbucketError::Io)?;
    Ok(line.trim_end().to_string())
}

fn prompt_secret(msg: &str) -> Result<String> {
    rpassword::prompt_password(msg).map_err(BitbucketError::Io)
}
