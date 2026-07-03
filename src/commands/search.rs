//! `bbr search` — code search across a workspace.

use serde::{Deserialize, Serialize};

use crate::api::BitbucketClient;
use crate::cli::GlobalArgs;
use crate::commands::{client, make_spinner, resolve_repo, SpinnerGuard};
use crate::error::Result;
use crate::output::Formatter;

#[derive(Debug, Serialize)]
pub struct SearchOut {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub total: u64,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub file: String,
    pub content_matches: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchApiResponse {
    #[serde(default)]
    pub(crate) values: Vec<SearchApiHit>,
    #[serde(default)]
    pub(crate) size: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchApiHit {
    pub(crate) file: Option<FileInfo>,
    pub(crate) content: Option<Vec<ContentBlock>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FileInfo {
    pub(crate) path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ContentBlock {
    pub(crate) lines: Option<Vec<ContentLine>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ContentLine {
    pub(crate) line: Option<u64>,
    pub(crate) segments: Option<Vec<Segment>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Segment {
    pub(crate) text: Option<String>,
    #[allow(dead_code)]
    pub(crate) r#match: Option<bool>,
}

impl BitbucketClient {
    pub(crate) async fn search_code(
        &self,
        workspace: &str,
        query: &str,
        repo: Option<&str>,
        limit: u32,
    ) -> Result<SearchApiResponse> {
        let q = match repo {
            Some(r) => format!("{query} repo:{r}"),
            None => query.to_string(),
        };
        let path = format!(
            "/workspaces/{workspace}/search/code?search_query={}&pagelen={}",
            crate::api::url_encode(&q),
            limit.min(100),
        );
        self.send(reqwest::Method::GET, &path, None).await
    }
}

pub async fn run(g: &GlobalArgs, query: &str, repo_filter: Option<&str>, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message(format!("Searching for '{query}'..."));
    let api_resp = client
        .search_code(&repo.workspace, query, repo_filter, limit)
        .await?;
    spinner.finish();

    let mut results = Vec::new();
    for hit in &api_resp.values {
        let file = hit
            .file
            .as_ref()
            .and_then(|f| f.path.as_deref())
            .unwrap_or("(unknown)")
            .to_string();

        let mut content_matches = Vec::new();
        if let Some(blocks) = &hit.content {
            for block in blocks {
                if let Some(lines) = &block.lines {
                    for entry in lines {
                        let text: String = entry
                            .segments
                            .as_ref()
                            .map(|segs| {
                                segs.iter()
                                    .filter_map(|s| s.text.as_deref())
                                    .collect::<Vec<_>>()
                                    .join("")
                            })
                            .unwrap_or_default();
                        if !text.is_empty() {
                            let line_info =
                                entry.line.map(|n| format!(":{}", n)).unwrap_or_default();
                            content_matches.push(format!("{file}{line_info}  {text}"));
                        }
                    }
                }
            }
        }

        results.push(SearchResult {
            file: file.clone(),
            content_matches,
        });
    }

    let out = SearchOut {
        query: query.to_string(),
        total: api_resp.size,
        results,
    };

    let human = render_search(&out);
    Formatter::from_json_flag(g.json).print(&out, &human)
}

fn render_search(out: &SearchOut) -> String {
    if out.results.is_empty() {
        return format!("No results for '{}'.", out.query);
    }
    let mut s = format!("{} result(s) for '{}'\n", out.total, out.query);
    for r in &out.results {
        let _ = std::fmt::Write::write_fmt(&mut s, format_args!("  {}\n", r.file));
        for m in &r.content_matches {
            let _ = std::fmt::Write::write_fmt(&mut s, format_args!("    {m}\n"));
        }
    }
    s
}
