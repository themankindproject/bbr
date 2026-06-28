//! `bb repo` — info, branches, commits.

use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::commands::{client, make_spinner, resolve_repo, truncate};
use crate::error::Result;
use crate::output::table::Table;
use crate::output::Formatter;

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

    let fmt = Formatter::from_json_flag(g.json);
    let mut human = format!(
        "workspace: {}\nslug:      {}\nfull name: {}\nscm:       {}\nprivate:   {}\nlanguage:  {}\nurl:       {}",
        out.workspace,
        out.slug,
        out.full_name,
        out.scm,
        out.private,
        out.language,
        out.web_url.as_deref().unwrap_or("-"),
    );
    if let Some(desc) = &out.description {
        human.push_str(&format!("\ndesc:      {desc}"));
    }
    fmt.print(&out, &human)
}

pub async fn list_branches(g: &GlobalArgs, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching branches...");
    let values = client
        .list_branches(&repo.workspace, &repo.slug, limit)
        .await?;
    spinner.finish_and_clear();

    let branches: Vec<BranchOut> = values
        .iter()
        .map(|b| BranchOut {
            name: b.name.clone(),
        })
        .collect();

    let fmt = Formatter::from_json_flag(g.json);
    let mut table = Table::new().headers(["Branch"]);
    for b in &branches {
        table = table.add_row([b.name.clone()]);
    }
    let human = table.render();
    fmt.print(&branches, &human)
}

pub async fn list_tags(g: &GlobalArgs, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching tags...");
    let values = client.list_tags(&repo.workspace, &repo.slug, limit).await?;
    spinner.finish_and_clear();

    let tags: Vec<TagOut> = values
        .iter()
        .map(|t| TagOut {
            name: t.name.clone(),
            target: t.target.as_ref().map(|c| c.hash.clone()),
            date: t.date.clone(),
        })
        .collect();

    let fmt = Formatter::from_json_flag(g.json);
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
    let human = table.render();
    fmt.print(&tags, &human)
}

pub async fn list_commits(g: &GlobalArgs, branch: Option<&str>, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let spinner = make_spinner(g.json);
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
    spinner.finish_and_clear();

    let fmt = Formatter::from_json_flag(g.json);
    let mut table = Table::new().headers(["Hash", "Date", "Message"]);
    for c in &commits {
        table = table.add_row([
            truncate(&c.hash, 10),
            c.date.as_deref().unwrap_or("-").to_string(),
            c.message.clone(),
        ]);
    }
    let human = table.render();
    fmt.print(&commits, &human)
}

pub async fn create(
    g: &GlobalArgs,
    slug: &str,
    is_private: bool,
    description: Option<&str>,
    language: Option<&str>,
) -> Result<()> {
    let ws = resolve_repo(g)?.workspace;
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Creating repository...");
    let repo = client
        .create_repo(&ws, slug, is_private, description, language)
        .await?;
    spinner.finish_and_clear();

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

    let fmt = Formatter::from_json_flag(g.json);
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
