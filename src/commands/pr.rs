//! `bbr pr` — list / view / create / comment.

use serde::Serialize;

use crate::api::pr::{
    CreateBranchRef, CreateNamed, CreatePrRequest, MergePrRequest, PrState, PullRequest,
    PullRequestComment, PullRequestConflict, PullRequestTask, ReviewerRef, UpdatePrRequest,
};
use crate::api::repo::Commit;
use crate::api::status::BuildStatus;
use crate::api::BitbucketClient;
use crate::cli::GlobalArgs;
use crate::commands::{
    client, confirm, current_head, make_spinner, resolve_body, resolve_repo, truncate,
};
use crate::error::{BitbucketError, Result};
use crate::git;
use crate::output::table::Table;
use crate::output::theme::Theme;
use crate::output::Formatter;

// ---- JSON output shapes ---------------------------------------------------

#[derive(Debug, Serialize)]
pub struct PrListOut {
    pub workspace: String,
    pub slug: String,
    pub state: String,
    pub pull_requests: Vec<PrSummary>,
}

#[derive(Debug, Serialize)]
pub struct PrSummary {
    pub id: u64,
    pub state: String,
    pub title: String,
    pub source: String,
    pub destination: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub updated_on: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PrViewOut {
    pub id: u64,
    pub state: String,
    pub title: String,
    pub description: Option<String>,
    pub source: String,
    pub destination: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub comment_count: u64,
    pub task_count: u64,
    pub close_source_branch: bool,
}

impl From<&PullRequest> for PrViewOut {
    fn from(pr: &PullRequest) -> Self {
        Self {
            id: pr.id,
            state: pr.state.clone(),
            title: pr.title.clone(),
            description: pr.description.clone(),
            source: pr.source_branch().to_string(),
            destination: pr.destination_branch().to_string(),
            author: pr.author.as_ref().map(|a| a.display_name.clone()),
            url: pr.links.html.href.clone(),
            comment_count: pr.comment_count,
            task_count: pr.task_count,
            close_source_branch: pr.close_source_branch,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PrCreateOut {
    pub id: u64,
    pub url: Option<String>,
    pub state: String,
}

#[derive(Debug, Serialize)]
pub struct PrCommentOut {
    pub pr_id: u64,
    pub posted: bool,
}

#[derive(Debug, Serialize)]
pub struct PrCommentsOut {
    pub pr_id: u64,
    pub comments: Vec<PrCommentSummary>,
}

#[derive(Debug, Serialize)]
pub struct PrCommentSummary {
    pub id: u64,
    pub body: String,
    pub author: Option<String>,
    pub parent_id: Option<u64>,
    pub deleted: bool,
    pub created_on: Option<String>,
    pub updated_on: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PrTasksOut {
    pub pr_id: u64,
    pub tasks: Vec<PrTaskSummary>,
}

#[derive(Debug, Serialize)]
pub struct PrTaskSummary {
    pub id: u64,
    pub state: String,
    pub body: String,
    pub creator: Option<String>,
    pub assignee: Option<String>,
    pub created_on: Option<String>,
    pub updated_on: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PrCommitsOut {
    pub pr_id: u64,
    pub commits: Vec<PrCommitSummary>,
}

#[derive(Debug, Serialize)]
pub struct PrCommitSummary {
    pub hash: String,
    pub message: String,
    pub author: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PrStatusesOut {
    pub pr_id: u64,
    pub statuses: Vec<PrStatusSummary>,
}

#[derive(Debug, Serialize)]
pub struct PrStatusSummary {
    pub state: String,
    pub key: String,
    pub name: String,
    pub url: String,
    pub description: Option<String>,
    pub refname: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PrConflictsOut {
    pub pr_id: u64,
    pub conflicts: Vec<PrConflictSummary>,
}

#[derive(Debug, Serialize)]
pub struct PrConflictSummary {
    pub path: String,
    pub conflict_type: Option<String>,
    pub kind: Option<String>,
}

// ---- commands -------------------------------------------------------------

pub async fn list(
    g: &GlobalArgs,
    state: &str,
    limit: u32,
    author: Option<&str>,
    source_branch: Option<&str>,
    reviewer: Option<&str>,
    sort: &str,
    order: &str,
) -> Result<()> {
    let state = PrState::parse(state)?;
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching pull requests...");
    let values = client
        .list_prs(
            &repo.workspace,
            &repo.slug,
            state,
            limit,
            author,
            source_branch,
            reviewer,
            Some(sort),
            Some(order),
        )
        .await?;
    spinner.finish_and_clear();

    let rows: Vec<PrSummary> = values.iter().map(summarize).collect();
    let out = PrListOut {
        workspace: repo.workspace.clone(),
        slug: repo.slug.clone(),
        state: state_label(state),
        pull_requests: rows,
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = render_list(&out);
    fmt.print(&out, &human)
}

pub async fn view(
    g: &GlobalArgs,
    id: Option<u64>,
    show_diff: bool,
    show_comments: bool,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let pr = match id {
        Some(id) => client.get_pr(&repo.workspace, &repo.slug, id).await?,
        None => {
            let head = current_head()?;
            client
                .pr_for_branch_light(&repo.workspace, &repo.slug, &head.branch)
                .await?
                .ok_or_else(|| {
                    BitbucketError::NotFound(format!("no open PR for branch '{}'", head.branch))
                })?
        }
    };

    let out = PrViewOut::from(&pr);

    let fmt = Formatter::from_json_flag(g.json);
    let mut human = render_view(&out);

    if show_diff {
        let spinner = make_spinner(g.json);
        spinner.set_message("Fetching diff...");
        let diff = client.pr_diff(&repo.workspace, &repo.slug, pr.id).await?;
        spinner.finish_and_clear();
        human.push_str(&format!("\n\n{}", diff));
    }

    if show_comments {
        let spinner = make_spinner(g.json);
        spinner.set_message("Fetching comments...");
        let comments = client
            .pr_comments(&repo.workspace, &repo.slug, pr.id, 100)
            .await?;
        spinner.finish_and_clear();
        let comments_out = PrCommentsOut {
            pr_id: pr.id,
            comments: comments
                .into_iter()
                .map(|c| PrCommentSummary {
                    id: c.id,
                    body: c.content.map(|m| m.raw).unwrap_or_default(),
                    author: c.user.map(|u| u.display_name),
                    parent_id: c.parent.as_ref().map(|p| p.id),
                    deleted: c.deleted,
                    created_on: c.created_on,
                    updated_on: c.updated_on,
                })
                .collect(),
        };
        human.push_str(&format!("\n\n{}", render_comments(&comments_out)));
    }

    fmt.print_paginated(&out, &human)
}

#[allow(clippy::too_many_arguments)]
pub async fn create(
    g: &GlobalArgs,
    title: &str,
    body: Option<&str>,
    body_file: Option<&str>,
    body_stdin: bool,
    src: Option<&str>,
    dst: Option<&str>,
    close_source_branch: bool,
    draft: bool,
    reviewers: &[String],
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let source_branch = match src {
        Some(s) => s.to_string(),
        None => current_head()?.branch,
    };
    let destination_branch = match dst {
        Some(d) => d.to_string(),
        None => infer_default_branch(&repo.workspace, &repo.slug, &client).await?,
    };

    let description = if body.is_some() || body_file.is_some() || body_stdin {
        Some(resolve_body(body, body_file, body_stdin)?)
    } else {
        None
    };

    let req = CreatePrRequest {
        title: title.to_string(),
        description,
        source: CreateBranchRef {
            branch: CreateNamed {
                name: source_branch.clone(),
            },
        },
        destination: CreateBranchRef {
            branch: CreateNamed {
                name: destination_branch.clone(),
            },
        },
        close_source_branch: if close_source_branch {
            Some(true)
        } else {
            None
        },
        reviewers: reviewers
            .iter()
            .map(|uuid| ReviewerRef { uuid: uuid.clone() })
            .collect(),
        draft: if draft { Some(true) } else { None },
    };

    let spinner = make_spinner(g.json);
    spinner.set_message("Creating pull request...");
    let pr = client.create_pr(&repo.workspace, &repo.slug, &req).await?;
    spinner.finish_and_clear();

    let out = PrCreateOut {
        id: pr.id,
        url: pr.links.html.href.clone(),
        state: pr.state.clone(),
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = format!(
        "Created PR #{}: {}",
        out.id,
        out.url.as_deref().unwrap_or("(no url)")
    );
    if !g.json {
        if out.url.is_some() {
            fmt.print(
                &out,
                &format!("{human}\nNext: bbr open pr {id}", id = out.id),
            )
        } else {
            fmt.print(&out, &human)
        }
    } else {
        fmt.print(&out, &human)
    }
}

pub async fn comment(
    g: &GlobalArgs,
    id: u64,
    body: Option<&str>,
    body_file: Option<&str>,
    body_stdin: bool,
    reply_to: Option<u64>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let text = resolve_body(body, body_file, body_stdin)?;
    client
        .comment_pr(&repo.workspace, &repo.slug, id, &text, reply_to)
        .await?;

    let out = PrCommentOut {
        pr_id: id,
        posted: true,
    };
    let fmt = Formatter::from_json_flag(g.json);
    let human = if reply_to.is_some() {
        format!("Replied to comment on PR #{}", id)
    } else {
        format!("Commented on PR #{}", id)
    };
    fmt.print(&out, &human)
}

pub async fn comments(g: &GlobalArgs, id: Option<u64>, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let id = resolve_pr_id(&client, &repo.workspace, &repo.slug, id).await?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching comments...");
    let comments = client
        .pr_comments(&repo.workspace, &repo.slug, id, limit)
        .await?;
    spinner.finish_and_clear();

    let out = PrCommentsOut {
        pr_id: id,
        comments: comments.iter().map(comment_summary).collect(),
    };
    let human = render_comments(&out);
    Formatter::from_json_flag(g.json).print_paginated(&out, &human)
}

pub async fn tasks(g: &GlobalArgs, id: Option<u64>, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let id = resolve_pr_id(&client, &repo.workspace, &repo.slug, id).await?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching tasks...");
    let tasks = client
        .pr_tasks(&repo.workspace, &repo.slug, id, limit)
        .await?;
    spinner.finish_and_clear();

    let out = PrTasksOut {
        pr_id: id,
        tasks: tasks.iter().map(task_summary).collect(),
    };
    let human = render_tasks(&out);
    Formatter::from_json_flag(g.json).print(&out, &human)
}

pub async fn commits(g: &GlobalArgs, id: Option<u64>, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let id = resolve_pr_id(&client, &repo.workspace, &repo.slug, id).await?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching PR commits...");
    let commits = client
        .pr_commits(&repo.workspace, &repo.slug, id, limit)
        .await?;
    spinner.finish_and_clear();

    let out = PrCommitsOut {
        pr_id: id,
        commits: commits.iter().map(commit_summary).collect(),
    };
    let human = render_commits(&out);
    Formatter::from_json_flag(g.json).print(&out, &human)
}

pub async fn statuses(g: &GlobalArgs, id: Option<u64>, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let id = resolve_pr_id(&client, &repo.workspace, &repo.slug, id).await?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching PR statuses...");
    let statuses = client
        .pr_statuses(&repo.workspace, &repo.slug, id, limit)
        .await?;
    spinner.finish_and_clear();

    let out = PrStatusesOut {
        pr_id: id,
        statuses: statuses.iter().map(status_summary).collect(),
    };
    let human = render_statuses(&out);
    Formatter::from_json_flag(g.json).print(&out, &human)
}

pub async fn conflicts(g: &GlobalArgs, id: Option<u64>, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let id = resolve_pr_id(&client, &repo.workspace, &repo.slug, id).await?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching PR conflicts...");
    let conflicts = client
        .pr_conflicts(&repo.workspace, &repo.slug, id, limit)
        .await?;
    spinner.finish_and_clear();

    let out = PrConflictsOut {
        pr_id: id,
        conflicts: conflicts.iter().map(conflict_summary).collect(),
    };
    let human = render_conflicts(&out);
    Formatter::from_json_flag(g.json).print(&out, &human)
}

pub async fn request_changes(g: &GlobalArgs, id: u64) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message("Requesting changes...");
    client
        .request_pr_changes(&repo.workspace, &repo.slug, id)
        .await?;
    spinner.finish_and_clear();
    Formatter::from_json_flag(g.json).print(
        &serde_json::json!({ "id": id, "changes_requested": true }),
        &format!("Requested changes on PR #{}", id),
    )
}

pub async fn unrequest_changes(g: &GlobalArgs, id: u64) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message("Clearing change request...");
    client
        .unrequest_pr_changes(&repo.workspace, &repo.slug, id)
        .await?;
    spinner.finish_and_clear();
    Formatter::from_json_flag(g.json).print(
        &serde_json::json!({ "id": id, "changes_requested": false }),
        &format!("Cleared change request on PR #{}", id),
    )
}

pub async fn approve(g: &GlobalArgs, id: u64, message: Option<&str>) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message("Approving...");
    if let Some(msg) = message {
        client
            .approve_pr_with_comment(&repo.workspace, &repo.slug, id, msg)
            .await?;
    } else {
        client.approve_pr(&repo.workspace, &repo.slug, id).await?;
    }
    spinner.finish_and_clear();
    let fmt = Formatter::from_json_flag(g.json);
    fmt.print(
        &serde_json::json!({ "id": id, "approved": true }),
        &format!("Approved PR #{}", id),
    )
}

pub async fn unapprove(g: &GlobalArgs, id: u64) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message("Removing approval...");
    client.unapprove_pr(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();
    let fmt = Formatter::from_json_flag(g.json);
    fmt.print(
        &serde_json::json!({ "id": id, "approved": false }),
        &format!("Removed approval from PR #{}", id),
    )
}

pub async fn decline(g: &GlobalArgs, id: u64) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message("Declining...");
    let pr = client.decline_pr(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();
    let out = PrViewOut::from(&pr);
    let fmt = Formatter::from_json_flag(g.json);
    fmt.print(&out, &format!("Declined PR #{}", id))
}

pub async fn merge(
    g: &GlobalArgs,
    id: u64,
    close_source_branch: bool,
    strategy: Option<&str>,
    message: Option<&str>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let merge_body = MergePrRequest {
        close_source_branch: if close_source_branch {
            Some(true)
        } else {
            None
        },
        merge_strategy: strategy.map(|s| s.to_string()),
        message: message.map(|m| m.to_string()),
    };

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching PR details...");
    let pr = client.get_pr(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();

    if !g.json
        && !confirm(&format!(
            "Merge PR #{} ({}) from {} into {}? [y/N] ",
            pr.id,
            pr.title,
            empty_as_unknown(pr.source_branch()),
            empty_as_unknown(pr.destination_branch()),
        ))?
    {
        let fmt = Formatter::from_json_flag(g.json);
        let human = "Aborted.".to_string();
        fmt.print(&(), &human)?;
        return Ok(());
    }

    let spinner = make_spinner(g.json);
    spinner.set_message("Merging...");
    let body = if close_source_branch || strategy.is_some() || message.is_some() {
        Some(&merge_body)
    } else {
        None
    };
    let pr = client
        .merge_pr(&repo.workspace, &repo.slug, id, body)
        .await?;
    spinner.finish_and_clear();

    let out = PrViewOut::from(&pr);

    let fmt = Formatter::from_json_flag(g.json);
    let human = format!("Merged PR #{}", id);
    if !g.json {
        fmt.print(&out, &format!("{human}\nNext: bbr status"))
    } else {
        fmt.print(&out, &human)
    }
}

pub async fn checkout(g: &GlobalArgs, id: u64) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching PR details...");
    let pr = client.get_pr(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();

    let branch = pr
        .source
        .branch
        .as_ref()
        .map(|b| b.name.clone())
        .ok_or_else(|| BitbucketError::Other("PR has no source branch".into()))?;

    let spinner = make_spinner(g.json);
    spinner.set_message(format!("Fetching '{branch}'..."));
    git::fetch_branch(&branch)?;
    spinner.set_message(format!("Checking out '{branch}'..."));
    git::checkout_branch(&branch)?;
    spinner.finish_and_clear();

    let fmt = Formatter::from_json_flag(g.json);
    fmt.print(
        &serde_json::json!({ "id": id, "branch": branch }),
        &format!("Checked out PR #{}: {}", id, branch),
    )
}

pub async fn update(
    g: &GlobalArgs,
    id: u64,
    new_title: Option<&str>,
    new_description: Option<&str>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let req = match (new_title, new_description) {
        (Some(t), Some(d)) => UpdatePrRequest {
            title: t.to_string(),
            description: Some(d.to_string()),
            close_source_branch: None,
        },
        (title, desc) => {
            let spinner = make_spinner(g.json);
            spinner.set_message("Fetching PR details...");
            let pr = client.get_pr(&repo.workspace, &repo.slug, id).await?;
            spinner.finish_and_clear();

            UpdatePrRequest {
                title: title.unwrap_or(&pr.title).to_string(),
                description: desc
                    .map(|d| d.to_string())
                    .or_else(|| pr.description.clone()),
                close_source_branch: None,
            }
        }
    };

    let spinner = make_spinner(g.json);
    spinner.set_message("Updating PR...");
    let pr = client
        .update_pr(&repo.workspace, &repo.slug, id, &req)
        .await?;
    spinner.finish_and_clear();

    let out = PrViewOut::from(&pr);

    let fmt = Formatter::from_json_flag(g.json);
    let human = format!("Updated PR #{}", id);
    fmt.print(&out, &human)
}

pub async fn diff(g: &GlobalArgs, id: u64) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching diff...");
    let body = client.pr_diff(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();

    let fmt = Formatter::from_json_flag(g.json);
    fmt.print_diff(&serde_json::json!({ "id": id, "diff": body }), &body)
}

pub async fn diffstat(g: &GlobalArgs, id: Option<u64>) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let id = match id {
        Some(i) => i,
        None => resolve_pr_id(&client, &repo.workspace, &repo.slug, None).await?,
    };

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching diffstat...");
    let stat = client.pr_diffstat(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();

    let fmt = Formatter::from_json_flag(g.json);
    let human = format!(
        "Diffstat for PR #{}\n{}",
        id,
        serde_json::to_string_pretty(&stat).unwrap_or_default()
    );
    fmt.print(&stat, &human)
}

pub async fn patch(g: &GlobalArgs, id: Option<u64>, output: Option<&str>) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let id = match id {
        Some(i) => i,
        None => resolve_pr_id(&client, &repo.workspace, &repo.slug, None).await?,
    };

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching patch...");
    let body = client.pr_patch(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();

    if let Some(path) = output {
        std::fs::write(path, &body)
            .map_err(|e| crate::error::BitbucketError::Other(format!("writing {path}: {e}")))?;
        let out = serde_json::json!({ "id": id, "file": path, "bytes": body.len() });
        let human = format!("Wrote patch to {path}");
        Formatter::from_json_flag(g.json).print(&out, &human)
    } else {
        let out = serde_json::json!({ "id": id, "patch": body });
        let fmt = Formatter::from_json_flag(g.json);
        fmt.print_diff(&out, &body)
    }
}

// ---- helpers --------------------------------------------------------------

fn summarize(pr: &PullRequest) -> PrSummary {
    PrSummary {
        id: pr.id,
        state: pr.state.clone(),
        title: pr.title.clone(),
        source: pr.source_branch().to_string(),
        destination: pr.destination_branch().to_string(),
        author: pr.author.as_ref().map(|a| a.display_name.clone()),
        url: pr.links.html.href.clone(),
        updated_on: pr.updated_on.clone(),
    }
}

async fn resolve_pr_id(
    client: &BitbucketClient,
    workspace: &str,
    slug: &str,
    id: Option<u64>,
) -> Result<u64> {
    if let Some(id) = id {
        return Ok(id);
    }
    let head = current_head()?;
    client
        .pr_for_branch_light(workspace, slug, &head.branch)
        .await?
        .map(|pr| pr.id)
        .ok_or_else(|| BitbucketError::NotFound(format!("no open PR for branch '{}'", head.branch)))
}

fn comment_summary(comment: &PullRequestComment) -> PrCommentSummary {
    PrCommentSummary {
        id: comment.id,
        body: comment
            .content
            .as_ref()
            .map(|c| c.raw.clone())
            .unwrap_or_default(),
        author: comment.user.as_ref().map(|u| u.display_name.clone()),
        parent_id: comment.parent.as_ref().map(|p| p.id).filter(|id| *id != 0),
        deleted: comment.deleted,
        created_on: comment.created_on.clone(),
        updated_on: comment.updated_on.clone(),
    }
}

fn task_summary(task: &PullRequestTask) -> PrTaskSummary {
    PrTaskSummary {
        id: task.id,
        state: task.state.clone(),
        body: task
            .content
            .as_ref()
            .map(|c| c.raw.clone())
            .unwrap_or_default(),
        creator: task.creator.as_ref().map(|u| u.display_name.clone()),
        assignee: task.assignee.as_ref().map(|u| u.display_name.clone()),
        created_on: task.created_on.clone(),
        updated_on: task.updated_on.clone(),
    }
}

fn commit_summary(commit: &Commit) -> PrCommitSummary {
    PrCommitSummary {
        hash: commit.hash.clone(),
        message: commit.message.lines().next().unwrap_or("").to_string(),
        author: commit.author.as_ref().map(|a| a.raw.clone()),
        date: commit.date.clone(),
    }
}

fn status_summary(status: &BuildStatus) -> PrStatusSummary {
    PrStatusSummary {
        state: status.state.clone(),
        key: status.key.clone(),
        name: status.name.clone(),
        url: status.url.clone(),
        description: status.description.clone(),
        refname: status.refname.clone(),
    }
}

fn conflict_summary(conflict: &PullRequestConflict) -> PrConflictSummary {
    PrConflictSummary {
        path: conflict.path.clone(),
        conflict_type: conflict.conflict_type.clone(),
        kind: conflict.kind.clone(),
    }
}

fn empty_as_unknown(s: &str) -> &str {
    if s.is_empty() {
        "?"
    } else {
        s
    }
}

fn state_label(s: PrState) -> String {
    match s {
        PrState::Open => "open".into(),
        PrState::Merged => "merged".into(),
        PrState::Declined => "declined".into(),
        PrState::All => "all".into(),
    }
}

async fn infer_default_branch(
    workspace: &str,
    slug: &str,
    client: &crate::api::BitbucketClient,
) -> Result<String> {
    let repo = client.get_repo(workspace, slug).await?;
    Ok(repo
        .mainbranch
        .and_then(|b| {
            if b.name.is_empty() {
                None
            } else {
                Some(b.name)
            }
        })
        .unwrap_or_else(|| "main".to_string()))
}

fn render_list(out: &PrListOut) -> String {
    if out.pull_requests.is_empty() {
        return format!("No pull requests (state: {}).", out.state);
    }
    let theme = Theme::current();
    let mut table = Table::new().headers([
        "ID",
        "State",
        "Title",
        "Source",
        "Destination",
        "Author",
        "URL",
    ]);
    for pr in &out.pull_requests {
        let state = match pr.state.to_ascii_uppercase().as_str() {
            "OPEN" => theme.bold(&pr.state),
            "MERGED" => theme.success(&pr.state),
            "DECLINED" | "SUPERSEDED" => theme.error(&pr.state),
            _ => theme.bold(&pr.state),
        };
        table = table.add_row([
            pr.id.to_string(),
            state.into_owned(),
            truncate(&pr.title, 55),
            truncate(&pr.source, 30),
            truncate(&pr.destination, 30),
            pr.author.as_deref().unwrap_or("-").to_string(),
            pr.url.as_deref().unwrap_or("").to_string(),
        ]);
    }
    table.render()
}

fn render_comments(out: &PrCommentsOut) -> String {
    if out.comments.is_empty() {
        return format!("No comments on PR #{}.", out.pr_id);
    }
    let theme = Theme::current();
    let mut s = format!("Comments on PR #{}\n", out.pr_id);
    s.push_str(&format!("{}\n", theme.separator()));
    for comment in &out.comments {
        let author = comment.author.as_deref().unwrap_or("-");
        let deleted = if comment.deleted { " deleted" } else { "" };
        s.push_str(&format!(
            "#{} by {}{}\n",
            comment.id,
            theme.bold(author),
            deleted
        ));
        if let Some(parent_id) = comment.parent_id {
            s.push_str(&format!("  reply to #{parent_id}\n"));
        }
        for line in comment.body.lines().take(12) {
            s.push_str(&format!("  {line}\n"));
        }
        if comment.body.lines().count() > 12 {
            s.push_str("  ...\n");
        }
        s.push('\n');
    }
    s
}

fn render_tasks(out: &PrTasksOut) -> String {
    if out.tasks.is_empty() {
        return format!("No tasks on PR #{}.", out.pr_id);
    }
    let theme = Theme::current();
    let mut table = Table::new().headers(["ID", "State", "Task", "Assignee"]);
    for task in &out.tasks {
        let state = if task.state.eq_ignore_ascii_case("RESOLVED") {
            theme.success(&task.state)
        } else {
            theme.warn(&task.state)
        };
        table = table.add_row([
            task.id.to_string(),
            state.into_owned(),
            truncate(&task.body, 80),
            task.assignee.as_deref().unwrap_or("-").to_string(),
        ]);
    }
    table.render()
}

fn render_commits(out: &PrCommitsOut) -> String {
    if out.commits.is_empty() {
        return format!("No commits on PR #{}.", out.pr_id);
    }
    let theme = Theme::current();
    let mut table = Table::new().headers(["Hash", "Author", "Date", "Message"]);
    for commit in &out.commits {
        table = table.add_row([
            truncate(&commit.hash, 10),
            commit.author.as_deref().unwrap_or("-").to_string(),
            theme
                .dim(commit.date.as_deref().unwrap_or("-"))
                .into_owned(),
            truncate(&commit.message, 60),
        ]);
    }
    table.render()
}

fn render_statuses(out: &PrStatusesOut) -> String {
    if out.statuses.is_empty() {
        return format!("No commit statuses on PR #{}.", out.pr_id);
    }
    let theme = Theme::current();
    let mut table = Table::new().headers(["State", "Key", "Name", "URL"]);
    for status in &out.statuses {
        table = table.add_row([
            theme.status_glyph(&status.state),
            status.key.clone(),
            if status.name.is_empty() {
                "-".into()
            } else {
                status.name.clone()
            },
            if status.url.is_empty() {
                "-".into()
            } else {
                status.url.clone()
            },
        ]);
    }
    table.render()
}

fn render_conflicts(out: &PrConflictsOut) -> String {
    if out.conflicts.is_empty() {
        return format!("No conflicts on PR #{}.", out.pr_id);
    }
    let mut table = Table::new().headers(["Path", "Type", "Kind"]);
    for conflict in &out.conflicts {
        table = table.add_row([
            conflict.path.clone(),
            conflict.conflict_type.as_deref().unwrap_or("-").to_string(),
            conflict.kind.as_deref().unwrap_or("-").to_string(),
        ]);
    }
    table.render()
}

fn render_view(out: &PrViewOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();
    s.push_str(&format!("PR #{} — {}\n", out.id, theme.bold(&out.state)));
    s.push_str(&format!("{}\n", theme.separator()));
    s.push_str(&format!("  {}{}\n", theme.label("Title:"), out.title));
    if let Some(d) = &out.description {
        s.push_str(&format!(
            "  {}{}\n",
            theme.label("Desc:"),
            truncate_desc(d, 200)
        ));
    }
    s.push_str(&format!(
        "  {} {} → {}\n",
        theme.label("Branches:"),
        out.source,
        out.destination
    ));
    s.push_str(&format!(
        "  {}{}\n",
        theme.label("Author:"),
        out.author.as_deref().unwrap_or("-")
    ));
    s.push_str(&format!(
        "  {} {}  |  {} {}\n",
        theme.label("Comments:"),
        out.comment_count,
        theme.label("Tasks:"),
        out.task_count
    ));
    s.push_str(&format!(
        "  {} {}",
        theme.label("Close src:"),
        if out.close_source_branch { "yes" } else { "no" }
    ));
    if let Some(u) = &out.url {
        s.push_str(&format!("\n  {}{u}", theme.label("URL:")));
    }
    s
}

fn truncate_desc(s: &str, n: usize) -> String {
    let first = s.lines().next().unwrap_or(s);
    if first.chars().count() <= n {
        if s.lines().count() > 1 {
            format!(
                "{first}\n  (... {}, use --json for full)",
                "multi-line body"
            )
        } else {
            first.to_string()
        }
    } else {
        let mut out: String = first.chars().take(n.saturating_sub(1)).collect();
        out.push('…');
        out.push_str(&format!("\n  (... {}, use --json for full)", "truncated"));
        out
    }
}
