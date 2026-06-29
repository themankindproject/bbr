//! `bbr status` / `bbr` — PR + CI for the current branch, or repo overview.

use serde::Serialize;

use crate::api::pipeline::{Pipeline, PipelineStep, StepSummary};
use crate::api::pr::{Participant, PrState, PullRequest};
use crate::cli::GlobalArgs;
use crate::commands::{client, current_head, human_duration, resolve_repo, truncate};
use crate::error::Result;
use crate::output::table::Table;
use crate::output::theme::Theme;
use crate::output::Formatter;

#[derive(Debug, Serialize)]
pub struct BuildStatusSummary {
    pub state: String,
    pub key: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct StatusOut {
    pub repo: RepoSummary,
    pub branch: String,
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<PrSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<PipelineSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commit_statuses: Vec<BuildStatusSummary>,
    pub suggested_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoSummary {
    pub workspace: String,
    pub slug: String,
    pub full_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrSummary {
    pub id: u64,
    pub state: String,
    pub title: String,
    pub source: String,
    pub destination: String,
    pub url: Option<String>,
    pub author: Option<String>,
    pub comment_count: u64,
    pub task_count: u64,
    pub reviewers: Vec<ReviewerSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewerSummary {
    pub display_name: String,
    pub approved: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineSummary {
    pub uuid: String,
    pub state: String,
    pub duration_seconds: u64,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub url: Option<String>,
    pub failing_steps: Vec<String>,
    pub steps: Vec<StepSummary>,
}

#[derive(Debug, Serialize)]
pub struct PrListEntry {
    pub id: u64,
    pub state: String,
    pub title: String,
    pub source: String,
    pub destination: String,
    pub author: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CiListEntry {
    pub build_number: u64,
    pub state: String,
    pub branch: Option<String>,
    pub duration_seconds: u64,
    pub commit: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OverviewOut {
    pub repo: RepoSummary,
    pub branch: String,
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<PrSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<PipelineSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_prs: Vec<PrListEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_ci: Vec<CiListEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commit_statuses: Vec<BuildStatusSummary>,
    pub suggested_commands: Vec<String>,
}

pub async fn run_watch(g: &GlobalArgs, interval_secs: u64) -> Result<()> {
    let theme = Theme::current();
    loop {
        // Run status and capture output
        let result = run_inner(g).await;
        match result {
            Ok(out) => {
                let human = render_human(&out);
                // Clear previous output — preserve scrollback buffer
                eprint!("\x1B[H\x1B[J");
                eprint!(
                    "{} (refreshing every {interval_secs}s — Ctrl+C to stop)\n\n",
                    theme.bold("bbr status --watch")
                );
                let fmt = Formatter::from_json_flag(g.json);
                fmt.print(&out, &human)?;
            }
            Err(e) => {
                eprint!("\x1B[2J\x1B[H");
                eprintln!("bbr: {e}");
                if matches!(
                    e,
                    crate::error::BitbucketError::AuthFailed(_)
                        | crate::error::BitbucketError::RateLimit(_)
                ) {
                    return Err(e);
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
    }
}

pub async fn run(g: &GlobalArgs) -> Result<()> {
    let out = run_inner(g).await?;
    let fmt = Formatter::from_json_flag(g.json);
    let human = render_human(&out);
    fmt.print(&out, &human)
}

pub async fn run_short(g: &GlobalArgs) -> Result<()> {
    let out = run_inner(g).await?;
    let fmt = Formatter::from_json_flag(g.json);
    let human = render_short(&out);
    fmt.print(&out, &human)
}

pub async fn run_overview(g: &GlobalArgs) -> Result<()> {
    let repo = resolve_repo(g)?;
    let head = current_head()?;
    let client = client(g)?;

    let (pr, pipeline, commit_statuses_page, recent_prs, recent_ci) = tokio::try_join!(
        client.pr_for_branch(&repo.workspace, &repo.slug, &head.branch),
        client.latest_pipeline(&repo.workspace, &repo.slug, Some(&head.branch)),
        client.commit_statuses(&repo.workspace, &repo.slug, &head.commit),
        client.list_prs(
            &repo.workspace,
            &repo.slug,
            PrState::Open,
            5,
            None,
            None,
            None
        ),
        client.list_pipelines(&repo.workspace, &repo.slug, None, 5),
    )?;

    let raw_steps = match &pipeline {
        Some(p) => client
            .list_steps(&repo.workspace, &repo.slug, &p.uuid)
            .await
            .map(|page| page.values)
            .unwrap_or_default(),
        None => Vec::new(),
    };

    let pr_summary = pr.as_ref().map(pr_summary);
    let pipeline_summary = pipeline.as_ref().map(|p| pipeline_summary(p, &raw_steps));

    let commit_statuses: Vec<BuildStatusSummary> = commit_statuses_page
        .values
        .iter()
        .map(|s| BuildStatusSummary {
            state: s.state.clone(),
            key: s.key.clone(),
            url: s.url.clone(),
        })
        .collect();
    let suggested = suggested_commands(&pr_summary, &pipeline_summary);

    let out = OverviewOut {
        repo: RepoSummary {
            workspace: repo.workspace.clone(),
            slug: repo.slug.clone(),
            full_name: format!("{}/{}", repo.workspace, repo.slug),
        },
        branch: head.branch.clone(),
        commit: head.commit.clone(),
        pr: pr_summary,
        pipeline: pipeline_summary,
        commit_statuses,
        recent_prs: recent_prs
            .into_iter()
            .map(|p| PrListEntry {
                id: p.id,
                state: p.state.clone(),
                title: p.title.clone(),
                source: p.source_branch().to_string(),
                destination: p.destination_branch().to_string(),
                author: p.author.map(|a| a.display_name),
            })
            .collect(),
        recent_ci: recent_ci
            .into_iter()
            .map(|c| CiListEntry {
                build_number: c.build_number,
                state: c.state_name().to_string(),
                branch: c.target.ref_name,
                duration_seconds: c.duration_in_seconds,
                commit: c.target.commit.as_ref().map(|cc| cc.hash.clone()),
            })
            .collect(),
        suggested_commands: suggested,
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = render_overview_human(&out);
    fmt.print(&out, &human)
}

pub async fn run_inner(g: &GlobalArgs) -> Result<StatusOut> {
    let repo = resolve_repo(g)?;
    let head = current_head()?;
    let client = client(g)?;

    let (pr, pipeline, commit_statuses_page) = tokio::try_join!(
        client.pr_for_branch(&repo.workspace, &repo.slug, &head.branch),
        client.latest_pipeline(&repo.workspace, &repo.slug, Some(&head.branch)),
        client.commit_statuses(&repo.workspace, &repo.slug, &head.commit),
    )?;

    let raw_steps = match &pipeline {
        Some(p) => client
            .list_steps(&repo.workspace, &repo.slug, &p.uuid)
            .await
            .map(|page| page.values)
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let pr_summary = pr.as_ref().map(pr_summary);
    let pipeline_summary = pipeline.as_ref().map(|p| pipeline_summary(p, &raw_steps));

    let commit_statuses: Vec<BuildStatusSummary> = commit_statuses_page
        .values
        .iter()
        .map(|s| BuildStatusSummary {
            state: s.state.clone(),
            key: s.key.clone(),
            url: s.url.clone(),
        })
        .collect();

    let suggested_commands = suggested_commands(&pr_summary, &pipeline_summary);

    Ok(StatusOut {
        repo: RepoSummary {
            workspace: repo.workspace.clone(),
            slug: repo.slug.clone(),
            full_name: format!("{}/{}", repo.workspace, repo.slug),
        },
        branch: head.branch.clone(),
        commit: head.commit.clone(),
        pr: pr_summary,
        pipeline: pipeline_summary,
        commit_statuses,
        suggested_commands,
    })
}

fn pr_summary(pr: &PullRequest) -> PrSummary {
    PrSummary {
        id: pr.id,
        state: pr.state.clone(),
        title: pr.title.clone(),
        source: pr.source_branch().to_string(),
        destination: pr.destination_branch().to_string(),
        url: pr.links.html.href.clone(),
        author: pr.author.as_ref().map(|a| a.display_name.clone()),
        comment_count: pr.comment_count,
        task_count: pr.task_count,
        reviewers: reviewers(pr),
    }
}

fn reviewers(pr: &PullRequest) -> Vec<ReviewerSummary> {
    let source = if pr.reviewers.is_empty() {
        pr.participants
            .iter()
            .filter(|p| p.role.eq_ignore_ascii_case("REVIEWER"))
            .collect::<Vec<_>>()
    } else {
        pr.reviewers.iter().collect::<Vec<_>>()
    };
    source.into_iter().map(reviewer_summary).collect()
}

fn reviewer_summary(p: &Participant) -> ReviewerSummary {
    ReviewerSummary {
        display_name: if p.display_name.is_empty() {
            p.user
                .as_ref()
                .map(|u| u.display_name.clone())
                .unwrap_or_default()
        } else {
            p.display_name.clone()
        },
        approved: p.approved,
    }
}

fn pipeline_summary(p: &Pipeline, raw_steps: &[PipelineStep]) -> PipelineSummary {
    PipelineSummary {
        uuid: p.uuid.clone(),
        state: p.state_name().to_string(),
        duration_seconds: p.duration_in_seconds,
        branch: p.target.ref_name.clone(),
        commit: p.target.commit.as_ref().map(|c| c.hash.clone()),
        url: p.links.html.href.clone(),
        failing_steps: raw_steps
            .iter()
            .filter(|s| s.is_failed())
            .map(|s| s.name.clone())
            .collect(),
        steps: raw_steps.iter().map(step_summary).collect(),
    }
}

fn step_summary(s: &PipelineStep) -> StepSummary {
    StepSummary {
        uuid: s.uuid.clone(),
        name: s.name.clone(),
        state: s.state_name().to_string(),
        duration_seconds: s.duration_in_seconds,
    }
}

fn suggested_commands(pr: &Option<PrSummary>, pipeline: &Option<PipelineSummary>) -> Vec<String> {
    let mut commands = Vec::new();
    if pr.is_some() {
        commands.push("bbr open pr".into());
    } else {
        commands.push("bbr pr create --title \"...\"".into());
    }
    match pipeline {
        Some(p) if !p.failing_steps.is_empty() || p.state.eq_ignore_ascii_case("FAILED") => {
            commands.push("bbr ci logs --failed".into());
            commands.push("bbr ci watch --logs".into());
        }
        Some(p)
            if p.state.eq_ignore_ascii_case("INPROGRESS")
                || p.state.eq_ignore_ascii_case("RUNNING") =>
        {
            commands.push("bbr ci watch --logs".into());
            commands.push("bbr open ci".into());
        }
        Some(_) => commands.push("bbr open ci".into()),
        None => commands.push("bbr ci status".into()),
    }
    commands
}

fn render_short(out: &StatusOut) -> String {
    let theme = Theme::current();
    let pr = match &out.pr {
        Some(p) => {
            let state = match p.state.to_ascii_uppercase().as_str() {
                "OPEN" => theme.bold(&p.state),
                "MERGED" => theme.success(&p.state),
                _ => theme.error(&p.state),
            };
            format!("{} {}", theme.bold(&format!("#{}", p.id)), state,)
        }
        None => theme.dim("no PR").to_string(),
    };
    let ci = match &out.pipeline {
        Some(p) => {
            let state = match p.state.to_ascii_uppercase().as_str() {
                "SUCCESSFUL" => theme.success(&p.state),
                "FAILED" => theme.error(&p.state),
                _ => theme.warn(&p.state),
            };
            format!("{}  {}", state, human_duration(p.duration_seconds))
        }
        None => theme.dim("no CI").to_string(),
    };
    format!(
        "{}  {}  {}  {} | {}",
        theme.bold(&out.repo.full_name),
        theme.bold(&out.branch),
        truncate(&out.commit, 10),
        pr,
        ci,
    )
}

fn render_human(out: &StatusOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();
    s.push_str(&format!("{}\n", theme.bold(&out.repo.full_name)));
    s.push_str(&format!("{}\n", theme.separator()));
    s.push_str(&format!(
        "{} {}\n",
        theme.label("Branch:"),
        theme.bold(&out.branch)
    ));
    s.push_str(&format!("{} {}\n", theme.label("Commit:"), &out.commit));

    match &out.pr {
        Some(pr) => {
            s.push_str(&format!(
                "\n{} PR #{} — {}\n",
                theme.bullet(),
                pr.id,
                pr.state.to_lowercase()
            ));
            s.push_str(&format!("{}\n", theme.separator()));
            s.push_str(&format!(
                "  {} {} → {}\n",
                theme.label("Branches:"),
                pr.source,
                pr.destination
            ));
            s.push_str(&format!("  {}{}\n", theme.label("Title:"), pr.title));
            if let Some(a) = &pr.author {
                s.push_str(&format!("  {}{a}\n", theme.label("Author:")));
            }
            if !pr.reviewers.is_empty() {
                let reviewers = pr
                    .reviewers
                    .iter()
                    .map(|r| {
                        format!(
                            "{}{}",
                            r.display_name,
                            if r.approved { " (approved)" } else { "" }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                s.push_str(&format!("  {}{reviewers}\n", theme.label("Reviewers:")));
            }
            s.push_str(&format!(
                "  {} {}  |  {} {}\n",
                theme.label("Comments:"),
                pr.comment_count,
                theme.label("Tasks:"),
                pr.task_count
            ));
            if let Some(u) = &pr.url {
                s.push_str(&format!("  {}{u}\n", theme.label("URL:")));
            }
        }
        None => s.push_str(&format!(
            "\n  {} PR: none\n",
            theme.dim("(no open PR for this branch)")
        )),
    }

    match &out.pipeline {
        Some(p) => {
            let dur_str = human_duration(p.duration_seconds);
            s.push_str(&format!("\n{} Pipeline\n", theme.bullet(),));
            s.push_str(&format!("{}\n", theme.separator()));
            s.push_str(&format!(
                "  {}  {}  ({dur_str})\n",
                theme.status_glyph(&p.state),
                p.state,
            ));
            if let Some(b) = &p.branch {
                s.push_str(&format!("  {}{b}\n", theme.label("Branch:")));
            }
            s.push_str(&format!(
                "  {}{}\n",
                theme.label("Commit:"),
                p.commit.as_deref().unwrap_or("-")
            ));
            if !p.failing_steps.is_empty() {
                s.push_str(&format!(
                    "  {}{}\n",
                    theme.label("Failing:"),
                    p.failing_steps.join(", ")
                ));
            }
            if let Some(u) = &p.url {
                s.push_str(&format!("  {}{u}\n", theme.label("URL:")));
            }
            if !p.steps.is_empty() {
                let max_width = p
                    .steps
                    .iter()
                    .map(|s| s.name.chars().count())
                    .max()
                    .unwrap_or(0)
                    .max(18);
                for st in &p.steps {
                    s.push_str(&format!(
                        "  {} {:<width$}  {}\n",
                        theme.status_glyph(&st.state),
                        st.name,
                        human_duration(st.duration_seconds),
                        width = max_width
                    ));
                }
            }
        }
        None => s.push_str(&format!(
            "\n  {} CI: none\n",
            theme.dim("(no pipeline for this branch)")
        )),
    }

    if !out.commit_statuses.is_empty() {
        s.push_str(&format!("\n{} Build Statuses\n", theme.bullet()));
        let mut table = Table::new().headers(["State", "Key"]);
        for cs in &out.commit_statuses {
            table = table.add_row([theme.status_glyph(&cs.state), cs.key.clone()]);
        }
        s.push_str(&table.render());
    }

    if !out.suggested_commands.is_empty() {
        s.push_str(&format!("\n{}\n", theme.label("Next:")));
        for cmd in &out.suggested_commands {
            s.push_str(&format!("  {cmd}\n"));
        }
    }

    s
}

fn render_overview_human(out: &OverviewOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();
    s.push_str(&format!("{}\n", theme.bold(&out.repo.full_name)));
    s.push_str(&format!(
        "Branch: {}  Commit: {}\n",
        theme.bold(&out.branch),
        out.commit
    ));
    s.push_str(&format!("{}\n", theme.separator()));

    match &out.pr {
        Some(pr) => {
            s.push_str(&format!(
                "\n{} PR #{} — {}\n",
                theme.bullet(),
                pr.id,
                pr.state.to_lowercase()
            ));
            s.push_str(&format!("{}\n", theme.separator()));
            s.push_str(&format!(
                "  {} {} → {}\n",
                theme.label("Branches:"),
                pr.source,
                pr.destination
            ));
            s.push_str(&format!("  {}{}\n", theme.label("Title:"), pr.title));
            if let Some(a) = &pr.author {
                s.push_str(&format!("  {}{a}\n", theme.label("Author:")));
            }
            if !pr.reviewers.is_empty() {
                let reviewers = pr
                    .reviewers
                    .iter()
                    .map(|r| {
                        format!(
                            "{}{}",
                            r.display_name,
                            if r.approved { " (approved)" } else { "" }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                s.push_str(&format!("  {}{reviewers}\n", theme.label("Reviewers:")));
            }
            s.push_str(&format!(
                "  {} {}  |  {} {}\n",
                theme.label("Comments:"),
                pr.comment_count,
                theme.label("Tasks:"),
                pr.task_count
            ));
            if let Some(u) = &pr.url {
                s.push_str(&format!("  {}{u}\n", theme.label("URL:")));
            }
        }
        None => s.push_str(&format!(
            "\n  {} PR: none\n",
            theme.dim("(no open PR for this branch)")
        )),
    }

    match &out.pipeline {
        Some(p) => {
            let dur_str = human_duration(p.duration_seconds);
            s.push_str(&format!("\n{} Pipeline\n", theme.bullet(),));
            s.push_str(&format!("{}\n", theme.separator()));
            s.push_str(&format!(
                "  {}  {}  ({dur_str})\n",
                theme.status_glyph(&p.state),
                p.state,
            ));
            if let Some(b) = &p.branch {
                s.push_str(&format!("  {}{b}\n", theme.label("Branch:")));
            }
            s.push_str(&format!(
                "  {}{}\n",
                theme.label("Commit:"),
                p.commit.as_deref().unwrap_or("-")
            ));
            if !p.failing_steps.is_empty() {
                s.push_str(&format!(
                    "  {}{}\n",
                    theme.label("Failing:"),
                    p.failing_steps.join(", ")
                ));
            }
            if let Some(u) = &p.url {
                s.push_str(&format!("  {}{u}\n", theme.label("URL:")));
            }
            if !p.steps.is_empty() {
                let max_width = p
                    .steps
                    .iter()
                    .map(|s| s.name.chars().count())
                    .max()
                    .unwrap_or(0)
                    .max(18);
                for st in &p.steps {
                    s.push_str(&format!(
                        "  {} {:<width$}  {}\n",
                        theme.status_glyph(&st.state),
                        st.name,
                        human_duration(st.duration_seconds),
                        width = max_width
                    ));
                }
            }
        }
        None => s.push_str(&format!(
            "\n  {} CI: none\n",
            theme.dim("(no pipeline for this branch)")
        )),
    }

    if !out.recent_prs.is_empty() {
        s.push_str(&format!("\n{} Recent PRs\n", theme.bullet()));
        let mut table =
            Table::new().headers(["ID", "State", "Title", "Source", "Destination", "Author"]);
        for pr in &out.recent_prs {
            let state = match pr.state.to_ascii_uppercase().as_str() {
                "OPEN" => theme.bold(&pr.state),
                "MERGED" => theme.success(&pr.state),
                _ => theme.dim(&pr.state),
            };
            table = table.add_row([
                pr.id.to_string(),
                state.into_owned(),
                truncate(&pr.title, 50),
                truncate(&pr.source, 25),
                truncate(&pr.destination, 25),
                pr.author.as_deref().unwrap_or("-").to_string(),
            ]);
        }
        s.push_str(&table.render());
    }

    if !out.recent_ci.is_empty() {
        s.push_str(&format!("\n{} Recent CI\n", theme.bullet()));
        let mut table = Table::new().headers(["#", "State", "Branch", "Duration"]);
        for ci in &out.recent_ci {
            let state = match ci.state.to_ascii_uppercase().as_str() {
                "SUCCESSFUL" => theme.success(&ci.state),
                "FAILED" => theme.error(&ci.state),
                "INPROGRESS" => theme.warn(&ci.state),
                _ => theme.dim(&ci.state),
            };
            table = table.add_row([
                format!("#{}", ci.build_number),
                state.into_owned(),
                ci.branch.as_deref().unwrap_or("-").to_string(),
                human_duration(ci.duration_seconds),
            ]);
        }
        s.push_str(&table.render());
    }

    if !out.commit_statuses.is_empty() {
        s.push_str(&format!("\n{} Build Statuses\n", theme.bullet()));
        let mut table = Table::new().headers(["State", "Key"]);
        for cs in &out.commit_statuses {
            table = table.add_row([theme.status_glyph(&cs.state), cs.key.clone()]);
        }
        s.push_str(&table.render());
    }

    if !out.suggested_commands.is_empty() {
        s.push_str(&format!("\n{}\n", theme.label("Next:")));
        for cmd in &out.suggested_commands {
            s.push_str(&format!("  {cmd}\n"));
        }
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::pr::Participant;

    #[test]
    fn renders_sections_and_separators() {
        let out = StatusOut {
            repo: RepoSummary {
                workspace: "ws".into(),
                slug: "repo".into(),
                full_name: "ws/repo".into(),
            },
            branch: "feat".into(),
            commit: "abc123".into(),
            pr: Some(PrSummary {
                id: 42,
                state: "OPEN".into(),
                title: "Add stuff".into(),
                source: "feat".into(),
                destination: "main".into(),
                url: Some("https://...".into()),
                author: Some("Alice".into()),
                comment_count: 3,
                task_count: 1,
                reviewers: vec![],
            }),
            pipeline: Some(PipelineSummary {
                uuid: "p-1".into(),
                state: "SUCCESSFUL".into(),
                duration_seconds: 468,
                branch: Some("feat".into()),
                commit: Some("abc123".into()),
                url: Some("https://...".into()),
                failing_steps: vec![],
                steps: vec![StepSummary {
                    uuid: "s-1".into(),
                    name: "Build".into(),
                    state: "SUCCESSFUL".into(),
                    duration_seconds: 300,
                }],
            }),
            commit_statuses: vec![],
            suggested_commands: vec!["bbr open pr".into(), "bbr open ci".into()],
        };
        let out = render_human(&out);
        assert!(out.contains("ws/repo"), "header with bold repo name");
        assert!(out.contains("Branch:"), "label prefix");
        assert!(out.contains("PR #42"), "PR section");
        assert!(out.contains("●") || out.contains("*"), "bullet marker");
        assert!(out.contains("SUCCESSFUL"), "pipeline state");
        assert!(out.contains("7m 48s"), "human duration");
        assert!(out.contains("Next:"), "suggestions section");
    }

    #[test]
    fn renders_empty_state() {
        let out = StatusOut {
            repo: RepoSummary {
                workspace: "w".into(),
                slug: "r".into(),
                full_name: "w/r".into(),
            },
            branch: "main".into(),
            commit: "abc".into(),
            pr: None,
            pipeline: None,
            commit_statuses: vec![],
            suggested_commands: vec![],
        };
        let out = render_human(&out);
        assert!(out.contains("no open PR"));
        assert!(out.contains("no pipeline"));
    }

    #[test]
    fn suggests_failed_log_command_for_failed_pipeline() {
        let pipeline = Some(PipelineSummary {
            uuid: "p".into(),
            state: "FAILED".into(),
            duration_seconds: 1,
            branch: Some("main".into()),
            commit: None,
            url: None,
            failing_steps: vec!["tests".into()],
            steps: Vec::new(),
        });
        let commands = suggested_commands(&None, &pipeline);
        assert!(commands.contains(&"bbr ci logs --failed".into()));
    }

    #[test]
    fn suggested_commands_with_pr_uses_open_pr() {
        let pr = Some(PrSummary {
            id: 1,
            state: "OPEN".into(),
            title: "fix".into(),
            source: "f".into(),
            destination: "m".into(),
            url: None,
            author: None,
            comment_count: 0,
            task_count: 0,
            reviewers: vec![],
        });
        let commands = suggested_commands(&pr, &None);
        assert!(commands.contains(&"bbr open pr".into()));
        assert!(!commands.contains(&"bbr pr create".into()));
    }

    #[test]
    fn suggested_commands_without_pr_suggests_create() {
        let commands = suggested_commands(&None, &None);
        assert!(commands.contains(&"bbr pr create --title \"...\"".into()));
    }

    #[test]
    fn suggested_commands_inprogress_pipeline_suggests_watch() {
        let pipeline = Some(PipelineSummary {
            uuid: "p".into(),
            state: "INPROGRESS".into(),
            duration_seconds: 10,
            branch: Some("main".into()),
            commit: None,
            url: None,
            failing_steps: vec![],
            steps: vec![],
        });
        let commands = suggested_commands(&None, &pipeline);
        assert!(commands.contains(&"bbr ci watch --logs".into()));
        assert!(commands.contains(&"bbr open ci".into()));
    }

    #[test]
    fn suggested_commands_successful_pipeline_suggests_open_ci() {
        let pipeline = Some(PipelineSummary {
            uuid: "p".into(),
            state: "SUCCESSFUL".into(),
            duration_seconds: 10,
            branch: Some("main".into()),
            commit: None,
            url: None,
            failing_steps: vec![],
            steps: vec![],
        });
        let commands = suggested_commands(&None, &pipeline);
        assert!(commands.contains(&"bbr open ci".into()));
    }

    #[test]
    fn suggested_commands_no_pipeline_suggests_ci_status() {
        let commands = suggested_commands(&None, &None);
        assert!(commands.contains(&"bbr ci status".into()));
    }

    #[test]
    fn reviewer_summary_uses_display_name() {
        let p = Participant {
            display_name: "Bob".into(),
            approved: true,
            ..Default::default()
        };
        let summary = reviewer_summary(&p);
        assert_eq!(summary.display_name, "Bob");
        assert!(summary.approved);
    }

    #[test]
    fn reviewer_summary_falls_back_to_user_display_name() {
        let p = Participant {
            display_name: String::new(),
            approved: false,
            user: Some(crate::api::pr::User {
                display_name: "Alice".into(),
                uuid: None,
                nickname: None,
                links: None,
            }),
            ..Default::default()
        };
        let summary = reviewer_summary(&p);
        assert_eq!(summary.display_name, "Alice");
    }

    #[test]
    fn reviewers_uses_reviewers_field_when_non_empty() {
        let pr = PullRequest {
            id: 1,
            title: "PR".into(),
            state: "OPEN".into(),
            source: crate::api::pr::BranchRef::default(),
            destination: crate::api::pr::BranchRef::default(),
            reviewers: vec![Participant {
                display_name: "Bob".into(),
                approved: false,
                ..Default::default()
            }],
            participants: vec![Participant {
                display_name: "Charlie".into(),
                approved: true,
                role: "REVIEWER".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let revs = reviewers(&pr);
        assert_eq!(revs.len(), 1);
        assert_eq!(revs[0].display_name, "Bob");
    }

    #[test]
    fn reviewers_falls_back_to_participants() {
        let pr = PullRequest {
            id: 1,
            title: "PR".into(),
            state: "OPEN".into(),
            source: crate::api::pr::BranchRef::default(),
            destination: crate::api::pr::BranchRef::default(),
            reviewers: vec![],
            participants: vec![Participant {
                display_name: "Dave".into(),
                approved: true,
                role: "REVIEWER".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let revs = reviewers(&pr);
        assert_eq!(revs.len(), 1);
        assert_eq!(revs[0].display_name, "Dave");
    }

    #[test]
    fn pipeline_summary_builds_from_pipeline() {
        let p = crate::api::pipeline::Pipeline {
            uuid: "{uuid}".into(),
            build_number: 42,
            state: crate::api::pipeline::PipelineState {
                name: "COMPLETED".into(),
                result: Some(crate::api::pipeline::PipelineResult {
                    name: "SUCCESSFUL".into(),
                    type_: None,
                }),
                stage: None,
            },
            duration_in_seconds: 120,
            target: crate::api::pipeline::PipelineTarget {
                ref_name: Some("main".into()),
                commit: Some(crate::api::pipeline::CommitRef { hash: "abc".into() }),
                ..Default::default()
            },
            links: crate::api::pr::Links {
                html: crate::api::pr::Link {
                    href: Some("https://url".into()),
                },
                self_: None,
            },
            ..Default::default()
        };
        let step = crate::api::pipeline::PipelineStep {
            uuid: "{s}".into(),
            name: "Build".into(),
            state: crate::api::pipeline::PipelineState {
                name: "SUCCESSFUL".into(),
                result: None,
                stage: None,
            },
            duration_in_seconds: 60,
            ..Default::default()
        };
        let summary = pipeline_summary(&p, &[step]);
        assert_eq!(summary.state, "SUCCESSFUL");
        assert_eq!(summary.duration_seconds, 120);
        assert_eq!(summary.steps.len(), 1);
        assert!(summary.failing_steps.is_empty());
    }

    #[test]
    fn pipeline_summary_collects_failing_steps() {
        let p = crate::api::pipeline::Pipeline {
            uuid: "{uuid}".into(),
            build_number: 42,
            state: crate::api::pipeline::PipelineState {
                name: "COMPLETED".into(),
                result: Some(crate::api::pipeline::PipelineResult {
                    name: "FAILED".into(),
                    type_: None,
                }),
                stage: None,
            },
            duration_in_seconds: 60,
            ..Default::default()
        };
        let step1 = crate::api::pipeline::PipelineStep {
            uuid: "{s1}".into(),
            name: "Build".into(),
            state: crate::api::pipeline::PipelineState {
                name: "SUCCESSFUL".into(),
                result: None,
                stage: None,
            },
            ..Default::default()
        };
        let step2 = crate::api::pipeline::PipelineStep {
            uuid: "{s2}".into(),
            name: "Test".into(),
            state: crate::api::pipeline::PipelineState {
                name: "FAILED".into(),
                result: None,
                stage: None,
            },
            ..Default::default()
        };
        let summary = pipeline_summary(&p, &[step1, step2]);
        assert_eq!(summary.failing_steps, vec!["Test"]);
    }

    #[test]
    fn pr_summary_builds_from_pr() {
        let pr = PullRequest {
            id: 7,
            title: "My PR".into(),
            state: "OPEN".into(),
            comment_count: 3,
            task_count: 1,
            source: crate::api::pr::BranchRef {
                branch: Some(crate::api::pr::Named {
                    name: "feature".into(),
                }),
                ..Default::default()
            },
            destination: crate::api::pr::BranchRef {
                branch: Some(crate::api::pr::Named {
                    name: "main".into(),
                }),
                ..Default::default()
            },
            author: Some(crate::api::pr::Participant {
                display_name: "Alice".into(),
                ..Default::default()
            }),
            links: crate::api::pr::Links {
                html: crate::api::pr::Link {
                    href: Some("https://url".into()),
                },
                self_: None,
            },
            ..Default::default()
        };
        let summary = pr_summary(&pr);
        assert_eq!(summary.id, 7);
        assert_eq!(summary.title, "My PR");
        assert_eq!(summary.author, Some("Alice".into()));
        assert_eq!(summary.comment_count, 3);
        assert_eq!(summary.task_count, 1);
    }

    #[test]
    fn render_short_shows_pr_and_ci() {
        let out = StatusOut {
            repo: RepoSummary {
                workspace: "w".into(),
                slug: "r".into(),
                full_name: "w/r".into(),
            },
            branch: "main".into(),
            commit: "abc123def456".into(),
            pr: Some(PrSummary {
                id: 1,
                state: "OPEN".into(),
                title: "fix".into(),
                source: "f".into(),
                destination: "m".into(),
                url: None,
                author: None,
                comment_count: 0,
                task_count: 0,
                reviewers: vec![],
            }),
            pipeline: Some(PipelineSummary {
                uuid: "p".into(),
                state: "SUCCESSFUL".into(),
                duration_seconds: 42,
                branch: None,
                commit: None,
                url: None,
                failing_steps: vec![],
                steps: vec![],
            }),
            commit_statuses: vec![],
            suggested_commands: vec![],
        };
        let short = render_short(&out);
        assert!(short.contains("#1"));
        assert!(short.contains("SUCCESSFUL"));
    }

    #[test]
    fn render_short_shows_dim_when_no_pr_or_ci() {
        let out = StatusOut {
            repo: RepoSummary {
                workspace: "w".into(),
                slug: "r".into(),
                full_name: "w/r".into(),
            },
            branch: "main".into(),
            commit: "abc".into(),
            pr: None,
            pipeline: None,
            commit_statuses: vec![],
            suggested_commands: vec![],
        };
        let short = render_short(&out);
        assert!(short.contains("no PR"));
        assert!(short.contains("no CI"));
    }

    #[test]
    fn status_out_serializes_to_json() {
        let out = StatusOut {
            repo: RepoSummary {
                workspace: "w".into(),
                slug: "r".into(),
                full_name: "w/r".into(),
            },
            branch: "b".into(),
            commit: "c".into(),
            pr: None,
            pipeline: None,
            commit_statuses: vec![BuildStatusSummary {
                state: "SUCCESSFUL".into(),
                key: "buildkite/test".into(),
                url: "https://url".into(),
            }],
            suggested_commands: vec!["bbr ci status".into()],
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["repo"]["full_name"], "w/r");
        assert_eq!(json["commit_statuses"][0]["key"], "buildkite/test");
        assert!(!json["suggested_commands"].as_array().unwrap().is_empty());
    }
}
