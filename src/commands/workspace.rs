//! Workspace operations (`bbr workspace`).

use crate::cli::GlobalArgs;
use crate::commands::{client, make_formatter, make_spinner, SpinnerGuard};
use crate::error::Result;
use crate::output::theme::Theme;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceOut {
    pub slug: String,
    pub name: String,
    pub uuid: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceMembership {
    workspace: Workspace,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    slug: String,
    name: Option<String>,
    uuid: String,
}

pub async fn list(g: &GlobalArgs, role: Option<&str>, limit: u32) -> Result<()> {
    let client = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching workspaces...");

    let mut path = format!("/user/workspaces?pagelen={}", limit.min(100));
    if let Some(r) = role {
        // `--role` is a closed set today; encode anyway so this filter can never
        // be broken out of by a future caller.
        path.push_str(&format!(
            "&q=permission%3D%22{}%22",
            crate::api::url_encode(r)
        ));
    }

    let memberships: Vec<WorkspaceMembership> =
        client.fetch_paginated(&path, limit as usize).await?;

    spinner.finish();

    let workspaces: Vec<WorkspaceOut> = memberships
        .into_iter()
        .map(|m| WorkspaceOut {
            slug: m.workspace.slug.clone(),
            name: m.workspace.name.unwrap_or_else(|| m.workspace.slug.clone()),
            uuid: m.workspace.uuid,
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

    make_formatter(g).print(&out, &human)
}
