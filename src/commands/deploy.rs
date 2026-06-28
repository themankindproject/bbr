//! `bbr deploy` — deployment and environment management.
use crate::cli::GlobalArgs;
use crate::commands::{client, make_spinner, resolve_repo, truncate};
use crate::error::{BitbucketError, Result};
use crate::output::table::Table;
use crate::output::Formatter;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DeploymentOut {
    pub uuid: String,
    pub environment: Option<String>,
    pub state: String,
    pub pipeline_build: Option<u64>,
    pub commit_hash: Option<String>,
    pub last_update: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EnvironmentOut {
    pub uuid: String,
    pub name: String,
    pub env_type: String,
    pub rank: u32,
}

#[derive(Debug, Serialize)]
pub struct EnvVarOut {
    pub uuid: String,
    pub key: String,
    pub secured: bool,
    pub value: Option<String>,
}

pub async fn list_deployments(g: &GlobalArgs, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching deployments...");
    let deployments = api
        .list_deployments(&repo.workspace, &repo.slug, limit)
        .await?;
    spinner.finish_and_clear();

    let out: Vec<DeploymentOut> = deployments
        .into_iter()
        .map(|d| DeploymentOut {
            uuid: d.uuid,
            environment: d.environment.map(|e| e.name),
            state: d.state.name,
            pipeline_build: d
                .deployable
                .as_ref()
                .and_then(|dep| dep.pipeline.as_ref())
                .map(|p| p.build_number),
            commit_hash: d
                .deployable
                .as_ref()
                .and_then(|dep| dep.commit.as_ref())
                .map(|c| c.hash.clone()),
            last_update: d
                .last_update_time
                .as_deref()
                .map(|s| s.chars().take(10).collect()),
        })
        .collect();

    let fmt = Formatter::from_json_flag(g.json);
    let mut table = Table::new().headers(["Environment", "State", "Build#", "Commit", "Date"]);
    for d in &out {
        table = table.add_row([
            d.environment.as_deref().unwrap_or("-").to_string(),
            d.state.clone(),
            d.pipeline_build
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into()),
            d.commit_hash
                .as_deref()
                .map(|h| truncate(h, 10))
                .unwrap_or_else(|| "-".into()),
            d.last_update.as_deref().unwrap_or("-").to_string(),
        ]);
    }
    let human = table.render();
    fmt.print(&out, &human)
}

pub async fn list_environments(g: &GlobalArgs) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching environments...");
    let mut envs = api.list_environments(&repo.workspace, &repo.slug).await?;
    spinner.finish_and_clear();

    // Sort by rank ascending
    envs.sort_by_key(|e| e.rank);

    let out: Vec<EnvironmentOut> = envs
        .into_iter()
        .map(|e| EnvironmentOut {
            uuid: e.uuid,
            name: e.name,
            env_type: e.environment_type.name,
            rank: e.rank,
        })
        .collect();

    let fmt = Formatter::from_json_flag(g.json);
    let mut table = Table::new().headers(["Name", "Type", "Rank", "UUID"]);
    for e in &out {
        table = table.add_row([
            e.name.clone(),
            e.env_type.clone(),
            e.rank.to_string(),
            e.uuid.clone(),
        ]);
    }
    let human = table.render();
    fmt.print(&out, &human)
}

pub async fn list_env_vars(g: &GlobalArgs, env_uuid: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching environment variables...");
    let vars = api
        .list_env_variables(&repo.workspace, &repo.slug, env_uuid)
        .await?;
    spinner.finish_and_clear();

    let out: Vec<EnvVarOut> = vars
        .into_iter()
        .map(|v| EnvVarOut {
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
        table = table.add_row([v.key.clone(), v.secured.to_string(), display_value]);
    }
    let human = table.render();
    fmt.print(&out, &human)
}

pub async fn set_env_var(
    g: &GlobalArgs,
    env_uuid: &str,
    key: &str,
    value: &str,
    secured: bool,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Checking existing variables...");
    let vars = api
        .list_env_variables(&repo.workspace, &repo.slug, env_uuid)
        .await?;
    spinner.finish_and_clear();

    if let Some(existing) = vars.iter().find(|v| v.key == key) {
        let spinner2 = make_spinner(g.json);
        spinner2.set_message(format!("Updating {key}..."));
        api.update_env_variable(
            &repo.workspace,
            &repo.slug,
            env_uuid,
            &existing.uuid,
            key,
            value,
            secured,
        )
        .await?;
        spinner2.finish_and_clear();
        if !g.json {
            println!("Updated {key}");
        }
    } else {
        let spinner2 = make_spinner(g.json);
        spinner2.set_message(format!("Creating {key}..."));
        api.create_env_variable(&repo.workspace, &repo.slug, env_uuid, key, value, secured)
            .await?;
        spinner2.finish_and_clear();
        if !g.json {
            println!("Created {key}");
        }
    }

    Ok(())
}

pub async fn delete_env_var(g: &GlobalArgs, env_uuid: &str, key: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching variables...");
    let vars = api
        .list_env_variables(&repo.workspace, &repo.slug, env_uuid)
        .await?;
    spinner.finish_and_clear();

    let var = vars
        .into_iter()
        .find(|v| v.key == key)
        .ok_or_else(|| BitbucketError::Other(format!("variable '{}' not found", key)))?;

    let spinner2 = make_spinner(g.json);
    spinner2.set_message(format!("Deleting {key}..."));
    api.delete_env_variable(&repo.workspace, &repo.slug, env_uuid, &var.uuid)
        .await?;
    spinner2.finish_and_clear();

    if !g.json {
        println!("Deleted {key}");
    }

    Ok(())
}
