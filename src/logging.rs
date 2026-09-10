//! Minimal leveled logging to stderr.
//!
//! bbr logs at exactly two levels from fifteen call sites, all of them plain
//! format strings — a retry notice, a low-rate-limit warning, a checksum
//! warning. The `tracing` + `tracing-subscriber` stack (with `env-filter`)
//! pulled in `matchers`, `regex-automata`, `regex-syntax`, `sharded-slab`,
//! `thread_local`, and `nu-ansi-term`, and installed a global subscriber with a
//! per-thread registry on **every** command, to serve those fifteen lines.
//!
//! This module does the same job in about sixty lines with no dependencies:
//!
//! * level comes from `-v`/`-vv`, overridable by `RUST_LOG`;
//! * output goes to stderr so it never contaminates `--json` on stdout;
//! * messages are prefixed with the level, matching the old output shape.

use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};

/// Log levels, ordered by increasing verbosity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    Off = 0,
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

/// Selected maximum level. `Warn` until [`init`] runs, so a warning emitted
/// during early setup is never lost.
static MAX_LEVEL: AtomicU8 = AtomicU8::new(Level::Warn as u8);

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Off | Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }
}

/// Parse a level name, accepting the spellings people actually type.
fn parse_level(s: &str) -> Option<Level> {
    match s.trim().to_ascii_lowercase().as_str() {
        "off" | "none" => Some(Level::Off),
        "error" | "err" => Some(Level::Error),
        "warn" | "warning" => Some(Level::Warn),
        "info" => Some(Level::Info),
        "debug" => Some(Level::Debug),
        "trace" => Some(Level::Trace),
        _ => None,
    }
}

/// Configure the logger from the `-v` count, honouring `RUST_LOG`.
///
/// `RUST_LOG` accepts a bare level (`debug`). The `target=level` directive
/// syntax that `tracing` supported is intentionally not implemented: bbr has a
/// single crate, so target filtering had nothing to filter, and reproducing
/// `regex-automata` to parse it is not worth the binary.
pub fn init(verbose: u8) {
    let from_env = std::env::var("RUST_LOG").ok().and_then(|v| {
        // Tolerate `debug,bbr=trace`-style input by taking the first bare
        // level we recognise; ignore anything that looks like a directive.
        v.split(',')
            .filter_map(|part| {
                let part = part.trim();
                if part.contains('=') {
                    None
                } else {
                    parse_level(part)
                }
            })
            .next()
    });

    let level = from_env.unwrap_or(match verbose {
        0 => Level::Warn,
        1 => Level::Debug,
        _ => Level::Trace,
    });

    MAX_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Whether a message at `level` would be emitted.
#[inline]
pub fn enabled(level: Level) -> bool {
    (level as u8) <= MAX_LEVEL.load(Ordering::Relaxed)
}

/// Write one already-formatted message. Called by the macros below.
///
/// A failure to write (a closed stderr) is deliberately ignored: logging must
/// never be the reason a command fails.
pub fn emit(level: Level, message: &str) {
    if !enabled(level) {
        return;
    }
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{} bbr: {message}", level.label());
}

/// Log at `warn` level. Drop-in for the `tracing::warn!` calls it replaces.
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        if $crate::logging::enabled($crate::logging::Level::Warn) {
            $crate::logging::emit($crate::logging::Level::Warn, &format!($($arg)*));
        }
    };
}

/// Log at `debug` level.
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        if $crate::logging::enabled($crate::logging::Level::Debug) {
            $crate::logging::emit($crate::logging::Level::Debug, &format!($($arg)*));
        }
    };
}

/// Log at `info` level.
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        if $crate::logging::enabled($crate::logging::Level::Info) {
            $crate::logging::emit($crate::logging::Level::Info, &format!($($arg)*));
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_level_names() {
        assert_eq!(parse_level("debug"), Some(Level::Debug));
        assert_eq!(parse_level("WARN"), Some(Level::Warn));
        assert_eq!(parse_level(" warning "), Some(Level::Warn));
        assert_eq!(parse_level("off"), Some(Level::Off));
        assert_eq!(parse_level("nonsense"), None);
    }

    #[test]
    fn levels_are_ordered_by_verbosity() {
        assert!(Level::Error < Level::Warn);
        assert!(Level::Warn < Level::Info);
        assert!(Level::Info < Level::Debug);
        assert!(Level::Debug < Level::Trace);
    }

    #[test]
    fn enabled_respects_the_configured_level() {
        // Save and restore the global level so this test does not perturb
        // others running in the same process.
        let saved = MAX_LEVEL.load(Ordering::Relaxed);

        MAX_LEVEL.store(Level::Warn as u8, Ordering::Relaxed);
        assert!(enabled(Level::Warn));
        assert!(enabled(Level::Error));
        assert!(!enabled(Level::Info));
        assert!(!enabled(Level::Debug));

        MAX_LEVEL.store(Level::Trace as u8, Ordering::Relaxed);
        assert!(enabled(Level::Debug));
        assert!(enabled(Level::Trace));

        MAX_LEVEL.store(Level::Off as u8, Ordering::Relaxed);
        assert!(!enabled(Level::Error));

        MAX_LEVEL.store(saved, Ordering::Relaxed);
    }
}
