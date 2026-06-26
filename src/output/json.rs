//! JSON output helper.

use std::io::{self, Write};

use crate::error::{BitbucketError, Result};

/// Pretty-print a `Serialize` value to stdout.
pub fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    let s = serde_json::to_string_pretty(value)?;
    let mut out = io::stdout().lock();
    out.write_all(s.as_bytes()).map_err(BitbucketError::Io)?;
    out.write_all(b"\n").map_err(BitbucketError::Io)?;
    Ok(())
}
