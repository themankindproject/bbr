//! `bb status` — PR + CI for the current branch (the killer feature).

use serde::Serialize;

use crate::api::pipeline::Pipeline;
use crate::api::pr::PullRequest;
use crate::cli::GlobalArgs;
use crate::commands::{client, current_repo};
use crate::error::Result;
use crate::git;
use crate::output::theme::Theme;
use crate::output::Formatter;

#[derive(Debug, Serialize)]
pub struct StatusOut {
    pub branch: String,
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<PrSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<PipelineSummary>,
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
}

#[derive(Debug, Serialize)]
pub struct PipelineSummary {
    pub uuid: String,
    pub state: String,
    pub duration_seconds: u64,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub steps: Vec<StepSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepSummary {
    pub name: String,
    pub state: String,
    pub duration_seconds: u64,
}

pub async fn run(g: &GlobalArgs) -> Result<()> {
    let repo = current_repo()?;
    let head = git::head()?;
    let client = client(g)?;

    let pr = client
        .pr_for_branch(&repo.workspace, &repo.slug, &head.branch)
        .await
        .ok()
        .flatten();
    let pipeline = client
        .latest_pipeline(&repo.workspace, &repo.slug, Some(&head.branch))
        .await
        .ok()
        .flatten();

    let steps = match &pipeline {
        Some(p) => client
            .list_steps(&repo.workspace, &repo.slug, &p.uuid)
            .await
            .map(|page| {
                page.values
                    .into_iter()
                    .map(|s| StepSummary {
                        name: s.name,
                        state: s.state.name,
                        duration_seconds: s.duration_in_seconds,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    };

    let out = StatusOut {
        branch: head.branch.clone(),
        commit: head.commit.clone(),
        pr: pr.as_ref().map(pr_summary),
        pipeline: pipeline
            .as_ref()
            .map(|p| pipeline_summary(p, steps.clone())),
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
    }
}

fn pipeline_summary(p: &Pipeline, steps: Vec<StepSummary>) -> PipelineSummary {
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
        steps,
    }
}

fn render_human(out: &StatusOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();
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
                "  {} ({}s)\n",
                theme.status_glyph(&p.state),
                p.duration_seconds
            ));
            if let Some(b) = &p.branch {
                s.push_str(&format!("  Branch: {b}"));
            }
            s.push_str(&format!(
                "  /  Commit: {}\n",
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
        None => s.push_str("\nCI: no pipeline found for this branch\n"),
    }

    s
}
