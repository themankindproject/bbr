//! `bbr ci vars` — pipeline variable management.
use crate::cli::GlobalArgs;
use crate::commands::{
    client, make_formatter, make_spinner, resolve_repo, table_or_empty, SpinnerGuard,
};
use crate::error::{BitbucketError, Result};
use crate::output::table::Table;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CiVarOut {
    pub uuid: String,
    pub key: String,
    pub secured: bool,
    pub value: Option<String>,
}

pub async fn list(g: &GlobalArgs) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching pipeline variables...");
    let vars = api
        .list_pipeline_variables(&repo.workspace, &repo.slug)
        .await?;
    spinner.finish();

    let out: Vec<CiVarOut> = vars
        .into_iter()
        .map(|v| CiVarOut {
            uuid: v.uuid,
            key: v.key,
            secured: v.secured,
            value: v.value,
        })
        .collect();

    let fmt = make_formatter(g);
    let mut table = Table::new().headers(["Key", "Secured", "Value"]);
    for v in &out {
        let display_value = if v.secured {
            "***".to_string()
        } else {
            v.value.as_deref().unwrap_or("-").to_string()
        };
        table = table.add_row([v.key.clone(), v.secured.to_string(), display_value]);
    }
    let human = table_or_empty(out.len(), "No pipeline variables found.", table.render());
    fmt.print(&out, &human)
}

pub async fn set(g: &GlobalArgs, key: &str, value: &str, secured: bool) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Checking existing variables...");
    let vars = api
        .list_pipeline_variables(&repo.workspace, &repo.slug)
        .await?;
    spinner.finish();

    let fmt = make_formatter(g);
    if let Some(existing) = vars.iter().find(|v| v.key == key) {
        let spinner2 = SpinnerGuard::new(make_spinner(g.json, g.quiet));
        spinner2.set_message(format!("Updating {key}..."));
        api.update_pipeline_variable(
            &repo.workspace,
            &repo.slug,
            &existing.uuid,
            key,
            value,
            secured,
        )
        .await?;
        spinner2.finish();
        let out = serde_json::json!({"action": "updated", "key": key});
        let human = format!("Updated {key}");
        fmt.print(&out, &human)?;
    } else {
        let spinner2 = SpinnerGuard::new(make_spinner(g.json, g.quiet));
        spinner2.set_message(format!("Creating {key}..."));
        api.create_pipeline_variable(&repo.workspace, &repo.slug, key, value, secured)
            .await?;
        spinner2.finish();
        let out = serde_json::json!({"action": "created", "key": key});
        let human = format!("Created {key}");
        fmt.print(&out, &human)?;
    }

    Ok(())
}

pub async fn delete(g: &GlobalArgs, key: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching variables...");
    let vars = api
        .list_pipeline_variables(&repo.workspace, &repo.slug)
        .await?;
    spinner.finish();

    let var = vars
        .into_iter()
        .find(|v| v.key == key)
        .ok_or_else(|| BitbucketError::Other(format!("variable '{}' not found", key)))?;

    let spinner2 = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner2.set_message(format!("Deleting {key}..."));
    api.delete_pipeline_variable(&repo.workspace, &repo.slug, &var.uuid)
        .await?;
    spinner2.finish();

    let fmt = make_formatter(g);
    let out = serde_json::json!({"action": "deleted", "key": key});
    let human = format!("Deleted {key}");
    fmt.print(&out, &human)
}
