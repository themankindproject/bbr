//! Issue tracker endpoints.
use super::BitbucketClient;
use crate::error::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Issue {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: Option<IssueContent>,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub assignee: Option<IssueUser>,
    #[serde(default)]
    pub reporter: Option<IssueUser>,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub updated_on: Option<String>,
    #[serde(default)]
    pub comment_count: u32,
    #[serde(default)]
    pub votes: u32,
    #[serde(default)]
    pub watches: u32,
    #[serde(default)]
    pub component: Option<IssueComponent>,
    #[serde(default)]
    pub milestone: Option<IssueMilestone>,
    #[serde(default)]
    pub version: Option<IssueVersion>,
    #[serde(default)]
    pub links: IssueLinks,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssueContent {
    #[serde(default)]
    pub raw: String,
    #[serde(default)]
    pub markup: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssueUser {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssueComponent {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssueMilestone {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssueVersion {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssueLinks {
    #[serde(default)]
    pub html: IssueLink,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssueLink {
    #[serde(default)]
    pub href: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssueComment {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub content: Option<IssueContent>,
    #[serde(default)]
    pub author: Option<IssueUser>,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub updated_on: Option<String>,
}

/// Build a BBQL query string from optional filters.
fn build_issue_query(
    status: Option<&str>,
    kind: Option<&str>,
    priority: Option<&str>,
    assignee: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(s) = status {
        parts.push(format!("state=\"{s}\""));
    }
    if let Some(k) = kind {
        parts.push(format!("kind=\"{k}\""));
    }
    if let Some(p) = priority {
        parts.push(format!("priority=\"{p}\""));
    }
    if let Some(a) = assignee {
        parts.push(format!("assignee.nickname=\"{a}\""));
    }
    parts.join(" AND ")
}

impl BitbucketClient {
    #[allow(clippy::too_many_arguments)]
    pub async fn list_issues(
        &self,
        workspace: &str,
        slug: &str,
        limit: u32,
        status: Option<&str>,
        kind: Option<&str>,
        priority: Option<&str>,
        assignee: Option<&str>,
        raw_query: Option<&str>,
    ) -> Result<Vec<Issue>> {
        let pagelen = limit.min(50);
        let q = raw_query
            .map(|s| s.to_string())
            .unwrap_or_else(|| build_issue_query(status, kind, priority, assignee));
        let path = if q.is_empty() {
            format!("/repositories/{workspace}/{slug}/issues?pagelen={pagelen}&sort=-updated_on")
        } else {
            // Basic URL-encoding for double quotes and spaces
            let encoded_q = q.replace(' ', "%20").replace('"', "%22");
            format!(
                "/repositories/{workspace}/{slug}/issues?pagelen={pagelen}&sort=-updated_on&q={encoded_q}"
            )
        };
        let page: super::Paginated<Issue> = self.send(reqwest::Method::GET, &path, None).await?;
        Ok(page.values)
    }

    pub async fn get_issue(&self, workspace: &str, slug: &str, id: u64) -> Result<Issue> {
        let path = format!("/repositories/{workspace}/{slug}/issues/{id}");
        self.send(reqwest::Method::GET, &path, None).await
    }

    pub async fn create_issue(
        &self,
        workspace: &str,
        slug: &str,
        title: &str,
        content: &str,
        kind: &str,
        priority: &str,
        assignee: Option<&str>,
    ) -> Result<Issue> {
        let path = format!("/repositories/{workspace}/{slug}/issues");
        let mut body = serde_json::json!({
            "title": title,
            "content": {"raw": content},
            "kind": kind,
            "priority": priority,
        });
        if let Some(a) = assignee {
            body["assignee"] = serde_json::json!({"nickname": a});
        }
        let raw = serde_json::to_string(&body)?;
        self.send(reqwest::Method::POST, &path, Some(&raw)).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_issue(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        title: Option<&str>,
        content: Option<&str>,
        status: Option<&str>,
        kind: Option<&str>,
        priority: Option<&str>,
        assignee: Option<&str>,
    ) -> Result<Issue> {
        let current = self.get_issue(workspace, slug, id).await?;
        let path = format!("/repositories/{workspace}/{slug}/issues/{id}");
        let empty = String::new();
        let body = serde_json::json!({
            "title": title.unwrap_or(&current.title),
            "content": {
                "raw": content.unwrap_or_else(|| {
                    current.content.as_ref().map(|c| c.raw.as_str()).unwrap_or("")
                })
            },
            "state": status.unwrap_or(&current.state),
            "kind": kind.unwrap_or(&current.kind),
            "priority": priority.unwrap_or(&current.priority),
        });
        // suppress unused-variable warning for `empty`
        let _ = &empty;
        if let Some(a) = assignee {
            let mut b = body;
            b["assignee"] = serde_json::json!({"nickname": a});
            let raw = serde_json::to_string(&b)?;
            return self.send(reqwest::Method::PUT, &path, Some(&raw)).await;
        }
        let raw = serde_json::to_string(&body)?;
        self.send(reqwest::Method::PUT, &path, Some(&raw)).await
    }

    pub async fn delete_issue(&self, workspace: &str, slug: &str, id: u64) -> Result<()> {
        let path = format!("/repositories/{workspace}/{slug}/issues/{id}");
        self.send_empty(reqwest::Method::DELETE, &path, None).await
    }

    pub async fn list_issue_comments(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        limit: u32,
    ) -> Result<Vec<IssueComment>> {
        let pagelen = limit.min(50);
        let path =
            format!("/repositories/{workspace}/{slug}/issues/{id}/comments?pagelen={pagelen}");
        let page: super::Paginated<IssueComment> =
            self.send(reqwest::Method::GET, &path, None).await?;
        Ok(page.values)
    }

    pub async fn create_issue_comment(
        &self,
        workspace: &str,
        slug: &str,
        id: u64,
        content: &str,
    ) -> Result<IssueComment> {
        let path = format!("/repositories/{workspace}/{slug}/issues/{id}/comments");
        let body = serde_json::json!({"content": {"raw": content}});
        let raw = serde_json::to_string(&body)?;
        self.send(reqwest::Method::POST, &path, Some(&raw)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_query_empty() {
        assert_eq!(build_issue_query(None, None, None, None), "");
    }

    #[test]
    fn build_query_single() {
        assert_eq!(
            build_issue_query(Some("open"), None, None, None),
            r#"state="open""#
        );
    }

    #[test]
    fn build_query_multiple() {
        let q = build_issue_query(Some("open"), Some("bug"), Some("major"), None);
        assert_eq!(q, r#"state="open" AND kind="bug" AND priority="major""#);
    }

    #[test]
    fn issue_default_derives() {
        let i = Issue::default();
        assert_eq!(i.id, 0);
        assert_eq!(i.state, "");
    }
}
