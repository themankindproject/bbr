//! Output formatting: pretty tables for humans, stable JSON for agents.

pub mod json;
pub mod table;
pub mod theme;

use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};

use crate::error::Result;

/// A formatter decides how a piece of data hits stdout.
///
/// `Human` formatters take already-rendered strings (tables / blocks);
/// `Json` formatters take any `Serialize` value.
pub enum Formatter {
    Human { no_pager: bool },
    Json,
}

impl Formatter {
    /// Pick a formatter from the `--json` flag.
    pub fn from_json_flag(json: bool) -> Self {
        if json {
            Formatter::Json
        } else {
            Formatter::Human { no_pager: false }
        }
    }

    /// Pick a formatter with pager control.
    pub fn from_args(json: bool, no_pager: bool) -> Self {
        if json {
            Formatter::Json
        } else {
            Formatter::Human { no_pager }
        }
    }

    /// Print a serializable value. For JSON, serialize directly. For human
    /// output, the caller must have already built a string.
    pub fn print<T: serde::Serialize>(&self, value: &T, human: &str) -> Result<()> {
        match self {
            Formatter::Json => json::print_json(value),
            Formatter::Human { .. } => print_block(human),
        }
    }

    /// Print a serializable value with pagination if stdout is a terminal.
    pub fn print_paginated<T: serde::Serialize>(&self, value: &T, human: &str) -> Result<()> {
        match self {
            Formatter::Json => json::print_json(value),
            Formatter::Human { no_pager } => {
                if *no_pager {
                    print_block(human)
                } else {
                    print_paginated(human)
                }
            }
        }
    }

    /// Print diff output with syntax highlighting (bat) and paging.
    pub fn print_diff<T: serde::Serialize>(&self, value: &T, human: &str) -> Result<()> {
        match self {
            Formatter::Json => json::print_json(value),
            Formatter::Human { no_pager } => {
                if *no_pager {
                    print_block(human)
                } else {
                    print_diff(human)
                }
            }
        }
    }
}

/// Write a human-readable block to stdout.
pub fn print_block(s: &str) -> Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(s.as_bytes())?;
    if !s.ends_with('\n') {
        out.write_all(b"\n")?;
    }
    Ok(())
}

/// Print a diff with syntax highlighting (via `bat`) and paging, falling
/// back to `print_paginated` if `bat` is not available.
pub fn print_diff(s: &str) -> Result<()> {
    if !io::stdout().is_terminal() {
        return print_block(s);
    }

    let pager_env = std::env::var("PAGER").unwrap_or_default();

    // If the user explicitly set PAGER, respect it instead of sniffing for bat.
    // Otherwise, try bat first.
    if pager_env.is_empty() {
        if let Ok(mut child) = Command::new("bat")
            .args(["--language=diff", "--paging=always", "--color=always"])
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(s.as_bytes());
                if !s.ends_with('\n') {
                    let _ = stdin.write_all(b"\n");
                }
            }
            let _ = child.wait();
            return Ok(());
        }
    }

    print_paginated(s)
}

/// Write a human-readable block to stdout with optional pagination using less/PAGER.
pub fn print_paginated(s: &str) -> Result<()> {
    if !io::stdout().is_terminal() {
        return print_block(s);
    }

    let pager_env = std::env::var("PAGER").unwrap_or_else(|_| "less".to_string());
    let mut cmd = if pager_env == "less" {
        let mut c = Command::new("less");
        c.args(["-F", "-R", "-X"]);
        c
    } else {
        let mut parts = pager_env.split_whitespace();
        if let Some(bin) = parts.next() {
            let mut c = Command::new(bin);
            for arg in parts {
                c.arg(arg);
            }
            c
        } else {
            return print_block(s);
        }
    };

    cmd.stdin(Stdio::piped());

    if let Ok(mut child) = cmd.spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(s.as_bytes());
            if !s.ends_with('\n') {
                let _ = stdin.write_all(b"\n");
            }
        }
        let _ = child.wait();
        Ok(())
    } else {
        print_block(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_paginated_falls_back_when_not_terminal() {
        let res = print_paginated("hello test");
        assert!(res.is_ok());
    }
}
