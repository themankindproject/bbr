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
fn run_command(cmd: &str, message: &str) {
    let expanded = cmd.replace("%m", message);
    let status = Command::new("sh").arg("-c").arg(&expanded).status();
    match status {
        Ok(s) if !s.success() => {
            eprintln!("warning: --notify command exited non-zero: {s}");
        }
        Err(e) => eprintln!("warning: failed to run --notify command: {e}"),
        _ => {}
    }
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
}
