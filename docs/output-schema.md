# `bbr` JSON output schema (v0.1)

All `bbr <cmd> --json` output is stable JSON, printed pretty to stdout. The
shape for each command is documented below. Field names are `snake_case` and
stable across v0.1.x (breaking changes are reserved for v0.2+).

## `bbr status --json`

```json
{
  "branch": "feat/x",
  "commit": "765d8bec",
  "pr": {
    "id": 467,
    "state": "OPEN",
    "title": "...",
    "source": "feat/x",
    "destination": "main",
    "url": "https://bitbucket.org/.../pull-requests/467",
    "author": "bravo1goingdark",
    "reviewers": [
      { "display_name": "bob", "approved": true, "state": "approved" }
    ]
  },
  "open_prs": [],
  "pipeline": {
    "uuid": "{abc-123}",
    "state": "SUCCESSFUL",
    "duration_seconds": 172,
    "branch": "test-ci",
    "commit": "4644ec4b",
    "steps": [
      { "name": "Run Tests", "state": "SUCCESSFUL", "duration_seconds": 172 }
    ]
  }
}
```

`pr` is the latest open PR for the branch (or `null`). `open_prs` lists all open PRs for the branch (includes `pr` when present). `pipeline` is `null` when absent.

## `bbr pr list --json`

```json
{
  "workspace": "sdadev",
  "slug": "bvrm",
  "state": "open",
  "pull_requests": [
    {
      "id": 467,
      "state": "OPEN",
      "title": "...",
      "source": "feat/x",
      "destination": "main",
      "author": "...",
      "url": "https://...",
      "updated_on": "2026-06-27T10:00:00Z"
    }
  ]
}
```

## `bbr pr view --json`

```json
{
  "id": 467,
  "state": "OPEN",
  "title": "...",
  "description": "...",
  "source": "feat/x",
  "destination": "main",
  "author": "...",
  "url": "https://...",
  "comment_count": 3,
  "task_count": 0,
  "close_source_branch": false
}
```

## `bbr pr create --json`

```json
{ "id": 468, "url": "https://...", "state": "OPEN" }
```

## `bbr pr comment --json`

```json
{ "pr_id": 467, "posted": true }
```

## `bbr pr comments --json`

```json
{
  "pr_id": 467,
  "comments": [
    {
      "id": 10,
      "body": "Looks good",
      "author": "Ash",
      "parent_id": null,
      "deleted": false,
      "created_on": "2026-06-27T10:00:00Z",
      "updated_on": "2026-06-27T10:00:00Z"
    }
  ]
}
```

## `bbr pr tasks --json`

```json
{
  "pr_id": 467,
  "tasks": [
    {
      "id": 20,
      "state": "UNRESOLVED",
      "body": "Update docs",
      "creator": "Ash",
      "assignee": "Sam",
      "created_on": "2026-06-27T10:00:00Z",
      "updated_on": "2026-06-27T10:00:00Z"
    }
  ]
}
```

## `bbr pr commits --json`

```json
{
  "pr_id": 467,
  "commits": [
    {
      "hash": "abc123",
      "message": "Fix bug",
      "author": "Dev <dev@example.com>",
      "date": "2026-06-27T10:00:00Z"
    }
  ]
}
```

## `bbr pr statuses --json`

```json
{
  "pr_id": 467,
  "statuses": [
    {
      "state": "SUCCESSFUL",
      "key": "lint",
      "name": "Lint",
      "url": "https://ci.example/lint",
      "description": "all good",
      "refname": "feat/x"
    }
  ]
}
```

## `bbr pr conflicts --json`

```json
{
  "pr_id": 467,
  "conflicts": [
    {
      "path": "src/lib.rs",
      "conflict_type": "content",
      "kind": null
    }
  ]
}
```

## `bbr pr request-changes --json`

```json
{ "id": 467, "changes_requested": true }
```

`bbr pr unrequest-changes --json` uses the same shape with
`"changes_requested": false`.

## `bbr ci status --json`

```json
{
  "branch": "feat/x",
  "pipeline": {
    "uuid": "{abc-123}",
    "build_number": 42,
    "state": "SUCCESSFUL",
    "duration_seconds": 172,
    "branch": "test-ci",
    "commit": "4644ec4b",
    "steps": [
      { "name": "Run Tests", "state": "SUCCESSFUL", "duration_seconds": 172 }
    ]
  }
}
```

## `bbr ci watch --json`

Emits a single JSON object when the pipeline reaches a terminal state:

```json
{
  "uuid": "{abc-123}",
  "final_state": "SUCCESSFUL",
  "duration_seconds": 172,
  "success": true
}
```

On failure, `success` is `false` and the process exits with code `5`. The
`failing_step` and `failure_log` fields appear when a step failed and
`--logs` was not used. `failure_log` contains the last portion of the failing
step's log (up to 64KB, fetched via an HTTP range request; the complete log
is used when the server does not support ranges).

## `bbr ci logs --json`

```json
{
  "pipeline_uuid": "abc-123",
  "step": "step-1",
  "log": "<raw log text>"
}
```

## `bbr auth status --json`

```json
{
  "authenticated": true,
  "username": "you@example.com",
  "credential_kind": "atlassian_api_token",
  "display_name": "Your Name",
  "account_id": "{...}",
  "source": "environment",
  "rate_limit_remaining": 950
}
```

`credential_kind` is `"atlassian_api_token"` or `null` when not authenticated.
`source` is `"environment"`, `"config-file"`, or `"none"`.
`rate_limit_remaining` is omitted until at least one API response has returned the header.

## `bbr doctor --json`

Emits a JSON array of check results. The command always exits 0 unless
`--strict` is passed and at least one check has status `fail`.

```json
[
  { "name": "git", "status": "ok", "detail": "git version 2.53.0" },
  { "name": "repo identity", "status": "ok", "detail": "ws/slug" },
  { "name": "credentials", "status": "fail", "detail": "none found — run `bbr auth setup` ..." },
  { "name": "creds permissions", "status": "warn", "detail": "no credentials file" },
  { "name": "pager tools", "status": "warn", "detail": "bat missing — ..." },
  { "name": "api reachable", "status": "fail", "detail": "HTTP 401: ..." },
  { "name": "rate limit", "status": "warn", "detail": "no rate-limit header seen yet" },
  { "name": "version", "status": "ok", "detail": "0.2.2 (latest)" }
]
```

`status` is one of `"ok" | "warn" | "fail"`. Check names are stable; new
checks may be added over time, so consume by name, not position.

## Themes

Diff and syntax-highlight colors follow a background preset:

```sh
bbr config set ui.theme dark    # deep tints + base16-ocean.dark (default look)
bbr config set ui.theme light   # pale tints + InspiredGitHub palette
bbr config set ui.theme auto    # detect via COLORFGBG, fall back to dark
```

The setting lives in `config.toml` under `[ui]` and applies to the next run.

## `bbr repo info --json`

```json
{
  "workspace": "sdadev",
  "slug": "bvrm",
  "full_name": "sdadev/bvrm",
  "scm": "git",
  "private": true,
  "language": "Rust",
  "description": "...",
  "web_url": "https://bitbucket.org/sdadev/bvrm"
}
```

## `bbr repo tags --json`

```json
[
  {
    "name": "v1.0.0",
    "target": "abc123",
    "date": "2026-06-27T10:00:00Z"
  }
]
```

## `bbr commit status set --json`

```json
{
  "commit": "abc123",
  "key": "lint",
  "state": "SUCCESSFUL",
  "name": "Lint",
  "url": "https://ci.example/lint",
  "description": "all good",
  "refname": "feat/x"
}
```

## `bbr pr diff --json`

```json
{
  "id": 467,
  "files": [
    {
      "status": "modified",
      "old_path": "src/main.rs",
      "new_path": "src/main.rs",
      "additions": 3,
      "deletions": 1,
      "hunks": [
        {
          "old_start": 42,
          "old_lines": 4,
          "new_start": 42,
          "new_lines": 6,
          "header": "fn foo()",
          "lines": [
            { "kind": "context",  "old_lineno": 42, "new_lineno": 42, "content": "fn foo() {" },
            { "kind": "deletion", "old_lineno": 43, "new_lineno": null, "content": "    bar()" },
            { "kind": "addition", "old_lineno": null, "new_lineno": 43, "content": "    baz()" },
            { "kind": "context",  "old_lineno": 44, "new_lineno": 44, "content": "}" }
          ]
        }
      ]
    },
    {
      "status": "added",
      "old_path": "",
      "new_path": "src/lib.rs",
      "additions": 5,
      "deletions": 0,
      "hunks": []
    },
    {
      "status": "modified",
      "old_path": "logo.png",
      "new_path": "logo.png",
      "binary": true,
      "hunks": []
    }
  ],
  "summary": {
    "files_changed": 3,
    "additions": 8,
    "deletions": 1
  }
}
```

`status` values: `"added"`, `"deleted"`, `"modified"`, `"renamed"`.  
`kind` values per line: `"context"`, `"addition"`, `"deletion"`.  
`old_lineno` is `null` for additions; `new_lineno` is `null` for deletions.  
`binary` is `true` for binary file changes (omitted when false).  
`hunks` is empty for binary files or files with no parseable diff content.

`--json` takes precedence over `--raw` / `--name-only` / `--name-status`:
when `--json` is set the structured shape above is always emitted (never the
legacy flat `{ "id", "diff" }` shape, which was ambiguous and produced
corrupted stdout when combined with `--json`).

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | success |
| 1 | generic error |
| 2 | auth error |
| 3 | not found |
| 4 | rate limited |
| 5 | pipeline failed (`bbr ci watch`) or deployment failed (`bbr deploy view --wait` / `bbr deploy trigger --wait`) |
| 64 | usage error (invalid flags/arguments — distinct from operation failures) |
