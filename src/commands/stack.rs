//! Stacked PRs CLI command (`bbr pr stack`).

use crate::api::pr::{CreateBranchRef, CreateNamed, CreatePrRequest, MergePrRequest};
use crate::cli::GlobalArgs;
use crate::commands::{client, confirm, current_head, make_spinner, resolve_repo, SpinnerGuard};
use crate::error::{BitbucketError, Result};
use crate::output::theme::Theme;
use crate::output::Formatter;
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
    let mut config = StackConfig::load().unwrap_or_default();

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

    config.save()?;

    let out = StackInitOut {
        name: name.to_string(),
        base_branch,
    };

    let human = format!(
        "Initialized empty stack '{}' onto base branch '{}'.",
        out.name, out.base_branch
    );
    Formatter::from_json_flag(g.json).print(&out, &human)
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
    crate::git::push_branch(branch)?;

    spinner.set_message(format!(
        "Creating pull request: {} → {}...",
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
    Formatter::from_json_flag(g.json).print(&out, &human)
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
        prs: prs_status,
    };

    let human = render_stack_list(&out);
    Formatter::from_json_flag(g.json).print(&out, &human)
}

pub fn rebase(g: &GlobalArgs, push: bool) -> Result<()> {
    if !crate::git::is_working_tree_clean()? {
        return Err(BitbucketError::Other(
            "Working directory is dirty. Please commit or stash changes before rebasing.".into(),
        ));
    }

    let config = StackConfig::load()?;
    let stack = config.active_stack()?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    let mut steps = Vec::new();

    for pr in &stack.prs {
        spinner.set_message(format!(
            "Rebasing {} onto {}...",
            pr.branch, pr.parent_branch
        ));
        match crate::git::rebase_branch(&pr.branch, &pr.parent_branch) {
            Ok(_) => {
                let mut push_msg = String::new();
                if push {
                    spinner.set_message(format!(
                        "Pushing branch {} with force-with-lease...",
                        pr.branch
                    ));
                    match crate::git::push_force_with_lease(&pr.branch) {
                        Ok(_) => push_msg = " and pushed".to_string(),
                        Err(e) => {
                            steps.push(StackRebaseStep {
                                branch: pr.branch.clone(),
                                status: "error".to_string(),
                                message: format!("Rebase succeeded but force-push failed: {}", e),
                            });
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
                break; // Stop rebase chain on conflict
            }
        }
    }

    spinner.finish();

    let out = StackRebaseOut { steps };
    let human = render_rebase(&out);
    Formatter::from_json_flag(g.json).print(&out, &human)
}

pub async fn land(g: &GlobalArgs, strategy: Option<&str>, yes: bool) -> Result<()> {
    if !crate::git::is_working_tree_clean()? {
        return Err(BitbucketError::Other(
            "Working directory is dirty. Please commit or stash changes before landing.".into(),
        ));
    }

    let config = StackConfig::load()?;
    let stack = config.active_stack()?.clone();

    if stack.prs.is_empty() {
        return Err(BitbucketError::Other(
            "Empty stack. Nothing to land.".into(),
        ));
    }

    let client = client(g)?;
    let repo = resolve_repo(g)?;

    if !yes
        && !confirm(&format!(
            "Merge and land {} stacked pull requests bottom-up? (y/n): ",
            stack.prs.len()
        ))
        .await?
    {
        return Ok(());
    }

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    let mut merged = Vec::new();
    let mut failed = Vec::new();

    for pr in &stack.prs {
        if let Some(id) = pr.pr_id {
            spinner.set_message(format!("Merging PR #{} (branch {})...", id, pr.branch));
            let merge_req = MergePrRequest {
                close_source_branch: Some(true),
                merge_strategy: strategy.map(|s| s.to_string()),
                message: None,
            };
            match client
                .merge_pr(&repo.workspace, &repo.slug, id, Some(&merge_req))
                .await
            {
                Ok(_) => {
                    merged.push(id);
                    // Also clean up local branch
                    let _ = crate::git::delete_branch_local(&pr.branch);
                }
                Err(e) => {
                    failed.push(StackLandFailure {
                        pr_id: id,
                        branch: pr.branch.clone(),
                        reason: e.to_string(),
                    });
                    break; // Stop landing chain on failure
                }
            }
        }
    }

    spinner.finish();

    // If all merged successfully, delete the stack config
    if failed.is_empty() {
        let _ = std::fs::remove_file(StackConfig::config_path());
    } else {
        // Update stack config with remaining PRs
        let mut new_config = StackConfig::load().unwrap_or_default();
        if let Some(s) = new_config.find_stack_mut(&stack.name) {
            s.prs.retain(|p| !merged.contains(&p.pr_id.unwrap_or(0)));
        }
        let _ = new_config.save();
    }

    let out = StackLandOut { merged, failed };
    let human = render_land(&out);
    Formatter::from_json_flag(g.json).print(&out, &human)
}

pub async fn abort(g: &GlobalArgs, yes: bool) -> Result<()> {
    let config = StackConfig::load()?;
    let stack = config.active_stack()?.clone();

    if !yes
        && !confirm(&format!(
            "Decline all PRs and delete branches for stack '{}'? (y/n): ",
            stack.name
        ))
        .await?
    {
        return Ok(());
    }

    let client = client(g)?;
    let repo = resolve_repo(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    let mut declined = Vec::new();
    let mut branches_deleted = Vec::new();

    for pr in &stack.prs {
        if let Some(id) = pr.pr_id {
            spinner.set_message(format!("Declining PR #{}...", id));
            if client
                .decline_pr(&repo.workspace, &repo.slug, id)
                .await
                .is_ok()
            {
                declined.push(id);
            }
        }
        spinner.set_message(format!("Deleting branch {}...", pr.branch));
        if crate::git::delete_branch_local(&pr.branch).is_ok() {
            branches_deleted.push(format!("local/{}", pr.branch));
        }
        if crate::git::delete_branch_remote(&pr.branch).is_ok() {
            branches_deleted.push(format!("remote/{}", pr.branch));
        }
    }

    spinner.finish();

    // Remove stack from configuration
    let mut new_config = StackConfig::load().unwrap_or_default();
    new_config.stacks.retain(|s| s.name != stack.name);
    new_config.save()?;

    let out = StackAbortOut {
        declined,
        branches_deleted,
    };

    let human = format!(
        "Stack '{}' aborted.\nDeclined {} pull requests.\nCleaned up {} branch references.",
        stack.name,
        out.declined.len(),
        out.branches_deleted.len()
    );
    Formatter::from_json_flag(g.json).print(&out, &human)
}

fn render_stack_list(out: &StackListOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();

    s.push_str(&format!(
        "{} Stack: {} (base: {})\n",
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
            s.push_str(&format!(
                "  {}. {:<16}  {:<8}  {:<10}  → {}\n",
                i + 1,
                pr.branch,
                id_str,
                state_str,
                pr.parent_branch
            ));
        }
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
            theme.success("  ✓")
        } else {
            theme.error("  ✗")
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
