//! Color / decoration theme. Respects `NO_COLOR` and TTY detection.

use std::borrow::Cow;
use std::io::{self, IsTerminal};
use std::sync::OnceLock;

use colored::Colorize;

/// Global theme singleton (cheap to compute once).
static THEME: OnceLock<Theme> = OnceLock::new();
static COLOR_OVERRIDE: OnceLock<bool> = OnceLock::new();
static UNICODE_OVERRIDE: OnceLock<bool> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    colors: bool,
    unicode: bool,
}

impl Theme {
    pub fn current() -> &'static Theme {
        THEME.get_or_init(|| {
            let colors = if let Some(&forced) = COLOR_OVERRIDE.get() {
                forced
            } else {
                let no_color = std::env::var_os("NO_COLOR").is_some();
                let is_tty = io::stdout().is_terminal();
                !no_color && is_tty
            };
            let unicode = UNICODE_OVERRIDE.get().copied().unwrap_or(true);
            Theme { colors, unicode }
        })
    }

    /// Set a color override. Must be called before the first `Theme::current()` access.
    /// Returns `Err` if the theme was already initialized.
    pub fn set_color_override(force_color: bool) {
        let _ = COLOR_OVERRIDE.set(force_color);
    }

    /// Set a unicode override. Must be called before the first `Theme::current()` access.
    /// Returns `Err` if the theme was already initialized.
    pub fn set_unicode_override(enable_unicode: bool) {
        let _ = UNICODE_OVERRIDE.set(enable_unicode);
    }

    pub fn colors_enabled(&self) -> bool {
        self.colors
    }

    pub fn unicode_enabled(&self) -> bool {
        self.unicode
    }

    /// Create a test theme with explicit settings. Only available in tests.
    #[cfg(test)]
    pub(crate) fn test_instance(colors: bool, unicode: bool) -> Theme {
        Theme { colors, unicode }
    }

    // --- semantic helpers -------------------------------------------------

    pub fn success<'a>(&self, s: &'a str) -> Cow<'a, str> {
        if self.colors {
            Cow::Owned(s.green().to_string())
        } else {
            Cow::Borrowed(s)
        }
    }

    pub fn warn<'a>(&self, s: &'a str) -> Cow<'a, str> {
        if self.colors {
            Cow::Owned(s.yellow().to_string())
        } else {
            Cow::Borrowed(s)
        }
    }

    pub fn error<'a>(&self, s: &'a str) -> Cow<'a, str> {
        if self.colors {
            Cow::Owned(s.red().to_string())
        } else {
            Cow::Borrowed(s)
        }
    }

    pub fn dim<'a>(&self, s: &'a str) -> Cow<'a, str> {
        if self.colors {
            Cow::Owned(s.dimmed().to_string())
        } else {
            Cow::Borrowed(s)
        }
    }

    pub fn bold<'a>(&self, s: &'a str) -> Cow<'a, str> {
        if self.colors {
            Cow::Owned(s.bold().to_string())
        } else {
            Cow::Borrowed(s)
        }
    }

    /// Dimmed label for field names (e.g. "Branch:", "Commit:").
    pub fn label(&self, s: &str) -> String {
        if self.colors {
            format!("{} ", s.dimmed())
        } else {
            format!("{s} ")
        }
    }

    /// Separator line matching the terminal width.
    pub fn separator(&self) -> String {
        let width = terminal_width().unwrap_or(80);
        let ch = if self.unicode { "─" } else { "-" };
        let line = ch.repeat(width.min(120));
        if self.colors {
            line.dimmed().to_string()
        } else {
            line
        }
    }

    /// A subtle section header glyph.
    pub fn bullet(&self) -> &'static str {
        if self.unicode {
            if self.colors {
                "●"
            } else {
                "*"
            }
        } else {
            "*"
        }
    }

    /// Standardized empty state message.
    pub fn empty(&self, msg: &str) -> String {
        if self.colors {
            format!("  {} {}\n", "—".dimmed(), msg.dimmed())
        } else {
            format!("  — {msg}\n")
        }
    }

    /// Standardized checkmark for success indicators.
    pub fn checkmark(&self) -> &'static str {
        if self.unicode {
            "✓"
        } else {
            "OK"
        }
    }

    /// Standardized cross for failure indicators.
    pub fn cross(&self) -> &'static str {
        if self.unicode {
            "✗"
        } else {
            "X"
        }
    }

    /// Status glyph that is safe in plain (no-color) output.
    pub fn status_glyph(&self, state: &str) -> String {
        if matches_ignore_ascii_case(state, &["SUCCESSFUL", "SUCCESS", "PASSED"]) {
            self.success("[ok]").into_owned()
        } else if matches_ignore_ascii_case(state, &["FAILED", "ERROR"]) {
            self.error("[X]").into_owned()
        } else if matches_ignore_ascii_case(state, &["STOPPED", "CANCELLED", "CANCELED"]) {
            self.warn("[!]").into_owned()
        } else if matches_ignore_ascii_case(state, &["INPROGRESS", "IN_PROGRESS", "RUNNING"]) {
            self.warn("[~]").into_owned()
        } else if matches_ignore_ascii_case(state, &["PENDING", "QUEUED"]) {
            self.dim("[.]").into_owned()
        } else {
            self.dim("[?]").into_owned()
        }
    }
}

fn matches_ignore_ascii_case(s: &str, values: &[&str]) -> bool {
    values.iter().any(|v| s.eq_ignore_ascii_case(v))
}

/// Best-effort terminal width.
///
/// Resolution order:
/// 1. `$COLUMNS` environment variable (works everywhere including CI overrides).
/// 2. `TIOCGWINSZ` ioctl on Unix (no subprocess, instant).
/// 3. Fall back to 80.
pub fn terminal_width() -> Option<usize> {
    // 1. Respect explicit override (useful in CI and scripts).
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(n) = cols.parse::<usize>() {
            if n > 0 {
                return Some(n);
            }
        }
    }

    // 2. ioctl(TIOCGWINSZ) — Linux / macOS only, no subprocess.
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        // Try stdout first, then stderr (one of them is likely a tty).
        for fd in [std::io::stdout().as_raw_fd(), std::io::stderr().as_raw_fd()] {
            if let Some(w) = tiocgwinsz(fd) {
                if w > 0 {
                    return Some(w);
                }
            }
        }
    }

    None
}

/// Call `TIOCGWINSZ` on the given file descriptor and return the column count.
#[cfg(unix)]
fn tiocgwinsz(fd: std::os::unix::io::RawFd) -> Option<usize> {
    // `libc::winsize` layout: ws_row, ws_col, ws_xpixel, ws_ypixel — all u16.
    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        _ws_xpixel: u16,
        _ws_ypixel: u16,
    }
    let mut ws = Winsize {
        ws_row: 0,
        ws_col: 0,
        _ws_xpixel: 0,
        _ws_ypixel: 0,
    };
    // SAFETY: `ws` is a valid C struct, `fd` is a raw file descriptor.
    // TIOCGWINSZ is 0x5413 on Linux / 0x40087468 on macOS — use the
    // platform constant via `libc`.
    let ret = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    if ret == 0 && ws.ws_col > 0 {
        Some(ws.ws_col as usize)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_disables() {
        let t = Theme {
            colors: false,
            unicode: true,
        };
        assert!(!t.colors_enabled());
    }

    #[test]
    fn status_glyph_maps_successful_states() {
        let t = Theme {
            colors: false,
            unicode: true,
        };
        assert_eq!(t.status_glyph("SUCCESSFUL"), "[ok]");
        assert_eq!(t.status_glyph("SUCCESS"), "[ok]");
        assert_eq!(t.status_glyph("PASSED"), "[ok]");
        assert_eq!(t.status_glyph("successful"), "[ok]");
    }

    #[test]
    fn status_glyph_maps_failed_states() {
        let t = Theme {
            colors: false,
            unicode: true,
        };
        assert_eq!(t.status_glyph("FAILED"), "[X]");
        assert_eq!(t.status_glyph("ERROR"), "[X]");
    }

    #[test]
    fn status_glyph_maps_stopped_states() {
        let t = Theme {
            colors: false,
            unicode: true,
        };
        assert_eq!(t.status_glyph("STOPPED"), "[!]");
        assert_eq!(t.status_glyph("CANCELLED"), "[!]");
        assert_eq!(t.status_glyph("CANCELED"), "[!]");
    }

    #[test]
    fn status_glyph_maps_inprogress_states() {
        let t = Theme {
            colors: false,
            unicode: true,
        };
        assert_eq!(t.status_glyph("INPROGRESS"), "[~]");
        assert_eq!(t.status_glyph("IN_PROGRESS"), "[~]");
        assert_eq!(t.status_glyph("RUNNING"), "[~]");
    }

    #[test]
    fn status_glyph_maps_pending_states() {
        let t = Theme {
            colors: false,
            unicode: true,
        };
        assert_eq!(t.status_glyph("PENDING"), "[.]");
        assert_eq!(t.status_glyph("QUEUED"), "[.]");
    }

    #[test]
    fn status_glyph_fallback_for_unknown() {
        let t = Theme {
            colors: false,
            unicode: true,
        };
        assert_eq!(t.status_glyph("UNKNOWN"), "[?]");
    }

    #[test]
    fn separator_uses_reasonable_width() {
        let t = Theme {
            colors: false,
            unicode: true,
        };
        let sep = t.separator();
        assert!(!sep.is_empty());
        let width = terminal_width().unwrap_or(80).min(120);
        assert_eq!(sep.chars().count(), width);
    }

    #[test]
    fn bullet_is_asterisk_when_no_color() {
        let t = Theme {
            colors: false,
            unicode: true,
        };
        assert_eq!(t.bullet(), "*");
    }

    #[test]
    fn label_appends_space() {
        let t = Theme {
            colors: false,
            unicode: true,
        };
        assert_eq!(t.label("Branch:"), "Branch: ");
    }

    #[test]
    fn matches_ignore_ascii_case_works() {
        assert!(matches_ignore_ascii_case(
            "SUCCESS",
            &["success", "SUCCESSFUL"]
        ));
        assert!(!matches_ignore_ascii_case(
            "FAILED",
            &["success", "SUCCESSFUL"]
        ));
    }
}
