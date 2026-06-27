//! JSON output helper.

use std::io::{self, Write};

use crate::error::Result;

/// Pretty-print a `Serialize` value to stdout.
pub fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer_pretty(&mut out, value)?;
    out.write_all(b"\n")?;
    Ok(())
}
