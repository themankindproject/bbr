//! `bb status` — PR + CI for the current branch (the killer feature).

use serde::Serialize;

use crate::api::pipeline::{Pipeline, PipelineStep};
use crate::api::pr::{Participant, PullRequest};
use crate::cli::GlobalArgs;
use crate::commands::{client, current_repo};
use crate::error::{BitbucketError, Result};
use crate::git;
use crate::output::theme::Theme;
use crate::output::Formatter;

#[derive(Debug, Serialize)]
pub struct StatusOut {
    pub repo: RepoSummary,
    pub branch: String,
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<PrSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<PipelineSummary>,
    pub suggested_commands: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RepoSummary {
    pub workspace: String,
    pub slug: String,
    pub full_name: String,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct ReviewerSummary {
    pub display_name: String,
    pub approved: bool,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
pub struct StepSummary {
    pub uuid: String,
    pub name: String,
    pub state: String,
    pub duration_seconds: u64,
}

pub async fn run(g: &GlobalArgs) -> Result<()> {
    let repo = current_repo()?;
    let head = git::head()?;
    let client = client(g)?;

    let pr = fetch_optional(
        client
            .pr_for_branch(&repo.workspace, &repo.slug, &head.branch)
            .await,
    )?;
    let pipeline = fetch_optional(
        client
            .latest_pipeline(&repo.workspace, &repo.slug, Some(&head.branch))
            .await,
    )?;

    let raw_steps = match &pipeline {
        Some(p) => client
            .list_steps(&repo.workspace, &repo.slug, &p.uuid)
            .await
            .map(|page| page.values)
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let steps = raw_steps.iter().map(step_summary).collect::<Vec<_>>();
    let pipeline_summary = pipeline
        .as_ref()
        .map(|p| pipeline_summary(p, &raw_steps, steps.clone()));
    let pr_summary = pr.as_ref().map(pr_summary);
    let suggested_commands = suggested_commands(&pr_summary, &pipeline_summary);

    let out = StatusOut {
        repo: RepoSummary {
            workspace: repo.workspace.clone(),
            slug: repo.slug.clone(),
            full_name: format!("{}/{}", repo.workspace, repo.slug),
        },
        branch: head.branch.clone(),
        commit: head.commit.clone(),
        pr: pr_summary,
        pipeline: pipeline_summary,
        suggested_commands,
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = render_human(&out);
    fmt.print(&out, &human)
}

fn fetch_optional<T>(r: Result<Option<T>>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(v),
        Err(BitbucketError::NotFound(_)) => Ok(None),
        Err(e @ BitbucketError::AuthFailed(_))
        | Err(e @ BitbucketError::NoCredentials)
        | Err(e @ BitbucketError::RateLimit(_))
        | Err(e @ BitbucketError::Http(_)) => Err(e),
        Err(_) => Ok(None),
    }
}

fn pr_summary(pr: &PullRequest) -> PrSummary {
    PrSummary {
        id: pr.id,
        state: pr.state.clone(),
        title: pr.title.clone(),
        source: pr
            .source
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
        destination: pr
            .destination
            .branch
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_default(),
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

fn pipeline_summary(
    p: &Pipeline,
    raw_steps: &[PipelineStep],
    steps: Vec<StepSummary>,
) -> PipelineSummary {
    PipelineSummary {
        uuid: p.uuid.clone(),
        state: p.state_name().to_string(),
        duration_seconds: p.duration_in_seconds,
        branch: p.target.ref_.as_ref().map(|r| r.name.clone()),
        commit: p
            .target
            .ref_
            .as_ref()
            .and_then(|r| r.target.as_ref())
            .map(|t| t.hash.clone()),
        url: p.links.html.href.clone(),
        failing_steps: raw_steps
            .iter()
            .filter(|s| s.is_failed())
            .map(|s| s.name.clone())
            .collect(),
        steps,
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
        commands.push("bb open pr".into());
    } else {
        commands.push("bb pr create --title \"...\"".into());
    }
    match pipeline {
        Some(p) if !p.failing_steps.is_empty() || p.state.eq_ignore_ascii_case("FAILED") => {
            commands.push("bb ci logs --failed".into());
            commands.push("bb ci watch --logs".into());
        }
        Some(p)
            if p.state.eq_ignore_ascii_case("INPROGRESS")
                || p.state.eq_ignore_ascii_case("RUNNING") =>
        {
            commands.push("bb ci watch --logs".into());
            commands.push("bb open ci".into());
        }
        Some(_) => commands.push("bb open ci".into()),
        None => commands.push("bb ci status".into()),
    }
    commands
}

fn render_human(out: &StatusOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();
    s.push_str(&format!("Repo: {}\n", theme.bold(&out.repo.full_name)));
    s.push_str(&format!(
        "On branch: {}  (commit {})\n",
        theme.bold(&out.branch),
        out.commit
    ));

    match &out.pr {
        Some(pr) => {
            s.push_str(&format!("\nPR #{} — {}\n", pr.id, pr.state.to_lowercase()));
            s.push_str(&format!("  {} -> {}\n", pr.source, pr.destination));
            s.push_str(&format!("  Title: {}\n", pr.title));
            if let Some(a) = &pr.author {
                s.push_str(&format!("  Author: {a}\n"));
            }
            if !pr.reviewers.is_empty() {
                let reviewers = pr
                    .reviewers
                    .iter()
                    .map(|r| {
                        format!(
                            "{} {}",
                            r.display_name,
                            if r.approved { "approved" } else { "pending" }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                s.push_str(&format!("  Reviewers: {reviewers}\n"));
            }
            s.push_str(&format!(
                "  Comments: {}  /  Tasks: {}\n",
                pr.comment_count, pr.task_count
            ));
            if let Some(u) = &pr.url {
                s.push_str(&format!("  URL:   {u}\n"));
            }
        }
        None => s.push_str("\nPR: none (no open PR for this branch)\n"),
    }

    match &out.pipeline {
        Some(p) => {
            s.push_str("\nCI - last pipeline\n");
            s.push_str(&format!(
                "  {} {} ({}s)\n",
                theme.status_glyph(&p.state),
                p.state,
                p.duration_seconds
            ));
            if let Some(b) = &p.branch {
                s.push_str(&format!("  Branch: {b}"));
            }
            s.push_str(&format!(
                "  /  Commit: {}\n",
                p.commit.as_deref().unwrap_or("-")
            ));
            if !p.failing_steps.is_empty() {
                s.push_str(&format!("  Failing: {}\n", p.failing_steps.join(", ")));
            }
            if let Some(u) = &p.url {
                s.push_str(&format!("  URL:    {u}\n"));
            }
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
        None => s.push_str("\nCI: no pipeline found for this branch\n"),
    }

    if !out.suggested_commands.is_empty() {
        s.push_str("\nNext:\n");
        for cmd in &out.suggested_commands {
            s.push_str(&format!("  {cmd}\n"));
        }
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(commands.contains(&"bb ci logs --failed".into()));
    }
}
