//! `bb repo` — info, branches, commits.

use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::commands::{client, current_repo, make_spinner, truncate};
use crate::error::Result;
use crate::output::table::Table;
use crate::output::theme::Theme;
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
pub struct CommitOut {
    pub hash: String,
    pub message: String,
    pub author: Option<String>,
    pub date: Option<String>,
}

pub async fn info(g: &GlobalArgs) -> Result<()> {
    let repo = current_repo()?;
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
    let repo = current_repo()?;
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching branches...");
    let page = client
        .list_branches(&repo.workspace, &repo.slug, limit)
        .await?;
    spinner.finish_and_clear();

    let branches: Vec<BranchOut> = page
        .values
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

pub async fn list_commits(g: &GlobalArgs, branch: Option<&str>, limit: u32) -> Result<()> {
    let repo = current_repo()?;
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching commits...");
    let page = client
        .list_commits(&repo.workspace, &repo.slug, branch, limit)
        .await?;
    spinner.finish_and_clear();

    let commits: Vec<CommitOut> = page
        .values
        .iter()
        .map(|c| CommitOut {
            hash: c.hash.clone(),
            message: first_line(&c.message),
            author: c.author.as_ref().map(|a| a.raw.clone()),
            date: c.date.clone(),
        })
        .collect();

    let fmt = Formatter::from_json_flag(g.json);
    let theme = Theme::current();
    let mut human = format!("{}\n", theme.separator());
    for c in &commits {
        human.push_str(&format!(
            "  {}  {}  {}\n",
            truncate(&c.hash, 10),
            theme.dim(c.date.as_deref().unwrap_or("-")),
            c.message,
        ));
    }
    fmt.print(&commits, &human)
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
}
