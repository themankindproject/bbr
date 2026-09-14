//! Stacked PRs CLI command (`bbr pr stack`).

use crate::api::pr::{CreateBranchRef, CreateNamed, CreatePrRequest, MergePrRequest};
use crate::cli::GlobalArgs;
use crate::commands::{
    aborted, client, confirm_destructive, current_head, make_formatter, make_spinner, resolve_repo,
    SpinnerGuard,
};
use crate::error::{BitbucketError, Result};
use crate::output::theme::Theme;
use crate::stack::{StackConfig, StackDef, StackPr};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct StackInitOut {
    pub name: String,
    pub base_branch: String,
}

#[derive(Debug, Serialize)]
pub struct StackAddOut {
    pub branch: String,
    pub pr_id: u64,
    pub url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StackListOut {
    pub name: String,
    pub base_branch: String,
    /// All configured stack names (active marked in human output).
    pub stacks: Vec<String>,
    pub prs: Vec<StackPrStatus>,
}

#[derive(Debug, Serialize, Clone)]
pub struct StackPrStatus {
    pub branch: String,
    pub pr_id: Option<u64>,
    pub state: Option<String>,
    pub parent_branch: String,
    pub url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StackRebaseOut {
    pub steps: Vec<StackRebaseStep>,
}

#[derive(Debug, Serialize, Clone)]
pub struct StackRebaseStep {
    pub branch: String,
    pub status: String, // "ok" | "conflict"
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct StackLandOut {
    pub merged: Vec<u64>,
    pub failed: Vec<StackLandFailure>,
}

#[derive(Debug, Serialize, Clone)]
pub struct StackLandFailure {
    pub pr_id: u64,
    pub branch: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct StackAbortOut {
    pub declined: Vec<u64>,
    pub branches_deleted: Vec<String>,
}

pub fn init(g: &GlobalArgs, name: &str, base: Option<&str>) -> Result<()> {
    let mut config = StackConfig::load()?;

    // Check if stack already exists
    if config.find_stack(name).is_some() {
        return Err(BitbucketError::Other(format!(
            "Stack '{}' already exists.",
            name
        )));
    }

    let base_branch = match base {
        Some(b) => b.to_string(),
        None => {
            let head = current_head()?;
            head.branch
        }
    };

    config.stacks.push(StackDef {
        name: name.to_string(),
        base_branch: base_branch.clone(),
        prs: Vec::new(),
    });
    config.active = Some(name.to_string());

    config.save()?;

    let out = StackInitOut {
        name: name.to_string(),
        base_branch,
    };

    let human = format!(
        "Initialized empty stack '{}' onto base branch '{}'.",
        out.name, out.base_branch
    );
    make_formatter(g).print(&out, &human)
}

pub fn use_stack(g: &GlobalArgs, name: &str) -> Result<()> {
    let mut config = StackConfig::load()?;
    config.set_active(name)?;
    config.save()?;

    #[derive(Serialize)]
    struct Out {
        active: String,
    }
    let out = Out {
        active: name.to_string(),
    };
    let human = format!("Active stack set to '{name}'.");
    make_formatter(g).print(&out, &human)
}

pub async fn add(g: &GlobalArgs, branch: &str, parent: Option<&str>) -> Result<()> {
    let mut config = StackConfig::load()?;
    let stack = config.active_stack_mut()?;
    let name = stack.name.clone();

    // Determine parent branch
    let parent_branch = match parent {
        Some(p) => p.to_string(),
        None => {
            if let Some(last) = stack.prs.last() {
                last.branch.clone()
            } else {
                stack.base_branch.clone()
            }
        }
    };

    let client = client(g)?;
    let repo = resolve_repo(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message(format!("Pushing branch {} to remote...", branch));

    // Push branch to remote
    crate::git::push_branch_async(branch).await?;

    spinner.set_message(format!(
        "Creating pull request: {} -> {}...",
        branch, parent_branch
    ));
    let pr_req = CreatePrRequest {
        title: format!("Stacked PR: {}", branch),
        description: Some(format!(
            "Dependant stacked PR in chain. Targets parent branch: `{}`.",
            parent_branch
        )),
        source: CreateBranchRef {
            branch: CreateNamed {
                name: branch.to_string(),
            },
        },
        destination: CreateBranchRef {
            branch: CreateNamed {
                name: parent_branch.clone(),
            },
        },
        close_source_branch: Some(true),
        reviewers: Vec::new(),
        draft: None,
    };

    let pr = client
        .create_pr(&repo.workspace, &repo.slug, &pr_req)
        .await?;
    spinner.finish();

    stack.prs.push(StackPr {
        branch: branch.to_string(),
        pr_id: Some(pr.id),
        parent_branch: parent_branch.clone(),
    });

    config.save()?;

    let out = StackAddOut {
        branch: branch.to_string(),
        pr_id: pr.id,
        url: pr.web_url().map(|u| u.to_string()),
    };

    let human = format!(
        "Added branch '{}' to stack '{}' (PR #{} created targeting '{}').\nURL: {}",
        out.branch,
        name,
        out.pr_id,
        parent_branch,
        out.url.as_deref().unwrap_or("-")
    );
    make_formatter(g).print(&out, &human)
}

pub async fn list(g: &GlobalArgs) -> Result<()> {
    let config = StackConfig::load()?;
    let stack = config.active_stack()?;

    let client = client(g)?;
    let repo = resolve_repo(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching pull request statuses...");

    let futures = stack.prs.iter().map(|pr| {
        let client = client.clone();
        let ws = repo.workspace.clone();
        let slug = repo.slug.clone();
        async move {
            let (state, url) = if let Some(id) = pr.pr_id {
                match client.get_pr(&ws, &slug, id).await {
                    Ok(full_pr) => {
                        let url = full_pr.web_url().map(|u| u.to_string());
                        (Some(full_pr.state), url)
                    }
                    Err(_) => (Some("UNKNOWN".to_string()), None),
                }
            } else {
                (None, None)
            };
            StackPrStatus {
                branch: pr.branch.clone(),
                pr_id: pr.pr_id,
                state,
                parent_branch: pr.parent_branch.clone(),
                url,
            }
        }
    });

    use futures::StreamExt;
    let prs_status = futures::stream::iter(futures)
        .buffered(5)
        .collect::<Vec<StackPrStatus>>()
        .await;

    spinner.finish();

    let out = StackListOut {
        name: stack.name.clone(),
        base_branch: stack.base_branch.clone(),
        stacks: config.stacks.iter().map(|s| s.name.clone()).collect(),
        prs: prs_status,
    };

    let human = render_stack_list(&out);
    make_formatter(g).print(&out, &human)
}

pub async fn rebase(g: &GlobalArgs, push: bool) -> Result<()> {
    if !crate::git::is_working_tree_clean()? {
        return Err(BitbucketError::Other(
            "Working directory is dirty. Please commit or stash changes before rebasing.".into(),
        ));
    }

    let config = StackConfig::load()?;
    let stack = config.active_stack()?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    let mut steps = Vec::new();
    let mut failure = None;

    for pr in &stack.prs {
        spinner.set_message(format!(
            "Rebasing {} onto {}...",
            pr.branch, pr.parent_branch
        ));
        match crate::git::rebase_branch_async(&pr.branch, &pr.parent_branch).await {
            Ok(_) => {
                let mut push_msg = String::new();
                if push {
                    spinner.set_message(format!(
                        "Pushing branch {} with force-with-lease...",
                        pr.branch
                    ));
                    match crate::git::push_force_with_lease_async(&pr.branch).await {
                        Ok(_) => push_msg = " and pushed".to_string(),
                        Err(e) => {
                            steps.push(StackRebaseStep {
                                branch: pr.branch.clone(),
                                status: "error".to_string(),
                                message: format!("Rebase succeeded but force-push failed: {}", e),
                            });
                            failure = Some(e);
                            break;
                        }
                    }
                }
                steps.push(StackRebaseStep {
                    branch: pr.branch.clone(),
                    status: "ok".to_string(),
                    message: format!("Successfully rebased{}", push_msg),
                });
            }
            Err(e) => {
                steps.push(StackRebaseStep {
                    branch: pr.branch.clone(),
                    status: "conflict".to_string(),
                    message: format!("Rebase failed (conflicts?): {}", e),
                });
                failure = Some(e);
                break; // Stop rebase chain on conflict
            }
        }
    }

    spinner.finish();

    let out = StackRebaseOut { steps };
    let human = render_rebase(&out);
    make_formatter(g).print(&out, &human)?;
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub async fn land(g: &GlobalArgs, strategy: Option<&str>, yes: bool) -> Result<()> {
    if !crate::git::is_working_tree_clean()? {
        return Err(BitbucketError::Other(
            "Working directory is dirty. Please commit or stash changes before landing.".into(),
        ));
    }

    let mut config = StackConfig::load()?;
    let stack = config.active_stack()?.clone();

    if stack.prs.is_empty() {
        return Err(BitbucketError::Other(
            "Empty stack. Nothing to land.".into(),
        ));
    }

    let mut ids = std::collections::HashSet::new();
    let mut entries = Vec::with_capacity(stack.prs.len());
    for pr in &stack.prs {
        let id = pr.pr_id.filter(|id| *id > 0).ok_or_else(|| {
            BitbucketError::Other(format!(
                "Stack branch {:?} has no valid PR ID; repair .bbr/stack.toml before landing",
                pr.branch
            ))
        })?;
        if !ids.insert(id) {
            return Err(BitbucketError::Other(format!(
                "Stack contains duplicate PR #{id}; repair .bbr/stack.toml before landing"
            )));
        }
        entries.push((pr, id));
    }
    let client = client(g)?;
    let repo = resolve_repo(g)?;

    if !confirm_destructive(
        g,
        yes,
        &format!(
            "Merge and land {} stacked pull requests bottom-up? (y/n): ",
            stack.prs.len()
        ),
    )
    .await?
    {
        return aborted();
    }

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    let mut merged = Vec::new();
    let mut failed = Vec::new();
    let mut failure = None;

    for (index, (pr, id)) in entries.into_iter().enumerate() {
        let result: Result<()> = async {
            ensure_stack_state_unchanged(&config)?;
            spinner.set_message(format!("Checking PR #{id} (branch {})...", pr.branch));
            let current = client.get_pr(&repo.workspace, &repo.slug, id).await?;
            if current.id != id {
                return Err(BitbucketError::Other(format!("API returned a different PR for #{id}; stopping landing")));
            }
            if !current.state.eq_ignore_ascii_case("MERGED") {
                if !current.state.eq_ignore_ascii_case("OPEN") {
                    return Err(BitbucketError::Other(format!(
                        "PR #{id} is {}, not OPEN or MERGED; inspect it before retrying", current.state
                    )));
                }
                ensure_stack_state_unchanged(&config)?;
                spinner.set_message(format!("Merging PR #{id} (branch {})...", pr.branch));
                let merge_req = MergePrRequest {
                    close_source_branch: Some(true),
                    merge_strategy: strategy.map(str::to_string),
                    message: None,
                };
                let response = client.merge_pr(&repo.workspace, &repo.slug, id, Some(&merge_req)).await?;
                if response.id != id || !response.state.eq_ignore_ascii_case("MERGED") {
                    return Err(BitbucketError::Other(format!(
                        "PR #{id} merge was not confirmed as MERGED; inspect Bitbucket before retrying"
                    )));
                }
            }
            merged.push(id);
            let checkpoint = (|| -> Result<()> {
                ensure_stack_state_unchanged(&config)?;
                let mut next = config.clone();
                // Keep only unmerged work until the last checkpoint; then remove
                // just this stack, retaining siblings and a valid empty file.
                crate::stack::apply_land_result(&mut next, &stack.name, &[id], index + 1 < stack.prs.len());
                next.save()?;
                config = next;
                Ok(())
            })();
            checkpoint.map_err(|e| BitbucketError::Other(format!(
                "PR #{id} is merged, but its local checkpoint failed: {e}. No further PRs were processed; inspect Bitbucket and .bbr/stack.toml before retrying"
            )))?;
            // Local cleanup is best-effort only after progress is safely saved.
            if let Err(e) = crate::git::delete_branch_local_safe_async(&pr.branch).await {
                crate::log_warn!("PR #{id} is merged; local branch {:?} was retained: {e}", pr.branch);
            }
            Ok(())
        }.await;
        if let Err(e) = result {
            failed.push(StackLandFailure {
                pr_id: id,
                branch: pr.branch.clone(),
                reason: e.to_string(),
            });
            failure = Some(e);
            break;
        }
    }
    spinner.finish();

    let out = StackLandOut { merged, failed };
    make_formatter(g).print(&out, &render_land(&out))?;
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Detect intervening edits without silently reloading an empty/default config.
/// This is an optimistic guard, not a cross-process transaction or file lock.
fn ensure_stack_state_unchanged(expected: &StackConfig) -> Result<()> {
    if StackConfig::load()? != *expected {
        return Err(BitbucketError::Other(
            "Stack configuration changed during the operation; inspect .bbr/stack.toml before retrying"
                .into(),
        ));
    }
    Ok(())
}

pub async fn abort(g: &GlobalArgs, yes: bool) -> Result<()> {
    let mut config = StackConfig::load()?;
    let mut names = std::collections::HashSet::new();
    if config.stacks.iter().any(|s| !names.insert(&s.name)) {
        return Err(BitbucketError::Other(
            "Duplicate stack names make cleanup ambiguous; repair .bbr/stack.toml before aborting"
                .into(),
        ));
    }
    let stack = config.active_stack()?.clone();
    let current_branch = if stack.prs.is_empty() {
        None
    } else {
        Some(crate::git::current_branch()?)
    };
    let mut ids = std::collections::HashSet::new();
    let mut branches = std::collections::HashSet::new();
    let mut entries = Vec::with_capacity(stack.prs.len());
    for pr in &stack.prs {
        crate::git::validate_branch_name(&pr.branch)?;
        if current_branch.as_deref() == Some(&pr.branch)
            || config.stacks.iter().any(|s| {
                s.base_branch == pr.branch
                    || (s.name != stack.name
                        && s.prs
                            .iter()
                            .any(|p| p.branch == pr.branch || p.parent_branch == pr.branch))
            })
        {
            return Err(BitbucketError::Other(format!(
                "Refusing to abort protected branch {:?}: it is checked out, a stack base, or shared by another stack", pr.branch
            )));
        }
        let id = pr.pr_id.filter(|id| *id > 0).ok_or_else(|| {
            BitbucketError::Other(format!(
                "Stack branch {:?} has no valid PR ID; repair .bbr/stack.toml before aborting",
                pr.branch
            ))
        })?;
        if !ids.insert(id) || !branches.insert(&pr.branch) {
            return Err(BitbucketError::Other(
                "Stack contains duplicate PR IDs or branches; repair .bbr/stack.toml before aborting".into(),
            ));
        }
        entries.push((pr, id));
    }

    if !confirm_destructive(
        g,
        yes,
        &format!(
            "Decline all PRs and safely delete branches for stack '{}'? (y/n): ",
            stack.name
        ),
    )
    .await?
    {
        return aborted();
    }

    let client = client(g)?;
    let repo = resolve_repo(g)?;
    let full_name = format!("{}/{}", repo.workspace, repo.slug);
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    let mut declined = Vec::new();
    let mut branches_deleted = Vec::new();
    let mut failure = None;

    for (pr, id) in entries {
        let result: Result<()> = async {
            ensure_stack_state_unchanged(&config)?;
            spinner.set_message(format!("Checking PR #{id}..."));
            let current = client.get_pr(&repo.workspace, &repo.slug, id).await?;
            validate_abort_pr(&current, id, &pr.branch, &full_name)?;
            if !current.state.eq_ignore_ascii_case("DECLINED") {
                if !current.state.eq_ignore_ascii_case("OPEN") {
                    return Err(BitbucketError::Other(format!(
                        "PR #{id} is {}, not OPEN or DECLINED; no branches were deleted for this entry", current.state
                    )));
                }
                ensure_stack_state_unchanged(&config)?;
                spinner.set_message(format!("Declining PR #{id}..."));
                let response = client.decline_pr(&repo.workspace, &repo.slug, id).await?;
                validate_abort_pr(&response, id, &pr.branch, &full_name)?;
                if !response.state.eq_ignore_ascii_case("DECLINED") {
                    return Err(BitbucketError::Other(format!(
                        "PR #{id} decline was not confirmed; inspect Bitbucket before retrying"
                    )));
                }
            }
            declined.push(id);
            ensure_stack_state_unchanged(&config)?;
            spinner.set_message(format!("Deleting remote branch {}...", pr.branch));
            // Use the resolved API repository, never an unrelated Git origin.
            match client.delete_branch(&repo.workspace, &repo.slug, &pr.branch).await {
                Ok(()) => branches_deleted.push(format!("remote/{}", pr.branch)),
                Err(BitbucketError::NotFound(_)) => {} // Already absent on retry.
                Err(error) => return Err(error),
            }
            ensure_stack_state_unchanged(&config)?;
            spinner.set_message(format!("Safely deleting local branch {}...", pr.branch));
            if crate::git::delete_local_branch_if_exists(&pr.branch).await? {
                branches_deleted.push(format!("local/{}", pr.branch));
            }
            checkpoint_abort(&mut config, &stack.name, Some(id))?;
            Ok(())
        }.await;
        if let Err(error) = result {
            failure = Some(error);
            break;
        }
    }
    if stack.prs.is_empty() {
        failure = checkpoint_abort(&mut config, &stack.name, None).err();
    }
    spinner.finish();
    let out = StackAbortOut {
        declined,
        branches_deleted,
    };
    let status = if failure.is_some() {
        "incomplete; inspect the remaining stack before retrying"
    } else {
        "aborted"
    };
    let human = format!(
        "Stack '{}' {status}.\nConfirmed declined: {} pull requests.\nDeleted {} branch references.",
        stack.name, out.declined.len(), out.branches_deleted.len()
    );
    make_formatter(g).print(&out, &human)?;
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn validate_abort_pr(
    pr: &crate::api::pr::PullRequest,
    id: u64,
    branch: &str,
    repository: &str,
) -> Result<()> {
    if pr.id != id
        || pr.source_branch() != branch
        || pr.source.repository.as_ref().map(|r| r.full_name.as_str()) != Some(repository)
    {
        return Err(BitbucketError::Other(format!(
            "PR #{id} source identity does not match the stack branch and repository; refusing cleanup"
        )));
    }
    Ok(())
}

/// Remove only a fully cleaned entry, keeping failed/unstarted work recoverable.
fn checkpoint_abort(config: &mut StackConfig, name: &str, id: Option<u64>) -> Result<()> {
    let checkpoint = (|| -> Result<()> {
        ensure_stack_state_unchanged(config)?;
        let mut next = config.clone();
        let stack = next
            .find_stack_mut(name)
            .ok_or_else(|| BitbucketError::Other("Stack disappeared during abort".into()))?;
        if let Some(id) = id {
            stack.prs.retain(|pr| pr.pr_id != Some(id));
        }
        if stack.prs.is_empty() {
            next.stacks.retain(|s| s.name != name);
            if next.active.as_deref() == Some(name) {
                next.active = next.stacks.first().map(|s| s.name.clone());
            }
        }
        next.save()?;
        *config = next;
        Ok(())
    })();
    checkpoint.map_err(|error| BitbucketError::Other(format!(
        "Abort checkpoint failed for stack {name:?}: {error}. Remote/local cleanup may already have happened; inspect Bitbucket and .bbr/stack.toml before retrying"
    )))
}

fn render_stack_list(out: &StackListOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();

    if out.stacks.len() > 1 {
        let names: Vec<String> = out
            .stacks
            .iter()
            .map(|n| {
                if n == &out.name {
                    format!("*{}", n)
                } else {
                    n.clone()
                }
            })
            .collect();
        s.push_str(&format!(
            "{} Stacks: {}\n",
            theme.bullet(),
            names.join(", ")
        ));
    }

    s.push_str(&format!(
        "{} Active stack: {} (base: {})\n",
        theme.bullet(),
        theme.bold(&out.name),
        out.base_branch
    ));
    s.push_str(&format!("{}\n", theme.separator()));

    if out.prs.is_empty() {
        s.push_str("  (No branches added to this stack yet)\n");
    } else {
        for (i, pr) in out.prs.iter().enumerate() {
            let id_str = pr
                .pr_id
                .map(|id| format!("PR #{}", id))
                .unwrap_or_else(|| "No PR".to_string());
            let state_str = pr.state.as_deref().unwrap_or("PENDING");
            let arrow = if theme.unicode_enabled() { "→" } else { "->" };
            s.push_str(&format!(
                "  {}. {:<16}  {:<8}  {:<10}  {arrow} {}\n",
                i + 1,
                pr.branch,
                id_str,
                state_str,
                pr.parent_branch
            ));
        }
    }

    if out.stacks.len() > 1 {
        s.push_str("\nHint: switch stacks with `bbr pr stack use <name>`.\n");
    }

    s
}

fn render_rebase(out: &StackRebaseOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();

    s.push_str(&format!("{}\n", theme.bold("Rebase Chain Results")));
    s.push_str(&format!("{}\n", theme.separator()));

    for step in &out.steps {
        let prefix = if step.status == "ok" {
            if theme.unicode_enabled() {
                theme.success("  ✓")
            } else {
                theme.success("  OK")
            }
        } else if theme.unicode_enabled() {
            theme.error("  ✗")
        } else {
            theme.error("  X")
        };
        s.push_str(&format!("{} {}: {}\n", prefix, step.branch, step.message));
    }

    s
}

fn render_land(out: &StackLandOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();

    s.push_str(&format!("{}\n", theme.bold("Stacked Land Results")));
    s.push_str(&format!("{}\n", theme.separator()));

    if !out.merged.is_empty() {
        s.push_str(&format!(
            "  Merged PRs: {}\n",
            out.merged
                .iter()
                .map(|id| format!("#{}", id))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !out.failed.is_empty() {
        s.push_str("\nFailed to merge:\n");
        for fail in &out.failed {
            s.push_str(&format!(
                "  PR #{} (branch {}): {}\n",
                fail.pr_id, fail.branch, fail.reason
            ));
        }
    }

    s
}
