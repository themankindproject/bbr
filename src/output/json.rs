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

/// Print one compact newline-delimited JSON event to stdout.
pub fn print_ndjson<T: serde::Serialize>(value: &T) -> Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, value)?;
    out.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[derive(serde::Serialize)]
    struct Event {
        n: u32,
    }

    #[test]
    fn ndjson_payload_is_compact_and_single_line() {
        let value = Event { n: 1 };
        let json = serde_json::to_value(&value).unwrap();

        assert_eq!(json.to_string(), r#"{"n":1}"#);
    }
}
