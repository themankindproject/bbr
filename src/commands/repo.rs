//! `bb repo info` — print the workspace/slug for the current directory.

use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::commands::{client, current_repo};
use crate::error::Result;
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
