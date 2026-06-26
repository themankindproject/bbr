//! Commit build-status endpoints (the green/red check on a commit).

use serde::{Deserialize, Serialize};

use super::BitbucketClient;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildStatus {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub refname: Option<String>,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub updated_on: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildStatusPage {
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub pagelen: u64,
    pub values: Vec<BuildStatus>,
}

impl BitbucketClient {
    /// `GET /repositories/{ws}/{slug}/commit/{commit}/statuses`
    pub async fn commit_statuses(
        &self,
        workspace: &str,
        slug: &str,
        commit: &str,
    ) -> Result<BuildStatusPage> {
        let path = format!("/repositories/{workspace}/{slug}/commit/{commit}/statuses");
        self.send(reqwest::Method::GET, &path, None).await
    }
}
