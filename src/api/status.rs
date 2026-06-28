//! Commit build-status endpoints (the green/red check on a commit).

use serde::{Deserialize, Serialize};

use super::BitbucketClient;
use crate::error::Result;

#[derive(Debug, Clone, Serialize)]
pub struct BuildStatusRequest {
    pub key: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refname: Option<String>,
}

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

impl BitbucketClient {
    /// `GET /repositories/{ws}/{slug}/commit/{commit}/statuses`
    pub async fn commit_statuses(
        &self,
        workspace: &str,
        slug: &str,
        commit: &str,
    ) -> Result<super::Paginated<BuildStatus>> {
        let path = format!("/repositories/{workspace}/{slug}/commit/{commit}/statuses?pagelen=25");
        self.send(reqwest::Method::GET, &path, None).await
    }

    /// `POST /repositories/{ws}/{slug}/commit/{commit}/statuses/build`
    pub async fn create_commit_status(
        &self,
        workspace: &str,
        slug: &str,
        commit: &str,
        body: &BuildStatusRequest,
    ) -> Result<BuildStatus> {
        let path = format!("/repositories/{workspace}/{slug}/commit/{commit}/statuses/build");
        self.post(&path, body).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_status_deserializes() {
        let json = serde_json::json!({
            "state": "SUCCESSFUL",
            "key": "BB-CI",
            "name": "Pipeline #123",
            "url": "https://bitbucket.org/ws/r/pipelines/results/123",
            "description": "All good"
        });
        let status: BuildStatus = serde_json::from_value(json).unwrap();
        assert_eq!(status.state, "SUCCESSFUL");
        assert_eq!(status.key, "BB-CI");
    }

    #[test]
    fn build_status_page_deserializes() {
        let json = serde_json::json!({
            "size": 2,
            "pagelen": 25,
            "values": [
                { "state": "SUCCESSFUL", "key": "k1", "name": "n1", "url": "https://..." },
                { "state": "FAILED", "key": "k2", "name": "n2", "url": "https://..." }
            ]
        });
        let page: crate::api::Paginated<BuildStatus> = serde_json::from_value(json).unwrap();
        assert_eq!(page.values.len(), 2);
        assert!(page.values[1].state == "FAILED");
    }

    #[test]
    fn build_status_defaults_on_empty() {
        let json = serde_json::json!({ "values": [] });
        let page: crate::api::Paginated<BuildStatus> = serde_json::from_value(json).unwrap();
        assert!(page.size == 0);
        assert!(page.values.is_empty());
    }
}
