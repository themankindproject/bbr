//! Export formatters for status and overview data (Slack mrkdwn, Markdown).

use crate::commands::status::{OverviewOut, StatusOut};

pub fn format_slack(out: &StatusOut) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "*Status for `{}` (`{}`)*\n",
        out.branch, out.repo.full_name
    ));
    match &out.pr {
        Some(pr) => {
            s.push_str(&format!(
                "• PR #{} \"{}\" — *{}*\n",
                pr.id,
                pr.title,
                pr.state.to_ascii_uppercase()
            ));
            s.push_str(&format!(
                "  → {} | by @{}",
                pr.destination,
                pr.author.as_deref().unwrap_or("unknown")
            ));
            s.push_str(&format!(
                " | {} comments, {} tasks\n",
                pr.comment_count, pr.task_count
            ));
            if !pr.reviewers.is_empty() {
                let revs: Vec<String> = pr
                    .reviewers
                    .iter()
                    .map(|r| {
                        if r.approved {
                            format!("@{} (approved)", r.display_name)
                        } else {
                            format!("@{}", r.display_name)
                        }
                    })
                    .collect();
                s.push_str(&format!("  Reviewers: {}\n", revs.join(", ")));
            }
        }
        None => {
            s.push_str("• PR: None\n");
        }
    }
    match &out.pipeline {
        Some(p) => {
            s.push_str(&format!(
                "• Pipeline — *{}*\n",
                p.state.to_ascii_uppercase()
            ));
            if !p.failing_steps.is_empty() {
                s.push_str(&format!(
                    "  → Build step \"{}\" failed\n",
                    p.failing_steps.join(", ")
                ));
            }
            s.push_str(&format!(
                "  Duration: {}\n",
                crate::commands::human_duration(p.duration_seconds)
            ));
        }
        None => {
            s.push_str("• Pipeline: None\n");
        }
    }
    if !out.commit_statuses.is_empty() {
        let checks: Vec<String> = out
            .commit_statuses
            .iter()
            .map(|c| {
                let glyph = if c.state.eq_ignore_ascii_case("SUCCESSFUL") {
                    "[ok]"
                } else if c.state.eq_ignore_ascii_case("FAILED") {
                    "[X]"
                } else {
                    "[~]"
                };
                format!("{} {}", glyph, c.key)
            })
            .collect();
        s.push_str(&format!("• Status checks: {}\n", checks.join(", ")));
    }
    s
}

pub fn format_markdown(out: &StatusOut) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "## Status for `{}` (`{}`)\n\n",
        out.branch, out.repo.full_name
    ));
    s.push_str("### Pull Request\n");
    match &out.pr {
        Some(pr) => {
            s.push_str(&format!(
                "- **#{}** \"{}\" — {} → {} (by @{})\n",
                pr.id,
                pr.title,
                pr.state.to_ascii_uppercase(),
                pr.destination,
                pr.author.as_deref().unwrap_or("unknown")
            ));
            s.push_str(&format!(
                "  - Comments: {} | Tasks: {}\n",
                pr.comment_count, pr.task_count
            ));
            if !pr.reviewers.is_empty() {
                let revs: Vec<String> = pr
                    .reviewers
                    .iter()
                    .map(|r| {
                        if r.approved {
                            format!("@{} ✅", r.display_name)
                        } else {
                            format!("@{}", r.display_name)
                        }
                    })
                    .collect();
                s.push_str(&format!("  - Reviewers: {}\n", revs.join(", ")));
            }
        }
        None => {
            s.push_str("- None\n");
        }
    }
    s.push_str("\n### Pipeline\n");
    match &out.pipeline {
        Some(p) => {
            s.push_str(&format!(
                "- **{}** — Duration: {}\n",
                p.state.to_ascii_uppercase(),
                crate::commands::human_duration(p.duration_seconds)
            ));
            if !p.failing_steps.is_empty() {
                s.push_str(&format!(
                    "  - Failing steps: {}\n",
                    p.failing_steps.join(", ")
                ));
            }
        }
        None => {
            s.push_str("- None\n");
        }
    }
    if !out.commit_statuses.is_empty() {
        s.push_str("\n### Commit Statuses\n");
        for c in &out.commit_statuses {
            let emoji = if c.state.eq_ignore_ascii_case("SUCCESSFUL") {
                "✅"
            } else if c.state.eq_ignore_ascii_case("FAILED") {
                "❌"
            } else {
                "⚠️"
            };
            s.push_str(&format!("- {} {}\n", emoji, c.key));
        }
    }
    s
}

pub fn format_overview_slack(out: &OverviewOut) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "*Overview for `{}` (`{}`)*\n",
        out.branch, out.repo.full_name
    ));
    if let Some(pr) = &out.pr {
        s.push_str(&format!(
            "• Current PR #{} \"{}\" — *{}*\n",
            pr.id,
            pr.title,
            pr.state.to_ascii_uppercase()
        ));
    }
    if !out.recent_prs.is_empty() {
        s.push_str("• *Recent PRs*:\n");
        for pr in &out.recent_prs {
            s.push_str(&format!(
                "  - #{} \"{}\" ({}) → {} by @{}\n",
                pr.id,
                pr.title,
                pr.state,
                pr.destination,
                pr.author.as_deref().unwrap_or("unknown")
            ));
        }
    }
    if !out.recent_ci.is_empty() {
        s.push_str("• *Recent Pipelines*:\n");
        for ci in &out.recent_ci {
            s.push_str(&format!(
                "  - #{} ({}) on branch {} (Duration: {})\n",
                ci.build_number,
                ci.state,
                ci.branch.as_deref().unwrap_or("unknown"),
                crate::commands::human_duration(ci.duration_seconds)
            ));
        }
    }
    s
}

pub fn format_overview_markdown(out: &OverviewOut) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "## Overview for `{}` (`{}`)\n\n",
        out.branch, out.repo.full_name
    ));
    if let Some(pr) = &out.pr {
        s.push_str(&format!(
            "### Current PR\n- **#{}** \"{}\" — {}\n\n",
            pr.id,
            pr.title,
            pr.state.to_ascii_uppercase()
        ));
    }
    if !out.recent_prs.is_empty() {
        s.push_str("### Recent PRs\n");
        for pr in &out.recent_prs {
            s.push_str(&format!(
                "- **#{}** \"{}\" ({}) → {} (by @{})\n",
                pr.id,
                pr.title,
                pr.state,
                pr.destination,
                pr.author.as_deref().unwrap_or("unknown")
            ));
        }
        s.push('\n');
    }
    if !out.recent_ci.is_empty() {
        s.push_str("### Recent Pipelines\n");
        for ci in &out.recent_ci {
            s.push_str(&format!(
                "- **#{}** ({}) on branch `{}` — Duration: {}\n",
                ci.build_number,
                ci.state,
                ci.branch.as_deref().unwrap_or("unknown"),
                crate::commands::human_duration(ci.duration_seconds)
            ));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::status::*;

    fn minimal_status() -> StatusOut {
        StatusOut {
            repo: RepoSummary {
                workspace: "ws".into(),
                slug: "repo".into(),
                full_name: "ws/repo".into(),
            },
            branch: "main".into(),
            commit: "abc123".into(),
            pr: None,
            open_prs: vec![],
            pipeline: None,
            commit_statuses: vec![],
            suggested_commands: vec![],
        }
    }

    #[test]
    fn format_slack_with_no_pr_or_pipeline() {
        let out = minimal_status();
        let s = format_slack(&out);
        assert!(s.contains("main"));
        assert!(s.contains("ws/repo"));
        assert!(s.contains("PR: None"));
        assert!(s.contains("Pipeline: None"));
    }

    #[test]
    fn format_slack_with_pr() {
        let mut out = minimal_status();
        out.pr = Some(PrSummary {
            id: 42,
            state: "OPEN".into(),
            title: "Fix bug".into(),
            source: "fix".into(),
            destination: "main".into(),
            url: None,
            author: Some("alice".into()),
            comment_count: 3,
            task_count: 1,
            reviewers: vec![],
            lines_added: None,
            lines_removed: None,
            created_on: None,
            description: None,
            conflicts: None,
        });
        let s = format_slack(&out);
        assert!(s.contains("#42"));
        assert!(s.contains("Fix bug"));
        assert!(s.contains("OPEN"));
        assert!(s.contains("@alice"));
    }

    #[test]
    fn format_markdown_with_no_pr_or_pipeline() {
        let out = minimal_status();
        let s = format_markdown(&out);
        assert!(s.contains("## Status"));
        assert!(s.contains("main"));
        assert!(s.contains("None"));
    }

    #[test]
    fn format_markdown_with_pipeline() {
        let mut out = minimal_status();
        out.pipeline = Some(PipelineSummary {
            uuid: "pipe-uuid".into(),
            state: "SUCCESSFUL".into(),
            duration_seconds: 45,
            branch: Some("main".into()),
            commit: Some("abc123".into()),
            url: None,
            failing_steps: vec![],
            steps: vec![],
        });
        let s = format_markdown(&out);
        assert!(s.contains("SUCCESSFUL"));
        assert!(s.contains("45s"));
    }
}
