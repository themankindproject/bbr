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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PullRequest {
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
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

    pub fn source_branch(&self) -> &str {
        self.source
            .branch
            .as_ref()
            .map(|b| b.name.as_str())
            .unwrap_or_default()
    }

    pub fn destination_branch(&self) -> &str {
        self.destination
            .branch
            .as_ref()
            .map(|b| b.name.as_str())
            .unwrap_or_default()
    }
}

/// `{branch_name} -> {branch_name}` plus repo metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BranchRef {
    #[serde(default)]
    pub branch: Option<Named>,
    #[serde(default)]
    pub repository: Option<RepoRef>,
    #[serde(default)]
    pub commit: Option<CommitRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Named {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRef {
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Participant {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub approved: bool,
    #[serde(default)]
    pub user: Option<User>,
    #[serde(default)]
    pub state: Option<String>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommentParentRef {
    #[serde(default)]
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestComment {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub content: Option<Markdown>,
    #[serde(default)]
    pub user: Option<User>,
    #[serde(default)]
    pub parent: Option<CommentParentRef>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub updated_on: Option<String>,
    #[serde(default)]
    pub links: Links,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestTask {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub content: Option<Markdown>,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub creator: Option<User>,
    #[serde(default)]
    pub assignee: Option<User>,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub updated_on: Option<String>,
    #[serde(default)]
    pub links: Links,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestConflict {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub conflict_type: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<CommentParent>,
}

#[derive(Debug, Serialize)]
pub struct CommentParent {
    pub id: u64,
}

#[derive(Debug, Serialize)]
pub struct CommentContent {
    pub raw: String,
}

/// Body for `PUT /repositories/{ws}/{slug}/pullrequests/{id}`.
#[derive(Debug, Serialize)]
pub struct UpdatePrRequest {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_source_branch: Option<bool>,
}

/// Body for `POST /repositories/{ws}/{slug}/pullrequests/{id}/merge`.
#[derive(Debug, Serialize)]
pub struct MergePrRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_source_branch: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl BitbucketClient {
    /// `GET /repositories/{ws}/{slug}/pullrequests`
    ///
    /// When `limit > 100` (Bitbucket's max page size), this follows `next`
    /// links across multiple pages automatically.
    #[allow(clippy::too_many_arguments)]
    pub async fn list_prs(
        &self,
        workspace: &str,
        slug: &str,
        state: PrState,
        limit: u32,
        author: Option<&str>,
        source_branch: Option<&str>,
        reviewer: Option<&str>,
        sort: Option<&str>,
        order: Option<&str>,
    ) -> Result<Vec<PullRequest>> {
        let pagelen = limit.min(100);
        let sort_field = sort.unwrap_or("updated_on");
        let sort_prefix = if order == Some("asc") { "" } else { "-" };

        // Build query params
        let mut q_parts: Vec<String> = Vec::new();
        if let Some(s) = state.as_query() {
            q_parts.push(format!("state%3D%22{s}%22"));
        }
        if let Some(a) = author {
            q_parts.push(format!(
                "author.display_name%3D%22{}%22",
                super::url_encode(a)
            ));
        }
        if let Some(b) = source_branch {
            q_parts.push(format!(
                "source.branch.name%3D%22{}%22",
                super::url_encode(b)
            ));
        }
        if let Some(r) = reviewer {
            q_parts.push(format!(
                "reviewers.display_name%3D%22{}%22",
                super::url_encode(r)
            ));
        }

        let q_param = if q_parts.is_empty() {
            String::new()
        } else {
            format!("&q={}", q_parts.join("+AND+"))
        };

        // Try with fields= first for smaller payloads, fallback without if it fails
        let fields = "values.id,values.state,values.title,\
             values.source.branch.name,values.destination.branch.name,\
             values.author.display_name,values.links.html.href,\
             values.comment_count,values.task_count,values.close_source_branch,\
             values.updated_on,values.reviewers,values.participants";

        let path_with_fields = format!(
            "/repositories/{workspace}/{slug}/pullrequests?\
             fields={fields}&pagelen={pagelen}&sort={sort_prefix}{sort_field}{q_param}"
        );

        let result: Result<super::Paginated<PullRequest>> = self
            .send(reqwest::Method::GET, &path_with_fields, None)
            .await;

        match result {
            Ok(page) => {
                if limit > 100 {
                    let mut all = page.values;
                    let mut next = page.next;
                    while all.len() < limit as usize {
                        if let Some(ref url) = next {
                            let next_path =
                                url.strip_prefix(&self.base_url).unwrap_or(url).to_string();
                            let next_page: super::Paginated<PullRequest> =
                                self.send(reqwest::Method::GET, &next_path, None).await?;
                            let remaining = limit as usize - all.len();
                            all.extend(next_page.values.into_iter().take(remaining));
                            next = next_page.next;
                        } else {
                            break;
                        }
                    }
                    Ok(all)
                } else {
                    Ok(page.values)
                }
            }
            Err(BitbucketError::BadRequest(_)) => {
                // Fallback: retry without fields= and with smaller pagelen
                let safe_pagelen = pagelen.min(50);
                let path_no_fields = format!(
                    "/repositories/{workspace}/{slug}/pullrequests?\
                     pagelen={safe_pagelen}&sort={sort_prefix}{sort_field}{q_param}"
                );
                if limit > safe_pagelen {
                    self.fetch_all_pages(&path_no_fields, limit as usize).await
                } else {
                    let page: super::Paginated<PullRequest> = self
                        .send(reqwest::Method::GET, &path_no_fields, None)
                        .await?;
                    Ok(page.values)
                }
            }
            Err(e) => Err(e),
        }
    }

    /// `GET /repositories/{ws}/{slug}/pullrequests/{id}`
    pub async fn get_pr(&self, workspace: &str, slug: &str, id: u64) -> Result<PullRequest> {
        let path = format!(
            "/repositories/{workspace}/{slug}/pullrequests/{id}?\
             fields=id,state,title,description,\
             source.branch.name,destination.branch.name,\
             author.display_name,links.html.href,\
             comment_count,task_count,close_source_branch,\
             participants.display_name,participants.role,participants.approved,\
             reviewers.display_name,reviewers.role,reviewers.approved"
        );
        self.send(reqwest::Method::GET, &path, None).await
    }

    /// Look up the open PR whose source branch is `branch`, if any.
    /// Like `pr_for_branch` but omits `participants`/`reviewers` fields —
    /// lighter and faster when you only need the PR identity.
    pub async fn pr_for_branch_light(
        &self,
        workspace: &str,
        slug: &str,
        branch: &str,
    ) -> Result<Option<PullRequest>> {
        let path = format!(
            "/repositories/{workspace}/{slug}/pullrequests?\
             fields=values.id,values.state,values.title,\
             values.source.branch.name,values.destination.branch.name,\
             values.author.display_name,values.links.html.href,\
             values.comment_count,values.task_count,values.close_source_branch,\
             values.updated_on&\
             pagelen=1&sort=-updated_on&q=source.branch.name%3D%22{}%22+AND+state%3D%22OPEN%22",
            super::url_encode(branch),
        );
        let page: super::Paginated<PullRequest> =
            self.send(reqwest::Method::GET, &path, None).await?;
        Ok(page.values.into_iter().next())
    }

    pub async fn pr_for_branch(
        &self,
        workspace: &str,
        slug: &str,
        branch: &str,
    ) -> Result<Option<PullRequest>> {
        // Bitbucket supports filtering by source branch name.
        let path = format!(
            "/repositories/{workspace}/{slug}/pullrequests?\
             fields=values.id,values.state,values.title,\
             values.source.branch.name,values.destination.branch.name,\
             values.author.display_name,values.links.html.href,\
             values.comment_count,values.task_count,values.close_source_branch,\
             values.updated_on,\
             values.participants.display_name,values.participants.role,values.participants.approved,\
             values.reviewers.display_name,values.reviewers.role,values.reviewers.approved&\
             pagelen=1&sort=-updated_on&q=source.branch.name%3D%22{}%22+AND+state%3D%22OPEN%22",
            super::url_encode(branch),
        );
        let page: super::Paginated<PullRequest> =
            self.send(reqwest::Method::GET, &path, None).await?;
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
    pub async fn comment_pr(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        text: &str,
        reply_to: Option<u64>,
    ) -> Result<()> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}/comments");
        let body = CreateCommentRequest {
            content: CommentContent {
                raw: text.to_string(),
            },
            parent: reply_to.map(|p| CommentParent { id: p }),
        };
        let _: serde_json::Value = self.post(&path, &body).await?;
        Ok(())
    }

    /// `PUT /repositories/{ws}/{slug}/pullrequests/{id}`
    pub async fn update_pr(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        body: &UpdatePrRequest,
    ) -> Result<PullRequest> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}");
        let raw = serde_json::to_string(body)?;
        self.send(reqwest::Method::PUT, &path, Some(&raw)).await
    }

    /// `POST /repositories/{ws}/{slug}/pullrequests/{id}/merge`
    pub async fn merge_pr(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        body: Option<&MergePrRequest>,
    ) -> Result<PullRequest> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}/merge");
        let raw = body.map(|b| serde_json::to_string(b).unwrap_or_else(|_| "{}".into()));
        self.send(reqwest::Method::POST, &path, raw.as_deref().or(Some("{}")))
            .await
    }

    /// `POST /repositories/{ws}/{slug}/pullrequests/{id}/approve`
    pub async fn approve_pr(&self, workspace: &str, slug: &str, id: u64) -> Result<()> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}/approve");
        let _: serde_json::Value = self.send(reqwest::Method::POST, &path, Some("{}")).await?;
        Ok(())
    }

    /// `DELETE /repositories/{ws}/{slug}/pullrequests/{id}/approve`
    pub async fn unapprove_pr(&self, workspace: &str, slug: &str, id: u64) -> Result<()> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}/approve");
        let _: serde_json::Value = self.send(reqwest::Method::DELETE, &path, None).await?;
        Ok(())
    }

    /// `POST /repositories/{ws}/{slug}/pullrequests/{id}/decline`
    pub async fn decline_pr(&self, workspace: &str, slug: &str, id: u64) -> Result<PullRequest> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}/decline");
        self.send(reqwest::Method::POST, &path, Some("{}")).await
    }

    /// `GET /repositories/{ws}/{slug}/pullrequests/{id}/diff`
    pub async fn pr_diff(&self, workspace: &str, slug: &str, id: u64) -> Result<String> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}/diff");
        self.send_raw(reqwest::Method::GET, &path, "text/plain")
            .await
    }

    /// `GET /repositories/{ws}/{slug}/pullrequests/{id}/diffstat`
    pub async fn pr_diffstat(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
    ) -> Result<serde_json::Value> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}/diffstat");
        self.send(reqwest::Method::GET, &path, None).await
    }

    /// `GET /repositories/{ws}/{slug}/pullrequests/{id}/patch`
    pub async fn pr_patch(&self, workspace: &str, slug: &str, id: u64) -> Result<String> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}/patch");
        self.send_raw(reqwest::Method::GET, &path, "text/plain")
            .await
    }

    /// `POST /repositories/{ws}/{slug}/pullrequests/{id}/approve`
    pub async fn approve_pr_with_comment(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        message: &str,
    ) -> Result<()> {
        self.comment_pr(workspace, slug, id, message, None).await?;
        self.approve_pr(workspace, slug, id).await?;
        Ok(())
    }

    /// `GET /repositories/{ws}/{slug}/pullrequests/{id}/comments`
    pub async fn pr_comments(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        limit: u32,
    ) -> Result<Vec<PullRequestComment>> {
        let pagelen = limit.min(100);
        let path = format!(
            "/repositories/{workspace}/{slug}/pullrequests/{id}/comments?pagelen={pagelen}"
        );
        if limit > 100 {
            self.fetch_all_pages(&path, limit as usize).await
        } else {
            let page: super::Paginated<PullRequestComment> =
                self.send(reqwest::Method::GET, &path, None).await?;
            Ok(page.values)
        }
    }

    /// `GET /repositories/{ws}/{slug}/pullrequests/{id}/tasks`
    pub async fn pr_tasks(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        limit: u32,
    ) -> Result<Vec<PullRequestTask>> {
        let pagelen = limit.min(100);
        let path =
            format!("/repositories/{workspace}/{slug}/pullrequests/{id}/tasks?pagelen={pagelen}");
        if limit > 100 {
            self.fetch_all_pages(&path, limit as usize).await
        } else {
            let page: super::Paginated<PullRequestTask> =
                self.send(reqwest::Method::GET, &path, None).await?;
            Ok(page.values)
        }
    }

    /// `GET /repositories/{ws}/{slug}/pullrequests/{id}/commits`
    pub async fn pr_commits(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        limit: u32,
    ) -> Result<Vec<super::repo::Commit>> {
        let pagelen = limit.min(100);
        let path =
            format!("/repositories/{workspace}/{slug}/pullrequests/{id}/commits?pagelen={pagelen}");
        if limit > 100 {
            self.fetch_all_pages(&path, limit as usize).await
        } else {
            let page: super::Paginated<super::repo::Commit> =
                self.send(reqwest::Method::GET, &path, None).await?;
            Ok(page.values)
        }
    }

    /// `GET /repositories/{ws}/{slug}/pullrequests/{id}/statuses`
    pub async fn pr_statuses(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        limit: u32,
    ) -> Result<Vec<super::status::BuildStatus>> {
        let pagelen = limit.min(100);
        let path = format!(
            "/repositories/{workspace}/{slug}/pullrequests/{id}/statuses?pagelen={pagelen}"
        );
        if limit > 100 {
            self.fetch_all_pages(&path, limit as usize).await
        } else {
            let page: super::Paginated<super::status::BuildStatus> =
                self.send(reqwest::Method::GET, &path, None).await?;
            Ok(page.values)
        }
    }

    /// `GET /repositories/{ws}/{slug}/pullrequests/{id}/conflicts`
    pub async fn pr_conflicts(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        limit: u32,
    ) -> Result<Vec<PullRequestConflict>> {
        let pagelen = limit.min(100);
        let path = format!(
            "/repositories/{workspace}/{slug}/pullrequests/{id}/conflicts?pagelen={pagelen}"
        );
        if limit > 100 {
            self.fetch_all_pages(&path, limit as usize).await
        } else {
            let page: super::Paginated<PullRequestConflict> =
                self.send(reqwest::Method::GET, &path, None).await?;
            Ok(page.values)
        }
    }

    /// `POST /repositories/{ws}/{slug}/pullrequests/{id}/request-changes`
    pub async fn request_pr_changes(&self, workspace: &str, slug: &str, id: u64) -> Result<()> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}/request-changes");
        let _: serde_json::Value = self.send(reqwest::Method::POST, &path, Some("{}")).await?;
        Ok(())
    }

    /// `DELETE /repositories/{ws}/{slug}/pullrequests/{id}/request-changes`
    pub async fn unrequest_pr_changes(&self, workspace: &str, slug: &str, id: u64) -> Result<()> {
        let path = format!("/repositories/{workspace}/{slug}/pullrequests/{id}/request-changes");
        let _: serde_json::Value = self.send(reqwest::Method::DELETE, &path, None).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_state_parse_valid() {
        assert_eq!(PrState::parse("open").unwrap(), PrState::Open);
        assert_eq!(PrState::parse("").unwrap(), PrState::Open);
        assert_eq!(PrState::parse("merged").unwrap(), PrState::Merged);
        assert_eq!(PrState::parse("declined").unwrap(), PrState::Declined);
        assert_eq!(PrState::parse("all").unwrap(), PrState::All);
    }

    #[test]
    fn pr_state_parse_case_insensitive() {
        assert_eq!(PrState::parse("OPEN").unwrap(), PrState::Open);
        assert_eq!(PrState::parse("Merged").unwrap(), PrState::Merged);
        assert_eq!(PrState::parse("DECLINED").unwrap(), PrState::Declined);
    }

    #[test]
    fn pr_state_parse_invalid() {
        let err = PrState::parse("invalid").unwrap_err();
        assert!(format!("{err}").contains("invalid --state"));
    }

    #[test]
    fn pr_state_as_query() {
        assert_eq!(PrState::Open.as_query(), Some("OPEN"));
        assert_eq!(PrState::Merged.as_query(), Some("MERGED"));
        assert_eq!(PrState::Declined.as_query(), Some("DECLINED"));
        assert_eq!(PrState::All.as_query(), None);
    }

    #[test]
    fn url_encode_does_not_change_alphanumeric() {
        assert_eq!(crate::api::url_encode("hello123"), "hello123");
    }

    #[test]
    fn url_encode_encodes_special_chars() {
        assert_eq!(crate::api::url_encode("a b"), "a%20b");
        assert_eq!(crate::api::url_encode("feature/test"), "feature%2Ftest");
        assert_eq!(crate::api::url_encode("a.b"), "a.b");
        assert_eq!(crate::api::url_encode("a~b"), "a~b");
    }

    #[test]
    fn url_encode_encodes_quotes() {
        assert_eq!(crate::api::url_encode("a\"b"), "a%22b");
    }

    #[test]
    fn pull_request_source_branch() {
        let pr = PullRequest {
            id: 1,
            title: "Test".into(),
            state: "OPEN".into(),
            source: BranchRef {
                branch: Some(Named {
                    name: "feature".into(),
                }),
                ..Default::default()
            },
            destination: BranchRef::default(),
            ..Default::default()
        };
        assert_eq!(pr.source_branch(), "feature");
    }

    #[test]
    fn pull_request_destination_branch() {
        let pr = PullRequest {
            id: 1,
            title: "Test".into(),
            state: "OPEN".into(),
            source: BranchRef::default(),
            destination: BranchRef {
                branch: Some(Named {
                    name: "main".into(),
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(pr.destination_branch(), "main");
    }

    #[test]
    fn pull_request_branch_fallback_empty() {
        let pr = PullRequest {
            id: 1,
            title: "Test".into(),
            state: "OPEN".into(),
            source: BranchRef::default(),
            destination: BranchRef::default(),
            ..Default::default()
        };
        assert_eq!(pr.source_branch(), "");
        assert_eq!(pr.destination_branch(), "");
    }

    #[test]
    fn web_url_returns_none_when_no_html_link() {
        let pr = PullRequest::default();
        assert!(pr.web_url().is_none());
    }

    #[test]
    fn create_pr_request_serializes_correctly() {
        let req = CreatePrRequest {
            title: "My PR".into(),
            description: Some("desc".into()),
            source: CreateBranchRef {
                branch: CreateNamed {
                    name: "feature".into(),
                },
            },
            destination: CreateBranchRef {
                branch: CreateNamed {
                    name: "main".into(),
                },
            },
            close_source_branch: Some(true),
            reviewers: vec![ReviewerRef { uuid: "r1".into() }],
            draft: Some(true),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["title"], "My PR");
        assert_eq!(json["description"], "desc");
        assert!(json
            .get("close_source_branch")
            .and_then(|v| v.as_bool())
            .unwrap());
        assert_eq!(json["reviewers"][0]["uuid"], "r1");
    }

    #[test]
    fn create_pr_request_skips_optional_fields() {
        let req = CreatePrRequest {
            title: "Minimal".into(),
            description: None,
            source: CreateBranchRef {
                branch: CreateNamed { name: "f".into() },
            },
            destination: CreateBranchRef {
                branch: CreateNamed { name: "m".into() },
            },
            close_source_branch: None,
            reviewers: vec![],
            draft: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("description").is_none());
        assert!(json.get("close_source_branch").is_none());
        assert!(json.get("reviewers").is_none());
    }

    #[test]
    fn update_pr_request_serializes() {
        let req = UpdatePrRequest {
            title: "New Title".into(),
            description: Some("New desc".into()),
            close_source_branch: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["title"], "New Title");
        assert_eq!(json["description"], "New desc");
        assert!(json.get("close_source_branch").is_none());
    }

    #[test]
    fn merge_pr_request_serializes() {
        let req = MergePrRequest {
            close_source_branch: Some(true),
            merge_strategy: Some("squash".into()),
            message: Some("merge msg".into()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["merge_strategy"], "squash");
        assert_eq!(json["message"], "merge msg");
    }

    #[test]
    fn pull_request_comment_deserializes() {
        let json =
            r#"{"id":1,"content":{"raw":"nice"},"user":{"display_name":"Alice"},"deleted":false}"#;
        let comment: PullRequestComment = serde_json::from_str(json).unwrap();
        assert_eq!(comment.id, 1);
        assert_eq!(comment.content.as_ref().unwrap().raw, "nice");
        assert_eq!(comment.user.as_ref().unwrap().display_name, "Alice");
    }

    #[test]
    fn pull_request_task_deserializes() {
        let json = r#"{"id":1,"content":{"raw":"todo"},"state":"UNRESOLVED"}"#;
        let task: PullRequestTask = serde_json::from_str(json).unwrap();
        assert_eq!(task.id, 1);
        assert_eq!(task.state, "UNRESOLVED");
    }

    #[test]
    fn pull_request_conflict_deserializes() {
        let json = r#"{"path":"src/main.rs","conflict_type":"merge"}"#;
        let conflict: PullRequestConflict = serde_json::from_str(json).unwrap();
        assert_eq!(conflict.path, "src/main.rs");
    }
}
