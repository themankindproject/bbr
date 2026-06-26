//! Pull request endpoints and types.

use serde::{Deserialize, Serialize};

use super::BitbucketClient;
use crate::error::{BitbucketError, Result};

/// Filter for listing pull requests. Matches Bitbucket's `q=state="..."` filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Merged,
    Declined,
    All,
}

impl PrState {
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "open" | "" => PrState::Open,
            "merged" => PrState::Merged,
            "declined" => PrState::Declined,
            "all" => PrState::All,
            other => {
                return Err(BitbucketError::Other(format!(
                    "invalid --state '{other}' (expected open|merged|declined|all)"
                )))
            }
        })
    }

    pub fn as_query(&self) -> Option<&'static str> {
        match self {
            PrState::Open => Some("OPEN"),
            PrState::Merged => Some("MERGED"),
            PrState::Declined => Some("DECLINED"),
            PrState::All => None,
        }
    }
}

/// A single pull request as returned by the Bitbucket API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub state: String,
    #[serde(default)]
    pub comment_count: u64,
    #[serde(default)]
    pub task_count: u64,
    #[serde(default)]
    pub close_source_branch: bool,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub updated_on: Option<String>,
    pub source: BranchRef,
    pub destination: BranchRef,
    #[serde(default)]
    pub links: Links,
    #[serde(default)]
    pub author: Option<Participant>,
    #[serde(default)]
    pub participants: Vec<Participant>,
    #[serde(default)]
    pub reviewers: Vec<Participant>,
    #[serde(default)]
    pub summary: Option<Markdown>,
}

impl PullRequest {
    pub fn web_url(&self) -> Option<&str> {
        self.links.html.href.as_deref()
    }
}

/// `{branch_name} -> {branch_name}` plus repo metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRef {
    #[serde(default)]
    pub branch: Option<Named>,
    pub repository: RepoRef,
    #[serde(default)]
    pub commit: Option<CommitRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Named {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRef {
    #[serde(default)]
    pub full_name: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRef {
    pub hash: String,
    #[serde(default)]
    pub links: Option<Links>,
}

/// Hypermedia links bag (`{ html: { href }, self: { href } }`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Links {
    #[serde(default)]
    pub html: Link,
    #[serde(default)]
    pub self_: Option<Link>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Link {
    pub href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participant {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub uuid: Option<String>,
    pub role: String,
    #[serde(default)]
    pub approved: bool,
    #[serde(default)]
    pub user: Option<User>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub display_name: String,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub links: Option<Links>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Markdown {
    #[serde(default)]
    pub raw: String,
    #[serde(default)]
    pub markup: Option<String>,
}

/// A paginated collection wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paginated<T> {
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub page: u64,
    #[serde(default)]
    pub pagelen: u64,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub previous: Option<String>,
    pub values: Vec<T>,
}

/// Body for `POST /repositories/{ws}/{slug}/pullrequests`.
#[derive(Debug, Serialize)]
pub struct CreatePrRequest {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source: CreateBranchRef,
    pub destination: CreateBranchRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_source_branch: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reviewers: Vec<ReviewerRef>,
}

#[derive(Debug, Serialize)]
pub struct CreateBranchRef {
    pub branch: CreateNamed,
}

#[derive(Debug, Serialize)]
pub struct CreateNamed {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ReviewerRef {
    pub uuid: String,
}

/// Body for `POST /repositories/{ws}/{slug}/pullrequests/{id}/comments`.
#[derive(Debug, Serialize)]
pub struct CreateCommentRequest {
    pub content: CommentContent,
}

#[derive(Debug, Serialize)]
pub struct CommentContent {
    pub raw: String,
}

impl BitbucketClient {
    /// `GET /repositories/{ws}/{slug}/pullrequests?pagelen=N&q=state="OPEN"`
    pub async fn list_prs(
        &self,
        workspace: &str,
        slug: &str,
        state: PrState,
        limit: u32,
    ) -> Result<Paginated<PullRequest>> {
        let mut path = format!(
            "/repositories/{workspace}/{slug}/pullrequests?pagelen={limit}&sort=-updated_on"
        );
        if let Some(s) = state.as_query() {
            // Bitbucket's q parameter uses double-quoted strings; URL-encode as %22.
            path.push_str(&format!("&q=state%3D%22{s}%22"));
        }
        self.send(reqwest::Method::GET, &path, None).await
    }

    /// `GET /repositories/{ws}/{slug}/pullrequests/{id}`
    pub async fn get_pr(&self, workspace: &str, slug: &str, id: u64) -> Result<PullRequest> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}");
        self.send(reqwest::Method::GET, &path, None).await
    }

    /// Look up the open PR whose source branch is `branch`, if any.
    pub async fn pr_for_branch(
        &self,
        workspace: &str,
        slug: &str,
        branch: &str,
    ) -> Result<Option<PullRequest>> {
        // Bitbucket supports filtering by source branch name.
        let path = format!(
            "/repositories/{workspace}/{slug}/pullrequests?pagelen=1&sort=-updated_on&q=source.branch.name%3D%22{}%22+AND+state%3D%22OPEN%22",
            url_encode(branch),
        );
        let page: Paginated<PullRequest> = self.send(reqwest::Method::GET, &path, None).await?;
        Ok(page.values.into_iter().next())
    }

    /// `POST /repositories/{ws}/{slug}/pullrequests`
    pub async fn create_pr(
        &self,
        workspace: &str,
        slug: &str,
        body: &CreatePrRequest,
    ) -> Result<PullRequest> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests");
        self.post(&path, body).await
    }

    /// `POST /repositories/{ws}/{slug}/pullrequests/{id}/comments`
    pub async fn comment_pr(&self, workspace: &str, slug: &str, id: u64, text: &str) -> Result<()> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}/comments");
        let body = CreateCommentRequest {
            content: CommentContent {
                raw: text.to_string(),
            },
        };
        let _: serde_json::Value = self.post(&path, &body).await?;
        Ok(())
    }
}

/// Minimal percent-encoder for the few query values we build by hand.
pub(crate) fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
