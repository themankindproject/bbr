//! `bbr ci` — status / watch / logs.

use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::time;

use crate::api::pipeline::{
    ensure_uuid_braces, normalize_uuid, PipelineStep, StepSummary, TestCase, TestReport,
};
use crate::api::BitbucketClient;
use crate::cli::GlobalArgs;
use crate::commands::{
    client, confirm, current_head, human_duration, make_formatter, make_spinner, resolve_repo,
    SpinnerGuard,
};
use crate::error::{BitbucketError, Result};
use crate::output::table::Table;
use crate::output::theme::Theme;

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

pub async fn list(g: &GlobalArgs, branch: Option<&str>, limit: u32, no_steps: bool) -> Result<()> {
    let repo = resolve_repo(g)?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => current_head()?.branch,
    };
    let client = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching pipelines...");
    let pipelines = client
        .list_pipelines(&repo.workspace, &repo.slug, Some(&branch), limit)
        .await?;
    spinner.finish();

    let fmt = make_formatter(g);
    if pipelines.is_empty() {
        let out = CiListOut {
            branch: branch.clone(),
            pipelines: Vec::new(),
        };
        let human = format!("No pipelines for branch '{branch}'.");
        return fmt.print(&out, &human);
    }

    let all_steps: Vec<Vec<PipelineStep>> = if no_steps {
        pipelines.iter().map(|_| Vec::new()).collect()
    } else {
        use futures::StreamExt;

        // Only fetch steps for non-terminal pipelines — terminal results are
        // stable and the pipeline-level state row is enough for listing.
        let step_futures = pipelines.iter().map(|p| {
            let client = &client;
            let workspace = &repo.workspace;
            let slug = &repo.slug;
            async move {
                if p.is_terminal() {
                    Ok(Vec::new())
                } else {
                    steps_for_pipeline(client, workspace, slug, &p.uuid).await
                }
            }
        });
        futures::stream::iter(step_futures)
            .buffer_unordered(5)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|r| r.unwrap_or_default())
            .collect()
    };

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

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
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
    spinner.finish();

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

    let fmt = make_formatter(g);
    let human = render_status(&out);
    fmt.print(&out, &human)
}

pub async fn watch(
    g: &GlobalArgs,
    branch: Option<&str>,
    interval: u64,
    include_logs: bool,
    notify: bool,
    line_numbers: bool,
    from_offset: u64,
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

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.println(format!("Watching pipeline {uuid} on {branch}..."));

    // Track per-step byte offsets, last known state for transition detection,
    // whether we've printed the step header yet, and per-step line numbers.
    struct StepLogState {
        offset: u64,
        prev_state: String,
        header_printed: bool,
        line_no: u64,
    }

    let mut log_state: Option<std::collections::HashMap<String, StepLogState>> = if include_logs {
        Some(std::collections::HashMap::new())
    } else {
        None
    };
    // --from-offset resumes the first step that streams (the reconnect case);
    // subsequent steps start at byte 0.
    let mut resume_offset_remaining = from_offset;

    let mut current = initial;
    let watch_started = Instant::now();
    let mut current_step_name: Option<String> = None;
    loop {
        let is_terminal = current.is_terminal();

        if include_logs {
            let steps = client
                .list_steps(&repo.workspace, &repo.slug, &uuid)
                .await
                .map(|s| s.values)
                .unwrap_or_default();

            // When parallel steps stream at once, tag each line with its step
            // name so interleaved output stays readable. Sequential pipelines
            // (one active step) keep the clean unlabeled format.
            let concurrent = steps.iter().filter(|s| !s.is_terminal()).count() >= 2;
            current_step_name = steps
                .iter()
                .find(|s| !s.is_terminal())
                .map(|s| s.name.clone());

            // --from-offset: seed the resume byte offset into the step that's
            // currently running (or the last step if the pipeline already
            // finished). Applied once, to the first poll that sees steps.
            if resume_offset_remaining > 0 {
                if let Some(target) = steps
                    .iter()
                    .find(|s| !s.is_terminal())
                    .or_else(|| steps.last())
                {
                    log_state.as_mut().unwrap().insert(
                        target.uuid.clone(),
                        StepLogState {
                            offset: resume_offset_remaining,
                            prev_state: String::new(),
                            header_printed: false,
                            line_no: 1,
                        },
                    );
                    resume_offset_remaining = 0;
                }
            }

            for step in &steps {
                let state = log_state
                    .as_mut()
                    .unwrap()
                    .entry(step.uuid.clone())
                    .or_insert_with(|| StepLogState {
                        offset: 0,
                        prev_state: String::new(),
                        header_printed: false,
                        line_no: 1,
                    });

                let step_state_name = step.state_name().to_string();

                // Print state transition
                if !state.prev_state.is_empty()
                    && state.prev_state != step_state_name
                    && state.header_printed
                {
                    let prefix = if theme.unicode_enabled() { "│" } else { "|" };
                    spinner.println(render_step_transition(
                        theme,
                        Some(&step.name),
                        prefix,
                        &state.prev_state,
                        &step_state_name,
                    ));
                }
                state.prev_state = step_state_name;

                let (chunk, range_honored) = client
                    .step_log_range_checked(
                        &repo.workspace,
                        &repo.slug,
                        &uuid,
                        &step.uuid,
                        state.offset,
                    )
                    .await
                    .unwrap_or_default();

                if !range_honored {
                    // Server ignored Range and sent the whole log — reset the
                    // offset so we don't re-append content already streamed.
                    state.offset = 0;
                }

                if !chunk.is_empty() {
                    if !state.header_printed {
                        spinner.println(render_watch_step_header(
                            theme,
                            &step.name,
                            step.state_name(),
                        ));
                        state.header_printed = true;
                    }

                    // Determine how much of the chunk contains complete lines.
                    // If the chunk doesn't end with '\n', the trailing partial
                    // line is held back — we only advance offset to the last
                    // complete newline so the next poll re-fetches it whole.
                    let printable_end = if chunk.ends_with('\n') {
                        chunk.len()
                    } else {
                        chunk.rfind('\n').map(|i| i + 1).unwrap_or(0)
                    };

                    if printable_end > 0 {
                        let label = concurrent.then(|| step_tag(&step.name));
                        for line in chunk[..printable_end].lines() {
                            let num = line_numbers.then_some(state.line_no);
                            spinner.println(render_watch_log_line(
                                theme,
                                label.as_deref(),
                                num,
                                line,
                            ));
                            state.line_no += 1;
                        }
                    }
                    state.offset += printable_end as u64;
                }
            }
        }

        if is_terminal {
            break;
        }
        // Rich spinner: state · current step · elapsed. Elapsed is derived
        // from the pipeline's created_on so it's correct even when we attach
        // to an already-running pipeline.
        let elapsed = pipeline_elapsed_secs(current.created_on.as_deref(), watch_started);
        let msg = match &current_step_name {
            Some(name) if include_logs => format!(
                "{} · {} · {}",
                current.state_name(),
                crate::commands::truncate(name, 30),
                human_duration(elapsed)
            ),
            _ => format!("{} · {}", current.state_name(), human_duration(elapsed)),
        };
        spinner.set_message(msg);
        time::sleep(Duration::from_secs(interval.max(1))).await;
        // Poll errors are transient (network blip, timeout, 5xx): warn and
        // keep watching on the next tick instead of aborting a long-running
        // watch. Only auth failures are fatal — retrying can't fix those.
        match client
            .get_pipeline(&repo.workspace, &repo.slug, &uuid)
            .await
        {
            Ok(p) => current = p,
            Err(e) => {
                if matches!(e, BitbucketError::AuthFailed(_)) {
                    return Err(e);
                }
                spinner.println(format!("warning: poll failed ({}), retrying next tick", e));
            }
        }
    }
    spinner.finish();
    if notify && !g.json {
        // Terminal bell: grab attention when a long build finishes in a
        // background pane. stderr keeps stdout clean for piping.
        eprint!("\x07");
    }

    let raw_steps = steps_for_pipeline(&client, &repo.workspace, &repo.slug, &uuid)
        .await
        .unwrap_or_default();
    let steps = raw_steps.iter().map(step_out).collect::<Vec<_>>();

    let final_state = current.state_name().to_string();
    let success = final_state.eq_ignore_ascii_case("SUCCESSFUL");
    let failing_step = raw_steps.iter().find(|s| s.is_failed());
    let failure_log = if !success && !include_logs {
        // If --logs was on, we already streamed everything — no need to
        // dump the tail again. Only fetch the tail when --logs is off
        // (classic behavior: show last 120 lines of failing step on failure).
        //
        // The excerpt only needs the end of the log, so request a bounded
        // suffix range instead of downloading a potentially multi-MB log
        // in full. Falls back to the complete log when the server (or an
        // intermediary) ignores the Range header.
        const FAILURE_LOG_TAIL_BYTES: u64 = 64 * 1024;
        let step = failing_step
            .or_else(|| raw_steps.last())
            .ok_or_else(|| BitbucketError::NotFound("no steps for pipeline".into()))?;
        let failure_log_text = client
            .step_log_range(
                &repo.workspace,
                &repo.slug,
                &uuid,
                &step.uuid,
                FAILURE_LOG_TAIL_BYTES.saturating_sub(1),
            )
            .await
            .unwrap_or_else(|_| {
                // Range rejected (or transport error): fall back to the
                // full log so the failure excerpt is still produced. The
                // fallback is synchronous on the already-received error, so
                // no second network call happens unless we truly need it —
                // but a full-log fetch is warranted here regardless.
                String::new()
            });
        let failure_log_text = if failure_log_text.is_empty() {
            client
                .step_log(&repo.workspace, &repo.slug, &uuid, &step.uuid)
                .await?
                .text
        } else {
            failure_log_text
        };
        Some(failure_log_text)
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

    let fmt = make_formatter(g);
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
        // Smart failure extraction: show the first error line with context
        // instead of a blind tail. Falls back to the last 120 lines when
        // nothing in the log classifies as an error.
        match extract_failure_context(&log, 10, 30) {
            Some((line_no, excerpt)) => {
                human.push_str(&format!(
                    "\n\n--- first error (line {line_no}) ± context ---\n"
                ));
                human.push_str(&highlight_log_chunk(theme, &excerpt));
            }
            None => {
                human.push_str("\n\n--- last 120 log lines ---\n");
                human.push_str(&last_lines(&log, 120));
            }
        }
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

/// Stream step logs in real time using HTTP Range requests.
///
/// With `all`, auto-advances to the next step when the current one finishes,
/// following the whole pipeline. `from_offset` resumes the first step from a
/// byte offset (reconnect after a dropped `tail`).
#[allow(clippy::too_many_arguments)]
pub async fn tail(
    g: &GlobalArgs,
    step: Option<&str>,
    pipeline: Option<&str>,
    branch: Option<&str>,
    interval: u64,
    follow: bool,
    notify: bool,
    all: bool,
    line_numbers: bool,
    from_offset: u64,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let branch: String = if let Some(b) = branch {
        b.to_string()
    } else {
        current_head()?.branch
    };
    let client = client(g)?;
    let theme = Theme::current();

    // Resolve the pipeline UUID.
    let pipeline_uuid = match pipeline {
        Some(u) => ensure_uuid_braces(u),
        None => {
            let p = client
                .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
                .await?
                .ok_or_else(|| {
                    BitbucketError::NotFound(format!("no pipeline for branch '{branch}'"))
                })?;
            p.uuid
        }
    };

    // Resolve which step to tail.
    let steps = client
        .list_steps(&repo.workspace, &repo.slug, &pipeline_uuid)
        .await?;

    let selected = match step {
        Some(selector) => {
            let selector_uuid = normalize_uuid(selector);
            steps
                .values
                .iter()
                .find(|s| {
                    normalize_uuid(&s.uuid) == selector_uuid
                        || s.name.eq_ignore_ascii_case(selector)
                })
                .ok_or_else(|| BitbucketError::NotFound(format!("no step matching '{selector}'")))?
                .clone()
        }
        None => steps
            .values
            .iter()
            .find(|s| !s.is_terminal())
            .or_else(|| steps.values.last())
            .ok_or_else(|| BitbucketError::NotFound("no steps for pipeline".into()))?
            .clone(),
    };

    let step_name = selected.name.clone();
    let step_uuid = selected.uuid.clone();
    let short_uuid = step_uuid.trim_matches(|c| c == '{' || c == '}');

    // Fetch the pipeline to get its build number.
    let pipeline = client
        .get_pipeline(&repo.workspace, &repo.slug, &pipeline_uuid)
        .await?;
    let build_number = pipeline.build_number;
    let pipe_state = pipeline.state_name().to_string();

    // Clear the spinner before streaming.
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.finish();

    let header = render_tail_header(
        theme,
        &step_name,
        build_number,
        short_uuid,
        pipe_state.as_str(),
    );
    if !g.json {
        crate::output::print_block(&format!("{header}\n"))?;
    }

    let mut offset: u64 = from_offset;
    let mut prev_state = selected.state_name().to_string();
    let mut step_is_terminal = selected.is_terminal();
    let started = Instant::now();
    let mut nums = LineNumState::new(1);
    let mut current_step_uuid = step_uuid.clone();
    let mut current_step_name = step_name.clone();
    // Steps already tailed — auto-advance never revisits a step.
    let mut tailed: std::collections::HashSet<String> =
        std::collections::HashSet::from([step_uuid.clone()]);

    'steps: loop {
        // Stream the current step until it reaches a terminal state.
        loop {
            let (chunk, range_honored) = client
                .step_log_range_checked(
                    &repo.workspace,
                    &repo.slug,
                    &pipeline_uuid,
                    &current_step_uuid,
                    offset,
                )
                .await
                .unwrap_or_default();

            if !range_honored {
                // Server ignored Range and sent the whole log — reset so the
                // accounting below doesn't re-append already-streamed bytes.
                offset = 0;
            }

            if !chunk.is_empty() {
                // Only print up to the last complete line. If the chunk
                // doesn't end with '\n', hold back the partial trailing line
                // so the next poll re-fetches it complete.
                let printable_end = if chunk.ends_with('\n') {
                    chunk.len()
                } else {
                    chunk.rfind('\n').map(|i| i + 1).unwrap_or(0)
                };

                if printable_end > 0 && !g.json {
                    let rendered = if line_numbers {
                        render_log_chunk_numbered(theme, &chunk[..printable_end], &mut nums)
                    } else {
                        highlight_log_chunk(theme, &chunk[..printable_end])
                    };
                    crate::output::print_block(&rendered)?;
                }
                offset += printable_end as u64;
            }

            if step_is_terminal {
                break;
            }

            time::sleep(Duration::from_secs(interval.max(1))).await;
            let fresh_steps = client
                .list_steps(&repo.workspace, &repo.slug, &pipeline_uuid)
                .await
                .map(|s| s.values)
                .unwrap_or_default();
            if let Some(fresh) = fresh_steps.iter().find(|s| s.uuid == current_step_uuid) {
                let new_state = fresh.state_name().to_string();
                if new_state != prev_state {
                    if !g.json {
                        let dash = if theme.unicode_enabled() {
                            "──"
                        } else {
                            "--"
                        };
                        crate::output::print_block(&format!(
                            "{}\n",
                            render_step_transition(theme, None, dash, &prev_state, &new_state)
                        ))?;
                    }
                    prev_state = new_state;
                }
                step_is_terminal = fresh.is_terminal();
            }
        }

        // --all: auto-advance to the next step when the pipeline is still
        // going. Prefer a step that's already running; fall back to the next
        // pending step so we attach before it starts.
        if all {
            let pipeline_done = client
                .get_pipeline(&repo.workspace, &repo.slug, &pipeline_uuid)
                .await
                .map(|p| p.is_terminal())
                .unwrap_or(true);
            if !pipeline_done {
                let fresh_steps = client
                    .list_steps(&repo.workspace, &repo.slug, &pipeline_uuid)
                    .await
                    .map(|s| s.values)
                    .unwrap_or_default();
                let next = fresh_steps
                    .iter()
                    .find(|s| !tailed.contains(&s.uuid) && !s.is_terminal())
                    .or_else(|| fresh_steps.iter().find(|s| !tailed.contains(&s.uuid)));
                if let Some(next_step) = next {
                    current_step_uuid = next_step.uuid.clone();
                    current_step_name = next_step.name.clone();
                    tailed.insert(current_step_uuid.clone());
                    offset = 0;
                    nums = LineNumState::new(1);
                    prev_state = next_step.state_name().to_string();
                    step_is_terminal = next_step.is_terminal();
                    if !g.json {
                        let short = current_step_uuid.trim_matches(|c| c == '{' || c == '}');
                        let header = render_tail_header(
                            theme,
                            &current_step_name,
                            build_number,
                            short,
                            next_step.state_name(),
                        );
                        crate::output::print_block(&format!("\n{header}\n"))?;
                    }
                    continue 'steps;
                }
            }
        }

        // Not advancing — if --follow, poll a bit after terminal to catch
        // flushing logs, then exit.
        if follow {
            for _ in 0..3 {
                time::sleep(Duration::from_secs(interval.max(1))).await;
                let chunk = client
                    .step_log_range(
                        &repo.workspace,
                        &repo.slug,
                        &pipeline_uuid,
                        &current_step_uuid,
                        offset,
                    )
                    .await
                    .unwrap_or_default();
                if chunk.is_empty() {
                    break;
                }
                if !g.json {
                    // Same partial-line holdback as the main stream loop: only
                    // print up to the last complete newline so a line split
                    // across the final flush isn't rendered twice.
                    let printable_end = if chunk.ends_with('\n') {
                        chunk.len()
                    } else {
                        chunk.rfind('\n').map(|i| i + 1).unwrap_or(0)
                    };
                    if printable_end > 0 {
                        let rendered = if line_numbers {
                            render_log_chunk_numbered(theme, &chunk[..printable_end], &mut nums)
                        } else {
                            highlight_log_chunk(theme, &chunk[..printable_end])
                        };
                        crate::output::print_block(&rendered)?;
                    }
                }
                offset += if chunk.ends_with('\n') {
                    chunk.len() as u64
                } else {
                    chunk.rfind('\n').map(|i| i + 1).unwrap_or(0) as u64
                };
            }
        }
        break;
    }

    // Exit summary
    if !g.json {
        let elapsed = started.elapsed();
        let summary = render_tail_exit_summary(
            theme,
            &current_step_name,
            &prev_state,
            elapsed.as_secs_f64(),
        );
        crate::output::print_block(&format!("\n{summary}\n"))?;
        if notify {
            // Terminal bell: grab attention when a long tail finishes in a
            // background pane. stderr keeps stdout clean for piping.
            eprint!("\x07");
        }
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
        let fmt = make_formatter(g);
        let human = format!("Wrote {} bytes to {path}", log.text.len());
        return fmt.print(&out, &human);
    }

    let fmt = make_formatter(g);
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

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching steps...");
    let raw = client
        .list_steps(&repo.workspace, &repo.slug, &uuid)
        .await?;
    spinner.finish();

    #[derive(Debug, Serialize)]
    pub struct CiStepsOut {
        pub uuid: String,
        pub steps: Vec<StepSummary>,
    }

    let out = CiStepsOut {
        uuid: uuid.clone(),
        steps: raw.values.iter().map(step_out).collect(),
    };

    let fmt = make_formatter(g);
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

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching steps...");
    let steps = client
        .list_steps(&repo.workspace, &repo.slug, &pipeline_uuid)
        .await?;
    spinner.finish();

    let selected = select_step(&steps.values, step, false, false, step.is_none())?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
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
    spinner.finish();

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

    let fmt = make_formatter(g);
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
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Stopping pipeline...");
    client
        .stop_pipeline(&repo.workspace, &repo.slug, &pipeline_uuid)
        .await?;
    spinner.finish();
    let fmt = make_formatter(g);
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

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching latest pipeline...");
    let pipeline = client
        .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
        .await?
        .ok_or_else(|| BitbucketError::NotFound(format!("no pipeline for branch '{branch}'")))?;
    spinner.finish();

    if !g.json
        && !confirm(&format!(
            "Rerun pipeline #{} (current state: {}) for branch '{}'? [y/N] ",
            pipeline.build_number,
            pipeline.state_name(),
            branch,
        ))
        .await?
    {
        let fmt = make_formatter(g);
        fmt.print(&(), "Aborted.")?;
        return Ok(());
    }

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Triggering rerun...");
    let new_pipeline = client
        .rerun_pipeline(&repo.workspace, &repo.slug, &pipeline.uuid)
        .await?;
    spinner.finish();
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
    let fmt = make_formatter(g);
    let human = format!("Reran pipeline #{}", new_pipeline.build_number);
    if !g.json {
        fmt.print(&out, &format!("{human}\nNext: bbr ci watch"))
    } else {
        fmt.print(&out, &human)
    }
}

pub async fn trigger(
    g: &GlobalArgs,
    branch: Option<&str>,
    vars: &[String],
    secured: &[String],
) -> Result<()> {
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

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message(format!("Triggering pipeline for '{branch}'..."));
    let pipeline = client
        .trigger_pipeline_with_variables(
            &repo.workspace,
            &repo.slug,
            &branch,
            variables_payload.as_deref(),
        )
        .await?;
    spinner.finish();

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
    let fmt = make_formatter(g);
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

/// Format the header line for `bbr ci tail`.
fn render_tail_header(
    theme: &Theme,
    step_name: &str,
    build_number: u64,
    step_uuid: &str,
    pipe_state: &str,
) -> String {
    format!(
        "{} {} :: #{} :: {} :: {}",
        theme.dim("==>"),
        theme.bold(step_name),
        build_number,
        theme.dim(step_uuid),
        theme.dim(pipe_state)
    )
}

/// Format a state transition line for step logs.
fn render_step_transition(
    theme: &Theme,
    step_name: Option<&str>,
    prefix: &str,
    prev_state: &str,
    new_state: &str,
) -> String {
    let prev_glyph = theme.status_glyph(prev_state);
    let curr_glyph = theme.status_glyph(new_state);
    let arrow = if theme.unicode_enabled() { "→" } else { "->" };
    if let Some(name) = step_name {
        format!(
            "{} {} {} {}{} {}",
            theme.dim(prefix),
            theme.bold(name),
            prev_glyph,
            theme.dim(arrow),
            curr_glyph,
            theme.dim(new_state)
        )
    } else {
        format!(
            "{} {} {}{} {}",
            theme.dim(prefix),
            prev_glyph,
            theme.dim(arrow),
            curr_glyph,
            theme.bold(new_state)
        )
    }
}

/// Format a step header for `bbr ci watch --logs`.
fn render_watch_step_header(theme: &Theme, step_name: &str, state: &str) -> String {
    let corner = if theme.unicode_enabled() { "┌" } else { "==" };
    format!(
        "{} {}  {} {}",
        theme.dim(corner),
        theme.bold(step_name),
        theme.status_glyph(state),
        theme.dim(state)
    )
}

/// Classification of a streamed log line, used to make failures pop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogLineKind {
    Error,
    Warn,
    Normal,
}

/// Elapsed seconds for a pipeline, anchored to its `created_on` timestamp so
/// the counter is correct even when `bbr ci watch` attaches mid-run. Falls
/// back to the local watch duration when the timestamp is missing/unparseable.
fn pipeline_elapsed_secs(created_on: Option<&str>, watch_started: Instant) -> u64 {
    if let Some(ts) = created_on {
        if let Ok(dt) = ::time::OffsetDateTime::parse(
            ts,
            &::time::format_description::well_known::Iso8601::DEFAULT,
        ) {
            let now = ::time::OffsetDateTime::now_utc();
            let secs = (now - dt).whole_seconds();
            if secs >= 0 {
                return secs as u64;
            }
        }
    }
    watch_started.elapsed().as_secs()
}

/// Heuristically classify a log line. Favors recall for real failures while
/// ignoring zero-count test summaries (e.g. "0 failed", "0 warnings").
fn classify_log_line(line: &str) -> LogLineKind {
    let lower = line.to_ascii_lowercase();

    // Unambiguous failure signals.
    const STRONG: &[&str] = &[
        "fatal",
        "panic",
        "traceback",
        "assertion failed",
        "segmentation fault",
        "segfault",
        "core dumped",
        "✗",
    ];
    if STRONG.iter().any(|m| lower.contains(m)) {
        return LogLineKind::Error;
    }

    // error / failed / failure, ignoring zero-count summaries.
    let has_err_word =
        lower.contains("error") || lower.contains("failed") || lower.contains("failure");
    let zero_err = lower.contains("0 failed")
        || lower.contains("failed: 0")
        || lower.contains("0 failures")
        || lower.contains("0 error")
        || lower.contains("errors: 0")
        || lower.contains("error: 0");
    if has_err_word && !zero_err {
        return LogLineKind::Error;
    }

    const WARN: &[&str] = &["warning", "warn:", "deprecated", "deprecation"];
    let has_warn_word = WARN.iter().any(|m| lower.contains(m));
    let zero_warn = lower.contains("0 warning") || lower.contains("warnings: 0");
    if has_warn_word && !zero_warn {
        return LogLineKind::Warn;
    }

    LogLineKind::Normal
}

/// Apply error/warning coloring to a single log line. Colors are already gated
/// on TTY / NO_COLOR by the theme, so piped output stays byte-identical.
fn highlight_log_line(theme: &Theme, line: &str) -> String {
    match classify_log_line(line) {
        LogLineKind::Error => theme.error(line).into_owned(),
        LogLineKind::Warn => theme.warn(line).into_owned(),
        LogLineKind::Normal => line.to_string(),
    }
}

/// Highlight error/warning lines across a whole log chunk, preserving line
/// endings and any trailing partial line byte-for-byte. When colors are off
/// the output is identical to the input.
fn highlight_log_chunk(theme: &Theme, chunk: &str) -> String {
    let mut out = String::with_capacity(chunk.len());
    for piece in chunk.split_inclusive('\n') {
        let (body, nl) = match piece.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (piece, ""),
        };
        out.push_str(&highlight_log_line(theme, body));
        out.push_str(nl);
    }
    out
}

/// Short bracketed tag used to disambiguate interleaved parallel-step logs.
fn step_tag(name: &str) -> String {
    format!("[{}]", crate::commands::truncate(name, 14))
}

/// Smart failure extraction: locate the first line that classifies as an
/// error and return its 1-based line number plus an excerpt of `before`
/// lines above and `after` lines below. When the window doesn't reach the
/// end of the log, a short tail is appended (with an omission marker) so the
/// final state stays visible. Returns `None` when nothing looks like an
/// error — callers should fall back to a plain tail of the log.
fn extract_failure_context(log: &str, before: usize, after: usize) -> Option<(usize, String)> {
    let lines: Vec<&str> = log.lines().collect();
    let idx = lines
        .iter()
        .position(|l| classify_log_line(l) == LogLineKind::Error)?;
    let start = idx.saturating_sub(before);
    let end = (idx + 1 + after).min(lines.len());
    let mut out = lines[start..end].join("\n");
    if end < lines.len() {
        let remaining = lines.len() - end;
        if remaining <= after {
            // Small gap — show it in full rather than an omission marker.
            out.push('\n');
            out.push_str(&lines[end..].join("\n"));
        } else {
            let tail_n = 20.min(remaining);
            out.push_str(&format!(
                "\n[... {} lines omitted ...]\n",
                remaining - tail_n
            ));
            out.push_str(&lines[lines.len() - tail_n..].join("\n"));
        }
    }
    Some((idx + 1, out))
}

/// Line-numbering state for streamed log output. Tracks the next line number
/// and whether the cursor is at the start of a fresh line, so a chunk that
/// continues a partial line doesn't get a spurious number.
struct LineNumState {
    next: u64,
    at_line_start: bool,
}

impl LineNumState {
    fn new(start_line: u64) -> Self {
        Self {
            next: start_line,
            at_line_start: true,
        }
    }
}

/// Render a log chunk with a dim line-number gutter, continuing numbering
/// across chunk boundaries. A trailing partial line gets a number when it
/// starts a line; its continuation in the next chunk does not.
fn render_log_chunk_numbered(theme: &Theme, chunk: &str, nums: &mut LineNumState) -> String {
    let mut out = String::with_capacity(chunk.len() + chunk.lines().count() * 8);
    for piece in chunk.split_inclusive('\n') {
        let (body, nl) = match piece.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (piece, ""),
        };
        if nums.at_line_start {
            out.push_str(&theme.dim(&format!("{:>6}  ", nums.next)));
            nums.next += 1;
        }
        out.push_str(&highlight_log_line(theme, body));
        out.push_str(nl);
        nums.at_line_start = !nl.is_empty();
    }
    out
}

/// Format a log line for live streaming with the gutter character. When
/// `label` is `Some`, it is printed between the gutter and the line body so
/// concurrent parallel steps stay readable. When `line_no` is `Some`, a dim
/// line number is printed after the gutter.
fn render_watch_log_line(
    theme: &Theme,
    label: Option<&str>,
    line_no: Option<u64>,
    line: &str,
) -> String {
    let gutter = if theme.unicode_enabled() { "│" } else { "|" };
    let body = highlight_log_line(theme, line);
    let num = line_no.map(|n| theme.dim(&format!("{n:>6}  ")).into_owned());
    match (label, num) {
        (Some(tag), Some(n)) => format!("{} {} {} {}", theme.dim(gutter), theme.dim(tag), n, body),
        (Some(tag), None) => format!("{} {} {}", theme.dim(gutter), theme.dim(tag), body),
        (None, Some(n)) => format!("{} {} {}", theme.dim(gutter), n, body),
        (None, None) => format!("{} {}", theme.dim(gutter), body),
    }
}

/// Format the exit summary for `bbr ci tail`.
fn render_tail_exit_summary(
    theme: &Theme,
    step_name: &str,
    final_state: &str,
    elapsed_secs: f64,
) -> String {
    let dash = if theme.unicode_enabled() {
        "──"
    } else {
        "--"
    };
    format!(
        "{} {} {}  {}  {}",
        theme.dim(dash),
        theme.bold(step_name),
        theme.status_glyph(final_state),
        theme.dim(final_state),
        theme.dim(&format!("({:.1}s)", elapsed_secs))
    )
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
    use crate::api::pipeline::PipelineState;

    fn step(uuid: &str, name: &str, state: &str) -> PipelineStep {
        PipelineStep {
            uuid: uuid.into(),
            name: name.into(),
            state: PipelineState {
                name: state.into(),
                result: None,
            },
            duration_in_seconds: 1,
            started_on: None,
            completed_on: None,
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

    // ---- UI formatting tests ------------------------------------------------

    fn no_color_theme() -> Theme {
        Theme::test_instance(false, true)
    }

    #[test]
    fn tail_header_includes_step_name_and_build_number() {
        let t = no_color_theme();
        let h = render_tail_header(&t, "Build & Test", 42, "abc-123", "IN_PROGRESS");
        assert!(h.contains("==>"));
        assert!(h.contains("Build & Test"));
        assert!(h.contains("#42"));
        assert!(h.contains("abc-123"));
        assert!(h.contains("IN_PROGRESS"));
    }

    #[test]
    fn tail_header_separator_is_double_colon() {
        let t = no_color_theme();
        let h = render_tail_header(&t, "Build", 1, "uuid", "PENDING");
        assert!(h.contains(" :: "), "header must use :: as field separator");
        // 5 fields: ==> step :: #N :: uuid :: state → 4 gaps → 3 " :: " strings
        assert_eq!(h.matches(" :: ").count(), 3);
    }

    #[test]
    fn tail_header_excludes_newline() {
        let t = no_color_theme();
        let h = render_tail_header(&t, "Build", 1, "uuid", "RUNNING");
        assert!(!h.contains('\n'));
    }

    #[test]
    fn tail_exit_summary_includes_step_and_state_and_elapsed() {
        let t = no_color_theme();
        let s = render_tail_exit_summary(&t, "Build", "SUCCESSFUL", 42.5);
        assert!(s.contains("Build"));
        assert!(s.contains("[ok]"));
        assert!(s.contains("SUCCESSFUL"));
        assert!(s.contains("42.5s"));
    }

    #[test]
    fn tail_exit_summary_starts_with_dash_prefix() {
        let t = no_color_theme();
        let s = render_tail_exit_summary(&t, "Build", "FAILED", 1.0);
        assert!(s.starts_with("──"));
    }

    #[test]
    fn tail_exit_summary_handles_exact_one_second() {
        let t = no_color_theme();
        let s = render_tail_exit_summary(&t, "Test", "SUCCESSFUL", 1.0);
        assert!(s.contains("(1.0s)"));
    }

    #[test]
    fn tail_exit_summary_handles_sub_second() {
        let t = no_color_theme();
        let s = render_tail_exit_summary(&t, "Build", "ERROR", 0.3);
        assert!(s.contains("(0.3s)"));
    }

    #[test]
    fn step_transition_with_name_includes_step_name() {
        let t = no_color_theme();
        let s = render_step_transition(&t, Some("Build"), "│", "IN_PROGRESS", "SUCCESSFUL");
        assert!(s.contains("Build"));
        assert!(s.contains("[~]")); // in_progress
        assert!(s.contains("[ok]")); // successful
        assert!(s.contains("→"));
    }

    #[test]
    fn step_transition_without_name_excludes_step_name() {
        let t = no_color_theme();
        let s = render_step_transition(&t, None, "──", "IN_PROGRESS", "FAILED");
        assert!(!s.contains("Build"));
        assert!(s.contains("[~]"));
        assert!(s.contains("[X]"));
        assert!(s.contains("→"));
    }

    #[test]
    fn step_transition_shows_old_then_new_state() {
        let t = no_color_theme();
        let s = render_step_transition(&t, None, "──", "PENDING", "SUCCESSFUL");
        let pending_pos = s.find("[.]").unwrap();
        let ok_pos = s.find("[ok]").unwrap();
        assert!(
            pending_pos < ok_pos,
            "old state [.] must appear before new state [ok]"
        );
    }

    #[test]
    fn step_transition_excludes_newline() {
        let t = no_color_theme();
        let s = render_step_transition(&t, Some("Build"), "│", "RUNNING", "SUCCESSFUL");
        assert!(!s.contains('\n'));
    }

    #[test]
    fn watch_step_header_uses_box_drawing_char() {
        let t = no_color_theme();
        let h = render_watch_step_header(&t, "Build", "IN_PROGRESS");
        assert!(h.contains("┌"), "header must start with box-drawing ┌");
    }

    #[test]
    fn watch_step_header_includes_step_name_and_state() {
        let t = no_color_theme();
        let h = render_watch_step_header(&t, "Test Suite", "FAILED");
        assert!(h.contains("Test Suite"));
        assert!(h.contains("[X]"));
        assert!(h.contains("FAILED"));
    }

    #[test]
    fn watch_step_header_shows_different_states() {
        let t = no_color_theme();
        let running = render_watch_step_header(&t, "Build", "IN_PROGRESS");
        let success = render_watch_step_header(&t, "Build", "SUCCESSFUL");
        let failed = render_watch_step_header(&t, "Build", "FAILED");
        assert!(running.contains("[~]"));
        assert!(success.contains("[ok]"));
        assert!(failed.contains("[X]"));
    }

    #[test]
    fn watch_log_line_uses_vertical_bar_gutter() {
        let t = no_color_theme();
        let line = render_watch_log_line(&t, None, None, "cargo build --release");
        assert!(
            line.starts_with("│ "),
            "log line must start with box gutter"
        );
        assert!(line.contains("cargo build --release"));
    }

    #[test]
    fn watch_log_line_preserves_user_content() {
        let t = no_color_theme();
        let line = render_watch_log_line(&t, None, None, "    Compiling bbr v0.2.0");
        assert!(line.contains("Compiling bbr v0.2.0"));
        assert!(line.starts_with("│     "));
    }

    #[test]
    fn watch_log_line_excludes_newline() {
        let t = no_color_theme();
        let line = render_watch_log_line(&t, None, None, "hello");
        assert!(!line.contains('\n'));
    }

    #[test]
    fn watch_log_line_with_label_inserts_tag_after_gutter() {
        let t = no_color_theme();
        let line = render_watch_log_line(&t, Some("[build]"), None, "running tests");
        assert!(
            line.starts_with("│ [build] "),
            "label must follow the gutter"
        );
        assert!(line.contains("running tests"));
    }

    #[test]
    fn watch_log_line_without_label_has_no_tag() {
        let t = no_color_theme();
        let line = render_watch_log_line(&t, None, None, "running tests");
        assert!(!line.contains("[build]"));
    }

    #[test]
    fn watch_log_line_with_line_number_inserts_number_after_gutter() {
        let t = no_color_theme();
        let line = render_watch_log_line(&t, None, Some(42), "running tests");
        assert!(
            line.starts_with("│     42  "),
            "line number must follow the gutter, got: {line}"
        );
        assert!(line.contains("running tests"));
    }

    #[test]
    fn watch_log_line_with_label_and_number_orders_label_then_number() {
        let t = no_color_theme();
        let line = render_watch_log_line(&t, Some("[build]"), Some(7), "x");
        let label_pos = line.find("[build]").unwrap();
        let num_pos = line.find("7").unwrap();
        assert!(label_pos < num_pos, "label must precede line number");
    }

    // ---- log line classification tests ------------------------------------

    #[test]
    fn classify_error_lines() {
        assert_eq!(
            classify_log_line("error[E0308]: mismatched types"),
            LogLineKind::Error
        );
        assert_eq!(classify_log_line("test foo ... FAILED"), LogLineKind::Error);
        assert_eq!(
            classify_log_line("thread 'main' panicked at src/main.rs:1"),
            LogLineKind::Error
        );
        assert_eq!(
            classify_log_line("fatal: not a git repository"),
            LogLineKind::Error
        );
        assert_eq!(
            classify_log_line("Traceback (most recent call last):"),
            LogLineKind::Error
        );
        assert_eq!(
            classify_log_line("Segmentation fault (core dumped)"),
            LogLineKind::Error
        );
    }

    #[test]
    fn classify_warn_lines() {
        assert_eq!(
            classify_log_line("warning: unused variable `x`"),
            LogLineKind::Warn
        );
        assert_eq!(
            classify_log_line("WARN: deprecated API in use"),
            LogLineKind::Warn
        );
    }

    #[test]
    fn classify_normal_lines() {
        assert_eq!(
            classify_log_line("Compiling bbr v0.2.1"),
            LogLineKind::Normal
        );
        assert_eq!(classify_log_line("running 42 tests"), LogLineKind::Normal);
        assert_eq!(
            classify_log_line("Finished release [optimized] target(s)"),
            LogLineKind::Normal
        );
    }

    #[test]
    fn classify_ignores_zero_count_summaries() {
        // "0 failed" / "0 warnings" are success summaries, not failures.
        assert_eq!(
            classify_log_line("test result: ok. 42 passed; 0 failed"),
            LogLineKind::Normal
        );
        assert_eq!(classify_log_line("0 warnings emitted"), LogLineKind::Normal);
        assert_eq!(
            classify_log_line("errors: 0, warnings: 0"),
            LogLineKind::Normal
        );
    }

    #[test]
    fn highlight_chunk_preserves_bytes_when_no_colors() {
        let t = no_color_theme();
        let chunk = "ok line\nerror: boom\n";
        assert_eq!(highlight_log_chunk(&t, chunk), chunk);
    }

    #[test]
    fn highlight_chunk_preserves_trailing_partial_line() {
        let t = no_color_theme();
        let chunk = "complete\npartial";
        assert_eq!(highlight_log_chunk(&t, chunk), chunk);
    }

    // ---- smart failure extraction tests (E) --------------------------------

    #[test]
    fn failure_context_finds_first_error_with_window() {
        let log = (1..=50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\nerror: boom\n"
            + "after 1\nafter 2\n";
        let (line_no, excerpt) = extract_failure_context(&log, 3, 2).unwrap();
        assert_eq!(line_no, 51);
        assert!(excerpt.contains("line 48"));
        assert!(excerpt.contains("line 50"));
        assert!(excerpt.contains("error: boom"));
        assert!(excerpt.contains("after 1"));
        assert!(excerpt.contains("after 2"));
        // Window reaches the end → no omission marker.
        assert!(!excerpt.contains("omitted"));
    }

    #[test]
    fn failure_context_appends_tail_when_error_is_early() {
        let mut log = String::from("error: early boom\n");
        for i in 1..=100 {
            log.push_str(&format!("filler {i}\n"));
        }
        let (line_no, excerpt) = extract_failure_context(&log, 5, 10).unwrap();
        assert_eq!(line_no, 1);
        assert!(excerpt.contains("error: early boom"));
        assert!(excerpt.contains("lines omitted"));
        assert!(excerpt.contains("filler 100"));
    }

    #[test]
    fn failure_context_none_when_no_error_lines() {
        let log = "Compiling bbr v0.2.1\nFinished release\ntest result: ok. 42 passed; 0 failed\n";
        assert!(extract_failure_context(log, 5, 10).is_none());
    }

    #[test]
    fn failure_context_clamps_window_at_log_start() {
        let log = "error: first line fails\nsecond\nthird\n";
        let (line_no, excerpt) = extract_failure_context(log, 10, 2).unwrap();
        assert_eq!(line_no, 1);
        assert!(excerpt.starts_with("error: first line fails"));
    }

    // ---- numbered chunk rendering tests (G) --------------------------------

    #[test]
    fn numbered_chunk_numbers_each_line() {
        let t = no_color_theme();
        let mut nums = LineNumState::new(1);
        let out = render_log_chunk_numbered(&t, "alpha\nbeta\n", &mut nums);
        assert!(out.contains("     1  alpha\n"));
        assert!(out.contains("     2  beta\n"));
        assert_eq!(nums.next, 3);
        assert!(nums.at_line_start);
    }

    #[test]
    fn numbered_chunk_continues_across_boundaries() {
        let t = no_color_theme();
        let mut nums = LineNumState::new(1);
        render_log_chunk_numbered(&t, "one\ntwo\n", &mut nums);
        let out = render_log_chunk_numbered(&t, "three\n", &mut nums);
        assert!(out.contains("     3  three\n"));
    }

    #[test]
    fn numbered_chunk_no_number_for_partial_line_continuation() {
        let t = no_color_theme();
        let mut nums = LineNumState::new(1);
        // Chunk ends mid-line: the partial line gets a number...
        let out1 = render_log_chunk_numbered(&t, "start of line", &mut nums);
        assert!(out1.contains("     1  start of line"));
        assert!(!nums.at_line_start);
        // ...and its continuation in the next chunk does not.
        let out2 = render_log_chunk_numbered(&t, " continued\n", &mut nums);
        assert!(out2.starts_with(" continued\n"), "got: {out2:?}");
        assert_eq!(nums.next, 2);
    }

    #[test]
    fn numbered_chunk_preserves_bytes_when_no_colors() {
        let t = no_color_theme();
        let mut nums = LineNumState::new(1);
        let out = render_log_chunk_numbered(&t, "plain\n", &mut nums);
        assert_eq!(out, "     1  plain\n");
    }

    #[test]
    fn step_tag_truncates_long_names() {
        let tag = step_tag("A Very Long Step Name That Exceeds The Limit");
        assert!(tag.starts_with('['));
        assert!(tag.ends_with(']'));
        // [ + 14 cols + ellipsis ("…" or "...") + ] → bounded either way.
        assert!(tag.chars().count() <= 20, "tag must be bounded: {tag}");
    }

    #[test]
    fn pipeline_elapsed_falls_back_to_watch_duration() {
        let started = Instant::now();
        // Unparseable timestamp → fall back to local elapsed (~0s).
        assert_eq!(pipeline_elapsed_secs(Some("not-a-date"), started), 0);
        assert_eq!(pipeline_elapsed_secs(None, started), 0);
    }

    #[test]
    fn pipeline_elapsed_uses_created_on() {
        // A timestamp 60s in the past should yield ~60s elapsed.
        let past = ::time::OffsetDateTime::now_utc() - ::time::Duration::seconds(60);
        let ts = past
            .format(&::time::format_description::well_known::Iso8601::DEFAULT)
            .unwrap();
        let started = Instant::now();
        let elapsed = pipeline_elapsed_secs(Some(&ts), started);
        assert!((58..=62).contains(&elapsed), "expected ~60s, got {elapsed}");
    }

    #[test]
    fn all_formatters_are_no_newline() {
        let t = no_color_theme();
        assert!(!render_tail_header(&t, "B", 1, "u", "s").contains('\n'));
        assert!(!render_watch_step_header(&t, "B", "s").contains('\n'));
        assert!(!render_watch_log_line(&t, None, None, "l").contains('\n'));
        assert!(!render_tail_exit_summary(&t, "B", "s", 1.0).contains('\n'));
        assert!(!render_step_transition(&t, None, "│", "a", "b").contains('\n'));
    }
}
