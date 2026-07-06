//! Deployment and environment endpoints.
use super::BitbucketClient;
use crate::error::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeploymentEnvironment {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub environment_type: EnvironmentType,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub rank: u32,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvironmentType {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub rank: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Deployment {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub state: DeploymentState,
    #[serde(default)]
    pub environment: Option<DeploymentEnvironment>,
    #[serde(default)]
    pub deployable: Option<DeploymentDeployable>,
    #[serde(default)]
    pub last_update_time: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeploymentState {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeploymentDeployable {
    #[serde(default)]
    pub pipeline: Option<DeployablePipeline>,
    #[serde(default)]
    pub commit: Option<DeployableCommit>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeployablePipeline {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub build_number: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeployableCommit {
    #[serde(default)]
    pub hash: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvVariable {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub secured: bool,
}

impl BitbucketClient {
    pub async fn list_deployments(
        &self,
        workspace: &str,
        slug: &str,
        limit: u32,
    ) -> Result<Vec<Deployment>> {
        let pagelen = limit.min(100);
        let path = format!("/repositories/{workspace}/{slug}/deployments?pagelen={pagelen}");
        let all = self
            .fetch_all_pages::<Deployment>(&path, limit as usize)
            .await?;
        Ok(all)
    }

    pub async fn list_environments(
        &self,
        workspace: &str,
        slug: &str,
    ) -> Result<Vec<DeploymentEnvironment>> {
        let path = format!("/repositories/{workspace}/{slug}/environments?pagelen=100");
        let all = self
            .fetch_all_pages::<DeploymentEnvironment>(&path, usize::MAX)
            .await?;
        Ok(all)
    }

    pub async fn create_environment(
        &self,
        workspace: &str,
        slug: &str,
        name: &str,
        env_type: &str,
    ) -> Result<DeploymentEnvironment> {
        let path = format!("/repositories/{workspace}/{slug}/environments");
        let body = serde_json::json!({
            "name": name,
            "environment_type": {"name": env_type},
        });
        let raw = serde_json::to_string(&body)?;
        self.send(reqwest::Method::POST, &path, Some(&raw)).await
    }

    pub async fn list_env_variables(
        &self,
        workspace: &str,
        slug: &str,
        env_uuid: &str,
    ) -> Result<Vec<EnvVariable>> {
        let path = format!("/repositories/{workspace}/{slug}/deployments_config/environments/{env_uuid}/variables?pagelen=100");
        let page: super::Paginated<EnvVariable> =
            self.send(reqwest::Method::GET, &path, None).await?;
        Ok(page.values)
    }

    pub async fn create_env_variable(
        &self,
        workspace: &str,
        slug: &str,
        env_uuid: &str,
        key: &str,
        value: &str,
        secured: bool,
    ) -> Result<EnvVariable> {
        let path = format!(
            "/repositories/{workspace}/{slug}/deployments_config/environments/{env_uuid}/variables"
        );
        let body = serde_json::json!({"key": key, "value": value, "secured": secured});
        self.post(&path, &body).await
    }

    pub async fn update_env_variable(
        &self,
        workspace: &str,
        slug: &str,
        env_uuid: &str,
        var_uuid: &str,
        key: &str,
        value: &str,
        secured: bool,
    ) -> Result<EnvVariable> {
        let path = format!("/repositories/{workspace}/{slug}/deployments_config/environments/{env_uuid}/variables/{var_uuid}");
        let body = serde_json::json!({"key": key, "value": value, "secured": secured});
        let raw = serde_json::to_string(&body)?;
        self.send(reqwest::Method::PUT, &path, Some(&raw)).await
    }

    pub async fn delete_env_variable(
        &self,
        workspace: &str,
        slug: &str,
        env_uuid: &str,
        var_uuid: &str,
    ) -> Result<()> {
        let path = format!("/repositories/{workspace}/{slug}/deployments_config/environments/{env_uuid}/variables/{var_uuid}");
        self.send_empty(reqwest::Method::DELETE, &path, None).await
    }

    pub async fn trigger_deployment(
        &self,
        workspace: &str,
        slug: &str,
        env_uuid: &str,
        commit: &str,
    ) -> Result<Deployment> {
        let path = format!(
            "/repositories/{workspace}/{slug}/deployments_config/environments/{env_uuid}/changes"
        );
        let body = serde_json::json!({
            "commit": {
                "hash": commit
            }
        });
        self.post(&path, &body).await
    }
}
