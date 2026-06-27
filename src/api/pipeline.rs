use serde::Deserialize;

use super::BitbucketClient;
use crate::error::{BitbucketError, Result};

#[derive(Debug, Clone, Default, Deserialize)]
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
        let name = self.effective_result_name();
        matches!(
            name,
            Some("SUCCESSFUL") | Some("FAILED") | Some("STOPPED") | Some("ERROR")
        )
    }

    pub fn state_name(&self) -> &str {
        if let Some(r) = self.effective_result_name() {
            if !r.is_empty() {
                return r;
            }
        }
        &self.state.name
    }

    fn effective_result_name(&self) -> Option<&str> {
        self.result
            .as_ref()
            .filter(|r| !r.name.is_empty())
            .map(|r| r.name.as_str())
            .or_else(|| {
                self.state
                    .result
                    .as_ref()
                    .filter(|r| !r.name.is_empty())
                    .map(|r| r.name.as_str())
            })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PipelineState {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub stage: Option<Named>,
    #[serde(default)]
    pub result: Option<PipelineResult>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PipelineResult {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    #[serde(rename = "type")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Named {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PipelineTarget {
    #[serde(default)]
    pub ref_name: Option<String>,
    #[serde(default)]
    pub ref_type: Option<String>,
    #[serde(default)]
    pub commit: Option<CommitRef>,
    #[serde(default)]
    pub selector: Option<Named>,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CommitRef {
    #[serde(default)]
    pub hash: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
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

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Command {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub name: Option<String>,
}

pub struct StepLog {
    pub text: String,
}

pub fn normalize_uuid(s: &str) -> String {
    s.trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .to_string()
}

impl BitbucketClient {
    /// `GET /repositories/{ws}/{slug}/pipelines/` with optional branch filter.
    pub async fn list_pipelines(
        &self,
        workspace: &str,
        slug: &str,
        branch: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Pipeline>> {
        let pagelen = limit.min(100);
        let mut path = format!(
            "/repositories/{workspace}/{slug}/pipelines/?\
             fields=values.uuid,values.build_number,values.state,values.result,\
             values.duration_in_seconds,values.target.ref_name,values.target.commit.hash&\
             pagelen={pagelen}&sort=-created_on"
        );
        if let Some(b) = branch {
            path.push_str(&format!(
                "&q=target.ref_name%3D%22{}%22",
                super::pr::url_encode(b)
            ));
        }
        if limit > 100 {
            self.fetch_all_pages(&path, limit as usize).await
        } else {
            let page: super::pr::Paginated<Pipeline> =
                self.send(reqwest::Method::GET, &path, None).await?;
            Ok(page.values)
        }
    }

    pub async fn latest_pipeline(
        &self,
        workspace: &str,
        slug: &str,
        branch: Option<&str>,
    ) -> Result<Option<Pipeline>> {
        let mut path = format!(
            "/repositories/{workspace}/{slug}/pipelines/?\
             fields=values.uuid,values.build_number,values.state,values.result,\
             values.duration_in_seconds,values.target.ref_name,values.target.commit.hash&\
             pagelen=1&sort=-created_on"
        );
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

    pub async fn get_pipeline(&self, workspace: &str, slug: &str, uuid: &str) -> Result<Pipeline> {
        let path = format!(
            "/repositories/{workspace}/{slug}/pipelines/{uuid}?\
             fields=uuid,build_number,state,result,duration_in_seconds,\
             target.ref_name,target.commit.hash,links.html.href"
        );
        self.send(reqwest::Method::GET, &path, None).await
    }

    pub async fn list_steps(
        &self,
        workspace: &str,
        slug: &str,
        uuid: &str,
    ) -> Result<super::pr::Paginated<PipelineStep>> {
        let path = format!(
            "/repositories/{workspace}/{slug}/pipelines/{uuid}/steps/?\
             fields=values.uuid,values.name,values.state,values.duration_in_seconds&\
             sort=order"
        );
        self.send(reqwest::Method::GET, &path, None).await
    }

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
            .header(reqwest::header::ACCEPT, "*/*")
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

    pub async fn rerun_pipeline(
        &self,
        workspace: &str,
        slug: &str,
        uuid: &str,
    ) -> Result<Pipeline> {
        let path = format!("/repositories/{workspace}/{slug}/pipelines/{uuid}/rerun");
        let body = serde_json::Value::Null;
        self.send(reqwest::Method::POST, &path, Some(&body.to_string()))
            .await
    }

    pub async fn stop_pipeline(&self, workspace: &str, slug: &str, uuid: &str) -> Result<()> {
        let path = format!("/repositories/{workspace}/{slug}/pipelines/{uuid}/stopPipeline");
        let body = serde_json::Value::Null;
        let _: serde_json::Value = self
            .send(reqwest::Method::POST, &path, Some(&body.to_string()))
            .await?;
        Ok(())
    }
}
