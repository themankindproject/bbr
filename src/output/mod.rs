//! Output formatting: pretty tables for humans, stable JSON for agents.

pub mod json;
pub mod table;
pub mod theme;

use std::io::{self, Write};

use crate::error::Result;

/// A formatter decides how a piece of data hits stdout.
///
/// `Human` formatters take already-rendered strings (tables / blocks);
/// `Json` formatters take any `Serialize` value.
pub enum Formatter {
    Human,
    Json,
}

impl Formatter {
    /// Pick a formatter from the `--json` flag.
    pub fn from_json_flag(json: bool) -> Self {
        if json {
            Formatter::Json
        } else {
            Formatter::Human
        }
    }

    /// Print a serializable value. For JSON, serialize directly. For human
    /// output, the caller must have already built a string.
    pub fn print<T: serde::Serialize>(&self, value: &T, human: &str) -> Result<()> {
        match self {
            Formatter::Json => json::print_json(value),
            Formatter::Human => print_block(human),
        }
    }
}

/// Write a human-readable block to stdout.
pub fn print_block(s: &str) -> Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(s.as_bytes())
        .map_err(crate::error::BitbucketError::Io)?;
    if !s.ends_with('\n') {
        out.write_all(b"\n")
            .map_err(crate::error::BitbucketError::Io)?;
    }
    Ok(())
}
