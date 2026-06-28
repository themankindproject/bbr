//! Export formatters for status and overview data (Slack mrkdwn, Markdown).

use crate::commands::status::{OverviewOut, StatusOut};

pub fn format_slack(out: &StatusOut) -> String {
    let mut s = String::new();
    s.push_str(&format!("*Status for `{}` (`{}`)*\n", out.branch, out.repo.full_name));
    match &out.pr {
        Some(pr) => {
            s.push_str(&format!("• PR #{} \"{}\" — *{}*\n", pr.id, pr.title, pr.state.to_ascii_uppercase()));
            s.push_str(&format!("  → {} | by @{}", pr.destination, pr.author.as_deref().unwrap_or("unknown")));
            s.push_str(&format!(" | {} comments, {} tasks\n", pr.comment_count, pr.task_count));
            if !pr.reviewers.is_empty() {
                let revs: Vec<String> = pr.reviewers.iter().map(|r| {
                    if r.approved {
                        format!("@{} (approved)", r.display_name)
                    } else {
                        format!("@{}", r.display_name)
                    }
                }).collect();
                s.push_str(&format!("  Reviewers: {}\n", revs.join(", ")));
            }
        }
        None => {
            s.push_str("• PR: None\n");
        }
    }
    match &out.pipeline {
        Some(p) => {
            s.push_str(&format!("• Pipeline — *{}*\n", p.state.to_ascii_uppercase()));
            if !p.failing_steps.is_empty() {
                s.push_str(&format!("  → Build step \"{}\" failed\n", p.failing_steps.join(", ")));
            }
            s.push_str(&format!("  Duration: {}\n", crate::commands::human_duration(p.duration_seconds)));
        }
        None => {
            s.push_str("• Pipeline: None\n");
        }
    }
    if !out.commit_statuses.is_empty() {
        let checks: Vec<String> = out.commit_statuses.iter().map(|c| {
            let glyph = if c.state.eq_ignore_ascii_case("SUCCESSFUL") {
                "[ok]"
            } else if c.state.eq_ignore_ascii_case("FAILED") {
                "[X]"
            } else {
                "[~]"
            };
            format!("{} {}", glyph, c.key)
        }).collect();
        s.push_str(&format!("• Status checks: {}\n", checks.join(", ")));
    }
    s
}

pub fn format_markdown(out: &StatusOut) -> String {
    let mut s = String::new();
    s.push_str(&format!("## Status for `{}` (`{}`)\n\n", out.branch, out.repo.full_name));
    s.push_str("### Pull Request\n");
    match &out.pr {
        Some(pr) => {
            s.push_str(&format!("- **#{}** \"{}\" — {} → {} (by @{})\n", pr.id, pr.title, pr.state.to_ascii_uppercase(), pr.destination, pr.author.as_deref().unwrap_or("unknown")));
            s.push_str(&format!("  - Comments: {} | Tasks: {}\n", pr.comment_count, pr.task_count));
            if !pr.reviewers.is_empty() {
                let revs: Vec<String> = pr.reviewers.iter().map(|r| {
                    if r.approved {
                        format!("@{} ✅", r.display_name)
                    } else {
                        format!("@{}", r.display_name)
                    }
                }).collect();
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
            s.push_str(&format!("- **{}** — Duration: {}\n", p.state.to_ascii_uppercase(), crate::commands::human_duration(p.duration_seconds)));
            if !p.failing_steps.is_empty() {
                s.push_str(&format!("  - Failing steps: {}\n", p.failing_steps.join(", ")));
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
    s.push_str(&format!("*Overview for `{}` (`{}`)*\n", out.branch, out.repo.full_name));
    if let Some(pr) = &out.pr {
        s.push_str(&format!("• Current PR #{} \"{}\" — *{}*\n", pr.id, pr.title, pr.state.to_ascii_uppercase()));
    }
    if !out.recent_prs.is_empty() {
        s.push_str("• *Recent PRs*:\n");
        for pr in &out.recent_prs {
            s.push_str(&format!("  - #{} \"{}\" ({}) → {} by @{}\n", pr.id, pr.title, pr.state, pr.destination, pr.author.as_deref().unwrap_or("unknown")));
        }
    }
    if !out.recent_ci.is_empty() {
        s.push_str("• *Recent Pipelines*:\n");
        for ci in &out.recent_ci {
            s.push_str(&format!("  - #{} ({}) on branch {} (Duration: {})\n", ci.build_number, ci.state, ci.branch.as_deref().unwrap_or("unknown"), crate::commands::human_duration(ci.duration_seconds)));
        }
    }
    s
}

pub fn format_overview_markdown(out: &OverviewOut) -> String {
    let mut s = String::new();
    s.push_str(&format!("## Overview for `{}` (`{}`)\n\n", out.branch, out.repo.full_name));
    if let Some(pr) = &out.pr {
        s.push_str(&format!("### Current PR\n- **#{}** \"{}\" — {}\n\n", pr.id, pr.title, pr.state.to_ascii_uppercase()));
    }
    if !out.recent_prs.is_empty() {
        s.push_str("### Recent PRs\n");
        for pr in &out.recent_prs {
            s.push_str(&format!("- **#{}** \"{}\" ({}) → {} (by @{})\n", pr.id, pr.title, pr.state, pr.destination, pr.author.as_deref().unwrap_or("unknown")));
        }
        s.push('\n');
    }
    if !out.recent_ci.is_empty() {
        s.push_str("### Recent Pipelines\n");
        for ci in &out.recent_ci {
            s.push_str(&format!("- **#{}** ({}) on branch `{}` — Duration: {}\n", ci.build_number, ci.state, ci.branch.as_deref().unwrap_or("unknown"), crate::commands::human_duration(ci.duration_seconds)));
        }
    }
    s
}
