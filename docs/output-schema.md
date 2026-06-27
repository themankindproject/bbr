# `bb` JSON output schema (v0.1)

All `bb <cmd> --json` output is stable JSON, printed pretty to stdout. The
shape for each command is documented below. Field names are `snake_case` and
stable across v0.1.x (breaking changes are reserved for v0.2+).

## `bb status --json`

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
    "author": "bravo1goingdark"
  },
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

`pr` and `pipeline` are `null` when absent.

## `bb pr list --json`

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

## `bb pr view --json`

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

## `bb pr create --json`

```json
{ "id": 468, "url": "https://...", "state": "OPEN" }
```

## `bb pr comment --json`

```json
{ "pr_id": 467, "posted": true }
```

## `bb pr comments --json`

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

## `bb pr tasks --json`

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

## `bb pr commits --json`

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

## `bb pr statuses --json`

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

## `bb pr conflicts --json`

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

## `bb pr request-changes --json`

```json
{ "id": 467, "changes_requested": true }
```

`bb pr unrequest-changes --json` uses the same shape with
`"changes_requested": false`.

## `bb ci status --json`

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

## `bb ci watch --json`

Emits a single JSON object when the pipeline reaches a terminal state:

```json
{
  "uuid": "{abc-123}",
  "final_state": "SUCCESSFUL",
  "duration_seconds": 172,
  "success": true
}
```

On failure, `success` is `false` and the process exits with code `5`.

## `bb ci logs --json`

```json
{
  "pipeline_uuid": "abc-123",
  "step": "step-1",
  "log": "<raw log text>"
}
```

## `bb auth status --json`

```json
{
  "authenticated": true,
  "username": "you@example.com",
  "credential_kind": "pat",
  "display_name": "Your Name",
  "account_id": "{...}",
  "source": "environment"
}
```

`credential_kind` is `"pat"`, `"app_password"`, or `null` when not authenticated.
`source` is `"environment"`, `"config-file"`, or `"none"`.

## `bb repo info --json`

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

## `bb repo tags --json`

```json
[
  {
    "name": "v1.0.0",
    "target": "abc123",
    "date": "2026-06-27T10:00:00Z"
  }
]
```

## `bb commit status set --json`

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

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | success |
| 1 | generic error |
| 2 | auth error |
| 3 | not found |
| 4 | rate limited |
| 5 | pipeline failed (`bb ci watch`) |
