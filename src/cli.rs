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

    /// Force ANSI color output.
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub color: bool,

    /// Disable ANSI color output.
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
    Path,
    /// Print the current config as JSON.
    Show,
    /// Set a config value (key value).
    Set {
        /// Config key (e.g. workspace).
        key: String,
        /// Config value.
        value: String,
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

    // Set color override before any Theme access
    if cli.global.no_color {
        crate::output::theme::Theme::set_color_override(false);
    } else if cli.global.color {
        crate::output::theme::Theme::set_color_override(true);
    }

    // Set unicode override before any Theme access
    if cli.global.no_unicode {
        crate::output::theme::Theme::set_unicode_override(false);
    }

    let result: Result<()> = dispatch(cli).await;

    match result {
        Ok(()) => AppExitCode::Success.as_process(),
        Err(e) => report(&e),
    }
}

async fn dispatch(cli: Cli) -> Result<()> {
    let g = &cli.global;
    match cli.command {
        None => {
            let result = commands::status::run_overview(g).await;
            commands::update::notify_if_outdated().await;
            result
        }
        Some(Command::Status {
            g,
            watch,
            interval,
            short,
            export,
        }) => dispatch_status(g, watch, interval, short, export).await,
        Some(Command::Pr { action }) => dispatch_pr(&cli.global, action).await,
        Some(Command::Ci { action }) => dispatch_ci(action).await,
        Some(Command::Repo { action }) => dispatch_repo(action).await,
        Some(Command::Commit { action }) => match action {
            CommitAction::Status { action } => match action {
                CommitStatusAction::Set {
                    commit,
                    key,
                    state,
                    name,
                    url,
                    description,
                    refname,
                    g,
                } => {
                    commands::commit::set_status(
                        &g,
                        commit.as_deref(),
                        &key,
                        &state,
                        name.as_deref(),
                        url.as_deref(),
                        description.as_deref(),
                        refname.as_deref(),
                    )
                    .await
                }
            },
        },
        Some(Command::Batch { action }) => dispatch_batch(&cli.global, action).await,
        Some(Command::Open { action, g }) => commands::open::run(&g, action).await,
        Some(Command::Auth { action }) => dispatch_auth(action).await,
        Some(Command::Completion { shell, install }) => {
            if install {
                commands::completion::install(shell)?;
            } else {
                let shell = shell.unwrap_or(Shell::Bash);
                generate(shell, &mut Cli::command(), "bbr", &mut io::stdout());
            }
            Ok(())
        }
        Some(Command::Config { action }) => commands::config::run(action),
        Some(Command::Api {
            method,
            path,
            data,
            paginate,
            g,
        }) => commands::api::run(&g, &method, &path, data.as_deref(), paginate).await,
        Some(Command::Webhook { action }) => dispatch_webhook(action).await,
        Some(Command::Src { action }) => match action {
            SrcAction::Cat { path, git_ref, g } => {
                commands::src_cmd::cat(&g, &path, git_ref.as_deref()).await
            }
            SrcAction::Ls { path, git_ref, g } => {
                commands::src_cmd::ls(&g, path.as_deref(), git_ref.as_deref()).await
            }
        },
        Some(Command::Deploy { action }) => dispatch_deploy(action).await,
        Some(Command::Issue { action }) => dispatch_issue(action).await,
        Some(Command::Search {
            query,
            repo,
            limit,
            g,
        }) => commands::search::run(&g, &query, repo.as_deref(), limit).await,
        Some(Command::Schema { model, g }) => commands::schema::run(&g, model.as_deref()),
        Some(Command::Update { check, g }) => commands::update::run(&g, check).await,
        Some(Command::Workspace { action }) => dispatch_workspace(action).await,
    }
}

async fn dispatch_workspace(action: WorkspaceAction) -> Result<()> {
    match action {
        WorkspaceAction::List { role, limit, g } => {
            commands::workspace::list(&g, role.as_deref(), limit).await
        }
    }
}

async fn dispatch_status(
    g: GlobalArgs,
    watch: bool,
    interval: u64,
    short: bool,
    export: Option<String>,
) -> Result<()> {
    if let Some(fmt) = export {
        let status_res = commands::status::run_inner(&g).await?;
        let text = match fmt.as_str() {
            "slack" => commands::export::format_slack(&status_res),
            "markdown" => commands::export::format_markdown(&status_res),
            _ => unreachable!(),
        };
        println!("{}", text);
        Ok(())
    } else if watch {
        commands::status::run_watch(&g, interval).await
    } else if short {
        commands::status::run_short(&g).await
    } else {
        commands::status::run(&g).await
    }
}

async fn dispatch_pr(g: &GlobalArgs, action: PrAction) -> Result<()> {
    match action {
        PrAction::List {
            state,
            limit,
            author,
            source_branch,
            reviewer,
            sort,
            order,
            g,
        } => {
            commands::pr::list(
                &g,
                &state,
                limit,
                author.as_deref(),
                source_branch.as_deref(),
                reviewer.as_deref(),
                &sort,
                &order,
            )
            .await
        }
        PrAction::View {
            id,
            diff,
            comments,
            g,
        } => commands::pr::view(&g, id, diff, comments).await,
        PrAction::Create {
            title,
            body,
            body_file,
            body_stdin,
            src,
            dst,
            close_source_branch,
            draft,
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
                draft,
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
        PrAction::Comments { id, limit, g } => commands::pr::comments(&g, id, limit).await,
        PrAction::Tasks { id, limit, g } => commands::pr::tasks(&g, id, limit).await,
        PrAction::Commits { id, limit, g } => commands::pr::commits(&g, id, limit).await,
        PrAction::Statuses { id, limit, g } => commands::pr::statuses(&g, id, limit).await,
        PrAction::Conflicts { id, limit, g } => commands::pr::conflicts(&g, id, limit).await,
        PrAction::RequestChanges { id, g } => commands::pr::request_changes(&g, id).await,
        PrAction::UnrequestChanges { id, g } => commands::pr::unrequest_changes(&g, id).await,
        PrAction::Merge {
            id,
            close_source_branch,
            strategy,
            message,
            g,
        } => {
            commands::pr::merge(
                &g,
                id,
                close_source_branch,
                strategy.as_deref(),
                message.as_deref(),
            )
            .await
        }
        PrAction::Approve { id, message, g } => {
            commands::pr::approve(&g, id, message.as_deref()).await
        }
        PrAction::Unapprove { id, g } => commands::pr::unapprove(&g, id).await,
        PrAction::Decline { id, g } => commands::pr::decline(&g, id).await,
        PrAction::Checkout { id, g } => commands::pr::checkout(&g, id).await,
        PrAction::Diff { id, g } => commands::pr::diff(&g, id).await,
        PrAction::Diffstat { id, g } => commands::pr::diffstat(&g, id).await,
        PrAction::Patch { id, output, g } => commands::pr::patch(&g, id, output.as_deref()).await,
        PrAction::Update {
            id,
            title,
            description,
            g,
        } => commands::pr::update(&g, id, title.as_deref(), description.as_deref()).await,
        PrAction::Dashboard { repos, filter, g } => {
            commands::dashboard::run_dashboard(&g, repos, filter.as_deref()).await
        }
        PrAction::Stack { action } => match action {
            StackAction::Init { name, base } => commands::stack::init(g, &name, base.as_deref()),
            StackAction::Add { branch, parent } => {
                commands::stack::add(g, &branch, parent.as_deref()).await
            }
            StackAction::List => commands::stack::list(g).await,
            StackAction::Rebase { push } => commands::stack::rebase(g, push),
            StackAction::Land { strategy, yes } => {
                commands::stack::land(g, strategy.as_deref(), yes).await
            }
            StackAction::Abort { yes } => commands::stack::abort(g, yes).await,
        },
    }
}

async fn dispatch_ci(action: CiAction) -> Result<()> {
    match action {
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
        CiAction::Trigger {
            branch,
            vars,
            secured,
            g,
        } => commands::ci::trigger(&g, branch.as_deref(), &vars, &secured).await,
        CiAction::Stop { uuid, branch, g } => {
            commands::ci::stop(&g, uuid.as_deref(), branch.as_deref()).await
        }
        CiAction::Steps { uuid, g } => commands::ci::steps(&g, uuid.as_deref()).await,
        CiAction::Tests {
            uuid,
            step,
            limit,
            g,
        } => commands::ci::tests(&g, uuid.as_deref(), step.as_deref(), limit).await,
        CiAction::Compare { a, b, g } => commands::ci_compare::compare(&g, &a, &b).await,
        CiAction::Vars { action } => match action {
            CiVarsAction::List { g } => commands::ci_vars::list(&g).await,
            CiVarsAction::Set {
                key,
                value,
                secured,
                g,
            } => commands::ci_vars::set(&g, &key, &value, secured).await,
            CiVarsAction::Delete { key, g } => commands::ci_vars::delete(&g, &key).await,
        },
    }
}

async fn dispatch_repo(action: RepoAction) -> Result<()> {
    match action {
        RepoAction::Info { g } => commands::repo::info(&g).await,
        RepoAction::Branches { limit, g } => commands::repo::list_branches(&g, limit).await,
        RepoAction::Tags { limit, g } => commands::repo::list_tags(&g, limit).await,
        RepoAction::Commits { branch, limit, g } => {
            commands::repo::list_commits(&g, branch.as_deref(), limit).await
        }
        RepoAction::Create {
            slug,
            private,
            description,
            language,
            enable_issues,
            g,
        } => {
            commands::repo::create(
                &g,
                &slug,
                private,
                description.as_deref(),
                language.as_deref(),
                enable_issues,
            )
            .await
        }
        RepoAction::Audit { slug, g } => commands::audit::run_audit(&g, slug.as_deref()).await,
        RepoAction::Delete { slug, yes, g } => commands::repo::delete(&g, &slug, yes).await,
        RepoAction::Fork {
            slug,
            name,
            target_workspace,
            g,
        } => {
            commands::repo::fork(
                &g,
                slug.as_deref(),
                name.as_deref(),
                target_workspace.as_deref(),
            )
            .await
        }
        RepoAction::CreateBranch { name, from, g } => {
            commands::repo::create_branch(&g, &name, from.as_deref()).await
        }
        RepoAction::CreateTag {
            name,
            target,
            message,
            g,
        } => commands::repo::create_tag(&g, &name, target.as_deref(), message.as_deref()).await,
        RepoAction::Permissions { g } => commands::repo::permissions(&g).await,
    }
}

async fn dispatch_batch(g: &GlobalArgs, action: BatchAction) -> Result<()> {
    match action {
        BatchAction::MergeApproved {
            repo,
            dry_run,
            strategy,
            yes,
        } => {
            commands::batch::merge_approved(g, repo.as_deref(), dry_run, strategy.as_deref(), yes)
                .await
        }
        BatchAction::RerunFailed {
            branch,
            repo,
            dry_run,
            yes,
        } => {
            commands::batch::rerun_failed(g, branch.as_deref(), repo.as_deref(), dry_run, yes).await
        }
        BatchAction::CleanupMergedBranches {
            repo,
            remote,
            dry_run,
            yes,
        } => {
            commands::batch::cleanup_merged_branches(g, repo.as_deref(), remote, dry_run, yes).await
        }
    }
}

async fn dispatch_auth(action: AuthAction) -> Result<()> {
    match action {
        AuthAction::Setup { username, token } => commands::auth::setup(username, token),
        AuthAction::Status { g } => commands::auth::status(&g).await,
        AuthAction::Logout { g } => commands::auth::logout(&g),
        AuthAction::Test { g } => commands::auth::test(&g).await,
    }
}

async fn dispatch_webhook(action: WebhookAction) -> Result<()> {
    match action {
        WebhookAction::List { g } => commands::webhook::list(&g).await,
        WebhookAction::View { uid, g } => commands::webhook::view(&g, &uid).await,
        WebhookAction::Create {
            url,
            events,
            description,
            active,
            secret,
            g,
        } => {
            commands::webhook::create(
                &g,
                &url,
                &events,
                description.as_deref(),
                active,
                secret.as_deref(),
            )
            .await
        }
        WebhookAction::Update {
            uid,
            url,
            events,
            description,
            active,
            g,
        } => {
            commands::webhook::update(
                &g,
                &uid,
                url.as_deref(),
                events.as_deref(),
                description.as_deref(),
                active,
            )
            .await
        }
        WebhookAction::Delete { uid, yes, g } => commands::webhook::delete(&g, &uid, yes).await,
    }
}

async fn dispatch_deploy(action: DeployAction) -> Result<()> {
    match action {
        DeployAction::List { limit, g } => commands::deploy::list_deployments(&g, limit).await,
        DeployAction::Trigger {
            env_uuid,
            commit,
            g,
        } => commands::deploy::trigger_deployment(&g, &env_uuid, &commit).await,
        DeployAction::Env { action } => match action {
            DeployEnvAction::List { g } => commands::deploy::list_environments(&g).await,
            DeployEnvAction::Create { name, env_type, g } => {
                commands::deploy::create_environment(&g, &name, &env_type).await
            }
            DeployEnvAction::Vars { action } => match action {
                DeployEnvVarsAction::List { env_uuid, g } => {
                    commands::deploy::list_env_vars(&g, &env_uuid).await
                }
                DeployEnvVarsAction::Set {
                    env_uuid,
                    key,
                    value,
                    secured,
                    g,
                } => commands::deploy::set_env_var(&g, &env_uuid, &key, &value, secured).await,
                DeployEnvVarsAction::Delete { env_uuid, key, g } => {
                    commands::deploy::delete_env_var(&g, &env_uuid, &key).await
                }
            },
        },
    }
}

async fn dispatch_issue(action: IssueAction) -> Result<()> {
    match action {
        IssueAction::List {
            limit,
            status,
            kind,
            priority,
            assignee,
            query,
            g,
        } => {
            commands::issue::list(
                &g,
                limit,
                status.as_deref(),
                kind.as_deref(),
                priority.as_deref(),
                assignee.as_deref(),
                query.as_deref(),
            )
            .await
        }
        IssueAction::View { id, comments, g } => commands::issue::view(&g, id, comments).await,
        IssueAction::Create {
            title,
            body,
            kind,
            priority,
            assignee,
            g,
        } => {
            commands::issue::create(&g, &title, &body, &kind, &priority, assignee.as_deref()).await
        }
        IssueAction::Update {
            id,
            title,
            body,
            status,
            kind,
            priority,
            assignee,
            g,
        } => {
            commands::issue::update(
                &g,
                id,
                title.as_deref(),
                body.as_deref(),
                status.as_deref(),
                kind.as_deref(),
                priority.as_deref(),
                assignee.as_deref(),
            )
            .await
        }
        IssueAction::Comment { id, body, g } => commands::issue::comment(&g, id, &body).await,
        IssueAction::Comments { id, limit, g } => {
            commands::issue::list_comments(&g, id, limit).await
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
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(io::stderr().is_terminal())
        .init();
}

pub use crate::output::Formatter;
