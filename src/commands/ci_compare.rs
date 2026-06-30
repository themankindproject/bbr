//! Pipeline / CI comparison (`bbr ci compare`).

use crate::api::pipeline::{Pipeline, PipelineStep};
use crate::cli::GlobalArgs;
use crate::commands::{client, current_head, human_duration, make_spinner, resolve_repo};
use crate::error::{BitbucketError, Result};
use crate::output::theme::Theme;
use crate::output::Formatter;
use serde::Serialize;
use std::fmt::Write as FmtWrite;

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
    let current_branch = head.map_or_else(|| "main".to_string(), |h| h.branch);

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
    // Fetch test reports for both pipelines concurrently
    let reports_a_futs: Vec<_> = steps_a
        .iter()
        .map(|step| client.test_report(workspace, slug, &pipe_a.uuid, &step.uuid))
        .collect();
    let reports_b_futs: Vec<_> = steps_b
        .iter()
        .map(|step| client.test_report(workspace, slug, &pipe_b.uuid, &step.uuid))
        .collect();

    let (reports_a, reports_b): (Vec<_>, Vec<_>) = tokio::join!(
        futures::future::join_all(reports_a_futs),
        futures::future::join_all(reports_b_futs)
    );

    let mut total_a = 0u64;
    let mut passed_a = 0u64;
    let mut failed_a = 0u64;
    let mut skipped_a = 0u64;
    let mut has_tests_a = false;

    for report in reports_a.into_iter().flatten() {
        total_a += report.total;
        passed_a += report.successful;
        failed_a += report.failed + report.errors;
        skipped_a += report.skipped;
        has_tests_a = true;
    }

    let mut total_b = 0u64;
    let mut passed_b = 0u64;
    let mut failed_b = 0u64;
    let mut skipped_b = 0u64;
    let mut has_tests_b = false;

    for report in reports_b.into_iter().flatten() {
        total_b += report.total;
        passed_b += report.successful;
        failed_b += report.failed + report.errors;
        skipped_b += report.skipped;
        has_tests_b = true;
    }

    if !has_tests_a && !has_tests_b {
        return Ok(None);
    }

    // Fetch test cases for both pipelines concurrently
    let cases_a_futs: Vec<_> = steps_a
        .iter()
        .map(|step| client.test_cases(workspace, slug, &pipe_a.uuid, &step.uuid, 100))
        .collect();
    let cases_b_futs: Vec<_> = steps_b
        .iter()
        .map(|step| client.test_cases(workspace, slug, &pipe_b.uuid, &step.uuid, 100))
        .collect();

    let (cases_a, cases_b): (Vec<_>, Vec<_>) = tokio::join!(
        futures::future::join_all(cases_a_futs),
        futures::future::join_all(cases_b_futs)
    );

    let mut failed_cases_a = Vec::new();
    for cases in cases_a.into_iter().flatten() {
        for c in cases {
            if c.status == "FAILED" || c.status == "ERROR" || c.status == "FAILURE" {
                if let Some(name) = c.test_name {
                    failed_cases_a.push(name);
                }
            }
        }
    }

    let mut failed_cases_b = Vec::new();
    for cases in cases_b.into_iter().flatten() {
        for c in cases {
            if c.status == "FAILED" || c.status == "ERROR" || c.status == "FAILURE" {
                if let Some(name) = c.test_name {
                    failed_cases_b.push(name);
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

    let _ = writeln!(s, "{}", theme.bold("Pipeline Comparison"));
    let _ = writeln!(
        s,
        "  A: #{} ({}) — {} — {}",
        out.a.build_number,
        out.a.branch.as_deref().unwrap_or("unknown"),
        theme.status_glyph(&out.a.state),
        human_duration(out.a.duration_seconds)
    );
    let _ = writeln!(
        s,
        "  B: #{} ({}) — {} — {}",
        out.b.build_number,
        out.b.branch.as_deref().unwrap_or("unknown"),
        theme.status_glyph(&out.b.state),
        human_duration(out.b.duration_seconds)
    );

    let _ = writeln!(s, "{}", theme.bold("Step Duration Deltas"));
    let _ = writeln!(s, "  Step              A       B       Δ");
    let _ = writeln!(s, "  {}", "─".repeat(50));

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

        let _ = writeln!(
            s,
            "  {:<17} {:<7} {:<7} {}{}",
            delta.name, a_str, b_str, delta_str, highlight
        );
    }

    if let Some(td) = &out.test_deltas {
        let _ = writeln!(s);
        let _ = writeln!(s, "{}", theme.bold("Test Results"));
        let _ = writeln!(s, "              A         B");
        let _ = writeln!(s, "  Passed      {:<9} {}", td.passed_a, td.passed_b);
        let _ = writeln!(s, "  Failed      {:<9} {}", td.failed_a, td.failed_b);
        if !td.new_failures.is_empty() {
            let _ = writeln!(s, "  New failures: {}", td.new_failures.join(", "));
        }
        if !td.fixed.is_empty() {
            let _ = writeln!(s, "  Fixed:        {}", td.fixed.join(", "));
        }
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::pipeline::{PipelineState, PipelineStep};

    fn make_step(name: &str, duration: u64, state_name: &str) -> PipelineStep {
        PipelineStep {
            uuid: format!("step-{name}"),
            name: name.to_string(),
            state: PipelineState {
                name: state_name.to_string(),
                ..Default::default()
            },
            duration_in_seconds: duration,
            ..Default::default()
        }
    }

    #[test]
    fn compute_step_deltas_matching_steps() {
        let steps_a = vec![
            make_step("build", 30, "SUCCESSFUL"),
            make_step("test", 60, "SUCCESSFUL"),
        ];
        let steps_b = vec![
            make_step("build", 25, "SUCCESSFUL"),
            make_step("test", 80, "SUCCESSFUL"),
        ];
        let deltas = compute_step_deltas(&steps_a, &steps_b);
        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0].duration_delta, Some(-5));
        assert_eq!(deltas[1].duration_delta, Some(20));
    }

    #[test]
    fn compute_step_deltas_missing_step_in_b() {
        let steps_a = vec![
            make_step("build", 30, "SUCCESSFUL"),
            make_step("lint", 10, "SUCCESSFUL"),
        ];
        let steps_b = vec![make_step("build", 30, "SUCCESSFUL")];
        let deltas = compute_step_deltas(&steps_a, &steps_b);
        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0].state_b, "SUCCESSFUL");
        assert_eq!(deltas[1].state_b, "ABSENT");
        assert_eq!(deltas[1].duration_b, None);
    }

    #[test]
    fn compute_step_deltas_new_step_in_b() {
        let steps_a = vec![make_step("build", 30, "SUCCESSFUL")];
        let steps_b = vec![
            make_step("build", 30, "SUCCESSFUL"),
            make_step("deploy", 45, "SUCCESSFUL"),
        ];
        let deltas = compute_step_deltas(&steps_a, &steps_b);
        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[1].name, "deploy");
        assert_eq!(deltas[1].state_a, "ABSENT");
        assert_eq!(deltas[1].duration_a, None);
    }

    #[test]
    fn compute_step_deltas_empty() {
        let deltas = compute_step_deltas(&[], &[]);
        assert!(deltas.is_empty());
    }

    #[test]
    fn render_compare_includes_pipeline_info() {
        let out = CiCompareOut {
            a: ComparedPipeline {
                uuid: "a-uuid".into(),
                build_number: 10,
                state: "SUCCESSFUL".into(),
                duration_seconds: 60,
                branch: Some("main".into()),
            },
            b: ComparedPipeline {
                uuid: "b-uuid".into(),
                build_number: 11,
                state: "FAILED".into(),
                duration_seconds: 90,
                branch: Some("main".into()),
            },
            step_deltas: vec![],
            test_deltas: None,
        };
        let rendered = render_compare(&out);
        assert!(rendered.contains("#10"));
        assert!(rendered.contains("#11"));
        assert!(rendered.contains("Pipeline Comparison"));
    }
}
