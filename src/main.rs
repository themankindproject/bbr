//! `bbr` binary entry point.

use std::process::ExitCode;

/// Restore the default SIGPIPE disposition on Unix.
///
/// Rust's stdlib sets `SIGPIPE` to `SIG_IGN` at startup, which turns every
/// write to a closed pipe into an `EPIPE` `io::Error`. Any write path that
/// doesn't explicitly swallow that error (most `eprint!`/`eprintln!` sites)
/// then panics — and with `panic = "abort"` in release builds that becomes an
/// abort and a core dump. This was reproducible with
/// `bbr status --watch 2>&1 | head -1`.
///
/// Resetting to `SIG_DFL` makes the kernel terminate the process cleanly on
/// the first write to a closed pipe, exactly like standard Unix tools
/// (`git`, `grep`, ...). This covers stdout, stderr, and the pager's stdin in
/// one stroke, so no write path can panic on `EPIPE` again.
#[cfg(unix)]
fn reset_sigpipe() {
    // SAFETY: `signal(SIGPIPE, SIG_DFL)` is a plain, well-defined libc call
    // with no memory-safety implications.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    #[cfg(unix)]
    reset_sigpipe();
    bbr::cli::run().await
}
