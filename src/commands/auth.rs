//! `bbr auth` — setup / status / logout.

use std::io::{self, BufRead, IsTerminal, Read, Write};

use secrecy::SecretString;
use serde::Serialize;

use crate::auth;
use crate::cli::GlobalArgs;
use crate::commands::{client, make_formatter};
use crate::config::{self, CredentialProfile, CredentialsFile};
use crate::error::{BitbucketError, Result};

const API_TOKEN_URL: &str = "https://id.atlassian.com/manage-profile/security/api-tokens";

/// Atlassian API-token scopes shown by `auth setup`, with what each enables.
///
/// The single source for the interactive prompt; tests check it against the
/// README scope table so the two cannot drift apart.
pub const TOKEN_SCOPES: &[(&str, &str)] = &[
    ("read:user:bitbucket", "auth test/status, pr dashboard"),
    (
        "read:repository:bitbucket",
        "repos, branches, commits, src, search",
    ),
    ("write:repository:bitbucket", "commit statuses"),
    (
        "read:pullrequest:bitbucket",
        "list/view PRs, comments, diffs",
    ),
    ("write:pullrequest:bitbucket", "create/merge/approve PRs"),
    ("read:pipeline:bitbucket", "pipelines, logs, test reports"),
    ("write:pipeline:bitbucket", "trigger/rerun/stop pipelines"),
    ("read:issue:bitbucket", "optional: bbr issue"),
    ("write:issue:bitbucket", "optional: create/edit issues"),
    ("read:webhook:bitbucket", "optional: bbr webhook list/view"),
    (
        "write:webhook:bitbucket",
        "optional: create/delete webhooks",
    ),
    (
        "read:ssh-key:bitbucket",
        "optional: bbr deploy-keys list/view",
    ),
    ("write:ssh-key:bitbucket", "optional: add deploy keys"),
    ("delete:ssh-key:bitbucket", "optional: delete deploy keys"),
    ("read:workspace:bitbucket", "optional: bbr workspace list"),
    (
        "delete:repository:bitbucket",
        "optional, destructive: bbr repo delete",
    ),
];

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

/// Credential setup (interactive or non-interactive). Does not verify online.
pub fn setup(
    g: &GlobalArgs,
    username: Option<String>,
    token: Option<String>,
    token_stdin: bool,
) -> Result<()> {
    if token_stdin && (token.is_some() || username.is_none()) {
        return Err(BitbucketError::Usage(
            "--token-stdin requires --username and cannot be combined with --token".into(),
        ));
    }
    // Validate before consuming stdin or touching existing credentials.
    if username.as_ref().is_some_and(|u| u.trim().is_empty()) {
        return Err(BitbucketError::Usage("username must not be empty".into()));
    }
    let token = if token_stdin {
        if io::stdin().is_terminal() {
            return Err(BitbucketError::Usage(
                "--token-stdin expects piped input; use `bbr auth setup` for a hidden terminal prompt".into(),
            ));
        }
        Some(read_setup_token(io::stdin().lock())?)
    } else {
        token
    };
    let (username, secret) = match (username, token) {
        (Some(u), Some(t)) => (u.trim().to_string(), t.trim().to_string()),
        (None, None) => {
            if g.json
                || std::env::var_os("BBR_NO_INTERACTIVE").is_some()
                || !io::stdin().is_terminal()
                || !io::stderr().is_terminal()
            {
                return Err(BitbucketError::Usage(
                    "interactive auth setup is unavailable; pass --username with --token-stdin, or set BITBUCKET_USERNAME + BITBUCKET_TOKEN".into(),
                ));
            }
            println!("bbr auth setup");
            println!("  Need an API token? {API_TOKEN_URL}");
            println!("  Token scopes (grant the ones for the commands you use):");
            let check = if crate::output::theme::Theme::current().unicode_enabled() {
                "✓"
            } else {
                "*"
            };
            let width = TOKEN_SCOPES.iter().map(|(s, _)| s.len()).max().unwrap_or(0);
            for (scope, enables) in TOKEN_SCOPES {
                println!("    {check} {scope:<width$}  ({enables})");
            }
            println!();

            let u = prompt("Bitbucket username (email): ")?;
            if u.trim().is_empty() {
                return Err(BitbucketError::Usage("username is required".into()));
            }
            let s = prompt_secret("API token: ")?;
            if s.is_empty() {
                return Err(BitbucketError::Usage("secret is required".into()));
            }
            (u.trim().to_string(), s)
        }
        (Some(_), None) => {
            return Err(BitbucketError::Usage(
                "--token or --token-stdin is required when --username is provided".into(),
            ));
        }
        (None, Some(_)) => {
            return Err(BitbucketError::Usage(
                "--username is required when --token is provided".into(),
            ));
        }
    };

    if secret.is_empty() {
        return Err(BitbucketError::Usage("API token must not be empty".into()));
    }

    let existing_workspace = if let Ok(Some(file)) = config::load_credentials() {
        file.default.workspace.clone()
    } else {
        None
    };

    let profile = CredentialProfile {
        username,
        token: Some(SecretString::from(secret)),
        workspace: existing_workspace,
    };

    let creds = CredentialsFile { default: profile };
    let path = config::save_credentials(&creds)?;
    let out = serde_json::json!({
        "saved": true,
        "username": creds.default.username,
        "path": path.display().to_string(),
    });
    let human = format!(
        "  Stored credentials in: {}\n  Run `bbr auth test` to verify.",
        path.display()
    );
    make_formatter(g).print(&out, &human)
}

/// Bound secret input so a mistakenly piped large file cannot exhaust memory.
fn read_setup_token(reader: impl Read) -> Result<String> {
    const MAX_TOKEN_BYTES: u64 = 64 * 1024;
    let mut bytes = Vec::new();
    reader.take(MAX_TOKEN_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TOKEN_BYTES {
        return Err(BitbucketError::Usage("token input exceeds 64 KiB".into()));
    }
    let token = String::from_utf8(bytes)
        .map_err(|_| BitbucketError::Usage("token input must be UTF-8 text".into()))?;
    Ok(token.trim().to_string())
}

/// Verify auth works by calling `GET /user`.
///
/// Unlike most commands this is a *status* command: it reports what it finds
/// rather than failing loudly. When no credentials exist the human output says
/// exactly that (exit 0 — the report is the answer). When credentials exist
/// but the API call fails, the failure is reported truthfully and mapped to
/// its stable exit code so scripts can branch on it.
pub async fn status(g: &GlobalArgs) -> Result<()> {
    let creds = auth::resolve_with_source();
    let (username, credential_kind, source) = match creds {
        Ok((c, source)) => (
            c.username,
            Some("atlassian_api_token".to_string()),
            source.as_str(),
        ),
        Err(BitbucketError::NoCredentials) => (String::new(), None, "none"),
        Err(e) => return Err(e),
    };

    let mut out = AuthStatusOut {
        authenticated: false,
        username,
        credential_kind: credential_kind.clone(),
        display_name: None,
        account_id: None,
        source,
        rate_limit_remaining: None,
    };

    // No credentials at all: report it (exit 0 — this *is* the status).
    if credential_kind.is_none() {
        let fmt = make_formatter(g);
        let human = "No Bitbucket credentials found. Run `bbr auth setup` or set \
                     BITBUCKET_USERNAME + BITBUCKET_TOKEN."
            .to_string();
        return fmt.print(&out, &human);
    }

    let client = client(g)?;
    match client.current_user().await {
        Ok(u) => {
            out.authenticated = true;
            out.display_name = Some(u.display_name);
            out.account_id = u.uuid;
            out.rate_limit_remaining = client.rate_limit_remaining();
        }
        Err(e) => {
            // Credentials exist but the API rejected/errored: surface the
            // real failure. Exit non-zero so scripts can detect it.
            return Err(e);
        }
    }

    let fmt = make_formatter(g);
    let mut human = format!(
        "Authenticated as {} ({}) via {}",
        out.display_name.as_deref().unwrap_or(&out.username),
        out.username,
        out.source
    );
    if let Some(remaining) = out.rate_limit_remaining {
        human.push_str(&format!("\nAPI rate limit remaining: {remaining}"));
        if remaining < 50 {
            human.push_str(" (low — consider slowing batch operations)");
        }
    }
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
        "{} Authenticated as {} ({})",
        if crate::output::theme::Theme::current().unicode_enabled() {
            "✓"
        } else {
            "OK"
        },
        user.display_name,
        creds.username
    );
    make_formatter(g).print(&out, &human)
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
    make_formatter(g).print(&out, &human)
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
    let check = if crate::output::theme::Theme::current().unicode_enabled() {
        "✓"
    } else {
        "OK"
    };
    eprintln!("  {check} Token read ({} characters)", s.len());
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
