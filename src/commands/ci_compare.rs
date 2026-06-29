//! Pipeline / CI comparison (`bbr ci compare`).

use crate::api::pipeline::{Pipeline, PipelineStep};
use crate::cli::GlobalArgs;
use crate::commands::{client, current_head, human_duration, make_spinner, resolve_repo};
use crate::error::{BitbucketError, Result};
use crate::output::theme::Theme;
use crate::output::Formatter;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CiCompareOut {
    pub a: ComparedPipeline,
    pub b: ComparedPipeline,
    pub step_deltas: Vec<StepDelta>,
    pub test_deltas: Option<TestDelta>,
}

#[derive(Debug, Serialize)]
pub struct ComparedPipeline {
    pub uuid: String,
    pub build_number: u64,
    pub state: String,
    pub duration_seconds: u64,
    pub branch: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct StepDelta {
    pub name: String,
    pub duration_a: Option<u64>,
    pub duration_b: Option<u64>,
    pub duration_delta: Option<i64>,
    pub state_a: String,
    pub state_b: String,
}

#[derive(Debug, Serialize)]
pub struct TestDelta {
    pub total_a: u64,
    pub total_b: u64,
    pub passed_a: u64,
    pub passed_b: u64,
    pub failed_a: u64,
    pub failed_b: u64,
    pub skipped_a: u64,
    pub skipped_b: u64,
    pub new_failures: Vec<String>,
    pub fixed: Vec<String>,
}

async fn resolve_pipeline_ref(
    client: &crate::api::BitbucketClient,
    workspace: &str,
    slug: &str,
    ref_str: &str,
    current_branch: &str,
) -> Result<Pipeline> {
    if ref_str.eq_ignore_ascii_case("last") || ref_str.eq_ignore_ascii_case("latest") {
        let p = client
            .latest_pipeline(workspace, slug, Some(current_branch))
            .await?;
        p.ok_or_else(|| {
            BitbucketError::Other(format!("No pipeline found on branch {current_branch}"))
        })
    } else if let Ok(build_num) = ref_str.parse::<u64>() {
        let pipelines = client.list_pipelines(workspace, slug, None, 100).await?;
        let p = pipelines.into_iter().find(|p| p.build_number == build_num);
        p.ok_or_else(|| {
            BitbucketError::Other(format!("No pipeline found with build number {build_num}"))
        })
    } else {
        // Treat as UUID
        let uuid = crate::api::pipeline::ensure_uuid_braces(ref_str);
        client.get_pipeline(workspace, slug, &uuid).await
    }
}

pub async fn compare(g: &GlobalArgs, a_ref: &str, b_ref: &str) -> Result<()> {
    let client = client(g)?;
    let repo = resolve_repo(g)?;
    let head = current_head().ok();
    let current_branch = head.map(|h| h.branch).unwrap_or_else(|| "main".to_string());

    let spinner = make_spinner(g.json);
    spinner.set_message("Resolving pipelines...");

    let pipe_a =
        resolve_pipeline_ref(&client, &repo.workspace, &repo.slug, a_ref, &current_branch).await?;
    let pipe_b =
        resolve_pipeline_ref(&client, &repo.workspace, &repo.slug, b_ref, &current_branch).await?;

    spinner.set_message("Fetching pipeline steps...");
    let (steps_a_res, steps_b_res) = tokio::join!(
        client.list_steps(&repo.workspace, &repo.slug, &pipe_a.uuid),
        client.list_steps(&repo.workspace, &repo.slug, &pipe_b.uuid)
    );
    let steps_a = steps_a_res?.values;
    let steps_b = steps_b_res?.values;

    spinner.set_message("Fetching test reports...");
    // Fetch test reports concurrently
    let test_deltas = compute_test_deltas(
        &client,
        &repo.workspace,
        &repo.slug,
        &pipe_a,
        &pipe_b,
        &steps_a,
        &steps_b,
    )
    .await?;

    spinner.finish_and_clear();

    let step_deltas = compute_step_deltas(&steps_a, &steps_b);

    let out = CiCompareOut {
        a: ComparedPipeline {
            uuid: pipe_a.uuid.clone(),
            build_number: pipe_a.build_number,
            state: pipe_a.state_name().to_string(),
            duration_seconds: pipe_a.duration_in_seconds,
            branch: pipe_a.target.ref_name.clone(),
        },
        b: ComparedPipeline {
            uuid: pipe_b.uuid.clone(),
            build_number: pipe_b.build_number,
            state: pipe_b.state_name().to_string(),
            duration_seconds: pipe_b.duration_in_seconds,
            branch: pipe_b.target.ref_name.clone(),
        },
        step_deltas,
        test_deltas,
    };

    let human = render_compare(&out);
    Formatter::from_json_flag(g.json).print(&out, &human)
}

fn compute_step_deltas(steps_a: &[PipelineStep], steps_b: &[PipelineStep]) -> Vec<StepDelta> {
    let mut deltas = Vec::new();
    for sa in steps_a {
        if let Some(sb) = steps_b.iter().find(|s| s.name == sa.name) {
            let duration_a = sa.duration_in_seconds;
            let duration_b = sb.duration_in_seconds;
            let delta = (duration_b as i64) - (duration_a as i64);
            deltas.push(StepDelta {
                name: sa.name.clone(),
                duration_a: Some(duration_a),
                duration_b: Some(duration_b),
                duration_delta: Some(delta),
                state_a: sa.state_name().to_string(),
                state_b: sb.state_name().to_string(),
            });
        } else {
            deltas.push(StepDelta {
                name: sa.name.clone(),
                duration_a: Some(sa.duration_in_seconds),
                duration_b: None,
                duration_delta: None,
                state_a: sa.state_name().to_string(),
                state_b: "ABSENT".to_string(),
            });
        }
    }
    for sb in steps_b {
        if !steps_a.iter().any(|s| s.name == sb.name) {
            deltas.push(StepDelta {
                name: sb.name.clone(),
                duration_a: None,
                duration_b: Some(sb.duration_in_seconds),
                duration_delta: None,
                state_a: "ABSENT".to_string(),
                state_b: sb.state_name().to_string(),
            });
        }
    }
    deltas
}

async fn compute_test_deltas(
    client: &crate::api::BitbucketClient,
    workspace: &str,
    slug: &str,
    pipe_a: &Pipeline,
    pipe_b: &Pipeline,
    steps_a: &[PipelineStep],
    steps_b: &[PipelineStep],
) -> Result<Option<TestDelta>> {
    let mut total_a = 0;
    let mut passed_a = 0;
    let mut failed_a = 0;
    let mut skipped_a = 0;
    let mut has_tests_a = false;

    for step in steps_a {
        if let Ok(report) = client
            .test_report(workspace, slug, &pipe_a.uuid, &step.uuid)
            .await
        {
            total_a += report.total;
            passed_a += report.successful;
            failed_a += report.failed + report.errors;
            skipped_a += report.skipped;
            has_tests_a = true;
        }
    }

    let mut total_b = 0;
    let mut passed_b = 0;
    let mut failed_b = 0;
    let mut skipped_b = 0;
    let mut has_tests_b = false;

    for step in steps_b {
        if let Ok(report) = client
            .test_report(workspace, slug, &pipe_b.uuid, &step.uuid)
            .await
        {
            total_b += report.total;
            passed_b += report.successful;
            failed_b += report.failed + report.errors;
            skipped_b += report.skipped;
            has_tests_b = true;
        }
    }

    if !has_tests_a && !has_tests_b {
        return Ok(None);
    }

    // Now gather failed cases to compute new failures and fixed failures
    let mut failed_cases_a = Vec::new();
    for step in steps_a {
        if let Ok(cases) = client
            .test_cases(workspace, slug, &pipe_a.uuid, &step.uuid, 100)
            .await
        {
            for c in cases {
                if c.status == "FAILED" || c.status == "ERROR" || c.status == "FAILURE" {
                    if let Some(name) = c.test_name {
                        failed_cases_a.push(name);
                    }
                }
            }
        }
    }

    let mut failed_cases_b = Vec::new();
    for step in steps_b {
        if let Ok(cases) = client
            .test_cases(workspace, slug, &pipe_b.uuid, &step.uuid, 100)
            .await
        {
            for c in cases {
                if c.status == "FAILED" || c.status == "ERROR" || c.status == "FAILURE" {
                    if let Some(name) = c.test_name {
                        failed_cases_b.push(name);
                    }
                }
            }
        }
    }

    let new_failures: Vec<String> = failed_cases_b
        .iter()
        .filter(|name| !failed_cases_a.contains(name))
        .cloned()
        .collect();

    let fixed: Vec<String> = failed_cases_a
        .iter()
        .filter(|name| !failed_cases_b.contains(name))
        .cloned()
        .collect();

    Ok(Some(TestDelta {
        total_a,
        total_b,
        passed_a,
        passed_b,
        failed_a,
        failed_b,
        skipped_a,
        skipped_b,
        new_failures,
        fixed,
    }))
}

fn render_compare(out: &CiCompareOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();

    s.push_str(&format!("{}\n", theme.bold("Pipeline Comparison")));
    s.push_str(&format!(
        "  A: #{} ({}) — {} — {}\n",
        out.a.build_number,
        out.a.branch.as_deref().unwrap_or("unknown"),
        theme.status_glyph(&out.a.state),
        human_duration(out.a.duration_seconds)
    ));
    s.push_str(&format!(
        "  B: #{} ({}) — {} — {}\n\n",
        out.b.build_number,
        out.b.branch.as_deref().unwrap_or("unknown"),
        theme.status_glyph(&out.b.state),
        human_duration(out.b.duration_seconds)
    ));

    s.push_str(&format!("{}\n", theme.bold("Step Duration Deltas")));
    s.push_str("  Step              A       B       Δ\n");
    s.push_str(&format!("  {}\n", "─".repeat(50)));

    // Find the step with the maximum absolute non-zero duration delta to highlight
    let mut max_abs_delta = 0;
    let mut highlight_index = None;
    for (i, delta) in out.step_deltas.iter().enumerate() {
        if let Some(d) = delta.duration_delta {
            let abs_d = d.abs();
            if abs_d > max_abs_delta {
                max_abs_delta = abs_d;
                highlight_index = Some(i);
            }
        }
    }

    for (i, delta) in out.step_deltas.iter().enumerate() {
        let a_str = delta
            .duration_a
            .map(|d| format!("{}s", d))
            .unwrap_or_else(|| "—".to_string());
        let b_str = delta
            .duration_b
            .map(|d| format!("{}s", d))
            .unwrap_or_else(|| "—".to_string());
        let delta_str = match delta.duration_delta {
            Some(d) => {
                if d > 0 {
                    format!("+{}s", d)
                } else {
                    format!("{}s", d)
                }
            }
            None => "—".to_string(),
        };

        let highlight = if Some(i) == highlight_index {
            "  ←"
        } else {
            ""
        };

        s.push_str(&format!(
            "  {:<17} {:<7} {:<7} {}{}\n",
            delta.name, a_str, b_str, delta_str, highlight
        ));
    }

    if let Some(td) = &out.test_deltas {
        s.push_str(&format!("\n{}\n", theme.bold("Test Results")));
        s.push_str("              A         B\n");
        s.push_str(&format!(
            "  Passed      {:<9} {}\n",
            td.passed_a, td.passed_b
        ));
        s.push_str(&format!(
            "  Failed      {:<9} {}\n",
            td.failed_a, td.failed_b
        ));
        if !td.new_failures.is_empty() {
            s.push_str(&format!("  New failures: {}\n", td.new_failures.join(", ")));
        }
        if !td.fixed.is_empty() {
            s.push_str(&format!("  Fixed:        {}\n", td.fixed.join(", ")));
        }
    }

    s
}
