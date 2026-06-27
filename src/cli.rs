//! Clap command definition and top-level dispatch.

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use tracing_subscriber::EnvFilter;

use crate::commands;
use crate::error::{report, ExitCode as AppExitCode, Result};
/// Default Bitbucket Cloud REST API base.
pub const DEFAULT_API_BASE: &str = "https://api.bitbucket.org/2.0";

/// Global flags available on every subcommand.
#[derive(Debug, Args, Clone)]
pub struct GlobalArgs {
    /// Emit stable JSON instead of pretty human output.
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub json: bool,

    /// Override the Bitbucket API base URL (mostly for tests).
    #[arg(long, global = true, env = "BITBUCKET_API_BASE", hide = true)]
    pub api_base: Option<String>,

    /// Increase verbosity (-v info, -vv debug).
    #[arg(short, long, global = true, action = ArgAction::Count)]
    pub verbose: u8,
}

/// `bb` — a Bitbucket Cloud CLI.
#[derive(Debug, Parser)]
#[command(
    name = "bb",
    version,
    about = "BitBucket Remote — a Bitbucket Cloud CLI for coding agents and humans",
    long_about = None,
    propagate_version = true,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// PR + CI for the current branch (the killer feature).
    Status {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Pull request operations.
    Pr {
        #[command(subcommand)]
        action: PrAction,
    },
    /// Pipeline / CI operations.
    Ci {
        #[command(subcommand)]
        action: CiAction,
    },
    /// Repository metadata.
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },
    /// Open Bitbucket pages in your browser.
    Open {
        #[command(subcommand)]
        action: Option<OpenAction>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Credential management.
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// Emit shell completions to stdout.
    Completion {
        /// Target shell.
        shell: Shell,
    },
}

#[derive(Debug, Subcommand)]
pub enum PrAction {
    /// List pull requests in the current repo.
    List {
        #[arg(long, default_value = "open")]
        state: String,
        #[arg(long, default_value_t = 25)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// View a single pull request (defaults to the current branch's PR).
    View {
        id: Option<u64>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Create a pull request.
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        body_file: Option<String>,
        #[arg(long, help = "read body from stdin")]
        body_stdin: bool,
        #[arg(long)]
        src: Option<String>,
        #[arg(long)]
        dst: Option<String>,
        #[arg(long, help = "close source branch after merge")]
        close_source_branch: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Comment on a pull request.
    Comment {
        id: u64,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        body_file: Option<String>,
        #[arg(long)]
        body_stdin: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Merge a pull request.
    Merge {
        id: u64,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum CiAction {
    /// Show the latest pipeline for a branch (default: current branch).
    Status {
        #[arg(long)]
        branch: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Live-tail a running pipeline; exit non-zero on failure.
    Watch {
        #[arg(long)]
        branch: Option<String>,
        #[arg(long, default_value_t = 5)]
        interval_secs: u64,
        #[arg(long, help = "print failing step log when the pipeline fails")]
        logs: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Fetch logs for a pipeline (defaults to latest pipeline on current branch).
    Logs {
        /// Pipeline UUID (with or without braces). Defaults to latest pipeline on current branch.
        uuid: Option<String>,
        #[arg(long, help = "specific step UUID or step name")]
        step: Option<String>,
        #[arg(long, help = "select the failing step automatically")]
        failed: bool,
        #[arg(long, help = "select the latest step automatically")]
        latest: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Rerun the latest pipeline for a branch.
    Rerun {
        #[arg(long)]
        branch: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum RepoAction {
    /// Print the workspace/slug for the current directory.
    Info {
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum OpenAction {
    /// Open the repository page.
    Repo,
    /// Open a pull request (defaults to current branch's open PR).
    Pr { id: Option<u64> },
    /// Open the latest pipeline for the current branch.
    Ci {
        #[arg(long)]
        branch: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthAction {
    /// Interactive credential setup.
    Setup,
    /// Verify stored credentials work.
    Status {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Remove stored credentials.
    Logout,
}

/// Resolve the API base URL (flag > env > default).
pub fn resolve_api_base(g: &GlobalArgs) -> String {
    g.api_base
        .clone()
        .unwrap_or_else(|| DEFAULT_API_BASE.to_string())
}

/// Entry point invoked by `main`. Returns a process exit code.
pub async fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            // clap prints its own message; honor its exit code (0 for --help).
            e.exit();
        }
    };

    init_tracing(cli.global.verbose);

    let result: Result<()> = dispatch(cli).await;

    match result {
        Ok(()) => AppExitCode::Success.as_process(),
        Err(e) => report(&e),
    }
}

async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        None => commands::status::run(&cli.global).await,
        Some(Command::Status { g }) => commands::status::run(&g).await,
        Some(Command::Pr { action }) => match action {
            PrAction::List { state, limit, g } => commands::pr::list(&g, &state, limit).await,
            PrAction::View { id, g } => commands::pr::view(&g, id).await,
            PrAction::Create {
                title,
                body,
                body_file,
                body_stdin,
                src,
                dst,
                close_source_branch,
                g,
            } => {
                commands::pr::create(
                    &g,
                    &title,
                    body.as_deref(),
                    body_file.as_deref(),
                    body_stdin,
                    src.as_deref(),
                    dst.as_deref(),
                    close_source_branch,
                )
                .await
            }
            PrAction::Comment {
                id,
                body,
                body_file,
                body_stdin,
                g,
            } => {
                commands::pr::comment(&g, id, body.as_deref(), body_file.as_deref(), body_stdin)
                    .await
            }
            PrAction::Merge { id, g } => commands::pr::merge(&g, id).await,
        },
        Some(Command::Ci { action }) => match action {
            CiAction::Status { branch, g } => commands::ci::status(&g, branch.as_deref()).await,
            CiAction::Watch {
                branch,
                interval_secs,
                logs,
                g,
            } => commands::ci::watch(&g, branch.as_deref(), interval_secs, logs).await,
            CiAction::Logs {
                uuid,
                step,
                failed,
                latest,
                g,
            } => commands::ci::logs(&g, uuid.as_deref(), step.as_deref(), failed, latest).await,
            CiAction::Rerun { branch, g } => commands::ci::rerun(&g, branch.as_deref()).await,
        },
        Some(Command::Repo { action }) => match action {
            RepoAction::Info { g } => commands::repo::info(&g).await,
        },
        Some(Command::Open { action, g }) => commands::open::run(&g, action).await,
        Some(Command::Auth { action }) => match action {
            AuthAction::Setup => commands::auth::setup(),
            AuthAction::Status { g } => commands::auth::status(&g).await,
            AuthAction::Logout => commands::auth::logout(),
        },
        Some(Command::Completion { shell }) => {
            generate(shell, &mut Cli::command(), "bb", &mut io::stdout());
            Ok(())
        }
    }
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    // Only emit logs to stderr; stdout is reserved for data.
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(io::stderr().is_terminal())
        .init();
}

/// Helper for commands that need to know if stdout is a terminal.
pub fn stdout_is_tty() -> bool {
    io::stdout().is_terminal()
}

// Re-export so command modules can construct a `--json` formatter easily.
pub use crate::output::Formatter;
