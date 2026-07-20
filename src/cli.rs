//! Clap command definition and CLI entry point.

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use tracing_subscriber::EnvFilter;

use crate::error::{report, ExitCode as AppExitCode, Result};

/// When to emit ANSI color output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    /// Use color if the output is a TTY and `NO_COLOR` is not set (default).
    Auto,
    /// Always use color, even when piped.
    Always,
    /// Never use color.
    Never,
}
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

    /// Override the workspace inferred from git remote.
    #[arg(long, global = true, env = "BB_WORKSPACE")]
    pub workspace: Option<String>,

    /// Override the repo slug inferred from git remote.
    #[arg(long = "slug", global = true, env = "BB_SLUG")]
    pub repo_slug: Option<String>,

    /// Disable output paging (no less).
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub no_pager: bool,

    /// Suppress non-essential output (for scripting).
    #[arg(short, long, global = true, action = ArgAction::SetTrue)]
    pub quiet: bool,

    /// When to use color: auto (default), always, or never.
    /// Also controlled by NO_COLOR / CLICOLOR / CLICOLOR_FORCE env vars.
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub color: ColorChoice,

    /// Disable ANSI color output (equivalent to --color never).
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub no_color: bool,

    /// Use ASCII characters instead of Unicode (for terminals that don't support UTF-8).
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub no_unicode: bool,

    /// HTTP request timeout in seconds (default: 30).
    #[arg(long, global = true, env = "BBR_TIMEOUT")]
    pub timeout: Option<u64>,
}

/// `bbr` — a Bitbucket Cloud CLI.
#[derive(Debug, Parser)]
#[command(
    name = "bbr",
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
        /// Compact single-line output.
        #[arg(long)]
        short: bool,
        /// Export format (slack|markdown).
        #[arg(long, value_parser = ["slack", "markdown"])]
        export: Option<String>,
    },
    /// Pull request operations.
    Pr {
        #[command(subcommand)]
        action: PrAction,
    },
    /// Batch operations on PRs and pipelines.
    Batch {
        #[command(subcommand)]
        action: BatchAction,
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
    /// Commit metadata and build statuses.
    Commit {
        #[command(subcommand)]
        action: CommitAction,
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
    /// Emit shell completions to stdout or install them.
    Completion {
        /// Target shell (auto-detected from $SHELL if omitted with --install).
        shell: Option<Shell>,
        /// Install the completion script for the detected shell.
        #[arg(long)]
        install: bool,
    },
    /// View or set configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Make an authenticated API request to any Bitbucket endpoint.
    Api {
        /// HTTP method (GET, POST, PUT, DELETE).
        method: String,
        /// API path (e.g. /repositories/ws/slug).
        path: String,
        /// JSON body to send (for POST/PUT).
        #[arg(long)]
        data: Option<String>,
        /// Follow pagination and emit merged values array.
        #[arg(long)]
        paginate: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Repository webhook management.
    Webhook {
        #[command(subcommand)]
        action: WebhookAction,
    },
    /// Browse remote source files.
    Src {
        #[command(subcommand)]
        action: SrcAction,
    },
    /// Deployment and environment operations.
    Deploy {
        #[command(subcommand)]
        action: DeployAction,
    },
    /// Manage repository issues.
    Issue {
        #[command(subcommand)]
        action: IssueAction,
    },
    /// Search code across the workspace.
    Search {
        /// Search query.
        query: String,
        /// Limit search to a specific repository.
        #[arg(long)]
        repo: Option<String>,
        /// Max results.
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Print JSON schema for a command's --json output.
    Schema {
        /// Name of the model schema to print (e.g. status, pr, ci, repo, webhook, src, issue).
        model: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Check for and install updates.
    Update {
        /// Check only, don't install.
        #[arg(long)]
        check: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Workspace operations.
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
    /// Manage pipeline variables (list, set, delete).
    Variable {
        #[command(subcommand)]
        action: VariableAction,
    },
    /// Manage repository deploy keys.
    #[command(name = "deploy-keys")]
    DeployKeys {
        #[command(subcommand)]
        action: DeployKeysAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceAction {
    /// List workspaces you have access to.
    List {
        /// Filter by role (member, contributor, admin).
        #[arg(long)]
        role: Option<String>,
        /// Max results.
        #[arg(long, default_value_t = 25)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum VariableAction {
    /// List pipeline variables for the repository.
    List {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Set a pipeline variable (creates or updates).
    Set {
        /// Variable key name.
        key: String,
        /// Variable value.
        value: String,
        /// Mark variable as secured/encrypted (value hidden after creation).
        #[arg(long)]
        secured: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Delete a pipeline variable by key name.
    Delete {
        /// Variable key name to delete.
        key: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum DeployKeysAction {
    /// List deploy keys for the current repository.
    List {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Add a new deploy key to the repository.
    Add {
        /// SSH public key (e.g. ssh-rsa AAAA...).
        #[arg(long)]
        key: String,
        /// Human-readable label for the key.
        #[arg(long)]
        label: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// View a specific deploy key by ID.
    View {
        /// Deploy key ID.
        key_id: u64,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Delete a deploy key by ID.
    Delete {
        /// Deploy key ID.
        key_id: u64,
        /// Skip confirmation prompt.
        #[arg(long, short)]
        yes: bool,
        #[command(flatten)]
        g: GlobalArgs,
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
        #[arg(long, help = "filter by reviewer display name")]
        reviewer: Option<String>,
        #[arg(
            long,
            help = "sort field (created_on|updated_on|title)",
            default_value = "updated_on"
        )]
        sort: String,
        #[arg(long, help = "sort direction (asc|desc)", default_value = "desc")]
        order: String,
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
        /// Show comments inline.
        #[arg(long)]
        comments: bool,
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
        #[arg(long, help = "create as draft")]
        draft: bool,
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
    /// List comments on a pull request.
    Comments {
        /// Pull request ID (defaults to current branch's open PR).
        id: Option<u64>,
        #[arg(long, help = "max results to return", default_value_t = 50)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// List tasks on a pull request.
    Tasks {
        /// Pull request ID (defaults to current branch's open PR).
        id: Option<u64>,
        #[arg(long, help = "max results to return", default_value_t = 50)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// List commits on a pull request.
    Commits {
        /// Pull request ID (defaults to current branch's open PR).
        id: Option<u64>,
        #[arg(long, help = "max results to return", default_value_t = 50)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// List commit statuses on a pull request.
    Statuses {
        /// Pull request ID (defaults to current branch's open PR).
        id: Option<u64>,
        #[arg(long, help = "max results to return", default_value_t = 50)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// List merge conflicts on a pull request.
    Conflicts {
        /// Pull request ID (defaults to current branch's open PR).
        id: Option<u64>,
        #[arg(long, help = "max results to return", default_value_t = 50)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Request changes on a pull request.
    RequestChanges {
        id: u64,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Clear your change request on a pull request.
    UnrequestChanges {
        id: u64,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Merge a pull request.
    Merge {
        id: u64,
        #[arg(long, help = "close source branch after merge")]
        close_source_branch: bool,
        #[arg(long, help = "merge strategy (merge_commit|squash|fast_forward)")]
        strategy: Option<String>,
        #[arg(long, help = "custom merge commit message")]
        message: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Approve a pull request.
    Approve {
        id: u64,
        /// Comment to post before approving.
        #[arg(long)]
        message: Option<String>,
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
        /// Output raw diff text (legacy behavior: pipe through bat/less).
        #[arg(long)]
        raw: bool,
        /// Use side-by-side view instead of unified.
        #[arg(long)]
        side_by_side: bool,
        /// Number of context lines around changes (default: 3).
        #[arg(long, default_value_t = 3)]
        context: usize,
        /// Disable syntax highlighting.
        #[arg(long)]
        no_syntax: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Show diffstat (file change summary) for a pull request.
    Diffstat {
        id: Option<u64>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Download the unified patch for a pull request.
    Patch {
        id: Option<u64>,
        #[arg(long, help = "write patch to file instead of stdout")]
        output: Option<String>,
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
    /// Cross-repo PR dashboard.
    Dashboard {
        /// Number of repos to scan (default: all).
        #[arg(long)]
        repos: Option<u32>,
        /// Only repos matching this pattern.
        #[arg(long)]
        filter: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Manage stacked PR chains.
    Stack {
        #[command(subcommand)]
        action: StackAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum StackAction {
    /// Start a new stack.
    Init {
        /// Stack name.
        name: String,
        /// Base branch (default: current branch).
        #[arg(long)]
        base: Option<String>,
    },
    /// Add a branch to the stack.
    Add {
        /// Branch to add.
        branch: String,
        /// Parent branch in the stack (default: previous branch or base).
        #[arg(long)]
        parent: Option<String>,
    },
    /// List the current stack status.
    List,
    /// Rebase all stacked branches onto their parents.
    Rebase {
        /// Push branches to origin after rebase.
        #[arg(long)]
        push: bool,
    },
    /// Merge all PRs in the stack bottom-up.
    Land {
        /// Merge strategy (merge_commit|squash|fast_forward).
        #[arg(long)]
        strategy: Option<String>,
        /// Skip confirmation prompt.
        #[arg(long, short)]
        yes: bool,
    },
    /// Close all stacked PRs and delete local/remote branches.
    Abort {
        /// Skip confirmation prompt.
        #[arg(long, short)]
        yes: bool,
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
        /// Skip fetching per-pipeline steps (faster listing).
        #[arg(long)]
        no_steps: bool,
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
        interval: u64,
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
    /// Show test reports for a pipeline step.
    Tests {
        /// Pipeline UUID (defaults to latest pipeline on current branch).
        uuid: Option<String>,
        #[arg(
            long,
            help = "step UUID or step name (default: first failed or latest)"
        )]
        step: Option<String>,
        #[arg(long, help = "max test cases to show", default_value_t = 50)]
        limit: u32,
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
    /// Compare two pipeline runs.
    Compare {
        /// First pipeline reference (UUID, build number, or "last").
        a: String,
        /// Second pipeline reference (UUID, build number, or "last").
        b: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Trigger a new pipeline for a branch.
    Trigger {
        #[arg(long, help = "branch name (default: current branch)")]
        branch: Option<String>,
        /// Set a pipeline variable (repeatable). Format: KEY=VALUE.
        #[arg(
            long = "var",
            help = "set a pipeline variable (repeatable, format: KEY=VALUE)"
        )]
        vars: Vec<String>,
        /// Mark a variable as secured/encrypted (repeatable).
        #[arg(
            long = "secured",
            help = "mark variable as secured/encrypted (repeatable)"
        )]
        secured: Vec<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Manage pipeline-level repository variables.
    Vars {
        #[command(subcommand)]
        action: CiVarsAction,
    },
    /// Manage pipeline schedules.
    Schedules {
        #[command(subcommand)]
        action: CiSchedulesAction,
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
    /// List remote tags.
    Tags {
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
    /// Create a new repository.
    Create {
        /// Repository slug (name).
        slug: String,
        /// Set the repository to private.
        #[arg(long)]
        private: bool,
        /// Repository description.
        #[arg(long)]
        description: Option<String>,
        /// Primary language.
        #[arg(long)]
        language: Option<String>,
        /// Enable the issue tracker.
        #[arg(long)]
        enable_issues: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Audit repository compliance settings.
    Audit {
        /// Specific repo slug to audit (default: all repos in workspace).
        slug: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Delete a repository (permanent).
    Delete {
        /// Repository slug to delete.
        slug: String,
        /// Skip confirmation prompt.
        #[arg(long, short)]
        yes: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Fork a repository.
    Fork {
        /// Repository to fork (default: current repo from git remote).
        slug: Option<String>,
        /// Fork name (default: original slug).
        #[arg(long)]
        name: Option<String>,
        /// Target workspace for the fork.
        #[arg(long = "target-workspace")]
        target_workspace: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Create a remote branch.
    CreateBranch {
        /// Branch name to create.
        name: String,
        /// Commit hash or branch to create from (default: current HEAD).
        #[arg(long)]
        from: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Create a remote tag.
    CreateTag {
        /// Tag name to create.
        name: String,
        /// Commit hash to tag (default: current HEAD).
        #[arg(long)]
        target: Option<String>,
        /// Tag message (for annotated tags).
        #[arg(long)]
        message: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// List user and group permissions for the repository.
    Permissions {
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum BatchAction {
    /// Merge all approved pull requests.
    MergeApproved {
        /// Limit to this repository slug.
        #[arg(long)]
        repo: Option<String>,
        /// Only show what would be done, don't execute.
        #[arg(long)]
        dry_run: bool,
        /// Merge strategy (merge_commit|squash|fast_forward).
        #[arg(long)]
        strategy: Option<String>,
        /// Skip confirmation.
        #[arg(long, short)]
        yes: bool,
        /// Maximum number of items to process (safety cap).
        #[arg(long)]
        max: Option<usize>,
    },
    /// Rerun all failed pipelines.
    RerunFailed {
        /// Branch filter (default: all branches).
        #[arg(long)]
        branch: Option<String>,
        /// Limit to this repository slug.
        #[arg(long)]
        repo: Option<String>,
        /// Only show what would be done, don't execute.
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation.
        #[arg(long, short)]
        yes: bool,
        /// Maximum number of items to process (safety cap).
        #[arg(long)]
        max: Option<usize>,
    },
    /// Delete branches that have been merged to their target.
    CleanupMergedBranches {
        /// Limit to this repository slug.
        #[arg(long)]
        repo: Option<String>,
        /// Delete remote branches too.
        #[arg(long)]
        remote: bool,
        /// Only show what would be done, don't execute.
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation.
        #[arg(long, short)]
        yes: bool,
        /// Maximum number of items to process (safety cap).
        #[arg(long)]
        max: Option<usize>,
    },
}

#[derive(Debug, Subcommand)]
pub enum CommitAction {
    /// Commit build-status operations.
    Status {
        #[command(subcommand)]
        action: CommitStatusAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum CommitStatusAction {
    /// Create or update a build status on a commit.
    Set {
        /// Commit hash (defaults to current HEAD).
        commit: Option<String>,
        #[arg(long, help = "status key, unique per integration")]
        key: String,
        #[arg(long, help = "state: successful|failed|inprogress|stopped")]
        state: String,
        #[arg(long, help = "display name")]
        name: Option<String>,
        #[arg(long, help = "target URL")]
        url: Option<String>,
        #[arg(long, help = "short status description")]
        description: Option<String>,
        #[arg(long, help = "branch/ref name to associate with pull requests")]
        refname: Option<String>,
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
    Setup {
        /// Username (email) for non-interactive setup.
        #[arg(long)]
        username: Option<String>,
        /// API token for non-interactive setup.
        #[arg(long)]
        token: Option<String>,
    },
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

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Print the config file path.
    Path {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Print the current config as JSON.
    Show {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Set a config value (key value).
    Set {
        /// Config key (e.g. workspace).
        key: String,
        /// Config value.
        value: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum WebhookAction {
    /// List webhooks for the current repository.
    List {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// View a specific webhook.
    View {
        /// Webhook UUID.
        uid: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Create a new webhook.
    Create {
        /// Target URL for the webhook.
        #[arg(long)]
        url: String,
        /// Comma-separated list of events (e.g. repo:push,pullrequest:created).
        #[arg(long)]
        events: String,
        /// Human-readable description.
        #[arg(long)]
        description: Option<String>,
        /// Activate the webhook immediately (default: true).
        #[arg(long, default_value_t = true)]
        active: bool,
        /// Shared secret for payload signing.
        #[arg(long)]
        secret: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Update an existing webhook.
    Update {
        /// Webhook UUID.
        uid: String,
        /// New target URL.
        #[arg(long)]
        url: Option<String>,
        /// New comma-separated event list.
        #[arg(long)]
        events: Option<String>,
        /// New description.
        #[arg(long)]
        description: Option<String>,
        /// Enable or disable the webhook.
        #[arg(long)]
        active: Option<bool>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Delete a webhook.
    Delete {
        /// Webhook UUID.
        uid: String,
        /// Skip confirmation prompt.
        #[arg(long, short)]
        yes: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum SrcAction {
    /// Print raw file content from the remote repository.
    Cat {
        /// File path within the repository.
        path: String,
        /// Git ref (branch, tag, or commit hash; defaults to current branch).
        #[arg(long, short = 'r')]
        git_ref: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// List directory contents in the remote repository.
    Ls {
        /// Directory path (default: repo root).
        path: Option<String>,
        /// Git ref (branch, tag, or commit hash; defaults to current branch).
        #[arg(long, short = 'r')]
        git_ref: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum DeployAction {
    /// List deployments in the current repository.
    List {
        /// Limit the number of results.
        #[arg(long, default_value_t = 25)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Trigger a deployment.
    Trigger {
        /// The environment UUID.
        env_uuid: String,
        /// The commit hash to deploy.
        #[arg(long)]
        commit: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Manage environments.
    Env {
        #[command(subcommand)]
        action: DeployEnvAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum DeployEnvAction {
    /// List environments.
    List {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Create a new environment.
    Create {
        /// Environment name.
        name: String,
        /// Environment type (test|staging|production).
        #[arg(long, default_value = "test")]
        env_type: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Manage environment variables.
    Vars {
        #[command(subcommand)]
        action: DeployEnvVarsAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum DeployEnvVarsAction {
    /// List variables for an environment.
    List {
        /// The environment UUID.
        env_uuid: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Set an environment variable (creates or updates).
    Set {
        /// The environment UUID.
        env_uuid: String,
        key: String,
        value: String,
        /// Mark variable as secured/encrypted.
        #[arg(long)]
        secured: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Delete an environment variable.
    Delete {
        /// The environment UUID.
        env_uuid: String,
        key: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum CiVarsAction {
    /// List pipeline variables.
    List {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Set a pipeline variable (creates or updates).
    Set {
        key: String,
        value: String,
        /// Mark variable as secured/encrypted.
        #[arg(long)]
        secured: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Delete a pipeline variable.
    Delete {
        key: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum CiSchedulesAction {
    /// List pipeline schedules.
    List {
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Create a pipeline schedule.
    Create {
        /// Cron expression (e.g. "0 2 * * *").
        #[arg(long)]
        cron: String,
        /// Branch to run the schedule on.
        #[arg(long)]
        branch: String,
        /// Pipeline selector name (optional).
        #[arg(long)]
        pipeline: Option<String>,
        /// Whether the schedule is enabled (default: true).
        #[arg(long, default_value_t = true)]
        enabled: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// View a pipeline schedule.
    View {
        /// Schedule UUID.
        uuid: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Update a pipeline schedule.
    Update {
        /// Schedule UUID.
        uuid: String,
        /// New cron expression.
        #[arg(long)]
        cron: Option<String>,
        /// Enable or disable the schedule.
        #[arg(long)]
        enabled: Option<bool>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Delete a pipeline schedule.
    Delete {
        /// Schedule UUID.
        uuid: String,
        /// Skip confirmation prompt.
        #[arg(long, short)]
        yes: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// List executions for a pipeline schedule.
    Executions {
        /// Schedule UUID.
        uuid: String,
        /// Max results to return.
        #[arg(long, default_value_t = 25)]
        limit: u32,
        #[command(flatten)]
        g: GlobalArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum IssueAction {
    /// List issues in the repository.
    List {
        /// Limit the number of results.
        #[arg(long, default_value_t = 25)]
        limit: u32,
        /// Filter by state (new|open|resolved|on hold|invalid|duplicate|wontfix|closed).
        #[arg(long)]
        status: Option<String>,
        /// Filter by kind (bug|enhancement|proposal|task).
        #[arg(long)]
        kind: Option<String>,
        /// Filter by priority (trivial|minor|major|critical|blocker).
        #[arg(long)]
        priority: Option<String>,
        /// Filter by assignee nickname.
        #[arg(long)]
        assignee: Option<String>,
        /// Custom BBQL query.
        #[arg(long)]
        query: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// View a specific issue.
    View {
        /// Issue ID.
        id: u64,
        /// Show comments inline.
        #[arg(long)]
        comments: bool,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Create a new issue.
    Create {
        /// Issue title.
        #[arg(long)]
        title: String,
        /// Issue description body.
        #[arg(long)]
        body: String,
        /// Issue kind (bug|enhancement|proposal|task).
        #[arg(long, default_value = "bug")]
        kind: String,
        /// Issue priority (trivial|minor|major|critical|blocker).
        #[arg(long, default_value = "major")]
        priority: String,
        /// Assignee nickname.
        #[arg(long)]
        assignee: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Update an existing issue.
    Update {
        /// Issue ID.
        id: u64,
        /// New title.
        #[arg(long)]
        title: Option<String>,
        /// New description body.
        #[arg(long)]
        body: Option<String>,
        /// New state.
        #[arg(long)]
        status: Option<String>,
        /// New kind.
        #[arg(long)]
        kind: Option<String>,
        /// New priority.
        #[arg(long)]
        priority: Option<String>,
        /// New assignee nickname.
        #[arg(long)]
        assignee: Option<String>,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// Post a comment on an issue.
    Comment {
        /// Issue ID.
        id: u64,
        /// Comment body.
        #[arg(long)]
        body: String,
        #[command(flatten)]
        g: GlobalArgs,
    },
    /// List comments on an issue.
    Comments {
        /// Issue ID.
        id: u64,
        /// Limit the number of results.
        #[arg(long, default_value_t = 25)]
        limit: u32,
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
            e.exit();
        }
    };

    init_tracing(cli.global.verbose);

    // Set color override before any Theme access.
    // --no-color takes precedence over --color.
    if cli.global.no_color || cli.global.color == ColorChoice::Never {
        crate::output::theme::Theme::set_color_override(false);
    } else if cli.global.color == ColorChoice::Always {
        crate::output::theme::Theme::set_color_override(true);
    }
    // ColorChoice::Auto: let Theme decide based on NO_COLOR + TTY detection.

    // Set unicode override before any Theme access
    if cli.global.no_unicode {
        crate::output::theme::Theme::set_unicode_override(false);
    }

    let result: Result<()> = crate::dispatch::dispatch(cli).await;

    match result {
        Ok(()) => AppExitCode::Success.as_process(),
        Err(e) => report(&e),
    }
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(io::stderr().is_terminal())
        .init();
}
