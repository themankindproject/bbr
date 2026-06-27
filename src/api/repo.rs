//! Repository metadata endpoints.

use serde::{Deserialize, Serialize};

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

impl BitbucketClient {
    /// `GET /repositories/{ws}/{slug}`
    pub async fn get_repo(&self, workspace: &str, slug: &str) -> Result<Repository> {
        let path = format!("/repositories/{workspace}/{slug}");
        self.send(reqwest::Method::GET, &path, None).await
    }

    /// `GET /repositories/{ws}/{slug}/refs/branches?pagelen=N&sort=target.date`
    pub async fn list_branches(
        &self,
        workspace: &str,
        slug: &str,
        limit: u32,
    ) -> Result<super::pr::Paginated<Branch>> {
        let path = format!(
            "/repositories/{workspace}/{slug}/refs/branches?pagelen={limit}&sort=target.date"
        );
        self.send(reqwest::Method::GET, &path, None).await
    }

    /// `GET /user` — verifies auth and returns the current user.
    pub async fn current_user(&self) -> Result<super::pr::User> {
        self.send(reqwest::Method::GET, "/user", None).await
    }
}
