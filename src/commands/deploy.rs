//! `bbr deploy` — deployment and environment management.
use std::time::Duration;

use crate::api::pipeline::ensure_uuid_braces;
use crate::cli::GlobalArgs;
use crate::commands::{
    client, confirm, make_formatter, make_spinner, resolve_repo, table_or_empty, truncate,
    SpinnerGuard,
};
use crate::error::{BitbucketError, Result};
use crate::output::table::Table;
use serde::Serialize;
use tokio::time;

/// Terminal deployment states (per the Bitbucket deployment state enum:
/// `IN_PROGRESS`, `SUCCESSFUL`, `FAILED`, `STOPPED`, `UNDEPLOYED`).
fn is_terminal_state(name: &str) -> bool {
    matches!(
        name.to_uppercase().as_str(),
        "SUCCESSFUL" | "FAILED" | "STOPPED" | "UNDEPLOYED"
    )
}

/// Extract (pipeline build, commit hash) from a deployment, reading from
/// `deployable` first and falling back to `release` (the API surfaces the
/// same data in one or the other depending on response version).
fn deployment_deployable(d: &crate::api::deploy::Deployment) -> (Option<u64>, Option<String>) {
    let deployable = d.deployable.as_ref();
    let release = d.release.as_ref();
    let build = deployable
        .and_then(|x| x.pipeline.as_ref())
        .map(|p| p.build_number);
    let commit = deployable
        .and_then(|x| x.commit.as_ref())
        .map(|c| c.hash.clone())
        .or_else(|| {
            release
                .and_then(|r| r.commit.as_ref())
                .map(|c| c.hash.clone())
        });
    (build, commit)
}

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

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching deployments...");
    let deployments = api
        .list_deployments(&repo.workspace, &repo.slug, limit)
        .await?;
    spinner.finish();

    let out: Vec<DeploymentOut> = deployments
        .into_iter()
        .map(|d| {
            let (pipeline_build, commit_hash) = deployment_deployable(&d);
            DeploymentOut {
                uuid: d.uuid,
                environment: d.environment.map(|e| e.name),
                state: d.state.name,
                pipeline_build,
                commit_hash,
                last_update: d
                    .last_update_time
                    .as_deref()
                    .map(|s| s.chars().take(10).collect()),
            }
        })
        .collect();

    let fmt = make_formatter(g);
    let mut table =
        Table::new().headers(["UUID", "Environment", "State", "Build#", "Commit", "Date"]);
    for d in &out {
        table = table.add_row([
            d.uuid.clone(),
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
    let human = table_or_empty(out.len(), "No deployments found.", table.render());
    fmt.print(&out, &human)
}

pub async fn list_environments(g: &GlobalArgs) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching environments...");
    let mut envs = api.list_environments(&repo.workspace, &repo.slug).await?;
    spinner.finish();

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

    let fmt = make_formatter(g);
    let mut table = Table::new().headers(["Name", "Type", "Rank", "UUID"]);
    for e in &out {
        table = table.add_row([
            e.name.clone(),
            e.env_type.clone(),
            e.rank.to_string(),
            e.uuid.clone(),
        ]);
    }
    let human = table_or_empty(out.len(), "No environments found.", table.render());
    fmt.print(&out, &human)
}

pub async fn create_environment(g: &GlobalArgs, name: &str, env_type: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message(format!("Creating environment '{name}'..."));
    let env = api
        .create_environment(&repo.workspace, &repo.slug, name, env_type)
        .await?;
    spinner.finish();

    let out = EnvironmentOut {
        uuid: env.uuid,
        name: env.name,
        env_type: env.environment_type.name,
        rank: env.rank,
    };
    let fmt = make_formatter(g);
    let human = format!(
        "Created environment '{}' (type: {}, rank: {})",
        out.name, out.env_type, out.rank
    );
    fmt.print(&out, &human)
}

pub async fn list_env_vars(g: &GlobalArgs, env_uuid: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching environment variables...");
    let vars = api
        .list_env_variables(&repo.workspace, &repo.slug, env_uuid)
        .await?;
    spinner.finish();

    let out: Vec<EnvVarOut> = vars
        .into_iter()
        .map(|v| EnvVarOut {
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
    let human = table_or_empty(out.len(), "No environment variables found.", table.render());
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

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Checking existing variables...");
    let vars = api
        .list_env_variables(&repo.workspace, &repo.slug, env_uuid)
        .await?;
    spinner.finish();

    let fmt = make_formatter(g);
    if let Some(existing) = vars.iter().find(|v| v.key == key) {
        let spinner2 = SpinnerGuard::new(make_spinner(g.json, g.quiet));
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
        spinner2.finish();
        let out = serde_json::json!({"action": "updated", "key": key});
        let human = format!("Updated {key}");
        fmt.print(&out, &human)?;
    } else {
        let spinner2 = SpinnerGuard::new(make_spinner(g.json, g.quiet));
        spinner2.set_message(format!("Creating {key}..."));
        api.create_env_variable(&repo.workspace, &repo.slug, env_uuid, key, value, secured)
            .await?;
        spinner2.finish();
        let out = serde_json::json!({"action": "created", "key": key});
        let human = format!("Created {key}");
        fmt.print(&out, &human)?;
    }

    Ok(())
}

pub async fn delete_env_var(g: &GlobalArgs, env_uuid: &str, key: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching variables...");
    let vars = api
        .list_env_variables(&repo.workspace, &repo.slug, env_uuid)
        .await?;
    spinner.finish();

    let var = vars
        .into_iter()
        .find(|v| v.key == key)
        .ok_or_else(|| BitbucketError::Other(format!("variable '{}' not found", key)))?;

    let spinner2 = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner2.set_message(format!("Deleting {key}..."));
    api.delete_env_variable(&repo.workspace, &repo.slug, env_uuid, &var.uuid)
        .await?;
    spinner2.finish();

    let fmt = make_formatter(g);
    let out = serde_json::json!({"action": "deleted", "key": key});
    let human = format!("Deleted {key}");
    fmt.print(&out, &human)
}

fn deployment_to_out(d: &crate::api::deploy::Deployment) -> DeploymentOut {
    let (pipeline_build, commit_hash) = deployment_deployable(d);
    DeploymentOut {
        uuid: d.uuid.clone(),
        environment: d.environment.as_ref().map(|e| e.name.clone()),
        state: d.state.name.clone(),
        pipeline_build,
        commit_hash,
        last_update: d
            .last_update_time
            .as_deref()
            .map(|s| s.chars().take(10).collect()),
    }
}

fn is_failed_state(state: &str) -> bool {
    state.eq_ignore_ascii_case("FAILED") || state.eq_ignore_ascii_case("STOPPED")
}

pub async fn trigger_deployment(
    g: &GlobalArgs,
    env_uuid: &str,
    commit: &str,
    wait: bool,
    interval: u64,
    timeout_secs: u64,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message(format!(
        "Triggering deployment to environment {env_uuid}..."
    ));
    let mut deployment = api
        .trigger_deployment(&repo.workspace, &repo.slug, env_uuid, commit)
        .await?;
    spinner.finish();

    let fmt = make_formatter(g);

    if wait {
        let final_state = wait_for_deployment(
            g,
            &api,
            &repo.workspace,
            &repo.slug,
            &deployment.uuid,
            interval,
            timeout_secs,
        )
        .await?;
        deployment = final_state;
        // Surface a failed deployment as exit 5 (mirrors `ci watch`).
        if is_failed_state(&deployment.state.name) {
            return Err(BitbucketError::DeployFailed {
                deployment: deployment.uuid.clone(),
                state: deployment.state.name.clone(),
                environment: deployment.environment.as_ref().map(|e| e.name.clone()),
            });
        }
    }

    let out = deployment_to_out(&deployment);
    let human = format!(
        "Triggered deployment {} to environment {} (commit: {})",
        out.uuid,
        out.environment.as_deref().unwrap_or("unknown"),
        commit
    );
    fmt.print(&out, &human)
}

/// Show one deployment; with `--wait`, poll it until terminal (or timeout).
pub async fn view_deployment(
    g: &GlobalArgs,
    deployment_uuid: &str,
    wait: bool,
    interval: u64,
    timeout_secs: u64,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let uuid = ensure_uuid_braces(deployment_uuid);

    let deployment = if wait {
        wait_for_deployment(
            g,
            &api,
            &repo.workspace,
            &repo.slug,
            &uuid,
            interval,
            timeout_secs,
        )
        .await?
    } else {
        let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
        spinner.set_message("Fetching deployment...");
        let d = api
            .get_deployment(&repo.workspace, &repo.slug, &uuid)
            .await?;
        spinner.finish();
        d
    };

    let fmt = make_formatter(g);
    let out = deployment_to_out(&deployment);
    let (pipeline_build, commit_hash) = deployment_deployable(&deployment);
    let release_name = deployment
        .release
        .as_ref()
        .and_then(|r| r.name.as_deref())
        .map(str::to_string);
    let release_url = deployment
        .release
        .as_ref()
        .and_then(|r| r.url.as_deref())
        .map(str::to_string);

    let json_out = serde_json::json!({
        "deployment": out,
        "pipeline_build": pipeline_build,
        "commit_hash": commit_hash,
        "release_name": deployment.release.as_ref().and_then(|r| r.name.clone()),
        "release_url": deployment.release.as_ref().and_then(|r| r.url.clone()),
        "state_url": deployment.state.url.clone(),
    });

    let mut human = format!("Deployment {} — {}", truncate(&out.uuid, 12), out.state);
    if let Some(env) = &out.environment {
        human.push_str(&format!("  [env: {env}]"));
    }
    if let Some(b) = pipeline_build {
        human.push_str(&format!("  build #{b}"));
    }
    if let Some(c) = &commit_hash {
        human.push_str(&format!("  commit {}", truncate(c, 10)));
    }
    if let Some(name) = release_name {
        human.push_str(&format!("  release {name}"));
    }
    if let Some(ts) = &out.last_update {
        human.push_str(&format!("  updated {ts}"));
    }
    if let Some(url) = &release_url {
        human.push_str(&format!("\n  {url}"));
    }

    if g.json {
        fmt.print(&json_out, &json_out.to_string())
    } else {
        fmt.print(&out, &human)
    }?;

    // Failed deployment with --wait: exit 5 (mirrors `ci watch`).
    if wait && is_failed_state(&deployment.state.name) {
        return Err(BitbucketError::DeployFailed {
            deployment: deployment.uuid.clone(),
            state: deployment.state.name.clone(),
            environment: deployment.environment.as_ref().map(|e| e.name.clone()),
        });
    }

    Ok(())
}

/// Roll back an environment to a previous deployment by re-deploying that
/// deployment's commit as a new change (Bitbucket has no atomic rollback).
///
/// `target`:
/// - `None` → the most recent *other* deployment (i.e., the one before the
///   current top of the history). This is the "revert last deploy" case.
/// - `Some(uuid)` → the specific deployment to restore.
pub async fn rollback_deployment(
    g: &GlobalArgs,
    env_uuid: &str,
    target: Option<&str>,
    wait: bool,
    interval: u64,
    timeout_secs: u64,
    yes: bool,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    // Pull the environment's deployment history (newest first).
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message(format!(
        "Fetching deployment history for environment {env_uuid}..."
    ));
    let history = api
        .list_deployments_for_environment(&repo.workspace, &repo.slug, env_uuid, 50)
        .await?;
    spinner.finish();

    if history.is_empty() {
        return Err(BitbucketError::NotFound(format!(
            "no deployments in environment {env_uuid}"
        )));
    }

    // Resolve the target deployment (the commit we'll re-deploy).
    let target_dep: crate::api::deploy::Deployment = match target {
        Some(t) => {
            let t_uuid = ensure_uuid_braces(t);
            history
                .iter()
                .find(|d| d.uuid == t_uuid || d.uuid == t)
                .cloned()
                .ok_or_else(|| {
                    BitbucketError::NotFound(format!(
                        "deployment {t} not in environment {env_uuid} history"
                    ))
                })?
        }
        None => {
            // Most recent *other* than the top. If there's only one
            // deployment, there's nothing to roll back to.
            if history.len() < 2 {
                return Err(BitbucketError::Other(format!(
                    "only one deployment in environment {env_uuid}; nothing to roll back to"
                )));
            }
            history[1].clone()
        }
    };

    let (target_build, target_commit) = deployment_deployable(&target_dep);
    let commit = target_commit.clone().ok_or_else(|| {
        BitbucketError::Other(format!(
            "deployment {} has no commit to re-deploy",
            truncate(&target_dep.uuid, 12)
        ))
    })?;

    // Confirm (destructive: kicks off a new deployment that replaces the
    // current one in this environment).
    if !yes
        && !g.json
        && !confirm(&format!(
            "Roll back environment {env_uuid} to deployment {} (commit {})? [y/N] ",
            truncate(&target_dep.uuid, 12),
            truncate(&commit, 10)
        ))
        .await?
    {
        let fmt = make_formatter(g);
        let human = "Aborted.".to_string();
        fmt.print(&(), &human)?;
        return Ok(());
    }

    let spinner2 = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner2.set_message(format!("Re-deploying commit {commit} as a new change..."));
    let new_dep = api
        .trigger_deployment(&repo.workspace, &repo.slug, env_uuid, &commit)
        .await?;
    spinner2.finish();

    let fmt = make_formatter(g);
    let out = deployment_to_out(&new_dep);
    let human = format!(
        "Rolled back {env_uuid} to commit {} (was build {}); new deployment {} {}",
        truncate(&commit, 10),
        target_build
            .map(|b| b.to_string())
            .unwrap_or_else(|| "-".into()),
        truncate(&out.uuid, 12),
        out.state
    );
    fmt.print(&out, &human)?;

    if wait {
        let final_state = wait_for_deployment(
            g,
            &api,
            &repo.workspace,
            &repo.slug,
            &new_dep.uuid,
            interval,
            timeout_secs,
        )
        .await?;
        if is_failed_state(&final_state.state.name) {
            return Err(BitbucketError::DeployFailed {
                deployment: final_state.uuid.clone(),
                state: final_state.state.name.clone(),
                environment: final_state.environment.as_ref().map(|e| e.name.clone()),
            });
        }
    }

    Ok(())
}

/// Poll a deployment until it reaches a terminal state or `timeout_secs`
/// elapses. Returns the last observed deployment. On timeout, returns the
/// last seen state without error — the caller prints a note and exits 0 so a
/// long deploy isn't mistaken for a failure.
async fn wait_for_deployment(
    g: &GlobalArgs,
    api: &crate::api::BitbucketClient,
    workspace: &str,
    slug: &str,
    deployment_uuid: &str,
    interval: u64,
    timeout_secs: u64,
) -> Result<crate::api::deploy::Deployment> {
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.println(format!(
        "Waiting for deployment {} (timeout {}s)...",
        truncate(deployment_uuid, 8),
        timeout_secs
    ));

    let started = std::time::Instant::now();
    let deadline = started + Duration::from_secs(timeout_secs.max(1));
    let mut current = api.get_deployment(workspace, slug, deployment_uuid).await?;
    while !is_terminal_state(&current.state.name) && std::time::Instant::now() < deadline {
        let elapsed = started.elapsed().as_secs();
        spinner.set_message(format!(
            "{} {} — {}s elapsed, {}s remaining",
            truncate(deployment_uuid, 8),
            current.state.name,
            elapsed,
            timeout_secs.saturating_sub(elapsed)
        ));
        time::sleep(Duration::from_secs(interval.max(1))).await;
        current = api.get_deployment(workspace, slug, deployment_uuid).await?;
    }
    spinner.finish();
    if !is_terminal_state(&current.state.name) {
        eprintln!(
            "note: deployment {} still {} after {}s; check `bbr deploy view` for status",
            truncate(deployment_uuid, 8),
            current.state.name,
            timeout_secs
        );
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::deploy::{
        DeployableCommit, DeployablePipeline, Deployment, DeploymentDeployable, DeploymentRelease,
        DeploymentState,
    };

    #[test]
    fn terminal_states_are_terminal() {
        assert!(is_terminal_state("SUCCESSFUL"));
        assert!(is_terminal_state("FAILED"));
        assert!(is_terminal_state("STOPPED"));
        assert!(is_terminal_state("UNDEPLOYED"));
        // Case-insensitive.
        assert!(is_terminal_state("successful"));
    }

    #[test]
    fn in_progress_is_not_terminal() {
        assert!(!is_terminal_state("IN_PROGRESS"));
        assert!(!is_terminal_state(""));
    }

    #[test]
    fn failed_states_cover_failed_and_stopped() {
        assert!(is_failed_state("FAILED"));
        assert!(is_failed_state("STOPPED"));
        assert!(is_failed_state("failed"));
        assert!(!is_failed_state("SUCCESSFUL"));
        assert!(!is_failed_state("IN_PROGRESS"));
    }

    #[test]
    fn deployment_deployable_reads_deployable_first() {
        let d = Deployment {
            deployable: Some(DeploymentDeployable {
                pipeline: Some(DeployablePipeline {
                    uuid: "{p}".into(),
                    build_number: 9,
                }),
                commit: Some(DeployableCommit { hash: "aaa".into() }),
            }),
            release: Some(DeploymentRelease {
                uuid: "{r}".into(),
                commit: Some(DeployableCommit { hash: "bbb".into() }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (build, commit) = deployment_deployable(&d);
        assert_eq!(build, Some(9));
        // deployable.commit wins over release.commit
        assert_eq!(commit.as_deref(), Some("aaa"));
    }

    #[test]
    fn deployment_deployable_falls_back_to_release() {
        let d = Deployment {
            release: Some(DeploymentRelease {
                uuid: "{r}".into(),
                commit: Some(DeployableCommit { hash: "bbb".into() }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (build, commit) = deployment_deployable(&d);
        assert_eq!(build, None);
        assert_eq!(commit.as_deref(), Some("bbb"));
    }

    #[test]
    fn deployment_deployable_empty() {
        let d = Deployment::default();
        let (build, commit) = deployment_deployable(&d);
        assert_eq!(build, None);
        assert_eq!(commit, None);
    }

    #[test]
    fn deployment_to_out_maps_fields() {
        let d = Deployment {
            uuid: "{d1234567890}".into(),
            state: DeploymentState {
                name: "IN_PROGRESS".into(),
                url: None,
            },
            environment: Some(crate::api::deploy::DeploymentEnvironment {
                uuid: "{e}".into(),
                name: "staging".into(),
                ..Default::default()
            }),
            deployable: Some(DeploymentDeployable {
                pipeline: Some(DeployablePipeline {
                    uuid: "{p}".into(),
                    build_number: 3,
                }),
                commit: Some(DeployableCommit {
                    hash: "abcdef1234567890".into(),
                }),
            }),
            release: None,
            last_update_time: Some("2026-08-30T10:00:00.000+00:00".into()),
        };
        let out = deployment_to_out(&d);
        assert_eq!(out.uuid, "{d1234567890}");
        assert_eq!(out.environment.as_deref(), Some("staging"));
        assert_eq!(out.state, "IN_PROGRESS");
        assert_eq!(out.pipeline_build, Some(3));
        assert_eq!(out.commit_hash.as_deref(), Some("abcdef1234567890"));
        assert_eq!(out.last_update.as_deref(), Some("2026-08-30"));
    }
}
