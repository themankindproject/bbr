//! Batch operations (`bbr batch`).

use crate::api::pr::{MergePrRequest, PrState};
use crate::cli::GlobalArgs;
use crate::commands::{client, confirm, make_spinner, resolve_repo, SpinnerGuard};
use crate::error::Result;
use crate::output::table::Table;
use crate::output::theme::Theme;
use crate::output::Formatter;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct BatchPlan<T: Serialize> {
    pub dry_run: bool,
    pub action_count: usize,
    pub actions: Vec<T>,
}

#[derive(Debug, Serialize, Clone)]
pub struct MergeAction {
    pub pr_id: u64,
    pub title: String,
    pub source: String,
    pub destination: String,
    pub approvals: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct RerunAction {
    pub pipeline_uuid: String,
    pub build_number: u64,
    pub branch: Option<String>,
    pub state: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct CleanupAction {
    pub branch_name: String,
    pub is_remote: bool,
}

#[derive(Debug, Serialize)]
pub struct BatchResult {
    pub succeeded: Vec<BatchActionOutcome>,
    pub failed: Vec<BatchActionOutcome>,
}

#[derive(Debug, Serialize)]
pub struct BatchActionOutcome {
    pub id: String,
    pub description: String,
    pub error: Option<String>,
}

pub async fn merge_approved(
    g: &GlobalArgs,
    repo_arg: Option<&str>,
    dry_run: bool,
    strategy: Option<&str>,
    yes: bool,
) -> Result<()> {
    let client = client(g)?;
    let repo = resolve_repo(g)?;
    let slug = repo_arg.unwrap_or(&repo.slug);

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching open pull requests...");

    let prs = client
        .list_prs(
            &repo.workspace,
            slug,
            PrState::Open,
            100,
            None,
            None,
            None,
            None,
            None,
        )
        .await?;

    let mut approved_actions = Vec::new();

    for pr in prs {
        // Count approvals from reviewers (preferred) or all participants
        let approval_count = if !pr.reviewers.is_empty() {
            pr.reviewers.iter().filter(|r| r.approved).count()
        } else {
            pr.participants.iter().filter(|p| p.approved).count()
        };

        let is_approved = approval_count > 0;

        if is_approved {
            approved_actions.push(MergeAction {
                pr_id: pr.id,
                title: pr.title.clone(),
                source: pr.source_branch().to_string(),
                destination: pr.destination_branch().to_string(),
                approvals: approval_count,
            });
        }
    }

    spinner.finish();

    let plan = BatchPlan {
        dry_run,
        action_count: approved_actions.len(),
        actions: approved_actions.clone(),
    };

    if approved_actions.is_empty() {
        if g.json {
            Formatter::from_json_flag(g.json).print(&plan, "")?;
        } else {
            eprintln!("No approved pull requests found to merge.");
        }
        return Ok(());
    }

    if !g.json {
        let theme = Theme::current();
        eprintln!("{}", theme.bold("Proposed Merge Plan:"));
        let mut table =
            Table::new().headers(["PR ID", "Title", "Source", "Destination", "Approvals"]);
        for act in &approved_actions {
            table = table.add_row([
                act.pr_id.to_string(),
                act.title.clone(),
                act.source.clone(),
                act.destination.clone(),
                act.approvals.to_string(),
            ]);
        }
        eprintln!("{}", table.render());
    }

    if dry_run {
        if g.json {
            Formatter::from_json_flag(g.json).print(&plan, "")?;
        }
        return Ok(());
    }

    if !yes
        && !confirm(&format!(
            "Merge {} pull requests? (y/n): ",
            approved_actions.len()
        )).await?
    {
        return Ok(());
    }

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    let run_spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    for act in approved_actions {
        run_spinner.set_message(format!("Merging PR #{}...", act.pr_id));
        let merge_req = MergePrRequest {
            close_source_branch: Some(true),
            merge_strategy: strategy.map(|s| s.to_string()),
            message: None,
        };
        match client
            .merge_pr(&repo.workspace, slug, act.pr_id, Some(&merge_req))
            .await
        {
            Ok(_) => succeeded.push(BatchActionOutcome {
                id: act.pr_id.to_string(),
                description: format!("PR #{} merged successfully", act.pr_id),
                error: None,
            }),
            Err(e) => failed.push(BatchActionOutcome {
                id: act.pr_id.to_string(),
                description: format!("PR #{} merge failed", act.pr_id),
                error: Some(e.to_string()),
            }),
        }
    }
    run_spinner.finish();

    let result = BatchResult { succeeded, failed };
    let human = render_results(&result);
    Formatter::from_json_flag(g.json).print(&result, &human)
}

pub async fn rerun_failed(
    g: &GlobalArgs,
    branch_filter: Option<&str>,
    repo_arg: Option<&str>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let client = client(g)?;
    let repo = resolve_repo(g)?;
    let slug = repo_arg.unwrap_or(&repo.slug);

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching recent pipelines...");

    let pipelines = client
        .list_pipelines(&repo.workspace, slug, branch_filter, 50)
        .await?;

    // Keep the latest pipeline per branch
    let mut latest_by_branch = std::collections::HashMap::new();
    for p in pipelines {
        if let Some(branch) = &p.target.ref_name {
            latest_by_branch.entry(branch.clone()).or_insert(p);
        }
    }

    let mut failed_actions = Vec::new();
    for (_, p) in latest_by_branch {
        if p.state_name().eq_ignore_ascii_case("FAILED")
            || p.state_name().eq_ignore_ascii_case("ERROR")
        {
            failed_actions.push(RerunAction {
                pipeline_uuid: p.uuid.clone(),
                build_number: p.build_number,
                branch: p.target.ref_name.clone(),
                state: p.state_name().to_string(),
            });
        }
    }

    spinner.finish();

    let plan = BatchPlan {
        dry_run,
        action_count: failed_actions.len(),
        actions: failed_actions.clone(),
    };

    if failed_actions.is_empty() {
        if g.json {
            Formatter::from_json_flag(g.json).print(&plan, "")?;
        } else {
            eprintln!("No failed pipelines found to rerun.");
        }
        return Ok(());
    }

    if !g.json {
        let theme = Theme::current();
        eprintln!("{}", theme.bold("Proposed Rerun Plan:"));
        let mut table = Table::new().headers(["Build #", "Branch", "State", "Pipeline UUID"]);
        for act in &failed_actions {
            table = table.add_row([
                act.build_number.to_string(),
                act.branch.clone().unwrap_or_else(|| "unknown".to_string()),
                act.state.clone(),
                act.pipeline_uuid.clone(),
            ]);
        }
        eprintln!("{}", table.render());
    }

    if dry_run {
        if g.json {
            Formatter::from_json_flag(g.json).print(&plan, "")?;
        }
        return Ok(());
    }

    if !yes
        && !confirm(&format!(
            "Rerun {} pipelines? (y/n): ",
            failed_actions.len()
        )).await?
    {
        return Ok(());
    }

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    let run_spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    for act in failed_actions {
        run_spinner.set_message(format!("Rerunning pipeline #{}...", act.build_number));
        match client
            .rerun_pipeline(&repo.workspace, slug, &act.pipeline_uuid)
            .await
        {
            Ok(new_p) => succeeded.push(BatchActionOutcome {
                id: act.build_number.to_string(),
                description: format!(
                    "Pipeline #{} rerun started. New build is #{}",
                    act.build_number, new_p.build_number
                ),
                error: None,
            }),
            Err(e) => failed.push(BatchActionOutcome {
                id: act.build_number.to_string(),
                description: format!("Pipeline #{} rerun failed", act.build_number),
                error: Some(e.to_string()),
            }),
        }
    }
    run_spinner.finish();

    let result = BatchResult { succeeded, failed };
    let human = render_results(&result);
    Formatter::from_json_flag(g.json).print(&result, &human)
}

pub async fn cleanup_merged_branches(
    g: &GlobalArgs,
    repo_arg: Option<&str>,
    remote: bool,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    let client = client(g)?;
    let repo = resolve_repo(g)?;
    let slug = repo_arg.unwrap_or(&repo.slug);

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Listing remote branches...");

    let branches = client.list_branches(&repo.workspace, slug, 100).await?;

    let protected_branches = ["main", "master", "develop", "production"];

    let mut cleanup_actions = Vec::new();
    for branch in branches {
        if branch.merged
            && !protected_branches.iter().any(|&p| {
                branch.name == p
                    || branch.name.starts_with("release/")
                    || branch.name.starts_with("hotfix/")
            })
        {
            // Local cleanup is always proposed
            cleanup_actions.push(CleanupAction {
                branch_name: branch.name.clone(),
                is_remote: false,
            });
            if remote {
                cleanup_actions.push(CleanupAction {
                    branch_name: branch.name.clone(),
                    is_remote: true,
                });
            }
        }
    }

    spinner.finish();

    let plan = BatchPlan {
        dry_run,
        action_count: cleanup_actions.len(),
        actions: cleanup_actions.clone(),
    };

    if cleanup_actions.is_empty() {
        if g.json {
            Formatter::from_json_flag(g.json).print(&plan, "")?;
        } else {
            eprintln!("No merged branches found to clean up.");
        }
        return Ok(());
    }

    if !g.json {
        let theme = Theme::current();
        eprintln!("{}", theme.bold("Proposed Cleanup Plan:"));
        let mut table = Table::new().headers(["Branch Name", "Scope"]);
        for act in &cleanup_actions {
            let scope = if act.is_remote { "Remote" } else { "Local" };
            table = table.add_row([act.branch_name.clone(), scope.to_string()]);
        }
        eprintln!("{}", table.render());
    }

    if dry_run {
        if g.json {
            Formatter::from_json_flag(g.json).print(&plan, "")?;
        }
        return Ok(());
    }

    if !yes
        && !confirm(&format!(
            "Delete/cleanup {} branch targets? (y/n): ",
            cleanup_actions.len()
        )).await?
    {
        return Ok(());
    }

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    let run_spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    for act in cleanup_actions {
        if act.is_remote {
            run_spinner.set_message(format!("Deleting remote branch {}...", act.branch_name));
            match client
                .delete_branch(&repo.workspace, slug, &act.branch_name)
                .await
            {
                Ok(_) => succeeded.push(BatchActionOutcome {
                    id: format!("remote/{}", act.branch_name),
                    description: format!("Deleted remote branch {}", act.branch_name),
                    error: None,
                }),
                Err(e) => failed.push(BatchActionOutcome {
                    id: format!("remote/{}", act.branch_name),
                    description: format!("Failed to delete remote branch {}", act.branch_name),
                    error: Some(e.to_string()),
                }),
            }
        } else {
            run_spinner.set_message(format!("Deleting local branch {}...", act.branch_name));
            // Run git branch -d <branch>
            let git_res = std::process::Command::new("git")
                .args(["branch", "-d", &act.branch_name])
                .output();
            match git_res {
                Ok(output) if output.status.success() => succeeded.push(BatchActionOutcome {
                    id: format!("local/{}", act.branch_name),
                    description: format!("Deleted local branch {}", act.branch_name),
                    error: None,
                }),
                Ok(output) => {
                    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    failed.push(BatchActionOutcome {
                        id: format!("local/{}", act.branch_name),
                        description: format!(
                            "Failed to delete local branch {} (use -D manually or prune)",
                            act.branch_name
                        ),
                        error: Some(err),
                    })
                }
                Err(e) => failed.push(BatchActionOutcome {
                    id: format!("local/{}", act.branch_name),
                    description: format!("Failed to run git branch -d {}", act.branch_name),
                    error: Some(e.to_string()),
                }),
            }
        }
    }
    run_spinner.finish();

    let result = BatchResult { succeeded, failed };
    let human = render_results(&result);
    Formatter::from_json_flag(g.json).print(&result, &human)
}

fn render_results(res: &BatchResult) -> String {
    let theme = Theme::current();
    let mut s = String::new();
    s.push_str(&format!("{}\n", theme.bold("Batch Operation Results")));
    s.push_str(&format!("  Succeeded: {}\n", res.succeeded.len()));
    s.push_str(&format!("  Failed:    {}\n\n", res.failed.len()));

    if !res.succeeded.is_empty() {
        s.push_str(&format!("{}\n", theme.success("Succeeded Actions:")));
        for act in &res.succeeded {
            s.push_str(&format!("  [ok] {}\n", act.description));
        }
    }
    if !res.failed.is_empty() {
        s.push_str(&format!("\n{}\n", theme.error("Failed Actions:")));
        for act in &res.failed {
            let err_part = act.error.as_deref().unwrap_or("unknown error");
            s.push_str(&format!(
                "  [X] {} (Error: {})\n",
                act.description, err_part
            ));
        }
    }
    s
}
