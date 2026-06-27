# bbr Usage Guide

> Complete reference and examples for `bbr`, the Bitbucket Cloud CLI for coding agents and humans.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Core Concepts](#core-concepts)
- [Commands](#commands)
  - [`bb status`](#bb-status) — the killer feature
  - [`bb pr`](#bb-pr) — pull request operations
  - [`bb ci`](#bb-ci) — pipeline operations
  - [`bb repo`](#bb-repo) — repository metadata
  - [`bb commit`](#bb-commit) — commit build statuses
  - [`bb open`](#bb-open) — browser shortcuts
  - [`bb auth`](#bb-auth) — credential management
  - [`bb completion`](#bb-completion) — shell completions
- [Authentication](#authentication)
- [Output Formats](#output-formats)
- [Exit Codes](#exit-codes)
- [JSON Schema](#json-schema)
- [Scripting Patterns](#scripting-patterns)
- [Error Handling](#error-handling)
- [Performance](#performance)
- [Roadmap](#roadmap)
- [Environment Variables](#environment-variables)
- [License](#license)

---

## Quick Start

### Install

```bash
# from source
cargo install --locked --git https://github.com/themankindproject/bbr

# pre-built binary (releases page)
curl -sSf https://github.com/themankindproject/bbr/releases/latest/download/bbr-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv bb /usr/local/bin/bb
```

### Authenticate

```bash
# Option A: env vars (CI / scripts)
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="..."

# Option B: interactive setup (local dev)
bb auth setup
```

### Use

```bash
bb status            # PR + CI for current branch
bb pr list           # open PRs
bb pr create --title "Fix X" --body-file pr.md
bb pr comments       # review comments for current branch's PR
bb pr tasks          # tasks for current branch's PR
bb ci status         # last pipeline
bb ci logs --failed  # failed step log from latest pipeline
bb commit status set --key lint --state successful --url "$CI_JOB_URL"
bb ci watch --logs   # live-tail and print failing logs on failure
bb open pr           # open current branch's PR
```

---

## Core Concepts

### What is bbr?

**BitBucket Remote** — a single-binary CLI that wraps the Bitbucket Cloud REST API. Designed for coding agents (fast, machine-readable) and humans (pretty output).

### Agent-first design

Every data command supports `--json` for stable, predictable output:

```bash
bb status --json
bb pr list --json
bb ci status --json
```

JSON output is designed for piping to other tools or consuming by coding agents.

### Human-first design

Default output uses pretty tables, color, and emoji. Respects `NO_COLOR` and auto-disables decoration when stdout is not a TTY.

### Exit codes

Stable exit codes for scripting:

| Code | Meaning |
|------|---------|
| 0 | success |
| 1 | generic error |
| 2 | auth error (no creds / bad creds) |
| 3 | not found (no PR / no pipeline) |
| 4 | API rate limit |
| 5 | pipeline failed (for `bb ci watch`) |

---

## Commands

### `bb status`

The killer feature — shows PR + CI for the current branch in one view.

```bash
$ bb status
Repo: sdadev/bvrm-backend
On branch: feat/av1-ffprobe-timeout  (commit 765d8bec)

PR #467 — open
  feat/av1-ffprobe-timeout -> main
  Title: create frame_utils_1_2 with ffprobe-based AV1 detection
  Author: bravo1goingdark
  Reviewers: Ash approved, Sam pending
  Comments: 5  /  Tasks: 1
  URL:   https://bitbucket.org/sdadev/bvrm-backend/pull-requests/467

CI - last pipeline
  [ok] SUCCESSFUL (172s)
  Branch: test-ci  /  Commit: 4644ec4b
  Steps:
    [ok] Run Tests        172s

Next:
  bb open pr
  bb open ci
```

#### JSON output

```bash
$ bb status --json
{
  "repo": {
    "workspace": "sdadev",
    "slug": "bvrm-backend",
    "full_name": "sdadev/bvrm-backend"
  },
  "branch": "feat/av1-ffprobe-timeout",
  "commit": "765d8bec",
  "pr": {
    "id": 467,
    "state": "OPEN",
    "title": "create frame_utils_1_2 with ffprobe-based AV1 detection",
    "source": "feat/av1-ffprobe-timeout",
    "destination": "main",
    "url": "https://bitbucket.org/sdadev/bvrm-backend/pull-requests/467",
    "author": "bravo1goingdark",
    "comment_count": 5,
    "task_count": 1,
    "reviewers": [
      { "display_name": "Ash", "approved": true },
      { "display_name": "Sam", "approved": false }
    ]
  },
  "pipeline": {
    "uuid": "{abc-123}",
    "state": "SUCCESSFUL",
    "duration_seconds": 172,
    "branch": "test-ci",
    "commit": "4644ec4b",
    "url": "https://bitbucket.org/sdadev/bvrm-backend/pipelines/results/42",
    "failing_steps": [],
    "steps": [
      { "uuid": "{step-1}", "name": "Run Tests", "state": "SUCCESSFUL", "duration_seconds": 172 }
    ]
  },
  "suggested_commands": ["bb open pr", "bb open ci"]
}
```

---

### `bb pr`

Pull request operations.

#### `bb pr list`

```bash
$ bb pr list
ID    State   Title                                                  Source -> Destination
467   OPEN    create frame_utils_1_2 with ffprobe-based AV1 detection feat/av1-ffprobe-timeout -> main
462   OPEN    Fix library flag to mark matches outside licensed...    fix/lib-flag -> main
458   MERGED  Increase download chunk size to 512KB                  feat/chunk-size -> main
```

Options:

| Flag | Description | Default |
|------|-------------|---------|
| `--state` | Filter by state: `open`, `merged`, `declined`, `all` | `open` |
| `--limit` | Max PRs to show | `25` |
| `--json` | Emit JSON | `false` |

#### `bb pr view`

View a single PR. Defaults to the current branch's PR.

```bash
bb pr view 467          # by ID
bb pr view              # current branch's PR
bb pr view --json       # JSON output
```

#### `bb pr create`

Create a pull request.

```bash
bb pr create --title "Fix X" --body "Description" --source feat/x --destination main
bb pr create --title "Fix X" --body-file pr-description.md
echo "Body from stdin" | bb pr create --title "Fix X" --body-stdin
```

Options:

| Flag | Description | Default |
|------|-------------|---------|
| `--title` | PR title (required) | — |
| `--body` | PR description | `null` |
| `--body-file` | Read body from file | — |
| `--body-stdin` | Read body from stdin | `false` |
| `--src` | Source branch | current branch |
| `--dst` | Destination branch | `main` |
| `--close-source-branch` | Close source branch after merge | `false` |
| `--json` | Emit JSON | `false` |

#### `bb pr comment`

Add a comment to a PR.

```bash
bb pr comment 467 --body "Looks good!"
bb pr comment 467 --body-file review.md
echo "Approved" | bb pr comment 467 --body-stdin
```

#### PR review data

Review subcommands default to the current branch's open PR when the ID is
omitted.

```bash
bb pr comments [467] [--limit 50]
bb pr tasks [467] [--limit 50]
bb pr commits [467] [--limit 50]
bb pr statuses [467] [--limit 50]
bb pr conflicts [467] [--limit 50]
```

Change requests are explicit write operations:

```bash
bb pr request-changes 467
bb pr unrequest-changes 467
```

---

### `bb ci`

Pipeline / CI operations.

#### `bb ci status`

Show the latest pipeline for a branch.

```bash
$ bb ci status
Branch: feat/x

Pipeline #42  [ok] SUCCESSFUL  (172s)
  Branch: test-ci  /  Commit: 4644ec4b
  Steps:
    [ok] Run Tests        172s
```

Options:

| Flag | Description | Default |
|------|-------------|---------|
| `--branch` | Branch to check | current branch |
| `--json` | Emit JSON | `false` |

#### `bb ci watch`

Live-tail a running pipeline. Exits non-zero on failure (for CI scripts).

```bash
$ bb ci watch
Watching pipeline {abc-123} on test-ci...
  [~] IN_PROGRESS      Run Tests (1m 23s)
  [ok] SUCCESSFUL       Run Tests (2m 52s)

Pipeline [ok] SUCCESSFUL in 172s
```

Options:

| Flag | Description | Default |
|------|-------------|---------|
| `--branch` | Branch to watch | current branch |
| `--interval-secs` | Poll interval | `5` |
| `--logs` | Print the failing step log when the pipeline fails | `false` |
| `--json` | Emit JSON | `false` |

#### `bb ci logs`

Fetch logs for a pipeline step. With no UUID, it uses the latest pipeline for the current branch and prefers a failed step.

```bash
bb ci logs                              # smart default: failed step, else latest step
bb ci logs --failed                     # require a failed step
bb ci logs --latest                     # latest step from latest pipeline
bb ci logs {abc-123}                    # first step's log for a specific pipeline
bb ci logs {abc-123} --failed           # failed step for a specific pipeline
bb ci logs {abc-123} --step {step-1}    # specific step UUID
bb ci logs {abc-123} --step "Run Tests" # specific step name
```

---

### `bb repo`

Repository metadata.

```bash
$ bb repo info
workspace: sdadev
slug:      bvrm-backend
full name: sdadev/bvrm-backend
scm:       git
private:   true
language:  Rust
url:       https://bitbucket.org/sdadev/bvrm-backend
```

```bash
bb repo branches [--limit 20]
bb repo tags [--limit 20]
bb repo commits [--branch main] [--limit 20]
```

---

### `bb commit`

Commit metadata and build statuses.

Create or update a build status for a commit. If `commit` is omitted, `bb`
uses the current HEAD commit.

```bash
bb commit status set [commit] \
  --key lint \
  --state successful \
  --name "Lint" \
  --url "$CI_JOB_URL" \
  --description "All checks passed" \
  --refname "$BITBUCKET_BRANCH"
```

Accepted states are `successful`, `failed`, `inprogress`, and `stopped`.

---

### `bb open`

Open Bitbucket pages in your browser. With `--json`, `bb` prints the URL and does not launch a browser.

```bash
bb open              # repository page
bb open repo         # repository page
bb open pr           # current branch's open PR
bb open pr 467       # PR by ID
bb open ci           # latest pipeline for current branch
bb open ci --branch main
```

---

### `bb auth`

Credential management.

#### `bb auth setup`

Interactive credential setup.

```bash
$ bb auth setup
  Need a Personal Access Token? https://id.atlassian.com/manage-profile/security/api-tokens
  Required scopes: account:read, repository:read, repository:write,
                   pullrequest:read, pullrequest:write, pipeline:read

Bitbucket username (email): you@example.com
  Credential type:
    1) Atlassian API Token (recommended, from id.atlassian.com)
    2) Personal Access Token (from bitbucket.org)
    3) App password (legacy)
Choose [1]: 1
Secret: ********
  Stored credentials in: ~/.config/bb/credentials.toml
  Run `bb auth status` to verify.
```

#### `bb auth status`

Verify stored credentials work.

```bash
$ bb auth status
Authenticated as Your Name (you@example.com) via config-file
```

#### `bb auth logout`

Remove stored credentials.

```bash
$ bb auth logout
Removed stored credentials.
```

---

### `bb completion`

Emit shell completions to stdout.

```bash
bb completion bash > /etc/bash_completion.d/bb
bb completion zsh > "${fpath[1]}/_bb"
bb completion fish > ~/.config/fish/completions/bb.fish
```

---

## Authentication

`bbr` tries three credential sources, in order:

### 1. Environment variables (CI / scripts)

```bash
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="..."              # PAT (preferred)
# or legacy:
export BITBUCKET_APP_PASSWORD="..."
```

### 2. Config file (local dev)

Created by `bb auth setup`:

```toml
# ~/.config/bb/credentials.toml
[default]
username = "you@example.com"
token = "..."
```

On macOS: `~/Library/Application Support/bb/credentials.toml`.
On Windows: `%APPDATA%\bb\credentials.toml`.

### 3. System keyring (planned for v0.3)

- macOS Keychain
- Linux Secret Service (gnome-keyring, kwallet)
- Windows Credential Manager

### PAT scopes

Required scopes for a Personal Access Token:

| Scope | Access |
|-------|--------|
| `account:read` | Read user info |
| `repository:read` | Read repos and branches |
| `repository:write` | Create PRs and create/update commit statuses |
| `pullrequest:read` | Read PRs |
| `pullrequest:write` | Create PRs/comments and request changes |
| `pipeline:read` | Read pipeline status |

### App password scopes (legacy)

| Scope | Access |
|-------|--------|
| `Pull requests: Read, Write` | Read and create PRs |
| `Pipelines: Read` | Read pipeline status |
| `Account: Read` | Read user info |
| `Repositories: Read` | Read repos and branches |

---

## Output Formats

### Human output (default)

Pretty tables, color, emoji. Respects `NO_COLOR` and auto-disables decoration when stdout is not a TTY.

```bash
bb pr list                # table output
bb status                 # merged PR + CI view
```

### JSON output (`--json`)

Stable, predictable JSON for coding agents.

```bash
bb pr list --json         # JSON array of PRs
bb status --json          # JSON object with PR + CI
bb ci status --json       # JSON object with pipeline
```

Schema documented in [`docs/output-schema.md`](docs/output-schema.md).

---

## Exit Codes

| Code | Meaning | When |
|------|---------|------|
| 0 | success | command completed successfully |
| 1 | generic error | network failure, invalid input, etc. |
| 2 | auth error | no credentials found or invalid credentials |
| 3 | not found | no PR or pipeline found |
| 4 | API rate limit | Bitbucket API rate limit exceeded |
| 5 | pipeline failed | `bb ci watch` when pipeline fails |

---

## JSON Schema

See [`docs/output-schema.md`](docs/output-schema.md) for the complete schema.

### Common shapes

#### `bb status --json`

```json
{
  "branch": "feat/x",
  "commit": "765d8bec",
  "pr": { "id": 467, "state": "OPEN", "title": "...", "source": "feat/x", "destination": "main", "url": "..." },
  "pipeline": { "uuid": "...", "state": "SUCCESSFUL", "duration_seconds": 172, "steps": [...] }
}
```

#### `bb pr list --json`

```json
{
  "workspace": "sdadev",
  "slug": "bvrm",
  "state": "open",
  "pull_requests": [
    { "id": 467, "state": "OPEN", "title": "...", "source": "feat/x", "destination": "main" }
  ]
}
```

---

## Scripting Patterns

### Check if PR is ready to merge

```bash
if bb status --json | jq -e '.pr.state == "OPEN"' > /dev/null; then
  echo "PR is open, checking CI..."
  if bb status --json | jq -e '.pipeline.state == "SUCCESSFUL"' > /dev/null; then
    echo "CI passed, ready to merge"
  fi
fi
```

### Create PR from script

```bash
bb pr create \
  --title "Fix: $TITLE" \
  --body-file /tmp/pr-body.md \
  --source "$BRANCH" \
  --destination main \
  --json | jq -r '.url'
```

### Watch CI in CI script

```bash
bb ci watch --branch "$BRANCH" --interval-secs 10
# exits non-zero if pipeline fails
```

### Batch check multiple PRs

```bash
bb pr list --state open --json | jq -r '.pull_requests[].id' | while read id; do
  echo "PR #$id:"
  bb pr view "$id" --json | jq '{title, state, url}'
done
```

---

## Error Handling

All errors go to stderr with a clear message:

```bash
bb status 2>&1
# bb: no Bitbucket credentials found; run `bb auth setup` or set BITBUCKET_USERNAME + BITBUCKET_TOKEN
# hint: run `bb auth setup`, or set BITBUCKET_USERNAME + BITBUCKET_TOKEN
```

Exit codes are stable and documented — scripts can branch on them.

---

## Performance

Measured on a typical dev machine:

| Operation | Cold start | Typical |
|-----------|-----------|---------|
| `bb --version` | < 10 ms | < 10 ms |
| `bb status` | 50-100 ms | 200-500 ms |
| `bb pr list` | 50-100 ms | 300-800 ms |
| `bb ci status` | 50-100 ms | 200-500 ms |
| `bb ci watch` | 50-100 ms | polls every 5s |

Cold start includes binary startup + auth resolution. Subsequent API calls are faster.

---

## Roadmap

The implemented command set now covers the common inner loop:

- find the current repo and branch
- inspect PR state, comments, tasks, commits, statuses, and conflicts
- create/update/comment/approve/merge PRs
- inspect and watch pipelines
- fetch logs and steps
- list branches/tags/commits
- publish commit build statuses

The next production-grade improvements should stay workflow-driven rather than
wrapping the whole Bitbucket API at once.

### Highest-value next commands

| Area | Candidate commands | Notes |
|------|--------------------|-------|
| Pipeline test reports | `bb ci tests`, `bb ci tests --failed` | Summarize failing test cases before falling back to full logs |
| Branch restrictions | `bb branch restrictions`, `bb branch protect` | Useful for repo policy audits and reproducible setup |
| Reports and annotations | `bb report publish`, `bb report annotations` | Lets CI and agents write lint/test findings into Bitbucket |
| Source browsing | `bb source ls`, `bb source cat`, `bb source history` | Inspect files at refs without cloning or switching branches |
| Webhooks | `bb webhooks list/create/delete` | Manage integrations as code |
| Downloads | `bb downloads list/upload/delete` | Release artifact workflows |

### Suggested implementation order

1. Add read-only CI test report commands.
2. Add read-only branch restriction listing before write commands.
3. Add report/annotation publishing for CI integrations.
4. Add source browsing as read-only commands.
5. Add webhook and downloads management.

### Design rules for new commands

- Every data command should support `--json`.
- JSON output should be documented before release.
- Write commands should have explicit names and avoid surprising defaults.
- Read commands should default to current repo/current branch when that matches
  existing `bb status`, `bb pr view`, and `bb ci logs` behavior.
- Mock-server integration tests should cover endpoint paths, auth headers, and
  representative response bodies.

---

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `BITBUCKET_USERNAME` | Bitbucket username (email) | — |
| `BITBUCKET_TOKEN` | Personal Access Token (PAT) | — |
| `BITBUCKET_APP_PASSWORD` | Legacy app password | — |
| `BITBUCKET_API_BASE` | API base URL | `https://api.bitbucket.org/2.0` |
| `NO_COLOR` | Disable color output | — |
| `XDG_CONFIG_HOME` | Config directory (Linux) | `~/.config` |

---

## License

MIT. See [LICENSE](LICENSE).
