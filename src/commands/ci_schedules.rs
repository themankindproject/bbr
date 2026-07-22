//! `bbr ci schedules` — pipeline schedule management.

use crate::api::pipeline::PipelineSchedule;
use crate::cli::GlobalArgs;
use crate::commands::{
    client, confirm, make_formatter, make_spinner, resolve_repo, table_or_empty, SpinnerGuard,
};
use crate::error::Result;
use crate::output::table::Table;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ScheduleOut {
    uuid: String,
    enabled: bool,
    cron_pattern: String,
    branch: Option<String>,
    pipeline: Option<String>,
    created_on: Option<String>,
    updated_on: Option<String>,
}

impl From<PipelineSchedule> for ScheduleOut {
    fn from(s: PipelineSchedule) -> Self {
        let (branch, pipeline) = match &s.target {
            Some(t) => (
                t.ref_name.clone(),
                t.selector.as_ref().and_then(|sel| sel.pattern.clone()),
            ),
            None => (None, None),
        };
        Self {
            uuid: s.uuid,
            enabled: s.enabled,
            cron_pattern: s.cron_pattern,
            branch,
            pipeline,
            created_on: s.created_on,
            updated_on: s.updated_on,
        }
    }
}

pub async fn list(g: &GlobalArgs) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching schedules…");
    let schedules = api.list_schedules(&repo.workspace, &repo.slug).await?;
    spinner.finish();

    let out: Vec<ScheduleOut> = schedules.into_iter().map(ScheduleOut::from).collect();

    let fmt = make_formatter(g);
    let mut table = Table::new().headers(["UUID", "Branch", "Cron", "Enabled", "Updated"]);
    for s in &out {
        table = table.add_row([
            s.uuid.clone(),
            s.branch.as_deref().unwrap_or("-").to_string(),
            s.cron_pattern.clone(),
            s.enabled.to_string(),
            s.updated_on.as_deref().unwrap_or("-").to_string(),
        ]);
    }
    let human = table_or_empty(out.len(), "No schedules found.", table.render());
    fmt.print(&out, &human)
}

pub async fn create(
    g: &GlobalArgs,
    cron: &str,
    branch: &str,
    pipeline: Option<&str>,
    enabled: bool,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Creating schedule…");
    let mut schedule = api
        .create_schedule(&repo.workspace, &repo.slug, cron, branch, pipeline)
        .await?;
    // If caller specified --enabled=false, update immediately after creation
    if !enabled {
        schedule = api
            .update_schedule(
                &repo.workspace,
                &repo.slug,
                &schedule.uuid,
                None,
                Some(false),
            )
            .await?;
    }
    spinner.finish();

    let out = ScheduleOut::from(schedule);
    let fmt = make_formatter(g);
    let human = format!(
        "Created schedule {} (cron: {}, branch: {}, enabled: {})",
        out.uuid, out.cron_pattern, branch, out.enabled
    );
    fmt.print(&out, &human)
}

pub async fn view(g: &GlobalArgs, uuid: &str) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching schedule…");
    let schedule = api.get_schedule(&repo.workspace, &repo.slug, uuid).await?;
    spinner.finish();

    let out = ScheduleOut::from(schedule);
    let fmt = make_formatter(g);
    let human = format!(
        "Schedule {}\n  Cron:    {}\n  Branch:  {}\n  Enabled: {}\n  Created: {}\n  Updated: {}",
        out.uuid,
        out.cron_pattern,
        out.branch.as_deref().unwrap_or("-"),
        out.enabled,
        out.created_on.as_deref().unwrap_or("-"),
        out.updated_on.as_deref().unwrap_or("-"),
    );
    fmt.print(&out, &human)
}

pub async fn update(
    g: &GlobalArgs,
    uuid: &str,
    cron: Option<&str>,
    enabled: Option<bool>,
) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Updating schedule…");
    let schedule = api
        .update_schedule(&repo.workspace, &repo.slug, uuid, cron, enabled)
        .await?;
    spinner.finish();

    let out = ScheduleOut::from(schedule);
    let fmt = make_formatter(g);
    let human = format!(
        "Updated schedule {} (cron: {}, enabled: {})",
        out.uuid, out.cron_pattern, out.enabled
    );
    fmt.print(&out, &human)
}

pub async fn delete(g: &GlobalArgs, uuid: &str, yes: bool) -> Result<()> {
    if !yes {
        let confirmed = confirm(&format!("Delete schedule {uuid}? [y/N] ")).await?;
        if !confirmed {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Deleting schedule…");
    api.delete_schedule(&repo.workspace, &repo.slug, uuid)
        .await?;
    spinner.finish();

    let fmt = make_formatter(g);
    let out = serde_json::json!({"action": "deleted", "uuid": uuid});
    let human = format!("Deleted schedule {uuid}");
    fmt.print(&out, &human)
}

pub async fn executions(g: &GlobalArgs, uuid: &str, limit: u32) -> Result<()> {
    let repo = resolve_repo(g)?;
    let api = client(g)?;

    let spinner = SpinnerGuard::new(make_spinner(g.json, g.quiet));
    spinner.set_message("Fetching executions…");
    let execs = api
        .schedule_executions(&repo.workspace, &repo.slug, uuid, limit)
        .await?;
    spinner.finish();

    let fmt = make_formatter(g);
    let mut table = Table::new().headers(["UUID", "State", "Created"]);
    for e in &execs {
        let state = e
            .state
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "-".to_string());
        table = table.add_row([
            e.uuid.as_deref().unwrap_or("-").to_string(),
            state,
            e.created_on.as_deref().unwrap_or("-").to_string(),
        ]);
    }
    let human = table.render();
    fmt.print(&execs, &human)
}
