//! `bbr schema` — prints JSON Schema specifications for --json outputs.
use crate::cli::GlobalArgs;
use crate::error::{BitbucketError, Result};

const SCHEMAS: &[(&str, &str, &str)] = &[
    (
        "auth",
        "JSON schema for `bbr auth status --json` output",
        r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "AuthStatusOut",
  "type": "object",
  "required": ["authenticated", "username", "source"],
  "properties": {
    "authenticated": { "type": "boolean" },
    "username": { "type": "string" },
    "credential_kind": { "type": ["string", "null"] },
    "display_name": { "type": ["string", "null"] },
    "account_id": { "type": ["string", "null"] },
    "source": { "type": "string", "enum": ["environment", "config-file", "none"] }
  }
}"#,
    ),
    (
        "status",
        "JSON schema for `bbr status --json` output",
        r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "StatusOut",
  "type": "object",
  "required": ["repo", "branch", "commit", "pr", "pipelines", "build_statuses"],
  "properties": {
    "repo": {
      "type": "object",
      "required": ["workspace", "slug", "full_name"],
      "properties": {
        "workspace": { "type": "string" },
        "slug": { "type": "string" },
        "full_name": { "type": "string" }
      }
    },
    "branch": { "type": "string" },
    "commit": { "type": "string" },
    "pr": { "type": ["object", "null"] },
    "pipelines": { "type": "array" },
    "build_statuses": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["key", "state"],
        "properties": {
          "key": { "type": "string" },
          "state": { "type": "string" },
          "name": { "type": ["string", "null"] },
          "description": { "type": ["string", "null"] },
          "url": { "type": ["string", "null"] }
        }
      }
    }
  }
}"#,
    ),
    (
        "pr",
        "JSON schema for `bbr pr list --json` output",
        r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "PrListOut",
  "type": "array",
  "items": {
    "type": "object",
    "required": ["id", "title", "state", "destination", "author"],
    "properties": {
      "id": { "type": "integer" },
      "title": { "type": "string" },
      "state": { "type": "string" },
      "destination": { "type": "string" },
      "author": { "type": "string" },
      "created_on": { "type": "string" },
      "updated_on": { "type": "string" }
    }
  }
}"#,
    ),
    (
        "ci",
        "JSON schema for `bbr ci list --json` output",
        r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "PipelineListOut",
  "type": "array",
  "items": {
    "type": "object",
    "required": ["uuid", "build_number", "state", "result_state", "branch", "commit_hash", "duration_seconds"],
    "properties": {
      "uuid": { "type": "string" },
      "build_number": { "type": "integer" },
      "state": { "type": "string" },
      "result_state": { "type": "string" },
      "branch": { "type": "string" },
      "commit_hash": { "type": "string" },
      "duration_seconds": { "type": "integer" }
    }
  }
}"#,
    ),
    (
        "webhook",
        "JSON schema for `bbr webhook list --json` output",
        r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "WebhookListOut",
  "type": "array",
  "items": {
    "type": "object",
    "required": ["uuid", "url", "active", "secret_set", "events"],
    "properties": {
      "uuid": { "type": "string" },
      "url": { "type": "string" },
      "active": { "type": "boolean" },
      "description": { "type": ["string", "null"] },
      "created_at": { "type": ["string", "null"] },
      "secret_set": { "type": "boolean" },
      "events": {
        "type": "array",
        "items": { "type": "string" }
      }
    }
  }
}"#,
    ),
    (
        "src",
        "JSON schema for `bbr src ls --json` output",
        r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "SrcEntryListOut",
  "type": "array",
  "items": {
    "type": "object",
    "required": ["entry_type", "path"],
    "properties": {
      "entry_type": { "type": "string", "enum": ["file", "dir"] },
      "path": { "type": "string" },
      "size": { "type": ["integer", "null"] },
      "commit_hash": { "type": ["string", "null"] },
      "commit_date": { "type": ["string", "null"] }
    }
  }
}"#,
    ),
    (
        "issue",
        "JSON schema for `bbr issue list --json` output",
        r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "IssueListOut",
  "type": "array",
  "items": {
    "type": "object",
    "required": ["id", "title", "state", "kind", "priority", "comment_count", "votes"],
    "properties": {
      "id": { "type": "integer" },
      "title": { "type": "string" },
      "state": { "type": "string" },
      "kind": { "type": "string" },
      "priority": { "type": "string" },
      "assignee": { "type": ["string", "null"] },
      "reporter": { "type": ["string", "null"] },
      "comment_count": { "type": "integer" },
      "votes": { "type": "integer" },
      "created_on": { "type": ["string", "null"] },
      "url": { "type": ["string", "null"] }
    }
  }
}"#,
    ),
];

pub fn run(g: &GlobalArgs, model: Option<&str>) -> Result<()> {
    let fmt = crate::output::Formatter::from_json_flag(g.json);

    if let Some(m) = model {
        let normalized = m.trim().to_lowercase();
        if let Some((_, _, schema)) = SCHEMAS.iter().find(|(n, _, _)| *n == normalized) {
            if g.json {
                let parsed: serde_json::Value = serde_json::from_str(schema)?;
                fmt.print(&parsed, "")
            } else {
                println!("{schema}");
                Ok(())
            }
        } else {
            let available: Vec<&str> = SCHEMAS.iter().map(|(n, _, _)| *n).collect();
            Err(BitbucketError::Other(format!(
                "Unknown schema model '{m}'. Available: {}",
                available.join(", ")
            )))
        }
    } else {
        #[derive(serde::Serialize)]
        struct SchemaItem {
            model: &'static str,
            description: &'static str,
        }
        let list: Vec<SchemaItem> = SCHEMAS
            .iter()
            .map(|(n, d, _)| SchemaItem {
                model: n,
                description: d,
            })
            .collect();

        let mut table = crate::output::table::Table::new().headers(["Model", "Description"]);
        for item in &list {
            table = table.add_row([item.model.to_string(), item.description.to_string()]);
        }
        let human = format!(
            "Available JSON Schema Models:\n\n{}\nRun `bbr schema <model>` to print the full JSON Schema spec.",
            table.render()
        );
        fmt.print(&list, &human)
    }
}
