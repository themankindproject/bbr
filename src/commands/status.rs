//! `bb status` — PR + CI for the current branch (the killer feature).

use serde::Serialize;

use crate::api::pipeline::{Pipeline, PipelineStep};
use crate::api::pr::{Participant, PullRequest};
use crate::cli::GlobalArgs;
use crate::commands::{client, current_head, current_repo, human_duration};
use crate::error::Result;
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
    let head = current_head()?;
    let client = client(g)?;

    let (pr, pipeline) = tokio::try_join!(
        client.pr_for_branch(&repo.workspace, &repo.slug, &head.branch),
        client.latest_pipeline(&repo.workspace, &repo.slug, Some(&head.branch)),
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

    let commit_statuses = client
        .commit_statuses(&repo.workspace, &repo.slug, &head.commit)
        .await
        .map(|page| {
            page.values
                .iter()
                .map(|s| BuildStatusSummary {
                    state: s.state.clone(),
                    key: s.key.clone(),
                    url: s.url.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

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
        commit_statuses,
        suggested_commands,
    };

    let fmt = Formatter::from_json_flag(g.json);
    let human = render_human(&out);
    fmt.print(&out, &human)
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
        branch: p.target.ref_name.clone(),
        commit: p.target.commit.as_ref().map(|c| c.hash.clone()),
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
                s.push_str(&format!("  {}\n", theme.label("Steps:")));
                for st in &p.steps {
                    s.push_str(&format!(
                        "    {} {:<18}  {}\n",
                        theme.status_glyph(&st.state),
                        st.name,
                        human_duration(st.duration_seconds)
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
        s.push_str(&format!("{}\n", theme.separator()));
        for cs in &out.commit_statuses {
            let (glyph, colored) = match cs.state.to_ascii_uppercase().as_str() {
                "SUCCESSFUL" => ("[ok]", theme.success(&cs.state)),
                "FAILED" => ("[X]", theme.error(&cs.state)),
                "INPROGRESS" => ("[~]", theme.warn(&cs.state)),
                _ => ("[?]", theme.dim(&cs.state)),
            };
            s.push_str(&format!("  {} {}  {}\n", glyph, colored, cs.key));
        }
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
            suggested_commands: vec!["bb open pr".into(), "bb open ci".into()],
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
        assert!(commands.contains(&"bb ci logs --failed".into()));
    }
}
