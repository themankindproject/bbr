//! `bbr auth` — setup / status / logout.

use std::io::{self, BufRead, Write};

use serde::Serialize;

use crate::auth::{self, CredentialKind};
use crate::cli::GlobalArgs;
use crate::commands::client;
use crate::config::{self, CredentialProfile, CredentialsFile};
use crate::error::{BitbucketError, Result};
use crate::output::Formatter;

const API_TOKEN_URL: &str = "https://id.atlassian.com/manage-profile/security/api-tokens";

#[derive(Debug, Serialize)]
pub struct AuthStatusOut {
    pub authenticated: bool,
    pub username: String,
    pub credential_kind: Option<String>,
    pub display_name: Option<String>,
    pub account_id: Option<String>,
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_remaining: Option<u64>,
}

/// Credential setup (interactive or non-interactive).
pub fn setup(username: Option<String>, token: Option<String>) -> Result<()> {
    let (username, secret) = match (username, token) {
        (Some(u), Some(t)) => (u.trim().to_string(), t.trim().to_string()),
        (None, None) => {
            println!("bbr auth setup");
            println!("  Need an API token? {API_TOKEN_URL}");
            println!("  Required scopes (select ALL for full CLI access):");
            println!("    ✓ read:user:bitbucket");
            println!("    ✓ read:repository:bitbucket");
            println!("    ✓ write:repository:bitbucket  (for commit statuses)");
            println!("    ✓ read:pullrequest:bitbucket");
            println!("    ✓ write:pullrequest:bitbucket  (create/merge/approve PRs)");
            println!("    ✓ read:pipeline:bitbucket");
            println!("    ✓ write:pipeline:bitbucket    (rerun/stop pipelines)");
            println!("    ✓ read:issue:bitbucket        (optional — issue tracking)");
            println!("    ✓ write:issue:bitbucket       (optional — create issues)");
            println!("    ✓ webhook:bitbucket           (optional — webhook management)");
            println!();

            let u = prompt("Bitbucket username (email): ")?;
            if u.trim().is_empty() {
                return Err(BitbucketError::Other("username is required".into()));
            }
            let s = prompt_secret("API token: ")?;
            if s.is_empty() {
                return Err(BitbucketError::Other("secret is required".into()));
            }
            (u.trim().to_string(), s)
        }
        (Some(_), None) => {
            return Err(BitbucketError::Other(
                "--token is required when --username is provided".into(),
            ));
        }
        (None, Some(_)) => {
            return Err(BitbucketError::Other(
                "--username is required when --token is provided".into(),
            ));
        }
    };

    let existing_workspace = if let Ok(Some(file)) = config::load_credentials() {
        file.default.workspace.clone()
    } else {
        None
    };

    let profile = CredentialProfile {
        username,
        token: Some(secret),
        workspace: existing_workspace,
    };

    let creds = CredentialsFile {
        default: profile.clone(),
    };
    let path = config::save_credentials(&creds)?;
    println!("  Stored credentials in: {}", path.display());
    println!("  Run `bbr auth test` to verify.");
    Ok(())
}

/// Verify auth works by calling `GET /user`.
pub async fn status(g: &GlobalArgs) -> Result<()> {
    let creds = auth::resolve();
    let (username, kind) = match creds {
        Ok(c) => (c.username, Some(c.kind)),
        Err(_) => (String::new(), None),
    };

    let source = if std::env::var(auth::ENV_TOKEN).is_ok() {
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
    let (authenticated, display_name, account_id, error_msg, rate_limit_remaining) = match client {
        Ok(c) => match c.current_user().await {
            Ok(u) => (
                true,
                Some(u.display_name),
                u.uuid,
                None,
                c.rate_limit_remaining(),
            ),
            Err(e) => (
                false,
                None,
                None,
                Some(e.to_string()),
                c.rate_limit_remaining(),
            ),
        },
        Err(e) => (false, None, None, Some(e.to_string()), None),
    };

    let out = AuthStatusOut {
        authenticated,
        username,
        credential_kind: kind.map(|k| match k {
            CredentialKind::ApiToken => "atlassian_api_token".into(),
        }),
        display_name,
        account_id,
        source,
        rate_limit_remaining,
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = if out.authenticated {
        let mut msg = format!(
            "Authenticated as {} ({}) via {}",
            out.display_name.as_deref().unwrap_or(&out.username),
            out.username,
            out.source
        );
        if let Some(remaining) = out.rate_limit_remaining {
            msg.push_str(&format!("\nAPI rate limit remaining: {remaining}"));
            if remaining < 50 {
                msg.push_str(" (low — consider slowing batch operations)");
            }
        }
        msg
    } else {
        let mut msg = String::new();
        if let Some(err) = &error_msg {
            msg.push_str(err);
        } else {
            msg.push_str("Not authenticated.");
        }
        msg.push_str("\nRun `bbr auth setup`.");
        msg
    };
    fmt.print(&out, &human)
}

/// Validate credentials by calling the API.
pub async fn test(g: &GlobalArgs) -> Result<()> {
    let creds = auth::resolve()?;
    let client = client(g)?;

    let user = client.current_user().await?;
    let out = serde_json::json!({
        "authenticated": true,
        "display_name": user.display_name,
        "uuid": user.uuid,
        "credential_type": "atlassian_api_token",
    });
    let human = format!(
        "✓ Authenticated as {} ({})",
        user.display_name, creds.username
    );
    Formatter::from_json_flag(g.json).print(&out, &human)
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
    let mut out = io::stderr().lock();
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
    let s = rpassword::prompt_password(msg).map_err(BitbucketError::Io)?;
    let s = strip_bracketed_paste(&s);
    let s = s.trim().to_string();
    eprintln!("  ✓ Token read ({} characters)", s.len());
    Ok(s)
}

/// Strip bracketed-paste escape sequences that modern terminals wrap pasted
/// text in (`\x1b[200~` … `\x1b[201~`).  These pass through in canonical
/// mode (which `rpassword` uses) and would corrupt the stored credential.
fn strip_bracketed_paste(s: &str) -> &str {
    const BP_START: &str = "\x1b[200~";
    const BP_END: &str = "\x1b[201~";
    let s = s.strip_prefix(BP_START).unwrap_or(s);
    s.strip_suffix(BP_END).unwrap_or(s)
}
