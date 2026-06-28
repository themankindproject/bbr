//! `bbr src` — remote source browser.
use serde::Serialize;
use crate::cli::GlobalArgs;
use crate::commands::{client, current_head, make_spinner, resolve_repo};
use crate::error::Result;
use crate::output::table::Table;
use crate::output::Formatter;

#[derive(Debug, Serialize)]
pub struct SrcCatOut {
    pub path: String,
    pub git_ref: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct SrcEntryOut {
    pub entry_type: String,
    pub path: String,
    pub size: Option<u64>,
    pub commit_hash: Option<String>,
    pub commit_date: Option<String>,
}

pub async fn cat(g: &GlobalArgs, path: &str, git_ref: Option<&str>) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    // Head.branch is String (not Option<String>), so no unwrap_or needed.
    let resolved_ref = match git_ref {
        Some(r) => r.to_string(),
        None => current_head()?.branch,
    };
    let spinner = make_spinner(g.json);
    spinner.set_message(format!("Fetching {path}..."));
    let content = client
        .get_file_raw(&repo.workspace, &repo.slug, &resolved_ref, path)
        .await?;
    spinner.finish_and_clear();

    let fmt = Formatter::from_json_flag(g.json);
    if g.json {
        let out = SrcCatOut {
            path: path.to_string(),
            git_ref: resolved_ref,
            content,
        };
        fmt.print(&out, "")
    } else {
        print!("{content}");
        Ok(())
    }
}

pub async fn ls(g: &GlobalArgs, path: Option<&str>, git_ref: Option<&str>) -> Result<()> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    // Head.branch is String (not Option<String>).
    let resolved_ref = match git_ref {
        Some(r) => r.to_string(),
        None => current_head()?.branch,
    };
    let dir = path.unwrap_or("");
    let spinner = make_spinner(g.json);
    spinner.set_message("Fetching directory listing...");
    let entries = client
        .list_src(&repo.workspace, &repo.slug, &resolved_ref, dir)
        .await?;
    spinner.finish_and_clear();

    let out: Vec<SrcEntryOut> = entries
        .iter()
        .map(|e| SrcEntryOut {
            entry_type: if e.entry_type.contains("directory") {
                "dir".into()
            } else {
                "file".into()
            },
            path: e.path.clone(),
            size: e.size,
            commit_hash: e
                .commit
                .as_ref()
                .map(|c| c.hash.chars().take(8).collect()),
            commit_date: e
                .commit
                .as_ref()
                .and_then(|c| c.date.as_ref())
                .map(|d| d.chars().take(10).collect()),
        })
        .collect();

    let fmt = Formatter::from_json_flag(g.json);
    let mut table = Table::new().headers(["Type", "Path", "Size", "Commit", "Date"]);
    for e in &out {
        table = table.add_row([
            e.entry_type.clone(),
            e.path.clone(),
            e.size
                .map(|s| format!("{s}B"))
                .unwrap_or_else(|| "-".into()),
            e.commit_hash.clone().unwrap_or_else(|| "-".into()),
            e.commit_date.clone().unwrap_or_else(|| "-".into()),
        ]);
    }
    let human = if out.is_empty() {
        format!("Empty directory: {dir}")
    } else {
        table.render()
    };
    fmt.print(&out, &human)
}
