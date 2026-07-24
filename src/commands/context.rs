use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::commands::make_formatter;
use crate::config::{self, ContextEntry};
use crate::error::{BitbucketError, Result};

#[derive(Debug, Serialize)]
struct ContextCreated {
    name: String,
    workspace: String,
    slug: Option<String>,
    active: bool,
    path: String,
}

#[derive(Debug, Serialize)]
struct ContextListOut {
    active_context: Option<String>,
    contexts: Vec<ContextListItem>,
}

#[derive(Debug, Serialize)]
struct ContextListItem {
    name: String,
    workspace: String,
    slug: Option<String>,
    active: bool,
}

#[derive(Debug, Serialize)]
struct ContextSwitched {
    name: String,
    workspace: String,
    slug: Option<String>,
}

#[derive(Debug, Serialize)]
struct ContextDeleted {
    name: String,
    was_active: bool,
}

pub fn create(
    g: &GlobalArgs,
    name: &str,
    workspace: &str,
    slug: Option<&str>,
    set_active: bool,
) -> Result<()> {
    let mut cfg = config::load_config()?;
    if cfg.contexts.contains_key(name) {
        return Err(BitbucketError::Other(format!(
            "context \"{name}\" already exists"
        )));
    }
    cfg.contexts.insert(
        name.to_string(),
        ContextEntry {
            workspace: workspace.to_string(),
            slug: slug.map(|s| s.to_string()),
        },
    );
    if set_active {
        cfg.active_context = Some(name.to_string());
    }
    let path = config::save_config(&cfg)?;

    let out = ContextCreated {
        name: name.to_string(),
        workspace: workspace.to_string(),
        slug: slug.map(|s| s.to_string()),
        active: set_active,
        path: path.display().to_string(),
    };
    let human = if set_active {
        format!(
            "Created context \"{name}\" (workspace={workspace}, slug={}) and set as active.",
            slug.unwrap_or("(none)")
        )
    } else {
        format!(
            "Created context \"{name}\" (workspace={workspace}, slug={}).",
            slug.unwrap_or("(none)")
        )
    };
    make_formatter(g).print(&out, &human)
}

pub fn list(g: &GlobalArgs) -> Result<()> {
    let cfg = config::load_config()?;
    let active = cfg.active_context.as_deref();

    let mut items: Vec<ContextListItem> = cfg
        .contexts
        .iter()
        .map(|(name, entry)| ContextListItem {
            name: name.clone(),
            workspace: entry.workspace.clone(),
            slug: entry.slug.clone(),
            active: active == Some(name.as_str()),
        })
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));

    let out = ContextListOut {
        active_context: cfg.active_context.clone(),
        contexts: items,
    };

    let human = if out.contexts.is_empty() {
        "No contexts defined. Use `bbr context create` to add one.".to_string()
    } else {
        let mut lines = Vec::new();
        for ctx in &out.contexts {
            let marker = if ctx.active { "* " } else { "  " };
            let slug_part = ctx
                .slug
                .as_deref()
                .map(|s| format!("/{s}"))
                .unwrap_or_default();
            lines.push(format!(
                "{marker}{} ({}{})",
                ctx.name, ctx.workspace, slug_part
            ));
        }
        lines.join("\n")
    };
    make_formatter(g).print(&out, &human)
}

pub fn use_context(g: &GlobalArgs, name: &str) -> Result<()> {
    let mut cfg = config::load_config()?;
    let entry = cfg
        .contexts
        .get(name)
        .ok_or_else(|| BitbucketError::Other(format!("context \"{name}\" does not exist")))?;
    let switched = ContextSwitched {
        name: name.to_string(),
        workspace: entry.workspace.clone(),
        slug: entry.slug.clone(),
    };
    cfg.active_context = Some(name.to_string());
    config::save_config(&cfg)?;

    let slug_part = switched
        .slug
        .as_deref()
        .map(|s| format!("/{s}"))
        .unwrap_or_default();
    let human = format!(
        "Switched to context \"{name}\" ({}{})",
        switched.workspace, slug_part
    );
    make_formatter(g).print(&switched, &human)
}

pub fn delete(g: &GlobalArgs, name: &str) -> Result<()> {
    let mut cfg = config::load_config()?;
    if !cfg.contexts.contains_key(name) {
        return Err(BitbucketError::Other(format!(
            "context \"{name}\" does not exist"
        )));
    }
    cfg.contexts.remove(name);
    let was_active = cfg.active_context.as_deref() == Some(name);
    if was_active {
        cfg.active_context = None;
    }
    config::save_config(&cfg)?;

    let out = ContextDeleted {
        name: name.to_string(),
        was_active,
    };
    let human = if was_active {
        format!("Deleted context \"{name}\" (was active, cleared active context).")
    } else {
        format!("Deleted context \"{name}\".")
    };
    make_formatter(g).print(&out, &human)
}
