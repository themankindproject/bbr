//! `bb ci` — status / watch / logs.

use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use tokio::time;

use crate::api::pipeline::{normalize_uuid, PipelineStep};
use crate::api::BitbucketClient;
use crate::cli::GlobalArgs;
use crate::commands::{client, current_repo};
use crate::error::{BitbucketError, Result};
use crate::git;
use crate::output::theme::Theme;
use crate::output::Formatter;

#[derive(Debug, Serialize)]
pub struct CiStatusOut {
    pub branch: String,
    pub pipeline: Option<PipelineOut>,
}

#[derive(Debug, Serialize)]
pub struct PipelineOut {
    pub uuid: String,
    pub build_number: u64,
    pub state: String,
    pub duration_seconds: u64,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub steps: Vec<StepOut>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepOut {
    pub uuid: String,
    pub name: String,
    pub state: String,
    pub duration_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct CiWatchOut {
    pub uuid: String,
    pub final_state: String,
    pub duration_seconds: u64,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failing_step: Option<StepOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_log: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CiLogsOut {
    pub pipeline_uuid: String,
    pub step: Option<String>,
    pub step_name: Option<String>,
    pub log: String,
}

pub async fn status(g: &GlobalArgs, branch: Option<&str>) -> Result<()> {
    let repo = current_repo()?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => git::current_branch()?,
    };
    let client = client(g)?;

    let pipeline = client
        .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
        .await?
        .ok_or_else(|| BitbucketError::NotFound(format!("no pipeline for branch '{branch}'")))?;

    let steps = steps_for_pipeline(&client, &repo.workspace, &repo.slug, &pipeline.uuid)
        .await
        .map(|steps| steps.iter().map(step_out).collect::<Vec<_>>())
        .unwrap_or_default();

    let out = CiStatusOut {
        branch: branch.clone(),
        pipeline: Some(PipelineOut {
            uuid: pipeline.uuid.clone(),
            build_number: pipeline.build_number,
            state: pipeline.state_name().to_string(),
            duration_seconds: pipeline.duration_in_seconds,
            branch: pipeline.target.ref_.as_ref().map(|r| r.name.clone()),
            commit: pipeline
                .target
                .ref_
                .as_ref()
                .and_then(|r| r.target.as_ref())
                .map(|t| t.hash.clone()),
            steps,
        }),
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = render_status(&out);
    fmt.print(&out, &human)
}

pub async fn watch(
    g: &GlobalArgs,
    branch: Option<&str>,
    interval_secs: u64,
    include_logs: bool,
) -> Result<()> {
    let repo = current_repo()?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => git::current_branch()?,
    };
    let client = client(g)?;

    let initial = client
        .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
        .await?
        .ok_or_else(|| BitbucketError::NotFound(format!("no pipeline for branch '{branch}'")))?;

    let uuid = initial.uuid.clone();
    let theme = Theme::current();

    let spinner = if g.json {
        ProgressBar::hidden()
    } else {
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(120));
        pb.set_style(
            ProgressStyle::with_template("{spinner} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb
    };
    spinner.println(format!("Watching pipeline {uuid} on {branch}..."));

    let mut current = initial;
    loop {
        if current.is_terminal() {
            break;
        }
        spinner.set_message(format!("state: {}", current.state_name()));
        time::sleep(Duration::from_secs(interval_secs.max(1))).await;
        current = client
            .get_pipeline(&repo.workspace, &repo.slug, &uuid)
            .await?;
    }
    spinner.finish_and_clear();

    let raw_steps = steps_for_pipeline(&client, &repo.workspace, &repo.slug, &uuid)
        .await
        .unwrap_or_default();
    let steps = raw_steps.iter().map(step_out).collect::<Vec<_>>();

    let final_state = current.state_name().to_string();
    let success = final_state.eq_ignore_ascii_case("SUCCESSFUL");
    let failing_step = raw_steps.iter().find(|s| s.is_failed());
    let failure_log = if !success && include_logs {
        let step = failing_step
            .or_else(|| raw_steps.last())
            .ok_or_else(|| BitbucketError::NotFound("no steps for pipeline".into()))?;
        Some(
            client
                .step_log(&repo.workspace, &repo.slug, &uuid, &step.uuid)
                .await?
                .text,
        )
    } else {
        None
    };

    let out = CiWatchOut {
        uuid: uuid.clone(),
        final_state: final_state.clone(),
        duration_seconds: current.duration_in_seconds,
        success,
        failing_step: failing_step.map(step_out),
        failure_log: failure_log.clone(),
    };

    let fmt = Formatter::from_json_flag(g.json);
    let mut human = format!(
        "Pipeline {} in {}s",
        theme.status_glyph(&final_state),
        out.duration_seconds
    );
    for s in &steps {
        human.push_str(&format!(
            "\n  {} {:<18}  {}s",
            theme.status_glyph(&s.state),
            s.name,
            s.duration_seconds
        ));
    }
    if let Some(log) = failure_log {
        if let Some(step) = &out.failing_step {
            human.push_str(&format!("\n\nFailing step: {}", step.name));
        }
        human.push_str("\n\n--- last 120 log lines ---\n");
        human.push_str(&last_lines(&log, 120));
    }
    fmt.print(&out, &human)?;

    if !success {
        return Err(BitbucketError::PipelineFailed);
    }
    Ok(())
}

pub async fn logs(
    g: &GlobalArgs,
    uuid: Option<&str>,
    step: Option<&str>,
    failed: bool,
    latest: bool,
) -> Result<()> {
    let repo = current_repo()?;
    let client = client(g)?;
    let (uuid, smart_default) = match uuid {
        Some(uuid) => (normalize_uuid(uuid), false),
        None => {
            let branch = git::current_branch()?;
            let pipeline = client
                .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
                .await?
                .ok_or_else(|| {
                    BitbucketError::NotFound(format!("no pipeline for branch '{branch}'"))
                })?;
            (normalize_uuid(&pipeline.uuid), true)
        }
    };

    let steps = steps_for_pipeline(&client, &repo.workspace, &repo.slug, &uuid).await?;
    let selected = select_step(&steps, step, failed, latest, smart_default)?;
    let log = client
        .step_log(&repo.workspace, &repo.slug, &uuid, &selected.uuid)
        .await?;

    let out = CiLogsOut {
        pipeline_uuid: uuid.clone(),
        step: Some(selected.uuid.clone()),
        step_name: Some(selected.name.clone()),
        log: log.text.clone(),
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = log.text;
    fmt.print(&out, &human)
}

async fn steps_for_pipeline(
    client: &BitbucketClient,
    workspace: &str,
    slug: &str,
    uuid: &str,
) -> Result<Vec<PipelineStep>> {
    client
        .list_steps(workspace, slug, uuid)
        .await
        .map(|page| page.values)
}

fn select_step<'a>(
    steps: &'a [PipelineStep],
    selector: Option<&str>,
    failed: bool,
    latest: bool,
    smart_default: bool,
) -> Result<&'a PipelineStep> {
    if steps.is_empty() {
        return Err(BitbucketError::NotFound("no steps for pipeline".into()));
    }
    if let Some(selector) = selector {
        let selector_uuid = normalize_uuid(selector);
        return steps
            .iter()
            .find(|s| {
                normalize_uuid(&s.uuid) == selector_uuid || s.name.eq_ignore_ascii_case(selector)
            })
            .ok_or_else(|| BitbucketError::NotFound(format!("no step matching '{selector}'")));
    }
    if failed || smart_default {
        if let Some(step) = steps.iter().find(|s| s.is_failed()) {
            return Ok(step);
        }
        if failed {
            return Err(BitbucketError::NotFound(
                "no failed step for pipeline".into(),
            ));
        }
    }
    if latest || smart_default {
        return steps
            .last()
            .ok_or_else(|| BitbucketError::NotFound("no steps for pipeline".into()));
    }
    steps
        .first()
        .ok_or_else(|| BitbucketError::NotFound("no steps for pipeline".into()))
}

fn step_out(s: &PipelineStep) -> StepOut {
    StepOut {
        uuid: s.uuid.clone(),
        name: s.name.clone(),
        state: s.state_name().to_string(),
        duration_seconds: s.duration_in_seconds,
    }
}

fn last_lines(s: &str, n: usize) -> String {
    let lines = s.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

fn render_status(out: &CiStatusOut) -> String {
    let theme = Theme::current();
    let mut s = format!("Branch: {}\n", theme.bold(&out.branch));
    match &out.pipeline {
        Some(p) => {
            s.push_str(&format!(
                "\nPipeline #{}  {}  ({}s)\n",
                p.build_number,
                theme.status_glyph(&p.state),
                p.duration_seconds
            ));
            s.push_str(&format!(
                "  Branch: {}  /  Commit: {}\n",
                p.branch.as_deref().unwrap_or("-"),
                p.commit.as_deref().unwrap_or("-")
            ));
            if !p.steps.is_empty() {
                s.push_str("  Steps:\n");
                for st in &p.steps {
                    s.push_str(&format!(
                        "    {} {:<18}  {}s\n",
                        theme.status_glyph(&st.state),
                        st.name,
                        st.duration_seconds
                    ));
                }
            }
        }
        None => s.push_str("\nNo pipeline found.\n"),
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::pipeline::{Named, PipelineState};

    fn step(uuid: &str, name: &str, state: &str) -> PipelineStep {
        PipelineStep {
            uuid: uuid.into(),
            name: name.into(),
            state: PipelineState {
                name: state.into(),
                stage: Some(Named { name: "x".into() }),
                result: None,
            },
            duration_in_seconds: 1,
            started_on: None,
            completed_on: None,
            setup_commands: None,
            commands: None,
            script_commands: None,
            links: Default::default(),
        }
    }

    #[test]
    fn selects_failed_step_first_for_smart_logs() {
        let steps = vec![
            step("{1}", "Build", "SUCCESSFUL"),
            step("{2}", "Test", "FAILED"),
        ];
        let selected = select_step(&steps, None, false, false, true).unwrap();
        assert_eq!(selected.name, "Test");
    }

    #[test]
    fn selector_matches_uuid_without_braces_or_name() {
        let steps = vec![step("{1}", "Build", "SUCCESSFUL")];
        assert_eq!(
            select_step(&steps, Some("1"), false, false, false)
                .unwrap()
                .name,
            "Build"
        );
        assert_eq!(
            select_step(&steps, Some("build"), false, false, false)
                .unwrap()
                .uuid,
            "{1}"
        );
    }
}
