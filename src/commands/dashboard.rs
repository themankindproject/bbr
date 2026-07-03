//! PR Dashboard (`bbr pr dashboard`).

use crate::api::pr::{Participant, PrState};
use crate::api::repo::Repository;
use crate::cli::GlobalArgs;
use crate::commands::{client, make_spinner, resolve_repo, truncate, SpinnerGuard};
use crate::error::Result;
use crate::output::theme::Theme;
use crate::output::Formatter;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DashboardOut {
    pub workspace: String,
    pub user: String,
    pub needs_review: Vec<DashboardPr>,
    pub my_prs: Vec<DashboardPr>,
    pub recent_activity: Vec<DashboardActivity>,
    pub repo_count: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct DashboardPr {
    pub repo: String,
    pub id: u64,
    pub title: String,
    pub state: String,
    pub author: Option<String>,
    pub created_on: Option<String>,
    pub url: Option<String>,
    pub approvals: usize,
    pub destination: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct DashboardActivity {
    pub repo: String,
    pub event: String, // "merged" | "declined" | "created"
    pub description: String,
    pub timestamp: Option<String>,
}

fn get_cache_path(workspace: &str) -> Option<std::path::PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("bbr");
    path.push("cache");
    let _ = std::fs::create_dir_all(&path);
    path.push(format!("dashboard-repos-{}.json", workspace));
    Some(path)
}

fn read_cached_repos(workspace: &str) -> Option<Vec<Repository>> {
    let path = get_cache_path(workspace)?;
    if !path.exists() {
        return None;
    }
    let metadata = std::fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    let elapsed = modified.elapsed().ok()?;
    if elapsed.as_secs() > 24 * 3600 {
        return None; // Cache expired (24h)
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cached_repos(workspace: &str, repos: &[Repository]) {
    if let Some(path) = get_cache_path(workspace) {
        if let Ok(content) = serde_json::to_string(repos) {
            let _ = std::fs::write(path, content);
        }
    }
}

pub async fn run_dashboard(
    g: &GlobalArgs,
    repos_limit: Option<u32>,
    filter: Option<&str>,
) -> Result<()> {
    let client = client(g)?;
    let repo = resolve_repo(g)?;
    let ws = &repo.workspace;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching current user...");
    let user = client.current_user().await?;
    let my_name = user.display_name.clone();

    // Check cache first for repos
    let repos = if let Some(cached) = read_cached_repos(ws) {
        cached
    } else {
        spinner.set_message("Scanning workspace repositories...");
        let limit = repos_limit.unwrap_or(200);
        let fetched = client.list_repos(ws, limit).await?;
        write_cached_repos(ws, &fetched);
        fetched
    };

    // Filter repos
    let filtered_repos: Vec<Repository> = if let Some(f) = filter {
        repos
            .into_iter()
            .filter(|r| r.slug.contains(f) || r.name.contains(f))
            .collect()
    } else {
        repos
    };

    let repo_count = filtered_repos.len();

    // Fetch PRs concurrently from filtered repositories
    spinner.set_message(format!("Fetching PRs from {} repos...", repo_count));
    let mut futures = Vec::new();
    // Fetch only from top 15 most recently updated repos to be fast and avoid hitting rate limits
    let scan_repos = if filtered_repos.len() > 15 {
        &filtered_repos[..15]
    } else {
        &filtered_repos[..]
    };

    for r in scan_repos {
        let ws_clone = ws.clone();
        let slug_clone = r.slug.clone();
        let client_ref = &client;
        futures.push(async move {
            let (open_prs, merged_prs) = tokio::join!(
                client_ref.list_prs(
                    &ws_clone,
                    &slug_clone,
                    PrState::Open,
                    20,
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                client_ref.list_prs(
                    &ws_clone,
                    &slug_clone,
                    PrState::Merged,
                    5,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            );
            (
                slug_clone,
                open_prs.unwrap_or_default(),
                merged_prs.unwrap_or_default(),
            )
        });
    }

    let results = futures::future::join_all(futures).await;

    let mut needs_review = Vec::new();
    let mut my_prs = Vec::new();
    let mut recent_activity = Vec::new();

    for (slug, open, merged) in results {
        for pr in open {
            // Count approvals from participants only (reviewers are a subset)
            let approvals = pr.participants.iter().filter(|p| p.approved).count();

            let matches_me = |p: &Participant| {
                if let (Some(u1), Some(u2)) = (&p.uuid, &user.uuid) {
                    if u1 == u2 {
                        return true;
                    }
                }
                if let (Some(n1), Some(n2)) = (&p.nickname, &user.nickname) {
                    if n1 == n2 {
                        return true;
                    }
                }
                p.display_name == user.display_name
            };

            let is_author = pr.author.as_ref().is_some_and(matches_me);

            let is_reviewer = pr.reviewers.iter().any(matches_me)
                || pr
                    .participants
                    .iter()
                    .any(|p| p.role.eq_ignore_ascii_case("REVIEWER") && matches_me(p));

            let my_approval = pr
                .reviewers
                .iter()
                .find(|r| matches_me(r))
                .is_some_and(|r| r.approved)
                || pr
                    .participants
                    .iter()
                    .find(|p| matches_me(p))
                    .is_some_and(|p| p.approved);

            let d_pr = DashboardPr {
                repo: slug.clone(),
                id: pr.id,
                title: pr.title.clone(),
                state: pr.state.clone(),
                author: pr.author.as_ref().map(|a| a.display_name.clone()),
                created_on: pr.created_on.clone(),
                url: pr.web_url().map(|u| u.to_string()),
                approvals,
                destination: pr.destination_branch().to_string(),
            };

            if is_author {
                my_prs.push(d_pr.clone());
            }

            if is_reviewer && !my_approval {
                needs_review.push(d_pr);
            }
        }

        for pr in merged {
            recent_activity.push(DashboardActivity {
                repo: slug.clone(),
                event: "merged".to_string(),
                description: format!(
                    "PR #{} \"{}\" by @{}",
                    pr.id,
                    pr.title,
                    pr.author.as_ref().map_or("unknown", |a| &a.display_name)
                ),
                timestamp: pr.updated_on.clone(),
            });
        }
    }

    spinner.finish();

    let out = DashboardOut {
        workspace: ws.clone(),
        user: my_name,
        needs_review,
        my_prs,
        recent_activity,
        repo_count,
    };

    let human = render_dashboard(&out);
    Formatter::from_json_flag(g.json).print(&out, &human)
}

fn render_dashboard(out: &DashboardOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();

    s.push_str(&format!(
        "{} PR Dashboard — {} (@{})\n",
        theme.bullet(),
        out.workspace,
        out.user
    ));
    s.push_str(&format!("{}\n\n", theme.separator()));

    s.push_str(&format!(
        "{}\n",
        theme.bold(&format!("Needs Your Review ({})", out.needs_review.len()))
    ));
    if out.needs_review.is_empty() {
        s.push_str("  (No pull requests pending your approval)\n");
    } else {
        for pr in &out.needs_review {
            s.push_str(&format!(
                "  PR #{:<3}  {:<14}  \"{}\" → {}  by @{}\n",
                pr.id,
                truncate(&pr.repo, 14),
                truncate(&pr.title, 35),
                pr.destination,
                pr.author.as_deref().unwrap_or("unknown")
            ));
        }
    }

    s.push_str(&format!(
        "\n{}\n",
        theme.bold(&format!("Your Open PRs ({})", out.my_prs.len()))
    ));
    if out.my_prs.is_empty() {
        s.push_str("  (No open pull requests)\n");
    } else {
        for pr in &out.my_prs {
            s.push_str(&format!(
                "  PR #{:<3}  {:<14}  \"{}\" → {}  OPEN  {} approvals\n",
                pr.id,
                truncate(&pr.repo, 14),
                truncate(&pr.title, 35),
                pr.destination,
                pr.approvals
            ));
        }
    }

    s.push_str(&format!("\n{}\n", theme.bold("Recent Activity")));
    if out.recent_activity.is_empty() {
        s.push_str("  (No recent activity)\n");
    } else {
        // Take up to 7 recent items
        let mut sorted_activity = out.recent_activity.clone();
        sorted_activity.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        for act in sorted_activity.iter().take(7) {
            let event_colored = if act.event == "merged" {
                theme.success("merged")
            } else {
                theme.error(&act.event)
            };
            s.push_str(&format!(
                "  {}  {:<14}  {}\n",
                event_colored,
                truncate(&act.repo, 14),
                act.description
            ));
        }
    }

    s
}
