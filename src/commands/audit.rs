//! Repo Audit command (`bbr repo audit`).

use crate::api::repo::{BranchRestriction, DefaultReviewer};
use crate::cli::GlobalArgs;
use crate::commands::{client, make_spinner, resolve_repo};
use crate::error::Result;
use crate::output::theme::Theme;
use crate::output::Formatter;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AuditOut {
    pub workspace: String,
    pub total_repos: usize,
    pub repos: Vec<RepoAuditEntry>,
    pub summary: AuditSummary,
}

#[derive(Debug, Serialize)]
pub struct AuditSummary {
    pub total_issues: usize,
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct RepoAuditEntry {
    pub slug: String,
    pub branch_restrictions_count: usize,
    pub has_approval_requirement: bool,
    pub required_approvals: Option<u32>,
    pub has_default_reviewers: bool,
    pub default_reviewer_count: usize,
    pub issues: Vec<AuditIssue>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AuditIssue {
    pub severity: String, // "error" | "warning" | "info"
    pub message: String,
}

pub async fn run_audit(g: &GlobalArgs, slug_arg: Option<&str>) -> Result<()> {
    let client = client(g)?;
    let repo = resolve_repo(g)?;
    let ws = &repo.workspace;

    let spinner = make_spinner(g.json);

    let repos = if let Some(s) = slug_arg {
        spinner.set_message(format!("Fetching repository {}...", s));
        vec![client.get_repo(ws, s).await?]
    } else {
        spinner.set_message("Scanning workspace repositories...");
        client.list_repos(ws, 100).await?
    };

    let mut audits = Vec::new();

    for r in &repos {
        spinner.set_message(format!("Auditing repository {}...", r.slug));

        let (restrictions_res, reviewers_res) = tokio::join!(
            client.list_branch_restrictions(ws, &r.slug),
            client.list_default_reviewers(ws, &r.slug)
        );

        let restrictions = restrictions_res.unwrap_or_default();
        let reviewers = reviewers_res.unwrap_or_default();

        let audit_entry = audit_repo(&r.slug, &restrictions, &reviewers);
        audits.push(audit_entry);
    }

    spinner.finish_and_clear();

    let mut total_issues = 0;
    let mut errors = 0;
    let mut warnings = 0;
    let mut info = 0;

    for audit in &audits {
        for issue in &audit.issues {
            total_issues += 1;
            match issue.severity.as_str() {
                "error" => errors += 1,
                "warning" => warnings += 1,
                "info" => info += 1,
                _ => {}
            }
        }
    }

    let out = AuditOut {
        workspace: ws.clone(),
        total_repos: repos.len(),
        repos: audits,
        summary: AuditSummary {
            total_issues,
            errors,
            warnings,
            info,
        },
    };

    let human = render_audit(&out);
    Formatter::from_json_flag(g.json).print(&out, &human)
}

fn matches_main(r: &BranchRestriction) -> bool {
    if let Some(p) = &r.pattern {
        p == "main" || p == "master" || p == "*" || p == "development"
    } else {
        r.branch_match_kind == "branching_model"
    }
}

fn audit_repo(
    slug: &str,
    restrictions: &[BranchRestriction],
    default_reviewers: &[DefaultReviewer],
) -> RepoAuditEntry {
    let mut has_approval_requirement = false;
    let mut required_approvals = None;
    let mut push_restricted_main = false;
    let mut force_restricted = false;
    let mut delete_restricted = false;

    for r in restrictions {
        let is_main = matches_main(r);
        if r.kind == "require_approvals_to_merge" {
            has_approval_requirement = true;
            if let Some(val) = &r.value {
                if let Some(num) = val.as_u64() {
                    required_approvals = Some(num as u32);
                }
            }
        }
        if r.kind == "push" && is_main {
            push_restricted_main = true;
        }
        if r.kind == "force" || r.kind == "rewrite_history" {
            force_restricted = true;
        }
        if r.kind == "delete" {
            delete_restricted = true;
        }
    }

    let mut issues = Vec::new();
    if restrictions.is_empty() {
        issues.push(AuditIssue {
            severity: "warning".to_string(),
            message: "No branch restrictions configured".to_string(),
        });
    } else {
        if !has_approval_requirement {
            issues.push(AuditIssue {
                severity: "warning".to_string(),
                message: "No approval requirement for pull requests".to_string(),
            });
        } else if let Some(app) = required_approvals {
            if app < 2 {
                issues.push(AuditIssue {
                    severity: "error".to_string(),
                    message: format!("Only {} required approver (recommend ≥ 2)", app),
                });
            }
        }
        if !push_restricted_main {
            issues.push(AuditIssue {
                severity: "error".to_string(),
                message: "Direct pushes allowed to main/master branch".to_string(),
            });
        }
        if !force_restricted {
            issues.push(AuditIssue {
                severity: "warning".to_string(),
                message: "Force pushing/rewriting history allowed".to_string(),
            });
        }
        if !delete_restricted {
            issues.push(AuditIssue {
                severity: "info".to_string(),
                message: "Branch deletion allowed".to_string(),
            });
        }
    }
    if default_reviewers.is_empty() {
        issues.push(AuditIssue {
            severity: "info".to_string(),
            message: "No default reviewers configured".to_string(),
        });
    }

    RepoAuditEntry {
        slug: slug.to_string(),
        branch_restrictions_count: restrictions.len(),
        has_approval_requirement,
        required_approvals,
        has_default_reviewers: !default_reviewers.is_empty(),
        default_reviewer_count: default_reviewers.len(),
        issues,
    }
}

fn render_audit(out: &AuditOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();

    s.push_str(&format!(
        "{} Audit — {} — {} repos\n",
        theme.bullet(),
        out.workspace,
        out.total_repos
    ));
    s.push_str(&format!("{}\n\n", theme.separator()));

    for repo in &out.repos {
        if repo.issues.is_empty() {
            s.push_str(&format!(
                "{} {} ✓\n",
                repo.slug,
                theme.success("(0 issues)")
            ));
        } else {
            s.push_str(&format!("{} ({} issues)\n", repo.slug, repo.issues.len()));
            for issue in &repo.issues {
                let prefix = match issue.severity.as_str() {
                    "error" => theme.error("  ✖"),
                    "warning" => theme.warn("  ⚠"),
                    "info" => theme.dim("  ℹ"),
                    _ => "  ?".into(),
                };
                s.push_str(&format!("{} {}\n", prefix, issue.message));
            }
        }
        s.push('\n');
    }

    s.push_str(&format!(
        "Summary: {} issues ({} errors, {} warnings, {} info)",
        out.summary.total_issues, out.summary.errors, out.summary.warnings, out.summary.info
    ));

    s
}
