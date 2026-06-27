//! Pipeline + step endpoints and types.

use serde::{Deserialize, Serialize};

use super::BitbucketClient;
use crate::error::{BitbucketError, Result};

/// A pipeline run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub build_number: u64,
    #[serde(default)]
    pub state: PipelineState,
    #[serde(default)]
    pub result: Option<PipelineResult>,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub completed_on: Option<String>,
    #[serde(default)]
    pub duration_in_seconds: u64,
    #[serde(default)]
    pub target: PipelineTarget,
    #[serde(default)]
    pub links: super::pr::Links,
}

impl Pipeline {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.result.as_ref().map(|r| r.name.as_str()),
            Some("SUCCESSFUL") | Some("FAILED") | Some("STOPPED") | Some("ERROR")
        )
    }

    pub fn state_name(&self) -> &str {
        if let Some(r) = &self.result {
            if !r.name.is_empty() {
                return &r.name;
            }
        }
        &self.state.name
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineState {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub stage: Option<Named>,
    #[serde(default)]
    pub result: Option<PipelineResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineResult {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Named {
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineTarget {
    #[serde(rename = "ref", default)]
    pub ref_: Option<PipelineRef>,
    #[serde(default)]
    pub selector: Option<Named>,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineRef {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub target: Option<CommitRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommitRef {
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub commit: Option<super::pr::CommitRef>,
}

/// A step within a pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub state: PipelineState,
    #[serde(default)]
    pub duration_in_seconds: u64,
    #[serde(default)]
    pub started_on: Option<String>,
    #[serde(default)]
    pub completed_on: Option<String>,
    #[serde(default)]
    pub setup_commands: Option<Vec<Command>>,
    #[serde(default)]
    pub commands: Option<Vec<Command>>,
    #[serde(default)]
    pub script_commands: Option<Vec<Command>>,
    #[serde(default)]
    pub links: super::pr::Links,
}

impl PipelineStep {
    pub fn state_name(&self) -> &str {
        if let Some(r) = &self.state.result {
            if !r.name.is_empty() {
                return &r.name;
            }
        }
        &self.state.name
    }

    pub fn is_failed(&self) -> bool {
        matches!(
            self.state_name().to_ascii_uppercase().as_str(),
            "FAILED" | "ERROR" | "STOPPED"
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// Log output (text/plain) for a step.
pub struct StepLog {
    pub text: String,
}

impl BitbucketClient {
    /// `GET /repositories/{ws}/{slug}/pipelines/?pagelen=1&sort=-created_on`
    /// (optionally filtered by branch).
    pub async fn latest_pipeline(
        &self,
        workspace: &str,
        slug: &str,
        branch: Option<&str>,
    ) -> Result<Option<Pipeline>> {
        let mut path =
            format!("/repositories/{workspace}/{slug}/pipelines/?pagelen=1&sort=-created_on");
        if let Some(b) = branch {
            path.push_str(&format!(
                "&q=target.ref_name%3D%22{}%22",
                super::pr::url_encode(b)
            ));
        }
        let page: super::pr::Paginated<Pipeline> =
            self.send(reqwest::Method::GET, &path, None).await?;
        Ok(page.values.into_iter().next())
    }

    /// `GET /repositories/{ws}/{slug}/pipelines/{uuid}`
    pub async fn get_pipeline(&self, workspace: &str, slug: &str, uuid: &str) -> Result<Pipeline> {
        let uuid = normalize_uuid(uuid);
        let path = format!("/repositories/{workspace}/{slug}/pipelines/{uuid}");
        self.send(reqwest::Method::GET, &path, None).await
    }

    /// `GET /repositories/{ws}/{slug}/pipelines/{uuid}/steps/`
    pub async fn list_steps(
        &self,
        workspace: &str,
        slug: &str,
        uuid: &str,
    ) -> Result<super::pr::Paginated<PipelineStep>> {
        let uuid = normalize_uuid(uuid);
        let path = format!("/repositories/{workspace}/{slug}/pipelines/{uuid}/steps/?sort=order");
        self.send(reqwest::Method::GET, &path, None).await
    }

    /// `GET /repositories/{ws}/{slug}/pipelines/{uuid}/steps/{step}/log`
    pub async fn step_log(
        &self,
        workspace: &str,
        slug: &str,
        uuid: &str,
        step: &str,
    ) -> Result<StepLog> {
        let uuid = normalize_uuid(uuid);
        let step = normalize_uuid(step);
        let url = self.url(&format!(
            "/repositories/{workspace}/{slug}/pipelines/{uuid}/steps/{step}/log"
        ));
        let resp = self
            .inner
            .get(&url)
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .header(reqwest::header::ACCEPT, "text/plain")
            .send()
            .await
            .map_err(BitbucketError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.map_err(BitbucketError::Http)?;
            return Err(super::map_error(status, &body));
        }
        let text = resp.text().await.map_err(BitbucketError::Http)?;
        Ok(StepLog { text })
    }
}

// Local helpers; `super::pr_url_encode` and `base64_encode` live in mod.rs / pr.rs.
// We re-expose them via small wrappers to keep `pr.rs` as the single encoder.

/// Strip the wrapping `{...}` that Bitbucket uses on UUIDs.
pub fn normalize_uuid(s: &str) -> String {
    s.trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .to_string()
}
