//! `bb ci` — status / watch / logs.

use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use tokio::time;

use crate::api::pipeline::{normalize_uuid, PipelineStep};
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

#[derive(Debug, Serialize)]
pub struct StepOut {
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
}

#[derive(Debug, Serialize)]
pub struct CiLogsOut {
    pub pipeline_uuid: String,
    pub step: Option<String>,
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

    let steps = client
        .list_steps(&repo.workspace, &repo.slug, &pipeline.uuid)
        .await
        .map(|p| p.values.iter().map(step_out).collect::<Vec<_>>())
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

pub async fn watch(g: &GlobalArgs, branch: Option<&str>, interval_secs: u64) -> Result<()> {
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

    // Fetch final steps for a summary line.
    let steps = client
        .list_steps(&repo.workspace, &repo.slug, &uuid)
        .await
        .map(|p| p.values.iter().map(step_out).collect::<Vec<_>>())
        .unwrap_or_default();

    let final_state = current.state_name().to_string();
    let success = final_state.eq_ignore_ascii_case("SUCCESSFUL");

    let out = CiWatchOut {
        uuid: uuid.clone(),
        final_state: final_state.clone(),
        duration_seconds: current.duration_in_seconds,
        success,
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
    fmt.print(&out, &human)?;

    if !success {
        return Err(BitbucketError::PipelineFailed);
    }
    Ok(())
}

pub async fn logs(g: &GlobalArgs, uuid: &str, step: Option<&str>) -> Result<()> {
    let repo = current_repo()?;
    let client = client(g)?;
    let uuid = normalize_uuid(uuid);

    // If no step given, fetch the first step and dump its log.
    let step_uuid = match step {
        Some(s) => normalize_uuid(s),
        None => {
            let page = client
                .list_steps(&repo.workspace, &repo.slug, &uuid)
                .await?;
            page.values
                .into_iter()
                .next()
                .map(|s| s.uuid)
                .ok_or_else(|| BitbucketError::NotFound("no steps for pipeline".into()))?
        }
    };

    let log = client
        .step_log(&repo.workspace, &repo.slug, &uuid, &step_uuid)
        .await?;

    let out = CiLogsOut {
        pipeline_uuid: uuid.clone(),
        step: Some(step_uuid.clone()),
        log: log.text.clone(),
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = log.text;
    fmt.print(&out, &human)
}

// ---- helpers --------------------------------------------------------------

fn step_out(s: &PipelineStep) -> StepOut {
    StepOut {
        name: s.name.clone(),
        state: s.state.name.clone(),
        duration_seconds: s.duration_in_seconds,
    }
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
