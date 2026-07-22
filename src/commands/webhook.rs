//! `bbr webhook` — repository webhook management.
use crate::cli::GlobalArgs;
use crate::commands::{
    client, confirm, make_formatter, make_spinner, resolve_repo, table_or_empty, truncate,
    SpinnerGuard,
};
use crate::error::{BitbucketError, Result};
use crate::output::table::Table;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct WebhookOut {
    pub uuid: String,
    pub url: String,
    pub active: bool,
    pub description: Option<String>,
    pub created_at: Option<String>,
    pub secret_set: bool,
    pub events: Vec<String>,
}

pub async fn list(g: &GlobalArgs) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching webhooks...");
    let hooks = client.list_webhooks(&repo.workspace, &repo.slug).await?;
    spinner.finish();

    let out: Vec<WebhookOut> = hooks
        .iter()
        .map(|h| WebhookOut {
            uuid: h.uuid.clone(),
            url: h.url.clone(),
            active: h.active,
            description: h.description.clone(),
            created_at: h.created_at.clone(),
            secret_set: h.secret_set,
            events: h.events.clone(),
        })
        .collect();

    let fmt = make_formatter(g);
    let mut table = Table::new().headers(["UUID", "Active", "Events", "URL"]);
    for h in &hooks {
        table = table.add_row([
            truncate(h.uuid.trim_matches('{').trim_matches('}'), 36),
            if h.active { "yes".into() } else { "no".into() },
            truncate(&h.events.join(", "), 50),
            truncate(&h.url, 60),
        ]);
    }
    let human = table_or_empty(hooks.len(), "No webhooks configured.", table.render());
    fmt.print(&out, &human)
}

pub async fn view(g: &GlobalArgs, uid: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching webhook...");
    let hook = client.get_webhook(&repo.workspace, &repo.slug, uid).await?;
    spinner.finish();

    let out = WebhookOut {
        uuid: hook.uuid.clone(),
        url: hook.url.clone(),
        active: hook.active,
        description: hook.description.clone(),
        created_at: hook.created_at.clone(),
        secret_set: hook.secret_set,
        events: hook.events.clone(),
    };

    let fmt = make_formatter(g);
    let human = format!(
        "Webhook {}\n  URL:    {}\n  Active: {}\n  Secret: {}\n  Events:\n{}",
        hook.uuid,
        hook.url,
        if hook.active { "yes" } else { "no" },
        if hook.secret_set { "set" } else { "not set" },
        hook.events
            .iter()
            .map(|e| format!("    - {e}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    fmt.print(&out, &human)
}

pub async fn create(
    g: &GlobalArgs,
    url: &str,
    events_csv: &str,
    description: Option<&str>,
    active: bool,
    secret: Option<&str>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let events: Vec<String> = events_csv
        .split(',')
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect();
    if events.is_empty() {
        return Err(BitbucketError::Other(
            "--events must be a non-empty comma-separated list".into(),
        ));
    }
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Creating webhook...");
    let hook = client
        .create_webhook(
            &repo.workspace,
            &repo.slug,
            url,
            description,
            &events,
            active,
            secret,
        )
        .await?;
    spinner.finish();

    let out = WebhookOut {
        uuid: hook.uuid.clone(),
        url: hook.url.clone(),
        active: hook.active,
        description: hook.description.clone(),
        created_at: hook.created_at.clone(),
        secret_set: hook.secret_set,
        events: hook.events.clone(),
    };
    let fmt = make_formatter(g);
    let human = format!("Created webhook {}\n  URL: {}", hook.uuid, hook.url);
    fmt.print(&out, &human)
}

pub async fn update(
    g: &GlobalArgs,
    uid: &str,
    url: Option<&str>,
    events_csv: Option<&str>,
    description: Option<&str>,
    active: Option<bool>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let events: Option<Vec<String>> = events_csv.map(|csv| {
        csv.split(',')
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
            .collect()
    });
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Updating webhook...");
    let hook = client
        .update_webhook(
            &repo.workspace,
            &repo.slug,
            uid,
            url,
            description,
            events.as_deref(),
            active,
        )
        .await?;
    spinner.finish();

    let out = WebhookOut {
        uuid: hook.uuid.clone(),
        url: hook.url.clone(),
        active: hook.active,
        description: hook.description.clone(),
        created_at: hook.created_at.clone(),
        secret_set: hook.secret_set,
        events: hook.events.clone(),
    };
    let fmt = make_formatter(g);
    let human = format!("Updated webhook {}", hook.uuid);
    fmt.print(&out, &human)
}

pub async fn delete(g: &GlobalArgs, uid: &str, yes: bool) -> Result<()> {
    if !yes {
        let ok = confirm(&format!("Delete webhook {uid}? [y/N] ")).await?;
        if !ok {
            return Ok(());
        }
    }
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Deleting webhook...");
    client
        .delete_webhook(&repo.workspace, &repo.slug, uid)
        .await?;
    spinner.finish();
    let fmt = make_formatter(g);
    let out = serde_json::json!({"deleted": uid});
    fmt.print(&out, &format!("Deleted webhook {uid}"))
}
