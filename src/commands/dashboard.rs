//! PR Dashboard (`bbr pr dashboard`).

use crate::api::pr::{Participant, PrState};
use crate::api::repo::Repository;
use crate::cli::GlobalArgs;
use crate::commands::{client, make_formatter, make_spinner, resolve_repo, truncate, SpinnerGuard};
use crate::error::Result;
use crate::output::theme::Theme;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DashboardOut {
    pub workspace: String,
    pub user: String,
    pub needs_review: Vec<DashboardPr>,
    pub my_prs: Vec<DashboardPr>,
    pub recent_activity: Vec<DashboardActivity>,
    pub repo_count: usize,
    /// Repos whose PR fetch failed. Reported instead of silently rendering as
    /// empty, which used to look like "no PRs".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<DashboardRepoError>,
}

#[derive(Debug, Serialize, Clone)]
pub struct DashboardRepoError {
    pub repo: String,
    pub error: String,
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
    // Use the same XDG-aware directory as the rest of bbr's config.
    let mut path = crate::config::config_dir()?;
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
            let _ = crate::config::write_private(&path, &content);
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

    // Check cache first for repos. The cache must satisfy the requested
    // `--repos` cap; using a 50-repo cache for `--repos 200` silently
    // truncated the scan.
    let wanted = repos_limit.unwrap_or(100);
    let repos = match read_cached_repos(ws) {
        Some(cached) if cached.len() >= wanted as usize => cached,
        _ => {
            spinner.set_message("Scanning workspace repositories...");
            let fetched = client.list_repos(ws, wanted).await?;
            write_cached_repos(ws, &fetched);
            fetched
        }
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

    // Fetch PRs concurrently from filtered repositories. Default to 15 to
    // stay fast; `--repos N` also raises the scan cap, not just the fetch cap.
    let scan_cap = repos_limit.unwrap_or(15) as usize;
    let scan_repos = if filtered_repos.len() > scan_cap {
        // `println` is dropped when the spinner is hidden (piped/quiet); the
        // cap must remain visible in that case too.
        spinner.println_visible(format!(
            "Note: scanning {scan_cap} of {} repos (pass a larger --repos to scan more).",
            filtered_repos.len()
        ));
        &filtered_repos[..scan_cap]
    } else {
        &filtered_repos[..]
    };
    spinner.set_message(format!("Fetching PRs from {} repos...", scan_repos.len()));

    let mut futures = Vec::new();

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
            match (open_prs, merged_prs) {
                (Ok(open), Ok(merged)) => Ok((slug_clone, open, merged)),
                (Err(e), _) | (_, Err(e)) => Err((slug_clone, e.to_string())),
            }
        });
    }

    // Cap concurrency: scanning many repos with unbounded fan-out can
    // burst past Bitbucket's rate limit. Order is irrelevant here — the
    // results are aggregated into lists below.
    use futures::StreamExt;
    let results = futures::stream::iter(futures)
        .buffer_unordered(5)
        .collect::<Vec<_>>()
        .await;

    let mut needs_review = Vec::new();
    let mut my_prs = Vec::new();
    let mut recent_activity = Vec::new();
    let mut errors = Vec::new();

    for result in results {
        let (slug, open, merged) = match result {
            Ok(row) => row,
            Err((repo, error)) => {
                errors.push(DashboardRepoError { repo, error });
                continue;
            }
        };
        for pr in open {
            let approvals = pr
                .reviewer_summaries()
                .iter()
                .filter(|r| r.approved)
                .count();

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
                .is_some_and(|r| r.is_approved())
                || pr
                    .participants
                    .iter()
                    .find(|p| matches_me(p))
                    .is_some_and(|p| p.is_approved());

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
        // Report the number of repos actually scanned, not the total matched.
        repo_count: scan_repos.len(),
        errors,
    };

    let human = render_dashboard(&out);
    make_formatter(g).print(&out, &human)
}

fn render_dashboard(out: &DashboardOut) -> String {
    let theme = Theme::current();
    let arrow = if theme.unicode_enabled() { "→" } else { "->" };
    let mut s = String::new();

    let dash = if theme.unicode_enabled() { "—" } else { "-" };
    s.push_str(&format!(
        "{} PR Dashboard {dash} {} (@{})\n",
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
                "  PR #{:<3}  {:<14}  \"{}\" {arrow} {}  by @{}\n",
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
                "  PR #{:<3}  {:<14}  \"{}\" {arrow} {}  OPEN  {} approvals\n",
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

    if !out.errors.is_empty() {
        s.push_str(&format!(
            "\n{}\n",
            theme.warn(&format!("Fetch Errors ({})", out.errors.len()))
        ));
        for err in &out.errors {
            s.push_str(&format!(
                "  {}: {}\n",
                truncate(&err.repo, 14),
                truncate(&err.error, 70)
            ));
        }
    }

    s
}
