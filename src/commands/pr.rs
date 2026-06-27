//! `bb pr` — list / view / create / comment.

use serde::Serialize;

use crate::api::pr::{
    CreateBranchRef, CreateNamed, CreatePrRequest, PrState, PullRequest, ReviewerRef, UpdatePrRequest,
};
use crate::cli::GlobalArgs;
use crate::commands::{client, confirm, current_repo, make_spinner, resolve_body};
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

// ---- commands -------------------------------------------------------------

pub async fn list(
    g: &GlobalArgs,
    state: &str,
    limit: u32,
    author: Option<&str>,
    source_branch: Option<&str>,
) -> Result<()> {
    let state = PrState::parse(state)?;
    let repo = current_repo()?;
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

pub async fn view(g: &GlobalArgs, id: Option<u64>, show_diff: bool) -> Result<()> {
    let repo = current_repo()?;
    let client = client(g)?;

    let pr = match id {
        Some(id) => client.get_pr(&repo.workspace, &repo.slug, id).await?,
        None => {
            let head = git::head()?;
            client
                .pr_for_branch(&repo.workspace, &repo.slug, &head.branch)
                .await?
                .ok_or_else(|| {
                    BitbucketError::NotFound(format!("no open PR for branch '{}'", head.branch))
                })?
        }
    };

    let out = PrViewOut {
        id: pr.id,
        state: pr.state.clone(),
        title: pr.title.clone(),
        description: pr.description.clone(),
        source: pr
            .source
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
        destination: pr
            .destination
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
        author: pr.author.as_ref().map(|a| a.display_name.clone()),
        url: pr.links.html.href.clone(),
        comment_count: pr.comment_count,
        task_count: pr.task_count,
        close_source_branch: pr.close_source_branch,
    };

    let fmt = Formatter::from_json_flag(g.json);
    let mut human = render_view(&out);

    if show_diff {
        let spinner = make_spinner(g.json);
        spinner.set_message("Fetching diff...");
        let diff = client.pr_diff(&repo.workspace, &repo.slug, pr.id).await?;
        spinner.finish_and_clear();
        human.push_str(&format!("\n\n{}", diff));
    }

    fmt.print(&out, &human)
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
    reviewers: &[String],
) -> Result<()> {
    let repo = current_repo()?;
    let client = client(g)?;

    let source_branch = match src {
        Some(s) => s.to_string(),
        None => git::current_branch()?,
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
                &format!("{human}\nNext: bb open pr {id}", id = out.id),
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
    let repo = current_repo()?;
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

pub async fn approve(g: &GlobalArgs, id: u64) -> Result<()> {
    let repo = current_repo()?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message("Approving...");
    client.approve_pr(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();
    let fmt = Formatter::from_json_flag(g.json);
    fmt.print(
        &serde_json::json!({ "id": id, "approved": true }),
        &format!("Approved PR #{}", id),
    )
}

pub async fn unapprove(g: &GlobalArgs, id: u64) -> Result<()> {
    let repo = current_repo()?;
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
    let repo = current_repo()?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message("Declining...");
    let pr = client.decline_pr(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();
    let out = PrViewOut {
        id: pr.id,
        state: pr.state.clone(),
        title: pr.title.clone(),
        description: pr.description.clone(),
        source: pr
            .source
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
        destination: pr
            .destination
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
        author: pr.author.as_ref().map(|a| a.display_name.clone()),
        url: pr.links.html.href.clone(),
        comment_count: pr.comment_count,
        task_count: pr.task_count,
        close_source_branch: pr.close_source_branch,
    };
    let fmt = Formatter::from_json_flag(g.json);
    fmt.print(&out, &format!("Declined PR #{}", id))
}

pub async fn merge(g: &GlobalArgs, id: u64) -> Result<()> {
    let repo = current_repo()?;
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching PR details...");
    let pr = client.get_pr(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();

    if !g.json
        && !confirm(&format!(
            "Merge PR #{} ({}) from {} into {}? [y/N] ",
            pr.id,
            pr.title,
            pr.source
                .branch
                .as_ref()
                .map(|b| b.name.as_str())
                .unwrap_or("?"),
            pr.destination
                .branch
                .as_ref()
                .map(|b| b.name.as_str())
                .unwrap_or("?"),
        ))?
    {
        let fmt = Formatter::from_json_flag(g.json);
        let human = "Aborted.".to_string();
        fmt.print(&(), &human)?;
        return Ok(());
    }

    let spinner = make_spinner(g.json);
    spinner.set_message("Merging...");
    let pr = client.merge_pr(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();

    let out = PrViewOut {
        id: pr.id,
        state: pr.state.clone(),
        title: pr.title.clone(),
        description: pr.description.clone(),
        source: pr
            .source
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
        destination: pr
            .destination
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
        author: pr.author.as_ref().map(|a| a.display_name.clone()),
        url: pr.links.html.href.clone(),
        comment_count: pr.comment_count,
        task_count: pr.task_count,
        close_source_branch: pr.close_source_branch,
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = format!("Merged PR #{}", id);
    if !g.json {
        fmt.print(&out, &format!("{human}\nNext: bb status"))
    } else {
        fmt.print(&out, &human)
    }
}

pub async fn checkout(g: &GlobalArgs, id: u64) -> Result<()> {
    let repo = current_repo()?;
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
    let repo = current_repo()?;
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching PR details...");
    let pr = client.get_pr(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();

    let req = UpdatePrRequest {
        title: new_title.unwrap_or(&pr.title).to_string(),
        description: new_description
            .map(|d| d.to_string())
            .or_else(|| pr.description.clone()),
        close_source_branch: None,
    };

    let spinner = make_spinner(g.json);
    spinner.set_message("Updating PR...");
    let pr = client
        .update_pr(&repo.workspace, &repo.slug, id, &req)
        .await?;
    spinner.finish_and_clear();

    let out = PrViewOut {
        id: pr.id,
        state: pr.state.clone(),
        title: pr.title.clone(),
        description: pr.description.clone(),
        source: pr
            .source
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
        destination: pr
            .destination
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
        author: pr.author.as_ref().map(|a| a.display_name.clone()),
        url: pr.links.html.href.clone(),
        comment_count: pr.comment_count,
        task_count: pr.task_count,
        close_source_branch: pr.close_source_branch,
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = format!("Updated PR #{}", id);
    fmt.print(&out, &human)
}

pub async fn diff(g: &GlobalArgs, id: u64) -> Result<()> {
    let repo = current_repo()?;
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching diff...");
    let body = client.pr_diff(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();

    let fmt = Formatter::from_json_flag(g.json);
    fmt.print(&serde_json::json!({ "id": id, "diff": body }), &body)
}

// ---- helpers --------------------------------------------------------------

fn summarize(pr: &PullRequest) -> PrSummary {
    PrSummary {
        id: pr.id,
        state: pr.state.clone(),
        title: pr.title.clone(),
        source: pr
            .source
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
        destination: pr
            .destination
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
        author: pr.author.as_ref().map(|a| a.display_name.clone()),
        url: pr.links.html.href.clone(),
        updated_on: pr.updated_on.clone(),
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
    let mut table =
        Table::new().headers(["ID", "State", "Title", "Source -> Destination", "Author"]);
    for pr in &out.pull_requests {
        let state = match pr.state.to_ascii_uppercase().as_str() {
            "OPEN" => theme.bold(&pr.state),
            "MERGED" => theme.success(&pr.state),
            "DECLINED" | "SUPERSEDED" => theme.error(&pr.state),
            _ => theme.bold(&pr.state),
        };
        table = table.add_row([
            pr.id.to_string(),
            state,
            truncate(&pr.title, 60),
            truncate(&format!("{} -> {}", pr.source, pr.destination), 50),
            pr.author.clone().unwrap_or_else(|| "-".into()),
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

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
        out.push('…');
        out
    }
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
