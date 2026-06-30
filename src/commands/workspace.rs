//! Workspace operations (`bbr workspace`).

use crate::cli::GlobalArgs;
use crate::commands::{client, make_spinner};
use crate::error::Result;
use crate::output::theme::Theme;
use crate::output::Formatter;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceOut {
    pub slug: String,
    pub name: String,
    pub uuid: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceResponse {
    values: Vec<Workspace>,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    slug: String,
    name: String,
    uuid: String,
}

pub async fn list(g: &GlobalArgs, role: Option<&str>, limit: u32) -> Result<()> {
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching workspaces...");

    let mut path = format!("/workspaces?pagelen={limit}");
    if let Some(r) = role {
        path.push_str(&format!("&role={r}"));
    }

    let page: WorkspaceResponse = client.send(reqwest::Method::GET, &path, None).await?;

    spinner.finish_and_clear();

    let workspaces: Vec<WorkspaceOut> = page
        .values
        .into_iter()
        .map(|w| WorkspaceOut {
            slug: w.slug,
            name: w.name,
            uuid: w.uuid,
        })
        .collect();

    let out = serde_json::json!({
        "workspaces": workspaces,
    });

    let theme = Theme::current();
    let mut human = String::new();
    human.push_str(&format!("{}\n", theme.bold("Workspaces")));
    human.push_str(&format!("{}\n", theme.separator()));

    if workspaces.is_empty() {
        human.push_str("No workspaces found.\n");
    } else {
        for ws in &workspaces {
            human.push_str(&format!("  {:<20} {}\n", ws.slug, ws.name));
        }
    }

    Formatter::from_json_flag(g.json).print(&out, &human)
}
