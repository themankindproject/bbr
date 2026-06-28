//! `bbr ci vars` — pipeline variable management.
use serde::Serialize;
use crate::cli::GlobalArgs;
use crate::commands::{client, make_spinner, resolve_repo};
use crate::error::{BitbucketError, Result};
use crate::output::table::Table;
use crate::output::Formatter;

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

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching pipeline variables...");
    let vars = api.list_pipeline_variables(&repo.workspace, &repo.slug).await?;
    spinner.finish_and_clear();

    let out: Vec<CiVarOut> = vars
        .into_iter()
        .map(|v| CiVarOut {
            uuid: v.uuid,
            key: v.key,
            secured: v.secured,
            value: v.value,
        })
        .collect();

    let fmt = Formatter::from_json_flag(g.json);
    let mut table = Table::new().headers(["Key", "Secured", "Value"]);
    for v in &out {
        let display_value = if v.secured {
            "***".to_string()
        } else {
            v.value.as_deref().unwrap_or("-").to_string()
        };
        table = table.add_row([
            v.key.clone(),
            v.secured.to_string(),
            display_value,
        ]);
    }
    let human = table.render();
    fmt.print(&out, &human)
}

pub async fn set(g: &GlobalArgs, key: &str, value: &str, secured: bool) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Checking existing variables...");
    let vars = api.list_pipeline_variables(&repo.workspace, &repo.slug).await?;
    spinner.finish_and_clear();

    if let Some(existing) = vars.iter().find(|v| v.key == key) {
        let spinner2 = make_spinner(g.json);
        spinner2.set_message(format!("Updating {key}..."));
        api.update_pipeline_variable(&repo.workspace, &repo.slug, &existing.uuid, key, value, secured).await?;
        spinner2.finish_and_clear();
        if !g.json {
            println!("Updated {key}");
        }
    } else {
        let spinner2 = make_spinner(g.json);
        spinner2.set_message(format!("Creating {key}..."));
        api.create_pipeline_variable(&repo.workspace, &repo.slug, key, value, secured).await?;
        spinner2.finish_and_clear();
        if !g.json {
            println!("Created {key}");
        }
    }

    Ok(())
}

pub async fn delete(g: &GlobalArgs, key: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching variables...");
    let vars = api.list_pipeline_variables(&repo.workspace, &repo.slug).await?;
    spinner.finish_and_clear();

    let var = vars
        .into_iter()
        .find(|v| v.key == key)
        .ok_or_else(|| BitbucketError::Other(format!("variable '{}' not found", key)))?;

    let spinner2 = make_spinner(g.json);
    spinner2.set_message(format!("Deleting {key}..."));
    api.delete_pipeline_variable(&repo.workspace, &repo.slug, &var.uuid).await?;
    spinner2.finish_and_clear();

    if !g.json {
        println!("Deleted {key}");
    }

    Ok(())
}
