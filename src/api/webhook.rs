//! Webhook (repository hook) endpoints.
use super::BitbucketClient;
use crate::error::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Webhook {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub secret_set: bool,
    #[serde(default)]
    pub events: Vec<String>,
}

impl BitbucketClient {
    pub async fn list_webhooks(&self, workspace: &str, slug: &str) -> Result<Vec<Webhook>> {
        let path = format!("/repositories/{workspace}/{slug}/hooks?pagelen=100");
        let all = self.fetch_all_pages::<Webhook>(&path, usize::MAX).await?;
        Ok(all)
    }

    pub async fn get_webhook(&self, workspace: &str, slug: &str, uid: &str) -> Result<Webhook> {
        let path = format!("/repositories/{workspace}/{slug}/hooks/{uid}");
        self.send(reqwest::Method::GET, &path, None).await
    }

    pub async fn create_webhook(
        &self,
        workspace: &str,
        slug: &str,
        url: &str,
        description: Option<&str>,
        events: &[String],
        active: bool,
        secret: Option<&str>,
    ) -> Result<Webhook> {
        let path = format!("/repositories/{workspace}/{slug}/hooks");
        let mut body = serde_json::json!({
            "url": url,
            "events": events,
            "active": active,
        });
        if let Some(desc) = description {
            body["description"] = serde_json::Value::String(desc.to_string());
        }
        if let Some(sec) = secret {
            body["secret"] = serde_json::Value::String(sec.to_string());
        }
        let raw = serde_json::to_string(&body)?;
        self.send(reqwest::Method::POST, &path, Some(&raw)).await
    }

    pub async fn update_webhook(
        &self,
        workspace: &str,
        slug: &str,
        uid: &str,
        url: Option<&str>,
        description: Option<&str>,
        events: Option<&[String]>,
        active: Option<bool>,
    ) -> Result<Webhook> {
        // NOTE: GET-then-PUT pattern has an inherent race condition.
        // Bitbucket API does not support ETags or PATCH for webhooks.
        // Concurrent modifications between GET and PUT will be lost.
        tracing::debug!("updating webhook {uid} (GET-then-PUT, no ETag support)");
        let current = self.get_webhook(workspace, slug, uid).await?;
        let path = format!("/repositories/{workspace}/{slug}/hooks/{uid}");
        let body = serde_json::json!({
            "url": url.unwrap_or(&current.url),
            "description": description.or(current.description.as_deref()),
            "events": events.unwrap_or(&current.events),
            "active": active.unwrap_or(current.active),
        });
        let raw = serde_json::to_string(&body)?;
        self.send(reqwest::Method::PUT, &path, Some(&raw)).await
    }

    pub async fn delete_webhook(&self, workspace: &str, slug: &str, uid: &str) -> Result<()> {
        let path = format!("/repositories/{workspace}/{slug}/hooks/{uid}");
        self.send_empty(reqwest::Method::DELETE, &path, None).await
    }
}
