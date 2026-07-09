//! Repository source / file content endpoints.
use super::BitbucketClient;
use crate::error::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceEntry {
    #[serde(rename = "type", default)]
    pub entry_type: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub attributes: Vec<String>,
    #[serde(default)]
    pub commit: Option<SourceCommit>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceCommit {
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

impl BitbucketClient {
    /// Get raw file content as text.
    pub async fn get_file_raw(
        &self,
        workspace: &str,
        slug: &str,
        git_ref: &str,
        path: &str,
    ) -> Result<String> {
        let path_clean = path.trim_start_matches('/');
        let ref_encoded = super::url_encode(git_ref);
        let path_encoded = path_clean
            .split('/')
            .map(super::url_encode)
            .collect::<Vec<_>>()
            .join("/");
        let endpoint = format!("/repositories/{workspace}/{slug}/src/{ref_encoded}/{path_encoded}");
        self.send_raw(reqwest::Method::GET, &endpoint, "*/*").await
    }

    /// List directory contents at a path and ref.
    pub async fn list_src(
        &self,
        workspace: &str,
        slug: &str,
        git_ref: &str,
        path: &str,
    ) -> Result<Vec<SourceEntry>> {
        let path_clean = path.trim_start_matches('/');
        let ref_encoded = super::url_encode(git_ref);
        let path_encoded = path_clean
            .split('/')
            .map(super::url_encode)
            .collect::<Vec<_>>()
            .join("/");
        let endpoint = if path_encoded.is_empty() {
            format!("/repositories/{workspace}/{slug}/src/{ref_encoded}/?pagelen=100")
        } else {
            format!(
                "/repositories/{workspace}/{slug}/src/{ref_encoded}/{path_encoded}/?pagelen=100"
            )
        };
        let page: super::Paginated<SourceEntry> =
            self.send(reqwest::Method::GET, &endpoint, None).await?;
        Ok(page.values)
    }
}
