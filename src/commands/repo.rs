//! `bbr repo` — info, branches, commits.

use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::commands::{
    client, make_formatter, make_spinner, resolve_repo, table_or_empty, truncate, SpinnerGuard,
};
use crate::error::Result;
use crate::output::table::Table;

#[derive(Debug, Serialize)]
pub struct RepoInfoOut {
    pub workspace: String,
    pub slug: String,
    pub full_name: String,
    pub scm: String,
    pub private: bool,
    pub language: String,
    pub description: Option<String>,
    pub web_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BranchOut {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct TagOut {
    pub name: String,
    pub target: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CommitOut {
    pub hash: String,
    pub message: String,
    pub author: Option<String>,
    pub date: Option<String>,
}

pub async fn info(g: &GlobalArgs) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let info = client.get_repo(&repo.workspace, &repo.slug).await?;

    let out = RepoInfoOut {
        workspace: repo.workspace.clone(),
        slug: repo.slug.clone(),
        full_name: info.full_name.clone(),
        scm: info.scm.clone(),
        private: info.is_private,
        language: info.language.clone(),
        description: info.description.clone(),
        web_url: info.links.html.href.clone(),
    };

    let fmt = make_formatter(g);
    let theme = crate::output::theme::Theme::current();
    let mut human = format!(
        "{}{}\n{}{}\n{}{}\n{}{}\n{}{}\n{}{}\n{}{}",
        theme.label("workspace:"),
        out.workspace,
        theme.label("slug:     "),
        out.slug,
        theme.label("full name:"),
        out.full_name,
        theme.label("scm:      "),
        out.scm,
        theme.label("private:  "),
        out.private,
        theme.label("language: "),
        out.language,
        theme.label("url:      "),
        out.web_url.as_deref().unwrap_or("-"),
    );
    if let Some(desc) = &out.description {
        human.push_str(&format!("\n{}{desc}", theme.label("desc:     ")));
    }
    fmt.print(&out, &human)
}

pub async fn list_branches(g: &GlobalArgs, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching branches...");
    let values = client
        .list_branches(&repo.workspace, &repo.slug, limit)
        .await?;
    spinner.finish();

    let branches: Vec<BranchOut> = values
        .iter()
        .map(|b| BranchOut {
            name: b.name.clone(),
        })
        .collect();

    let fmt = make_formatter(g);
    let mut table = Table::new().headers(["Branch"]);
    for b in &branches {
        table = table.add_row([b.name.clone()]);
    }
    let human = table_or_empty(branches.len(), "No branches found.", table.render());
    fmt.print(&branches, &human)
}

pub async fn list_tags(g: &GlobalArgs, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching tags...");
    let values = client.list_tags(&repo.workspace, &repo.slug, limit).await?;
    spinner.finish();

    let tags: Vec<TagOut> = values
        .iter()
        .map(|t| TagOut {
            name: t.name.clone(),
            target: t.target.as_ref().map(|c| c.hash.clone()),
            date: t.date.clone(),
        })
        .collect();

    let fmt = make_formatter(g);
    let mut table = Table::new().headers(["Tag", "Target", "Date"]);
    for t in &tags {
        table = table.add_row([
            t.name.clone(),
            t.target
                .as_deref()
                .map(|hash| truncate(hash, 12))
                .unwrap_or_else(|| "-".into()),
            t.date.clone().unwrap_or_else(|| "-".into()),
        ]);
    }
    let human = table_or_empty(tags.len(), "No tags found.", table.render());
    fmt.print(&tags, &human)
}

pub async fn list_commits(g: &GlobalArgs, branch: Option<&str>, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching commits...");
    let commits: Vec<CommitOut> = client
        .list_commits(&repo.workspace, &repo.slug, branch, limit)
        .await?
        .into_iter()
        .map(|c| CommitOut {
            hash: c.hash,
            message: first_line(&c.message),
            author: c.author.map(|a| a.raw),
            date: c.date,
        })
        .collect();
    spinner.finish();

    let fmt = make_formatter(g);
    let mut table = Table::new().headers(["Hash", "Date", "Author", "Message"]);
    for c in &commits {
        table = table.add_row([
            truncate(&c.hash, 10),
            c.date.as_deref().unwrap_or("-").to_string(),
            c.author.as_deref().unwrap_or("-").to_string(),
            c.message.clone(),
        ]);
    }
    let human = table_or_empty(commits.len(), "No commits found.", table.render());
    fmt.print(&commits, &human)
}

pub async fn create(
    g: &GlobalArgs,
    slug: &str,
    is_private: bool,
    description: Option<&str>,
    language: Option<&str>,
    enable_issues: bool,
) -> Result<()> {
    let ws = resolve_repo(g)?.workspace;
    let client = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Creating repository...");
    let repo = client
        .create_repo(&ws, slug, is_private, description, language, enable_issues)
        .await?;
    spinner.finish();

    let out = RepoInfoOut {
        workspace: ws,
        slug: slug.to_string(),
        full_name: repo.full_name,
        scm: repo.scm,
        private: repo.is_private,
        language: repo.language,
        description: repo.description,
        web_url: repo.links.html.href,
    };

    let fmt = make_formatter(g);
    let human = format!(
        "Created repository {}/{} ({})\nprivate: {}\nlanguage: {}\nurl:       {}",
        out.workspace,
        out.slug,
        out.scm,
        out.private,
        out.language,
        out.web_url.as_deref().unwrap_or("-"),
    );
    fmt.print(&out, &human)
}

pub async fn delete(g: &GlobalArgs, slug: &str, yes: bool) -> Result<()> {
    let ws = resolve_repo(g)?.workspace;
    let client = client(g)?;

    if !yes
        && !crate::commands::confirm(&format!("Delete {ws}/{slug}? This is permanent. (y/n): "))
            .await?
    {
        return Ok(());
    }

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message(format!("Deleting {ws}/{slug}..."));
    client.delete_repo(&ws, slug).await?;
    spinner.finish();

    let out = serde_json::json!({"deleted": true, "workspace": ws, "slug": slug});
    let human = format!("Deleted {ws}/{slug}");
    make_formatter(g).print(&out, &human)
}

pub async fn fork(
    g: &GlobalArgs,
    slug: Option<&str>,
    name: Option<&str>,
    workspace: Option<&str>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let slug = slug.unwrap_or(&repo.slug);
    let client = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message(format!("Forking {slug}..."));
    let forked = client
        .fork_repo(&repo.workspace, slug, name, workspace)
        .await?;
    spinner.finish();

    let out = serde_json::json!({
        "workspace": forked.full_name.split('/').next().unwrap_or(""),
        "slug": forked.slug,
        "full_name": forked.full_name,
        "url": forked.links.html.href,
    });
    let human = format!(
        "Forked to {} ({})",
        forked.full_name,
        forked.links.html.href.as_deref().unwrap_or("-")
    );
    make_formatter(g).print(&out, &human)
}

pub async fn create_branch(g: &GlobalArgs, name: &str, from: Option<&str>) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let target_hash = match from {
        Some(h) => h.to_string(),
        None => crate::git::current_commit()?,
    };

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message(format!("Creating branch {name}..."));
    let branch = client
        .create_branch(&repo.workspace, &repo.slug, name, &target_hash)
        .await?;
    spinner.finish();

    let out = serde_json::json!({
        "name": branch.name,
        "target": branch.target.as_ref().map(|t| &t.hash),
    });
    let human = format!(
        "Created branch {} at {}",
        branch.name,
        branch
            .target
            .as_ref()
            .map(|t| t.hash.as_str())
            .unwrap_or("?")
    );
    make_formatter(g).print(&out, &human)
}

pub async fn create_tag(
    g: &GlobalArgs,
    name: &str,
    target: Option<&str>,
    message: Option<&str>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let target_hash = match target {
        Some(h) => h.to_string(),
        None => crate::git::current_commit()?,
    };

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message(format!("Creating tag {name}..."));
    let tag = client
        .create_tag(&repo.workspace, &repo.slug, name, &target_hash, message)
        .await?;
    spinner.finish();

    let out = serde_json::json!({
        "name": tag.name,
        "target": tag.target.as_ref().map(|t| &t.hash),
        "message": tag.message,
    });
    let human = format!(
        "Created tag {} at {}",
        tag.name,
        tag.target.as_ref().map(|t| t.hash.as_str()).unwrap_or("?")
    );
    make_formatter(g).print(&out, &human)
}

pub async fn permissions(g: &GlobalArgs) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching permissions...");
    let (users, groups) = tokio::join!(
        client.list_user_permissions(&repo.workspace, &repo.slug),
        client.list_group_permissions(&repo.workspace, &repo.slug),
    );
    let users = users.unwrap_or_default();
    let groups = groups.unwrap_or_default();
    spinner.finish();

    let out = serde_json::json!({
        "workspace": repo.workspace,
        "slug": repo.slug,
        "users": users,
        "groups": groups,
    });

    let theme = crate::output::theme::Theme::current();
    let mut human = String::new();
    human.push_str(&format!(
        "{} {}/{}\n",
        theme.bullet(),
        repo.workspace,
        repo.slug
    ));
    human.push_str(&format!("{}\n", theme.separator()));

    let mut table = Table::new().headers(["Type", "Name", "Permission"]);
    for u in &users {
        let name = u
            .display_name
            .as_deref()
            .or_else(|| u.user.as_ref().map(|u| u.display_name.as_str()))
            .unwrap_or("unknown");
        table = table.add_row(["User".into(), name.to_string(), u.permission.clone()]);
    }
    for g in &groups {
        let name = g
            .display_name
            .as_deref()
            .or_else(|| g.group.as_ref().and_then(|grp| grp.display_name.as_deref()))
            .unwrap_or("unknown");
        table = table.add_row(["Group".into(), name.to_string(), g.permission.clone()]);
    }
    human.push_str(&table.render());

    make_formatter(g).print(&out, &human)
}

pub async fn list_default_reviewers(g: &GlobalArgs) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching default reviewers...");
    let reviewers = client
        .list_default_reviewers(&repo.workspace, &repo.slug)
        .await?;
    spinner.finish();

    let out: Vec<serde_json::Value> = reviewers
        .iter()
        .map(|r| {
            serde_json::json!({
                "display_name": r.user.as_ref().map(|u| &u.display_name),
                "uuid": r.user.as_ref().and_then(|u| u.uuid.as_ref()),
                "nickname": r.user.as_ref().and_then(|u| u.nickname.as_ref()),
            })
        })
        .collect();

    let mut table = Table::new().headers(["Name", "Nickname", "UUID"]);
    for r in &reviewers {
        let u = r.user.as_ref();
        table = table.add_row([
            u.map(|u| u.display_name.as_str())
                .unwrap_or("-")
                .to_string(),
            u.and_then(|u| u.nickname.as_deref())
                .unwrap_or("-")
                .to_string(),
            u.and_then(|u| u.uuid.as_deref())
                .map(|id| truncate(id, 40))
                .unwrap_or_else(|| "-".into()),
        ]);
    }
    let human = table_or_empty(
        out.len(),
        "No default reviewers configured.",
        table.render(),
    );
    make_formatter(g).print(&out, &human)
}

pub async fn add_default_reviewer(g: &GlobalArgs, user: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Resolving user...");
    let uuid = client.resolve_user_uuid(user).await?;
    spinner.set_message("Adding default reviewer...");
    client
        .add_default_reviewer(&repo.workspace, &repo.slug, &uuid)
        .await?;
    spinner.finish();
    let out = serde_json::json!({
        "action": "added",
        "user": user,
        "uuid": uuid,
    });
    make_formatter(g).print(&out, &format!("Added {user} as a default reviewer"))
}

pub async fn remove_default_reviewer(g: &GlobalArgs, user: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Resolving user...");
    let uuid = client.resolve_user_uuid(user).await?;
    spinner.set_message("Removing default reviewer...");
    client
        .remove_default_reviewer(&repo.workspace, &repo.slug, &uuid)
        .await?;
    spinner.finish();
    let out = serde_json::json!({
        "action": "removed",
        "user": user,
        "uuid": uuid,
    });
    make_formatter(g).print(&out, &format!("Removed {user} from default reviewers"))
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_uses_scm_field_not_scim() {
        let out = RepoInfoOut {
            workspace: "w".into(),
            slug: "s".into(),
            full_name: "w/s".into(),
            scm: "git".into(),
            private: true,
            language: "rust".into(),
            description: None,
            web_url: None,
        };
        let json = serde_json::to_value(out).unwrap();
        assert_eq!(json.get("scm").and_then(|v| v.as_str()), Some("git"));
        assert!(json.get("scim").is_none());
    }

    #[test]
    fn branch_out_serializes() {
        let out = BranchOut {
            name: "feature".into(),
        };
        let json = serde_json::to_value(out).unwrap();
        assert_eq!(json["name"], "feature");
    }

    #[test]
    fn tag_out_serializes() {
        let out = TagOut {
            name: "v1.0".into(),
            target: Some("abc123".into()),
            date: Some("2024-01-01".into()),
        };
        let json = serde_json::to_value(out).unwrap();
        assert_eq!(json["name"], "v1.0");
        assert_eq!(json["target"], "abc123");
    }

    #[test]
    fn tag_out_serializes_with_null_optionals() {
        let out = TagOut {
            name: "v1.0".into(),
            target: None,
            date: None,
        };
        let json = serde_json::to_value(out).unwrap();
        assert!(json.get("target").unwrap().is_null());
        assert!(json.get("date").unwrap().is_null());
    }

    #[test]
    fn commit_out_serializes() {
        let out = CommitOut {
            hash: "abc123".into(),
            message: "fix bug".into(),
            author: Some("Alice".into()),
            date: Some("2024-01-01".into()),
        };
        let json = serde_json::to_value(out).unwrap();
        assert_eq!(json["hash"], "abc123");
        assert_eq!(json["message"], "fix bug");
    }

    #[test]
    fn first_line_extracts_first_line() {
        assert_eq!(first_line("hello\nworld"), "hello");
    }

    #[test]
    fn first_line_handles_empty() {
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn first_line_returns_full_if_single_line() {
        assert_eq!(first_line("hello world"), "hello world");
    }
}
