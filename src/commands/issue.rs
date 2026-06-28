//! `bbr issue` — repository issue tracker.
use crate::cli::GlobalArgs;
use crate::commands::{client, make_spinner, resolve_repo, truncate};
use crate::error::Result;
use crate::output::table::Table;
use crate::output::Formatter;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct IssueOut {
    pub id: u64,
    pub title: String,
    pub state: String,
    pub kind: String,
    pub priority: String,
    pub assignee: Option<String>,
    pub reporter: Option<String>,
    pub comment_count: u32,
    pub votes: u32,
    pub created_on: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IssueDetailOut {
    pub id: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub kind: String,
    pub priority: String,
    pub assignee: Option<String>,
    pub reporter: Option<String>,
    pub comment_count: u32,
    pub votes: u32,
    pub watches: u32,
    pub component: Option<String>,
    pub milestone: Option<String>,
    pub version: Option<String>,
    pub created_on: Option<String>,
    pub updated_on: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IssueCommentOut {
    pub id: u64,
    pub author: Option<String>,
    pub body: String,
    pub created_on: Option<String>,
}

fn issue_to_out(i: &crate::api::issue::Issue) -> IssueOut {
    IssueOut {
        id: i.id,
        title: i.title.clone(),
        state: i.state.clone(),
        kind: i.kind.clone(),
        priority: i.priority.clone(),
        assignee: i.assignee.as_ref().map(|u| u.display_name.clone()),
        reporter: i.reporter.as_ref().map(|u| u.display_name.clone()),
        comment_count: i.comment_count,
        votes: i.votes,
        created_on: i.created_on.as_ref().map(|d| d.chars().take(10).collect()),
        url: i.links.html.href.clone(),
    }
}

pub async fn list(
    g: &GlobalArgs,
    limit: u32,
    status: Option<&str>,
    kind: Option<&str>,
    priority: Option<&str>,
    assignee: Option<&str>,
    query: Option<&str>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching issues...");
    let issues = client
        .list_issues(
            &repo.workspace,
            &repo.slug,
            limit,
            status,
            kind,
            priority,
            assignee,
            query,
        )
        .await?;
    spinner.finish_and_clear();

    let out: Vec<IssueOut> = issues.iter().map(issue_to_out).collect();

    let fmt = Formatter::from_json_flag(g.json);
    let mut table = Table::new().headers([
        "ID", "State", "Kind", "Priority", "Title", "Assignee", "Comments",
    ]);
    for (i, issue) in issues.iter().enumerate() {
        table = table.add_row([
            issue.id.to_string(),
            issue.state.clone(),
            issue.kind.clone(),
            issue.priority.clone(),
            truncate(&issue.title, 50),
            out[i].assignee.clone().unwrap_or_else(|| "-".into()),
            issue.comment_count.to_string(),
        ]);
    }
    let human = if issues.is_empty() {
        "No issues found.".into()
    } else {
        table.render()
    };
    fmt.print(&out, &human)
}

pub async fn view(g: &GlobalArgs, id: u64, show_comments: bool) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message(format!("Fetching issue #{id}..."));
    let issue = client.get_issue(&repo.workspace, &repo.slug, id).await?;
    spinner.finish_and_clear();

    let body = issue
        .content
        .as_ref()
        .map(|c| c.raw.as_str())
        .unwrap_or("")
        .to_string();

    let out = IssueDetailOut {
        id: issue.id,
        title: issue.title.clone(),
        body: body.clone(),
        state: issue.state.clone(),
        kind: issue.kind.clone(),
        priority: issue.priority.clone(),
        assignee: issue.assignee.as_ref().map(|u| u.display_name.clone()),
        reporter: issue.reporter.as_ref().map(|u| u.display_name.clone()),
        comment_count: issue.comment_count,
        votes: issue.votes,
        watches: issue.watches,
        component: issue.component.as_ref().map(|c| c.name.clone()),
        milestone: issue.milestone.as_ref().map(|m| m.name.clone()),
        version: issue.version.as_ref().map(|v| v.name.clone()),
        created_on: issue
            .created_on
            .as_ref()
            .map(|d| d.chars().take(10).collect()),
        updated_on: issue
            .updated_on
            .as_ref()
            .map(|d| d.chars().take(10).collect()),
        url: issue.links.html.href.clone(),
    };

    let fmt = Formatter::from_json_flag(g.json);
    let sep = "─".repeat(50);
    let human = format!(
        "Issue #{id} — {title}\n{sep}\n  State: {state:<12} Kind: {kind:<12} Priority: {priority}\n  Reporter: {reporter:<20} Assignee: {assignee}\n  Created: {created}\n  URL: {url}\n\nDescription:\n{body_indented}\n\n[{cmts} comments, {votes} votes, {watches} watchers]",
        id = issue.id,
        title = issue.title,
        sep = sep,
        state = issue.state,
        kind = issue.kind,
        priority = issue.priority,
        reporter = out.reporter.as_deref().unwrap_or("-"),
        assignee = out.assignee.as_deref().unwrap_or("-"),
        created = out.created_on.as_deref().unwrap_or("-"),
        url = out.url.as_deref().unwrap_or("-"),
        body_indented = body
            .lines()
            .map(|l| format!("  {l}"))
            .collect::<Vec<_>>()
            .join("\n"),
        cmts = issue.comment_count,
        votes = issue.votes,
        watches = issue.watches,
    );

    fmt.print(&out, &human)?;

    if show_comments && issue.comment_count > 0 {
        let comments = client
            .list_issue_comments(&repo.workspace, &repo.slug, id, 50)
            .await?;
        println!("\nComments ({})", comments.len());
        println!("{}", "─".repeat(50));
        for c in &comments {
            let author = c
                .author
                .as_ref()
                .map(|u| u.display_name.as_str())
                .unwrap_or("?");
            let date: String = c
                .created_on
                .as_ref()
                .map(|d| d.chars().take(10).collect())
                .unwrap_or_else(|| "-".into());
            let body = c.content.as_ref().map(|ct| ct.raw.as_str()).unwrap_or("");
            println!("  @{author} on {date}\n  {body}\n");
        }
    }
    Ok(())
}

pub async fn create(
    g: &GlobalArgs,
    title: &str,
    body: &str,
    kind: &str,
    priority: &str,
    assignee: Option<&str>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message("Creating issue...");
    let issue = client
        .create_issue(
            &repo.workspace,
            &repo.slug,
            title,
            body,
            kind,
            priority,
            assignee,
        )
        .await?;
    spinner.finish_and_clear();
    let out = issue_to_out(&issue);
    let fmt = Formatter::from_json_flag(g.json);
    let human = format!(
        "Created issue #{}\n  {}",
        issue.id,
        out.url.as_deref().unwrap_or("-")
    );
    fmt.print(&out, &human)
}

pub async fn update(
    g: &GlobalArgs,
    id: u64,
    title: Option<&str>,
    body: Option<&str>,
    status: Option<&str>,
    kind: Option<&str>,
    priority: Option<&str>,
    assignee: Option<&str>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message(format!("Updating issue #{id}..."));
    let issue = client
        .update_issue(
            &repo.workspace,
            &repo.slug,
            id,
            title,
            body,
            status,
            kind,
            priority,
            assignee,
        )
        .await?;
    spinner.finish_and_clear();
    let out = issue_to_out(&issue);
    let fmt = Formatter::from_json_flag(g.json);
    let human = format!("Updated issue #{}", issue.id);
    fmt.print(&out, &human)
}

pub async fn comment(g: &GlobalArgs, id: u64, body: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message("Posting comment...");
    let c = client
        .create_issue_comment(&repo.workspace, &repo.slug, id, body)
        .await?;
    spinner.finish_and_clear();
    let out = IssueCommentOut {
        id: c.id,
        author: c.author.as_ref().map(|u| u.display_name.clone()),
        body: c
            .content
            .as_ref()
            .map(|ct| ct.raw.clone())
            .unwrap_or_default(),
        created_on: c.created_on.as_ref().map(|d| d.chars().take(10).collect()),
    };
    let fmt = Formatter::from_json_flag(g.json);
    let human = format!("Posted comment #{} on issue #{}", c.id, id);
    fmt.print(&out, &human)
}

pub async fn list_comments(g: &GlobalArgs, id: u64, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = make_spinner(g.json);
    spinner.set_message(format!("Fetching comments for issue #{id}..."));
    let comments = client
        .list_issue_comments(&repo.workspace, &repo.slug, id, limit)
        .await?;
    spinner.finish_and_clear();

    let out: Vec<IssueCommentOut> = comments
        .iter()
        .map(|c| IssueCommentOut {
            id: c.id,
            author: c.author.as_ref().map(|u| u.display_name.clone()),
            body: c
                .content
                .as_ref()
                .map(|ct| ct.raw.clone())
                .unwrap_or_default(),
            created_on: c.created_on.as_ref().map(|d| d.chars().take(10).collect()),
        })
        .collect();

    let fmt = Formatter::from_json_flag(g.json);
    let mut table = Table::new().headers(["ID", "Author", "Date", "Body"]);
    for c in &out {
        table = table.add_row([
            c.id.to_string(),
            c.author.clone().unwrap_or_else(|| "-".into()),
            c.created_on.clone().unwrap_or_else(|| "-".into()),
            truncate(&c.body, 80),
        ]);
    }
    let human = if comments.is_empty() {
        format!("No comments on issue #{id}")
    } else {
        table.render()
    };
    fmt.print(&out, &human)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_out_serializes() {
        let out = IssueOut {
            id: 42,
            title: "Test bug".into(),
            state: "open".into(),
            kind: "bug".into(),
            priority: "major".into(),
            assignee: None,
            reporter: Some("Alice".into()),
            comment_count: 3,
            votes: 1,
            created_on: Some("2024-01-01".into()),
            url: Some("https://bitbucket.org/".into()),
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["id"], 42);
        assert_eq!(json["state"], "open");
    }

    #[test]
    fn issue_comment_out_serializes() {
        let out = IssueCommentOut {
            id: 1,
            author: Some("Bob".into()),
            body: "Confirmed".into(),
            created_on: Some("2024-01-02".into()),
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["author"], "Bob");
    }
}
