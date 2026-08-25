//! `bbr doctor` — environment self-check.
//!
//! Reports on everything that commonly breaks: git presence, repo identity,
//! credentials and their file permissions, API reachability, token scopes,
//! rate-limit headroom, pager tooling, and version currency. Every check
//! reports a finding; nothing aborts the run. Exit code stays 0 unless
//! `--strict` is passed AND at least one check failed — so `bbr doctor`
//! is safe to paste into bug reports without breaking scripts.

use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::commands::client;
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    fn glyph(self) -> String {
        let theme = crate::output::theme::Theme::current();
        match self {
            Status::Ok => theme.status_glyph("SUCCESSFUL"),
            Status::Warn => theme.warn("!").into_owned(),
            Status::Fail => theme.status_glyph("FAILED"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
}

fn check(name: &'static str, status: Status, detail: impl Into<String>) -> Check {
    Check {
        name,
        status,
        detail: detail.into(),
    }
}

/// Run all checks sequentially. Each check absorbs its own errors — a
/// failing check is data, not an early exit.
async fn run_checks(g: &GlobalArgs) -> Vec<Check> {
    let mut checks = vec![
        check_git(),
        check_repo_identity(g),
        check_credentials(),
        check_creds_permissions(),
        check_pager_tools(),
    ];
    // Online checks last; each needs the client.
    match online_checks(g).await {
        Ok(mut online) => checks.append(&mut online),
        Err(e) => checks.push(check(
            "api",
            Status::Fail,
            format!("could not build client: {e}"),
        )),
    }
    checks
}

fn check_git() -> Check {
    match std::process::Command::new("git").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            check("git", Status::Ok, v)
        }
        Ok(out) => check(
            "git",
            Status::Fail,
            format!("git --version exited {}", out.status),
        ),
        Err(e) => check("git", Status::Fail, format!("cannot spawn git: {e}")),
    }
}

fn check_repo_identity(g: &GlobalArgs) -> Check {
    match crate::commands::resolve_repo(g) {
        Ok(repo) => check(
            "repo identity",
            Status::Ok,
            format!("{}/{}", repo.workspace, repo.slug),
        ),
        Err(e) => check(
            "repo identity",
            Status::Fail,
            format!("{e} (or pass --workspace/--slug)"),
        ),
    }
}

fn check_credentials() -> Check {
    match crate::auth::resolve() {
        Ok(creds) => {
            let source = if std::env::var(crate::auth::ENV_TOKEN).is_ok() {
                "environment"
            } else {
                "config file"
            };
            check(
                "credentials",
                Status::Ok,
                format!("{} (from {})", creds.username, source),
            )
        }
        Err(_) => check(
            "credentials",
            Status::Fail,
            "none found — run `bbr auth setup` or set BITBUCKET_USERNAME + BITBUCKET_TOKEN",
        ),
    }
}

fn check_creds_permissions() -> Check {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Some(path) = crate::config::credentials_path() else {
            return check(
                "creds permissions",
                Status::Warn,
                "no config dir resolvable",
            );
        };
        if !path.exists() {
            return check("creds permissions", Status::Warn, "no credentials file");
        }
        match std::fs::metadata(&path) {
            Ok(meta) => {
                let mode = meta.permissions().mode() & 0o777;
                if mode & 0o077 == 0 {
                    check(
                        "creds permissions",
                        Status::Ok,
                        format!("{:o}", mode & 0o777),
                    )
                } else {
                    check(
                        "creds permissions",
                        Status::Warn,
                        format!(
                            "{:o} is too permissive — bbr will fix it to 0600 on next auth use",
                            mode & 0o777
                        ),
                    )
                }
            }
            Err(e) => check(
                "creds permissions",
                Status::Warn,
                format!("stat failed: {e}"),
            ),
        }
    }
    #[cfg(not(unix))]
    check(
        "creds permissions",
        Status::Warn,
        "not applicable on this platform",
    )
}

fn check_pager_tools() -> Check {
    let less = which_exists("less");
    let bat = which_exists("bat");
    match (less, bat) {
        (true, true) => check("pager tools", Status::Ok, "less, bat"),
        (true, false) => check(
            "pager tools",
            Status::Warn,
            "bat missing — diffs page but aren't syntax-highlighted by bat (bbr has built-in highlighting)",
        ),
        (false, _) => check("pager tools", Status::Warn, "less missing — output won't page"),
    }
}

fn which_exists(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let p = dir.join(bin);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    p.is_file()
                        && std::fs::metadata(&p)
                            .map(|m| m.permissions().mode() & 0o111 != 0)
                            .unwrap_or(false)
                }
                #[cfg(not(unix))]
                p.is_file()
            })
        })
        .unwrap_or(false)
}

/// Online checks: reachability, quota, version currency. Scopes are NOT
/// probed with writes; a scope table only appears when the user's own calls
/// produce a 403 with the standard envelope.
async fn online_checks(g: &GlobalArgs) -> Result<Vec<Check>> {
    let mut checks = Vec::new();
    let client = client(g)?;

    match client.current_user().await {
        Ok(user) => {
            checks.push(check(
                "api reachable",
                Status::Ok,
                format!(
                    "authenticated as {} ({})",
                    user.display_name,
                    client
                        .rate_limit_remaining()
                        .map(|r| r.to_string())
                        .as_deref()
                        .unwrap_or("quota unknown")
                ),
            ));
            match client.rate_limit_remaining() {
                Some(remaining) if remaining < 100 => checks.push(check(
                    "rate limit",
                    Status::Warn,
                    format!(
                        "{remaining} requests left this hour — batch operations will pace down"
                    ),
                )),
                Some(remaining) => checks.push(check(
                    "rate limit",
                    Status::Ok,
                    format!("{remaining} requests remaining this hour"),
                )),
                None => checks.push(check(
                    "rate limit",
                    Status::Warn,
                    "no rate-limit header seen yet",
                )),
            }
        }
        Err(crate::error::BitbucketError::AuthFailed(msg)) => {
            checks.push(check("api reachable", Status::Fail, msg));
            checks.push(check(
                "token scopes",
                Status::Fail,
                "run `bbr auth test`; required scopes: read:user, read:repository, read:pullrequest, read:pipeline (+ write variants for PR/pipeline actions)",
            ));
        }
        Err(e) => {
            checks.push(check("api reachable", Status::Fail, e.to_string()));
        }
    }

    // Version currency — non-fatal on any error.
    match crate::commands::update::outdated_version().await {
        Ok(Some(latest)) => checks.push(check(
            "version",
            Status::Warn,
            format!(
                "{} in use, {latest} available (`bbr update`)",
                env!("CARGO_PKG_VERSION")
            ),
        )),
        Ok(None) => checks.push(check(
            "version",
            Status::Ok,
            format!("{} (latest)", env!("CARGO_PKG_VERSION")),
        )),
        Err(e) => checks.push(check(
            "version",
            Status::Warn,
            format!("update check failed: {e}"),
        )),
    }

    Ok(checks)
}

fn render_human(checks: &[Check]) -> String {
    let theme = crate::output::theme::Theme::current();
    let mut s = format!("{} bbr doctor\n", theme.bold("Environment check"));
    s.push_str(&format!("{}\n", theme.separator()));
    let name_w = checks.iter().map(|c| c.name.len()).max().unwrap_or(4);
    for c in checks {
        let colored_label = match c.status {
            Status::Ok => theme.success(c.status.label()).into_owned(),
            Status::Warn => theme.warn(c.status.label()).into_owned(),
            Status::Fail => theme.error(c.status.label()).into_owned(),
        };
        // Pad glyphs to a fixed 4-column gutter so ok/fail rows ([ok] / [X])
        // and the warn row (!) stay vertically aligned.
        s.push_str(&format!(
            " {:<4} {:<name$}  {}\n",
            c.status.glyph(),
            theme.bold(c.name),
            format_args!("{:<5} {}", colored_label, c.detail),
            name = name_w
        ));
    }
    let fails = checks.iter().filter(|c| c.status == Status::Fail).count();
    let warns = checks.iter().filter(|c| c.status == Status::Warn).count();
    s.push_str(&format!(
        "\n{} ok, {warns} warn, {fails} fail\n",
        checks.len() - fails - warns
    ));
    s
}

pub async fn run(g: &GlobalArgs, strict: bool) -> Result<()> {
    let checks = run_checks(g).await;
    let fmt = crate::commands::make_formatter(g);
    fmt.print(&checks, &render_human(&checks))?;

    let any_fail = checks.iter().any(|c| c.status == Status::Fail);
    if strict && any_fail {
        return Err(crate::error::BitbucketError::Other(
            "one or more checks failed (--strict)".into(),
        ));
    }
    Ok(())
}
