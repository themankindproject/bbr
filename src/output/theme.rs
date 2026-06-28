//! Color / decoration theme. Respects `NO_COLOR` and TTY detection.

use std::borrow::Cow;
use std::io::{self, IsTerminal};
use std::sync::OnceLock;

use colored::Colorize;

/// Global theme singleton (cheap to compute once).
static THEME: OnceLock<Theme> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    colors: bool,
}

impl Theme {
    pub fn current() -> &'static Theme {
        THEME.get_or_init(Theme::detect)
    }

    fn detect() -> Theme {
        let no_color = std::env::var_os("NO_COLOR").is_some();
        let is_tty = io::stdout().is_terminal();
        Theme {
            colors: !no_color && is_tty,
        }
    }

    pub fn colors_enabled(&self) -> bool {
        self.colors
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
        let line = "─".repeat(width.min(120));
        if self.colors {
            line.dimmed().to_string()
        } else {
            line
        }
    }

    /// A subtle section header glyph.
    pub fn bullet(&self) -> &'static str {
        if self.colors {
            "●"
        } else {
            "*"
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

/// Best-effort terminal width via `stty size` or `$COLUMNS`.
fn terminal_width() -> Option<usize> {
    static WIDTH: OnceLock<Option<usize>> = OnceLock::new();
    *WIDTH.get_or_init(|| {
        if let Ok(cols) = std::env::var("COLUMNS") {
            if let Ok(n) = cols.parse::<usize>() {
                if n > 0 {
                    return Some(n);
                }
            }
        }
        let output = std::process::Command::new("stty")
            .arg("size")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let fields: Vec<&str> = std::str::from_utf8(&output.stdout)
            .ok()?
            .split_whitespace()
            .collect();
        fields.get(1)?.parse::<usize>().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_disables() {
        std::env::set_var("NO_COLOR", "1");
        let t = Theme::detect();
        assert!(!t.colors_enabled());
        std::env::remove_var("NO_COLOR");
    }
}
