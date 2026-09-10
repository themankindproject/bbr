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

/// Install a panic hook so `panic = "abort"` still yields a usable message.
///
/// Without this, any reachable panic prints the default Rust message and then
/// the process dies with `Aborted (core dumped)` — which tells a user nothing
/// about what to report. The hook runs before the abort, so it can print a
/// short, actionable line.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());
        eprintln!(
            "bbr: internal error at {location}\n  {payload}\n               This is a bug. Please report it at https://github.com/themankindproject/bbr/issues"
        );
    }));
}

fn main() -> ExitCode {
    #[cfg(unix)]
    reset_sigpipe();
    install_panic_hook();

    // Parse argument errors *before* building a runtime: `--help`, `--version`
    // and bad flags need no executor at all, and building one (previously
    // multi-threaded) cost several thread spawns and cgroup probes on every
    // invocation.
    let cli = match bbr::cli::parse_args() {
        Ok(cli) => cli,
        Err(code) => return code,
    };

    // A current-thread runtime is sufficient: all concurrency in bbr is
    // explicit and bounded (`buffer_unordered`) and is I/O-bound, so the
    // reactor makes progress on a single thread. Blocking work already goes
    // through `spawn_blocking`, which exists under `rt` as well.
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("bbr: failed to start async runtime: {e}");
            return ExitCode::from(1);
        }
    };

    bbr::cli::run(cli, &rt)
}
