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

/// Minimal branch/tag projection used to resolve a ref name to its commit.
#[derive(Debug, Clone, Default, Deserialize)]
struct RefTarget {
    #[serde(default)]
    name: String,
    #[serde(default)]
    target: Option<SourceCommit>,
}

impl BitbucketClient {
    /// Turn a branch or tag name into the revision used in `src/{revision}/…`.
    ///
    /// The `src` endpoint takes the revision as a single path segment, so a
    /// name containing `/` (`feature/login`) cannot be expressed there: an
    /// encoded `%2F` is not treated as part of the ref. Such names are resolved
    /// to their commit hash through a filtered branch (then tag) lookup, which
    /// carries the name safely in the query string. Names without `/` are used
    /// as-is, so plain branches, tags, and SHAs need no extra request.
    pub async fn resolve_src_revision(
        &self,
        workspace: &str,
        slug: &str,
        git_ref: &str,
    ) -> Result<String> {
        if !git_ref.contains('/') {
            return Ok(git_ref.to_string());
        }
        if git_ref.contains('"') {
            return Err(crate::error::BitbucketError::Usage(format!(
                "invalid ref {git_ref:?}: quotes are not allowed"
            )));
        }
        let q = super::url_encode(&format!("name=\"{git_ref}\""));
        for kind in ["branches", "tags"] {
            let path = format!(
                "/repositories/{workspace}/{slug}/refs/{kind}?q={q}&fields=values.name,values.target.hash&pagelen=10"
            );
            let page: super::Paginated<RefTarget> =
                self.send(reqwest::Method::GET, &path, None).await?;
            if let Some(hash) = page
                .values
                .into_iter()
                .find(|r| r.name == git_ref)
                .and_then(|r| r.target)
                .map(|t| t.hash)
                .filter(|h| !h.is_empty())
            {
                return Ok(hash);
            }
        }
        Err(crate::error::BitbucketError::NotFound(format!(
            "no branch or tag named {git_ref:?}"
        )))
    }

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
        self.fetch_all_pages(&endpoint, usize::MAX).await
    }
}
