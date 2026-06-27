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
        /// Watch mode — refresh every N seconds.
        #[arg(long)]
        watch: bool,
        /// Poll interval in seconds (used with --watch).
        #[arg(long, default_value_t = 5)]
        interval: u64,
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
        #[arg(
            long,
            help = "filter by state (open|merged|declined|all)",
            default_value = "open"
        )]
        state: String,
        #[arg(long, help = "max results to return", default_value_t = 25)]
        limit: u32,
        #[arg(long, help = "filter by author display name")]
        author: Option<String>,
        #[arg(long, help = "filter by source branch name")]
        source_branch: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// View a single pull request (defaults to the current branch's PR).
    View {
        /// Pull request ID (defaults to current branch's open PR).
        id: Option<u64>,
        /// Show the diff inline.
        #[arg(long)]
        diff: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Create a pull request.
    Create {
        #[arg(long, help = "PR title (required)")]
        title: String,
        #[arg(long, help = "PR description body")]
        body: Option<String>,
        #[arg(long, help = "read body from file")]
        body_file: Option<String>,
        #[arg(long, help = "read body from stdin")]
        body_stdin: bool,
        #[arg(long, help = "source branch (default: current branch)")]
        src: Option<String>,
        #[arg(long, help = "destination branch (default: repo default)")]
        dst: Option<String>,
        #[arg(long, help = "close source branch after merge")]
        close_source_branch: bool,
        #[arg(long, help = "reviewer UUID (repeatable)")]
        reviewer: Vec<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Comment on a pull request.
    Comment {
        /// Pull request ID.
        id: u64,
        #[arg(long, help = "comment body")]
        body: Option<String>,
        #[arg(long, help = "read body from file")]
        body_file: Option<String>,
        #[arg(long, help = "read body from stdin")]
        body_stdin: bool,
        #[arg(long, help = "reply to a specific comment ID")]
        reply_to: Option<u64>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Merge a pull request.
    Merge {
        id: u64,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Approve a pull request.
    Approve {
        id: u64,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Remove your approval from a pull request.
    Unapprove {
        id: u64,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Decline a pull request.
    Decline {
        id: u64,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Checkout a pull request's source branch locally.
    Checkout {
        id: u64,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Show the diff for a pull request.
    Diff {
        id: u64,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Update a pull request (title, description).
    Update {
        id: u64,
        #[arg(long, help = "new title")]
        title: Option<String>,
        #[arg(long, help = "new description body")]
        description: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum CiAction {
    /// List recent pipelines (default: current branch).
    List {
        #[arg(long, help = "branch name (default: current branch)")]
        branch: Option<String>,
        #[arg(long, help = "max results to return", default_value_t = 10)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Show the latest pipeline for a branch (default: current branch).
    Status {
        #[arg(long, help = "branch name (default: current branch)")]
        branch: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Live-tail a running pipeline; exit non-zero on failure.
    Watch {
        #[arg(long, help = "branch name (default: current branch)")]
        branch: Option<String>,
        #[arg(long, help = "poll interval in seconds", default_value_t = 5)]
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
        #[arg(long, help = "write log to file instead of stdout")]
        output: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// List steps for a pipeline (defaults to latest pipeline on current branch).
    Steps {
        /// Pipeline UUID (defaults to latest pipeline on current branch).
        uuid: Option<String>,
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
    /// Stop a running pipeline.
    Stop {
        /// Pipeline UUID (defaults to latest pipeline on current branch).
        uuid: Option<String>,
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
    /// List remote branches.
    Branches {
        #[arg(long, help = "max results", default_value_t = 20)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// List recent commits.
    Commits {
        #[arg(long, help = "branch name (default: current branch)")]
        branch: Option<String>,
        #[arg(long, help = "max results", default_value_t = 20)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum OpenAction {
    /// Open the repository page.
    Repo,
    /// Open the pull request list for the repo.
    PrList,
    /// Open a pull request (defaults to current branch's open PR).
    Pr { id: Option<u64> },
    /// Open the pipelines list for the repo.
    Pipelines,
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
    /// Show current credential status.
    Status {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Remove stored credentials.
    Logout {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Validate credentials by calling the API.
    Test {
        #[command(flatten)]
        g: GlobalArgs,
    },
}

/// Resolve the API base URL (flag > env > default).
pub fn resolve_api_base(g: &GlobalArgs) -> &str {
    g.api_base.as_deref().unwrap_or(DEFAULT_API_BASE)
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
        Some(Command::Status { g, watch, interval }) => {
            if watch {
                commands::status::run_watch(&g, interval).await
            } else {
                commands::status::run(&g).await
            }
        }
        Some(Command::Pr { action }) => match action {
            PrAction::List {
                state,
                limit,
                author,
                source_branch,
                g,
            } => {
                commands::pr::list(
                    &g,
                    &state,
                    limit,
                    author.as_deref(),
                    source_branch.as_deref(),
                )
                .await
            }
            PrAction::View { id, diff, g } => commands::pr::view(&g, id, diff).await,
            PrAction::Create {
                title,
                body,
                body_file,
                body_stdin,
                src,
                dst,
                close_source_branch,
                reviewer,
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
                    &reviewer,
                )
                .await
            }
            PrAction::Comment {
                id,
                body,
                body_file,
                body_stdin,
                reply_to,
                g,
            } => {
                commands::pr::comment(
                    &g,
                    id,
                    body.as_deref(),
                    body_file.as_deref(),
                    body_stdin,
                    reply_to,
                )
                .await
            }
            PrAction::Merge { id, g } => commands::pr::merge(&g, id).await,
            PrAction::Approve { id, g } => commands::pr::approve(&g, id).await,
            PrAction::Unapprove { id, g } => commands::pr::unapprove(&g, id).await,
            PrAction::Decline { id, g } => commands::pr::decline(&g, id).await,
            PrAction::Checkout { id, g } => commands::pr::checkout(&g, id).await,
            PrAction::Diff { id, g } => commands::pr::diff(&g, id).await,
            PrAction::Update {
                id,
                title,
                description,
                g,
            } => commands::pr::update(&g, id, title.as_deref(), description.as_deref()).await,
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
                output,
                g,
            } => {
                commands::ci::logs(
                    &g,
                    uuid.as_deref(),
                    step.as_deref(),
                    failed,
                    latest,
                    output.as_deref(),
                )
                .await
            }
            CiAction::List { branch, limit, g } => {
                commands::ci::list(&g, branch.as_deref(), limit).await
            }
            CiAction::Rerun { branch, g } => commands::ci::rerun(&g, branch.as_deref()).await,
            CiAction::Stop { uuid, branch, g } => {
                commands::ci::stop(&g, uuid.as_deref(), branch.as_deref()).await
            }
            CiAction::Steps { uuid, g } => commands::ci::steps(&g, uuid.as_deref()).await,
        },
        Some(Command::Repo { action }) => match action {
            RepoAction::Info { g } => commands::repo::info(&g).await,
            RepoAction::Branches { limit, g } => commands::repo::list_branches(&g, limit).await,
            RepoAction::Commits { branch, limit, g } => {
                commands::repo::list_commits(&g, branch.as_deref(), limit).await
            }
        },
        Some(Command::Open { action, g }) => commands::open::run(&g, action).await,
        Some(Command::Auth { action }) => match action {
            AuthAction::Setup => commands::auth::setup(),
            AuthAction::Status { g } => commands::auth::status(&g).await,
            AuthAction::Logout { g } => commands::auth::logout(&g),
            AuthAction::Test { g } => commands::auth::test(&g).await,
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

// Re-export so command modules can construct a `--json` formatter easily.
pub use crate::output::Formatter;
