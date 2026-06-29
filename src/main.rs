//! `bbr` binary entry point.

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    bbr::cli::run().await
}
