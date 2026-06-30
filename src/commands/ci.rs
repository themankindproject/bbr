//! `bbr ci` — status / watch / logs.

use std::time::Duration;

use serde::Serialize;
use tokio::time;

use crate::api::pipeline::{
    ensure_uuid_braces, normalize_uuid, PipelineStep, StepSummary, TestCase, TestReport,
};
use crate::api::BitbucketClient;
use crate::cli::GlobalArgs;
use crate::commands::{client, confirm, current_head, human_duration, make_spinner, resolve_repo};
use crate::error::{BitbucketError, Result};
use crate::output::table::Table;
use crate::output::theme::Theme;
use crate::output::Formatter;

#[derive(Debug, Serialize)]
pub struct CiStatusOut {
    pub branch: String,
    pub pipeline: Option<PipelineOut>,
}

#[derive(Debug, Serialize)]
pub struct CiListOut {
    pub branch: String,
    pub pipelines: Vec<PipelineOut>,
}

#[derive(Debug, Serialize)]
pub struct PipelineOut {
    pub uuid: String,
    pub build_number: u64,
    pub state: String,
    pub duration_seconds: u64,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub steps: Vec<StepSummary>,
}

#[derive(Debug, Serialize)]
pub struct CiWatchOut {
    pub uuid: String,
    pub final_state: String,
    pub duration_seconds: u64,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failing_step: Option<StepSummary>,
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

pub async fn list(g: &GlobalArgs, branch: Option<&str>, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => current_head()?.branch,
    };
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching pipelines...");
    let pipelines = client
        .list_pipelines(&repo.workspace, &repo.slug, Some(&branch), limit)
        .await?;
    spinner.finish_and_clear();

    let fmt = Formatter::from_json_flag(g.json);
    if pipelines.is_empty() {
        let out = CiListOut {
            branch: branch.clone(),
            pipelines: Vec::new(),
        };
        let human = format!("No pipelines for branch '{branch}'.");
        return fmt.print(&out, &human);
    }

    let step_futures: Vec<_> = pipelines
        .iter()
        .map(|p| steps_for_pipeline(&client, &repo.workspace, &repo.slug, &p.uuid))
        .collect();
    let all_steps: Vec<Vec<PipelineStep>> = futures::future::join_all(step_futures)
        .await
        .into_iter()
        .map(|r| r.unwrap_or_default())
        .collect();

    let pips: Vec<PipelineOut> = pipelines
        .iter()
        .zip(all_steps.iter())
        .map(|(p, raw_steps)| PipelineOut {
            uuid: p.uuid.clone(),
            build_number: p.build_number,
            state: p.state_name().to_string(),
            duration_seconds: p.duration_in_seconds,
            branch: p.target.ref_name.clone(),
            commit: p.target.commit.as_ref().map(|c| c.hash.clone()),
            steps: raw_steps.iter().map(step_out).collect(),
        })
        .collect();

    let theme = Theme::current();
    let mut human = format!("Branch: {}\n", theme.bold(&branch));
    human.push_str(&format!("{}\n", theme.separator()));
    let mut table = Table::new().headers(["#", "State", "Step", "Duration"]);
    for p in &pips {
        let state_label = match p.state.to_ascii_uppercase().as_str() {
            "SUCCESSFUL" => theme.success(&p.state),
            "FAILED" => theme.error(&p.state),
            "IN_PROGRESS" | "PENDING" => theme.warn(&p.state),
            _ => theme.dim(&p.state),
        };
        if p.steps.is_empty() {
            table = table.add_row([
                format!("#{}", p.build_number),
                state_label.to_string(),
                theme.dim("-").into_owned(),
                human_duration(p.duration_seconds),
            ]);
        }
        for s in &p.steps {
            table = table.add_row([
                format!("#{}", p.build_number),
                state_label.to_string(),
                format!("{} {}", theme.status_glyph(&s.state), theme.bold(&s.name),),
                human_duration(s.duration_seconds),
            ]);
        }
    }
    human.push_str(&table.render());
    let out = CiListOut {
        branch,
        pipelines: pips,
    };
    fmt.print(&out, &human)
}

pub async fn status(g: &GlobalArgs, branch: Option<&str>) -> Result<()> {
    let repo = resolve_repo(g)?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => current_head()?.branch,
    };
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching pipeline...");
    let pipeline = client
        .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
        .await?
        .ok_or_else(|| BitbucketError::NotFound(format!("no pipeline for branch '{branch}'")))?;
    spinner.set_message("Fetching steps...");
    let steps = steps_for_pipeline(&client, &repo.workspace, &repo.slug, &pipeline.uuid)
        .await
        .map(|steps| steps.iter().map(step_out).collect::<Vec<_>>())
        .unwrap_or_default();
    spinner.finish_and_clear();

    let out = CiStatusOut {
        branch: branch.clone(),
        pipeline: Some(PipelineOut {
            uuid: pipeline.uuid.clone(),
            build_number: pipeline.build_number,
            state: pipeline.state_name().to_string(),
            duration_seconds: pipeline.duration_in_seconds,
            branch: pipeline.target.ref_name.clone(),
            commit: pipeline.target.commit.as_ref().map(|c| c.hash.clone()),
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
    let repo = resolve_repo(g)?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => current_head()?.branch,
    };
    let client = client(g)?;

    let initial = client
        .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
        .await?
        .ok_or_else(|| BitbucketError::NotFound(format!("no pipeline for branch '{branch}'")))?;

    let uuid = initial.uuid.clone();
    let theme = Theme::current();

    let spinner = make_spinner(g.json);
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
        "Pipeline {} in {}",
        theme.status_glyph(&final_state),
        human_duration(out.duration_seconds)
    );
    let max_width = steps
        .iter()
        .map(|s| s.name.chars().count())
        .max()
        .unwrap_or(0)
        .max(18);
    for s in &steps {
        human.push_str(&format!(
            "\n  {} {:<width$}  {}",
            theme.status_glyph(&s.state),
            s.name,
            human_duration(s.duration_seconds),
            width = max_width
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
        return Err(BitbucketError::PipelineFailed {
            build_number: Some(current.build_number),
            branch: Some(branch),
        });
    }
    Ok(())
}

pub async fn logs(
    g: &GlobalArgs,
    uuid: Option<&str>,
    step: Option<&str>,
    failed: bool,
    latest: bool,
    output: Option<&str>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let (uuid, smart_default) = match uuid {
        Some(uuid) => (ensure_uuid_braces(uuid), false),
        None => {
            let branch = current_head()?.branch;
            let pipeline = client
                .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
                .await?
                .ok_or_else(|| {
                    BitbucketError::NotFound(format!("no pipeline for branch '{branch}'"))
                })?;
            (pipeline.uuid, true)
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

    if let Some(path) = output {
        std::fs::write(path, &log.text)
            .map_err(|e| BitbucketError::Other(format!("writing {path}: {e}")))?;
        let fmt = Formatter::from_json_flag(g.json);
        let human = format!("Wrote {} bytes to {path}", log.text.len());
        return fmt.print(&out, &human);
    }

    let fmt = Formatter::from_json_flag(g.json);
    let human = log.text;
    fmt.print_paginated(&out, &human)
}

pub async fn steps(g: &GlobalArgs, uuid: Option<&str>) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let uuid = match uuid {
        Some(u) => ensure_uuid_braces(u),
        None => {
            let branch = current_head()?.branch;
            client
                .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
                .await?
                .ok_or_else(|| {
                    BitbucketError::NotFound(format!("no pipeline for branch '{branch}'"))
                })?
                .uuid
        }
    };

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching steps...");
    let raw = client
        .list_steps(&repo.workspace, &repo.slug, &uuid)
        .await?;
    spinner.finish_and_clear();

    #[derive(Debug, Serialize)]
    pub struct CiStepsOut {
        pub uuid: String,
        pub steps: Vec<StepSummary>,
    }

    let out = CiStepsOut {
        uuid: uuid.clone(),
        steps: raw.values.iter().map(step_out).collect(),
    };

    let fmt = Formatter::from_json_flag(g.json);
    let theme = Theme::current();
    let mut table = Table::new().headers(["Step", "State", "Duration"]);
    for (i, s) in raw.values.iter().enumerate() {
        table = table.add_row([
            format!("{}. {}", i + 1, s.name),
            theme.status_glyph(s.state_name()),
            human_duration(s.duration_in_seconds),
        ]);
    }
    fmt.print(&out, &table.render())
}

pub async fn tests(
    g: &GlobalArgs,
    uuid: Option<&str>,
    step: Option<&str>,
    limit: u32,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let pipeline_uuid = match uuid {
        Some(u) => ensure_uuid_braces(u),
        None => {
            let branch = current_head()?.branch;
            client
                .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
                .await?
                .ok_or_else(|| {
                    BitbucketError::NotFound(format!("no pipeline for branch '{branch}'"))
                })?
                .uuid
        }
    };

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching steps...");
    let steps = client
        .list_steps(&repo.workspace, &repo.slug, &pipeline_uuid)
        .await?;
    spinner.finish_and_clear();

    let selected = select_step(&steps.values, step, false, false, step.is_none())?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching test report...");
    let report = client
        .test_report(&repo.workspace, &repo.slug, &pipeline_uuid, &selected.uuid)
        .await?;
    spinner.set_message("Fetching test cases...");
    let cases = client
        .test_cases(
            &repo.workspace,
            &repo.slug,
            &pipeline_uuid,
            &selected.uuid,
            limit,
        )
        .await?;
    spinner.finish_and_clear();

    #[derive(Debug, Serialize)]
    pub struct CiTestsOut {
        pub pipeline_uuid: String,
        pub step_uuid: String,
        pub step_name: String,
        pub report: TestReport,
        pub test_cases: Vec<TestCase>,
    }

    let out = CiTestsOut {
        pipeline_uuid: pipeline_uuid.clone(),
        step_uuid: selected.uuid.clone(),
        step_name: selected.name.clone(),
        report,
        test_cases: cases,
    };

    let fmt = Formatter::from_json_flag(g.json);
    let theme = Theme::current();
    let mut human = format!(
        "Test report for {} / {}\n",
        theme.bold(&selected.name),
        theme.dim(&pipeline_uuid)
    );
    human.push_str(&format!("{}\n", theme.separator()));
    human.push_str(&format!(
        "  {}  {}  {}  {}  {}\n",
        theme.status_glyph("SUCCESSFUL"),
        theme.status_glyph("FAILED"),
        theme.status_glyph("SKIPPED"),
        theme.status_glyph("ERROR"),
        theme.dim("Total"),
    ));
    human.push_str(&format!(
        "  {:>4}      {:>4}      {:>4}      {:>4}    {:>4}\n",
        out.report.successful,
        out.report.failed,
        out.report.skipped,
        out.report.errors,
        out.report.total,
    ));

    if !out.test_cases.is_empty() {
        human.push_str(&format!("\n{}{}\n", theme.label("Test cases:"), ""));
        let mut table = Table::new().headers(["Status", "Name", "Duration"]);
        for case in &out.test_cases {
            let state = match case.status.to_uppercase().as_str() {
                "SUCCESS" | "SUCCESSFUL" | "PASSED" => theme.success(&case.status),
                "FAILED" | "ERROR" => theme.error(&case.status),
                "SKIPPED" => theme.dim(&case.status),
                _ => theme.warn(&case.status),
            };
            table = table.add_row([
                state.into_owned(),
                case.test_name
                    .as_deref()
                    .or(case.test_key.as_deref())
                    .unwrap_or("-")
                    .to_string(),
                case.duration_in_seconds
                    .map(|d| format!("{d:.2}s"))
                    .unwrap_or_else(|| "-".into()),
            ]);
        }
        human.push_str(&table.render());
    }
    fmt.print(&out, &human)
}

pub async fn stop(g: &GlobalArgs, uuid: Option<&str>, branch: Option<&str>) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let pipeline_uuid = match uuid {
        Some(u) => ensure_uuid_braces(u),
        None => {
            let branch = match branch {
                Some(b) => b.to_string(),
                None => current_head()?.branch,
            };
            client
                .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
                .await?
                .ok_or_else(|| {
                    BitbucketError::NotFound(format!("no pipeline for branch '{branch}'"))
                })?
                .uuid
        }
    };
    let spinner = make_spinner(g.json);
    spinner.set_message("Stopping pipeline...");
    client
        .stop_pipeline(&repo.workspace, &repo.slug, &pipeline_uuid)
        .await?;
    spinner.finish_and_clear();
    let fmt = Formatter::from_json_flag(g.json);
    fmt.print(
        &serde_json::json!({ "uuid": pipeline_uuid, "stopped": true }),
        &format!("Stopped pipeline {pipeline_uuid}"),
    )
}

pub async fn rerun(g: &GlobalArgs, branch: Option<&str>) -> Result<()> {
    let repo = resolve_repo(g)?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => current_head()?.branch,
    };
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching latest pipeline...");
    let pipeline = client
        .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
        .await?
        .ok_or_else(|| BitbucketError::NotFound(format!("no pipeline for branch '{branch}'")))?;
    spinner.finish_and_clear();

    if !g.json
        && !confirm(&format!(
            "Rerun pipeline #{} (current state: {}) for branch '{}'? [y/N] ",
            pipeline.build_number,
            pipeline.state_name(),
            branch,
        ))?
    {
        let fmt = Formatter::from_json_flag(g.json);
        fmt.print(&(), "Aborted.")?;
        return Ok(());
    }

    let spinner = make_spinner(g.json);
    spinner.set_message("Triggering rerun...");
    let new_pipeline = client
        .rerun_pipeline(&repo.workspace, &repo.slug, &pipeline.uuid)
        .await?;
    spinner.finish_and_clear();
    let out = CiStatusOut {
        branch: branch.clone(),
        pipeline: Some(PipelineOut {
            uuid: new_pipeline.uuid.clone(),
            build_number: new_pipeline.build_number,
            state: new_pipeline.state_name().to_string(),
            duration_seconds: new_pipeline.duration_in_seconds,
            branch: new_pipeline.target.ref_name.clone(),
            commit: new_pipeline.target.commit.as_ref().map(|c| c.hash.clone()),
            steps: Vec::new(),
        }),
    };
    let fmt = Formatter::from_json_flag(g.json);
    let human = format!("Reran pipeline #{}", new_pipeline.build_number);
    if !g.json {
        fmt.print(&out, &format!("{human}\nNext: bbr ci watch"))
    } else {
        fmt.print(&out, &human)
    }
}

pub async fn trigger(g: &GlobalArgs, branch: Option<&str>, vars: &[String], secured: &[String]) -> Result<()> {
    let repo = resolve_repo(g)?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => current_head()?.branch,
    };
    let client = client(g)?;

    // Parse --var KEY=VALUE pairs
    let variables: Vec<(String, String)> = vars
        .iter()
        .filter_map(|v| {
            let (key, value) = v.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect();

    // Build variables payload if any
    let variables_payload: Option<Vec<serde_json::Value>> = if variables.is_empty() {
        None
    } else {
        Some(
            variables
                .iter()
                .map(|(key, value)| {
                    let is_secured = secured.iter().any(|s| s == key);
                    serde_json::json!({
                        "key": key,
                        "value": value,
                        "secured": is_secured
                    })
                })
                .collect(),
        )
    };

    let spinner = make_spinner(g.json);
    spinner.set_message(format!("Triggering pipeline for '{branch}'..."));
    let pipeline = client
        .trigger_pipeline_with_variables(&repo.workspace, &repo.slug, &branch, variables_payload.as_deref())
        .await?;
    spinner.finish_and_clear();

    let out = CiStatusOut {
        branch: branch.clone(),
        pipeline: Some(PipelineOut {
            uuid: pipeline.uuid.clone(),
            build_number: pipeline.build_number,
            state: pipeline.state_name().to_string(),
            duration_seconds: pipeline.duration_in_seconds,
            branch: pipeline.target.ref_name.clone(),
            commit: pipeline.target.commit.as_ref().map(|c| c.hash.clone()),
            steps: Vec::new(),
        }),
    };
    let fmt = Formatter::from_json_flag(g.json);
    let human = format!(
        "Triggered pipeline #{} for '{}'",
        pipeline.build_number, branch
    );
    if !g.json {
        fmt.print(&out, &format!("{human}\nNext: bbr ci watch"))
    } else {
        fmt.print(&out, &human)
    }
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

fn step_out(s: &PipelineStep) -> StepSummary {
    StepSummary {
        uuid: s.uuid.clone(),
        name: s.name.clone(),
        state: s.state_name().to_string(),
        duration_seconds: s.duration_in_seconds,
    }
}

fn last_lines(s: &str, n: usize) -> String {
    let mut result: Vec<&str> = s.lines().rev().take(n).collect();
    result.reverse();
    result.join("\n")
}

fn render_status(out: &CiStatusOut) -> String {
    let theme = Theme::current();
    let mut s = format!("{}\n", theme.bold(&out.branch));
    s.push_str(&format!("{}\n", theme.separator()));
    match &out.pipeline {
        Some(p) => {
            s.push_str(&format!(
                "\n  {}  Pipeline #{}  {}  ({})\n",
                theme.bullet(),
                p.build_number,
                theme.status_glyph(&p.state),
                human_duration(p.duration_seconds)
            ));
            s.push_str(&format!(
                "  {}{}\n",
                theme.label("Branch:"),
                p.branch.as_deref().unwrap_or("-")
            ));
            s.push_str(&format!(
                "  {}{}\n",
                theme.label("Commit:"),
                p.commit.as_deref().unwrap_or("-")
            ));
            if !p.steps.is_empty() {
                let max_width = p
                    .steps
                    .iter()
                    .map(|s| s.name.chars().count())
                    .max()
                    .unwrap_or(0)
                    .max(18);
                s.push_str(&format!("  {}\n", theme.label("Steps:")));
                for st in &p.steps {
                    s.push_str(&format!(
                        "    {} {:<width$}  {}\n",
                        theme.status_glyph(&st.state),
                        st.name,
                        human_duration(st.duration_seconds),
                        width = max_width
                    ));
                }
            }
        }
        None => s.push_str(&format!("  {}\n", theme.dim("No pipeline found."))),
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

    #[test]
    fn select_step_returns_first_when_no_flags() {
        let steps = vec![
            step("{1}", "Build", "SUCCESSFUL"),
            step("{2}", "Test", "SUCCESSFUL"),
        ];
        let selected = select_step(&steps, None, false, false, false).unwrap();
        assert_eq!(selected.name, "Build");
    }

    #[test]
    fn select_step_returns_last_when_latest() {
        let steps = vec![
            step("{1}", "Build", "SUCCESSFUL"),
            step("{2}", "Test", "SUCCESSFUL"),
        ];
        let selected = select_step(&steps, None, false, true, false).unwrap();
        assert_eq!(selected.name, "Test");
    }

    #[test]
    fn select_step_failed_flag_errors_when_no_failed() {
        let steps = vec![step("{1}", "Build", "SUCCESSFUL")];
        let err = select_step(&steps, None, true, false, false).unwrap_err();
        assert!(format!("{err}").contains("no failed step"));
    }

    #[test]
    fn select_step_errors_on_empty() {
        let steps: Vec<PipelineStep> = vec![];
        let err = select_step(&steps, None, false, false, false).unwrap_err();
        assert!(format!("{err}").contains("no steps"));
    }

    #[test]
    fn select_step_errors_on_unknown_selector() {
        let steps = vec![step("{1}", "Build", "SUCCESSFUL")];
        let err = select_step(&steps, Some("nonexistent"), false, false, false).unwrap_err();
        assert!(format!("{err}").contains("no step matching"));
    }

    #[test]
    fn last_lines_returns_last_n_lines() {
        let s = "a\nb\nc\nd\ne";
        assert_eq!(last_lines(s, 3), "c\nd\ne");
    }

    #[test]
    fn last_lines_returns_all_when_fewer_lines_than_n() {
        let s = "a\nb";
        assert_eq!(last_lines(s, 5), "a\nb");
    }

    #[test]
    fn last_lines_handles_empty_string() {
        assert_eq!(last_lines("", 5), "");
    }

    #[test]
    fn last_lines_single_line() {
        assert_eq!(last_lines("hello", 1), "hello");
    }

    #[test]
    fn step_out_transforms_step() {
        let s = step("{uuid}", "Build", "SUCCESSFUL");
        let out = step_out(&s);
        assert_eq!(out.uuid, "{uuid}");
        assert_eq!(out.name, "Build");
        assert_eq!(out.state, "SUCCESSFUL");
        assert_eq!(out.duration_seconds, 1);
    }
}
