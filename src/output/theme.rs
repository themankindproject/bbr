//! Color / decoration theme. Respects `NO_COLOR` and TTY detection.

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

    pub fn success(&self, s: &str) -> String {
        if self.colors {
            s.green().to_string()
        } else {
            s.to_string()
        }
    }

    pub fn warn(&self, s: &str) -> String {
        if self.colors {
            s.yellow().to_string()
        } else {
            s.to_string()
        }
    }

    pub fn error(&self, s: &str) -> String {
        if self.colors {
            s.red().to_string()
        } else {
            s.to_string()
        }
    }

    pub fn dim(&self, s: &str) -> String {
        if self.colors {
            s.dimmed().to_string()
        } else {
            s.to_string()
        }
    }

    pub fn bold(&self, s: &str) -> String {
        if self.colors {
            s.bold().to_string()
        } else {
            s.to_string()
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
        let width = terminal_width().unwrap_or(72);
        let line = "─".repeat(width.min(72));
        if self.colors {
            line.dimmed().to_string()
        } else {
            line
        }
    }

    /// A subtle section header glyph.
    pub fn bullet(&self) -> String {
        if self.colors {
            "●".to_string()
        } else {
            "*".to_string()
        }
    }

    /// Status glyph that is safe in plain (no-color) output.
    pub fn status_glyph(&self, state: &str) -> String {
        let upper = state.to_ascii_uppercase();
        match upper.as_str() {
            "SUCCESSFUL" | "SUCCESS" | "PASSED" => self.success("[ok]"),
            "FAILED" | "ERROR" => self.error("[X]"),
            "STOPPED" | "CANCELLED" | "CANCELED" => self.warn("[!]"),
            "INPROGRESS" | "IN_PROGRESS" | "RUNNING" => self.warn("[~]"),
            "PENDING" | "QUEUED" => self.dim("[.]"),
            _ => self.dim("[?]"),
        }
        .to_string()
    }
}

/// Best-effort terminal width via `stty size`.
fn terminal_width() -> Option<usize> {
    use std::process::Command;
    let output = Command::new("stty")
        .arg("size")
        .arg("-F")
        .arg("/dev/stderr")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let fields: Vec<&str> = stdout.split_whitespace().collect();
    if fields.len() < 2 {
        return None;
    }
    fields[1].parse::<usize>().ok()
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
