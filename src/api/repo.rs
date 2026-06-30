//! Repository metadata endpoints.

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::BitbucketClient;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub scm: String,
    #[serde(default)]
    pub is_private: bool,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub updated_on: Option<String>,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub links: super::pr::Links,
    #[serde(default)]
    pub owner: Option<super::pr::User>,
    #[serde(default)]
    pub mainbranch: Option<super::pr::Named>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub merged: bool,
    #[serde(default)]
    pub target: Option<super::pr::CommitRef>,
    #[serde(default)]
    pub links: super::pr::Links,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub target: Option<super::pr::CommitRef>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub tagger: Option<CommitAuthor>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub links: super::pr::Links,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub links: super::pr::Links,
    #[serde(default)]
    pub author: Option<CommitAuthor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitAuthor {
    #[serde(default)]
    pub raw: String,
    #[serde(default)]
    pub user: Option<super::pr::User>,
}

impl BitbucketClient {
    /// `GET /repositories/{ws}/{slug}`
    pub async fn get_repo(&self, workspace: &str, slug: &str) -> Result<Repository> {
        let path = format!("/repositories/{workspace}/{slug}");
        self.send(reqwest::Method::GET, &path, None).await
    }

    /// `GET /repositories/{ws}/{slug}/refs/branches`
    pub async fn list_branches(
        &self,
        workspace: &str,
        slug: &str,
        limit: u32,
    ) -> Result<Vec<Branch>> {
        let pagelen = limit.min(100);
        let path = format!(
            "/repositories/{workspace}/{slug}/refs/branches?pagelen={pagelen}&sort=target.date"
        );
        if limit > 100 {
            self.fetch_all_pages(&path, limit as usize).await
        } else {
            let page: super::Paginated<Branch> =
                self.send(reqwest::Method::GET, &path, None).await?;
            Ok(page.values)
        }
    }

    /// `GET /repositories/{ws}/{slug}/refs/tags`
    pub async fn list_tags(&self, workspace: &str, slug: &str, limit: u32) -> Result<Vec<Tag>> {
        let pagelen = limit.min(100);
        let path = format!(
            "/repositories/{workspace}/{slug}/refs/tags?pagelen={pagelen}&sort=-target.date"
        );
        if limit > 100 {
            self.fetch_all_pages(&path, limit as usize).await
        } else {
            let page: super::Paginated<Tag> = self.send(reqwest::Method::GET, &path, None).await?;
            Ok(page.values)
        }
    }

    /// `GET /repositories/{ws}/{slug}/commits?pagelen=N&include=branch`
    pub async fn list_commits(
        &self,
        workspace: &str,
        slug: &str,
        branch: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Commit>> {
        let pagelen = limit.min(100);
        let mut path = format!("/repositories/{workspace}/{slug}/commits?pagelen={pagelen}");
        if let Some(b) = branch {
            path.push_str(&format!("&include={}", super::url_encode(b)));
        }
        if limit > 100 {
            self.fetch_all_pages(&path, limit as usize).await
        } else {
            let page: super::Paginated<Commit> =
                self.send(reqwest::Method::GET, &path, None).await?;
            Ok(page.values)
        }
    }

    /// `GET /user` — verifies auth and returns the current user.
    pub async fn current_user(&self) -> Result<super::pr::User> {
        self.send(reqwest::Method::GET, "/user", None).await
    }

    /// `POST /repositories/{workspace}/{slug}` — create a new repository.
    pub async fn create_repo(
        &self,
        workspace: &str,
        slug: &str,
        is_private: bool,
        description: Option<&str>,
        language: Option<&str>,
        has_issues: bool,
    ) -> Result<Repository> {
        let path = format!("/repositories/{workspace}/{slug}");
        let mut body = json!({
            "scm": "git",
            "is_private": is_private,
            "has_issues": has_issues,
        });
        if let Some(d) = description {
            body["description"] = json!(d);
        }
        if let Some(l) = language {
            body["language"] = json!(l);
        }
        let raw = serde_json::to_string(&body)?;
        self.send(reqwest::Method::POST, &path, Some(&raw)).await
    }

    /// `DELETE /repositories/{ws}/{slug}/refs/branches/{name}` — delete a remote branch.
    pub async fn delete_branch(&self, workspace: &str, slug: &str, name: &str) -> Result<()> {
        let encoded = super::url_encode(name);
        let path = format!("/repositories/{workspace}/{slug}/refs/branches/{encoded}");
        let _: serde_json::Value = self.send(reqwest::Method::DELETE, &path, None).await?;
        Ok(())
    }

    /// `DELETE /repositories/{workspace}/{slug}` — delete a repository.
    pub async fn delete_repo(&self, workspace: &str, slug: &str) -> Result<()> {
        let path = format!("/repositories/{workspace}/{slug}");
        self.send_empty(reqwest::Method::DELETE, &path, None).await
    }

    /// `POST /repositories/{workspace}/{slug}/forks` — fork a repository.
    pub async fn fork_repo(
        &self,
        workspace: &str,
        slug: &str,
        name: Option<&str>,
        target_workspace: Option<&str>,
    ) -> Result<Repository> {
        let path = format!("/repositories/{workspace}/{slug}/forks");
        let mut body = json!({});
        if let Some(n) = name {
            body["name"] = json!(n);
        }
        if let Some(tw) = target_workspace {
            body["workspace"] = json!({"slug": tw});
        }
        let raw = serde_json::to_string(&body)?;
        self.send(reqwest::Method::POST, &path, Some(&raw)).await
    }

    /// `POST /repositories/{ws}/{slug}/refs/branches` — create a branch.
    pub async fn create_branch(
        &self,
        workspace: &str,
        slug: &str,
        name: &str,
        target_hash: &str,
    ) -> Result<Branch> {
        let path = format!("/repositories/{workspace}/{slug}/refs/branches");
        let body = json!({"name": name, "target": {"hash": target_hash}});
        let raw = serde_json::to_string(&body)?;
        self.send(reqwest::Method::POST, &path, Some(&raw)).await
    }

    /// `POST /repositories/{ws}/{slug}/refs/tags` — create a tag.
    pub async fn create_tag(
        &self,
        workspace: &str,
        slug: &str,
        name: &str,
        target_hash: &str,
        message: Option<&str>,
    ) -> Result<Tag> {
        let path = format!("/repositories/{workspace}/{slug}/refs/tags");
        let mut body = json!({"name": name, "target": {"hash": target_hash}});
        if let Some(m) = message {
            body["message"] = json!(m);
        }
        let raw = serde_json::to_string(&body)?;
        self.send(reqwest::Method::POST, &path, Some(&raw)).await
    }

    /// `GET /repositories/{workspace}` — list repositories in a workspace.
    pub async fn list_repos(&self, workspace: &str, limit: u32) -> Result<Vec<Repository>> {
        let pagelen = limit.min(100);
        let path = format!("/repositories/{workspace}?pagelen={pagelen}&sort=-updated_on");
        if limit > 100 {
            self.fetch_all_pages(&path, limit as usize).await
        } else {
            let page: super::Paginated<Repository> =
                self.send(reqwest::Method::GET, &path, None).await?;
            Ok(page.values)
        }
    }

    /// `GET /repositories/{ws}/{slug}/branch-restrictions` — list branch restrictions.
    pub async fn list_branch_restrictions(
        &self,
        workspace: &str,
        slug: &str,
    ) -> Result<Vec<BranchRestriction>> {
        let path = format!("/repositories/{workspace}/{slug}/branch-restrictions?pagelen=100");
        let page: super::Paginated<BranchRestriction> =
            self.send(reqwest::Method::GET, &path, None).await?;
        Ok(page.values)
    }

    /// `GET /repositories/{ws}/{slug}/default-reviewers` — list default reviewers.
    pub async fn list_default_reviewers(
        &self,
        workspace: &str,
        slug: &str,
    ) -> Result<Vec<DefaultReviewer>> {
        let path = format!("/repositories/{workspace}/{slug}/default-reviewers");
        let page: super::Paginated<DefaultReviewer> =
            self.send(reqwest::Method::GET, &path, None).await?;
        Ok(page.values)
    }

    /// `GET /repositories/{ws}/{slug}/permissions-config/users` — list user permissions.
    pub async fn list_user_permissions(
        &self,
        workspace: &str,
        slug: &str,
    ) -> Result<Vec<PermissionEntry>> {
        let path = format!("/repositories/{workspace}/{slug}/permissions-config/users?pagelen=100");
        let page: super::Paginated<PermissionEntry> =
            self.send(reqwest::Method::GET, &path, None).await?;
        Ok(page.values)
    }

    /// `GET /repositories/{ws}/{slug}/permissions-config/groups` — list group permissions.
    pub async fn list_group_permissions(
        &self,
        workspace: &str,
        slug: &str,
    ) -> Result<Vec<PermissionEntry>> {
        let path = format!("/repositories/{workspace}/{slug}/permissions-config/groups?pagelen=100");
        let page: super::Paginated<PermissionEntry> =
            self.send(reqwest::Method::GET, &path, None).await?;
        Ok(page.values)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRestriction {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub branch_match_kind: String,
    #[serde(default)]
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultReviewer {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub user: Option<super::pr::User>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionEntry {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub permission: String,
    #[serde(default)]
    pub user: Option<super::pr::User>,
    #[serde(default)]
    pub group: Option<GroupInfo>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupInfo {
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_deserializes_minimal() {
        let json = serde_json::json!({ "slug": "my-repo" });
        let repo: Repository = serde_json::from_value(json).unwrap();
        assert_eq!(repo.slug, "my-repo");
        assert_eq!(repo.name, "");
        assert!(repo.links.html.href.is_none());
    }

    #[test]
    fn repository_deserializes_full() {
        let json = serde_json::json!({
            "slug": "bvrm",
            "full_name": "ws/bvrm",
            "name": "bvrm",
            "scm": "git",
            "is_private": true,
            "language": "Rust",
            "description": "A repo",
            "mainbranch": { "name": "main" }
        });
        let repo: Repository = serde_json::from_value(json).unwrap();
        assert_eq!(repo.slug, "bvrm");
        assert_eq!(
            repo.mainbranch.as_ref().map(|b| &b.name),
            Some(&"main".into())
        );
    }

    #[test]
    fn branch_deserializes() {
        let json = serde_json::json!({
            "name": "feature-x",
            "target": { "hash": "abc123" }
        });
        let branch: Branch = serde_json::from_value(json).unwrap();
        assert_eq!(branch.name, "feature-x");
        assert_eq!(
            branch.target.as_ref().map(|t| &t.hash),
            Some(&"abc123".into())
        );
    }

    #[test]
    fn tag_deserializes() {
        let json = serde_json::json!({
            "name": "v1.0.0",
            "target": { "hash": "def456" },
            "message": "Release"
        });
        let tag: Tag = serde_json::from_value(json).unwrap();
        assert_eq!(tag.name, "v1.0.0");
        assert_eq!(tag.target.as_ref().map(|t| t.hash.as_str()), Some("def456"));
    }
}
