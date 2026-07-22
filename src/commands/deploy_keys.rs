//! `bbr deploy-keys` — repository deploy key management.
use crate::cli::GlobalArgs;
use crate::commands::{
    client, confirm, make_formatter, make_spinner, resolve_repo, table_or_empty, truncate,
    SpinnerGuard,
};
use crate::error::Result;
use crate::output::table::Table;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DeployKeyOut {
    pub id: u64,
    pub key: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<String>,
}

pub async fn list(g: &GlobalArgs) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching deploy keys...");
    let keys = client.list_deploy_keys(&repo.workspace, &repo.slug).await?;
    spinner.finish();

    let out: Vec<DeployKeyOut> = keys
        .iter()
        .map(|k| DeployKeyOut {
            id: k.id,
            key: k.key.clone(),
            label: k.label.clone(),
            comment: k.comment.clone(),
            created_on: k.created_on.clone(),
            last_used: k.last_used.clone(),
        })
        .collect();

    let fmt = make_formatter(g);
    let mut table = Table::new().headers(["ID", "Label", "Key", "Created"]);
    for k in &keys {
        table = table.add_row([
            k.id.to_string(),
            k.label.clone(),
            truncate(&k.key, 40),
            k.created_on.clone().unwrap_or_default(),
        ]);
    }
    let human = table_or_empty(keys.len(), "No deploy keys configured.", table.render());
    fmt.print(&out, &human)
}

pub async fn add(g: &GlobalArgs, key: &str, label: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Adding deploy key...");
    let dk = client
        .add_deploy_key(&repo.workspace, &repo.slug, key, label)
        .await?;
    spinner.finish();

    let out = DeployKeyOut {
        id: dk.id,
        key: dk.key.clone(),
        label: dk.label.clone(),
        comment: dk.comment.clone(),
        created_on: dk.created_on.clone(),
        last_used: dk.last_used.clone(),
    };
    let fmt = make_formatter(g);
    let human = format!(
        "Added deploy key #{}\n  Label: {}\n  Key:   {}",
        dk.id,
        dk.label,
        truncate(&dk.key, 60)
    );
    fmt.print(&out, &human)
}

pub async fn view(g: &GlobalArgs, key_id: u64) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching deploy key...");
    let dk = client
        .get_deploy_key(&repo.workspace, &repo.slug, key_id)
        .await?;
    spinner.finish();

    let out = DeployKeyOut {
        id: dk.id,
        key: dk.key.clone(),
        label: dk.label.clone(),
        comment: dk.comment.clone(),
        created_on: dk.created_on.clone(),
        last_used: dk.last_used.clone(),
    };
    let fmt = make_formatter(g);
    let human = format!(
        "Deploy Key #{}\n  Label:   {}\n  Key:     {}\n  Comment: {}\n  Created: {}\n  Used:    {}",
        dk.id,
        dk.label,
        dk.key,
        dk.comment.as_deref().unwrap_or("-"),
        dk.created_on.as_deref().unwrap_or("-"),
        dk.last_used.as_deref().unwrap_or("never"),
    );
    fmt.print(&out, &human)
}

pub async fn delete(g: &GlobalArgs, key_id: u64, yes: bool) -> Result<()> {
    if !yes {
        let ok = confirm(&format!("Delete deploy key #{key_id}? [y/N] ")).await?;
        if !ok {
            return Ok(());
        }
    }
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Deleting deploy key...");
    client
        .delete_deploy_key(&repo.workspace, &repo.slug, key_id)
        .await?;
    spinner.finish();
    let fmt = make_formatter(g);
    let out = serde_json::json!({"deleted": key_id});
    fmt.print(&out, &format!("Deleted deploy key #{key_id}"))
}
