# bbr Usage Guide

> Complete reference for `bbr`, the Bitbucket Cloud CLI.

---

- [Quick Start](#quick-start)
- [Commands](#commands)
  - [`bb status`](#bb-status)
  - [`bb pr`](#bb-pr)
  - [`bb ci`](#bb-ci)
  - [`bb repo`](#bb-repo)
  - [`bb commit`](#bb-commit)
  - [`bb open`](#bb-open)
  - [`bb auth`](#bb-auth)
  - [`bb completion`](#bb-completion)
- [Authentication](#authentication)
- [Exit Codes](#exit-codes)
- [JSON Schema](#json-schema)
- [Scripting Patterns](#scripting-patterns)
- [Error Handling](#error-handling)
- [Environment Variables](#environment-variables)

---

## Quick Start

```bash
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="<pat-from-id.atlassian.com>"

bb status              # PR + CI for current branch
bb pr list             # open PRs
bb pr create --title T --body B
bb ci list             # pipelines for this branch
bb open pr             # open current PR in browser
```

---

## Commands

### `bb status`

PR + CI for the current branch in one view. The killer feature.

```bash
bb status                        # full overview
bb status --short                # compact single-line
bb status --watch [--interval N] # live refresh every N seconds (default 5)
bb status --json                 # machine-readable
```

#### `--short`

```
sdadev/bvrm-backend  feat/av1-ffprobe-timeout  cedc6b27d5  #467 OPEN | SUCCESSFUL  7m 48s
```

#### `--watch`

```
bb status --watch (refreshing every 5s — Ctrl+C to stop)

sdadev/bvrm-backend
feat/av1-ffprobe-timeout  cedc6b27d5

PR #467 — open
  feat/av1-ffprobe-timeout -> main
  Title: create frame_utils_1_2 with ffprobe-based AV1 detection
  Comments: 0  /  Tasks: 0

Pipeline
────────────────────────────────────
  [ok] SUCCESSFUL  (7m 48s)
  Branch: test-ci  /  Commit: 4644ec4b
  [ok] Run Tests           7m 48s
```

---

### `bb pr`

Pull request operations.

#### `bb pr list`

```bash
bb pr list                          # open PRs (default)
bb pr list --state merged           # merged PRs
bb pr list --state all --limit 50   # all states, more results
bb pr list --json                   # JSON array
```

Output uses a table with columns: ID, State, Title, Source, Destination, Author.

#### `bb pr view`

```bash
bb pr view                          # current branch's open PR
bb pr view 467                      # by ID
bb pr view --diff                   # append diff to output
bb pr view --comments               # show comments inline
bb pr view --json
```

#### `bb pr create`

```bash
bb pr create --title "Fix X" --body "Description"
bb pr create --title "Fix X" --body-file pr.md
bb pr create --title "Fix X" --body-stdin       # body from stdin
bb pr create --title "Fix X" \
  --src feat/x --dst main                       # explicit branches
bb pr create --title "Fix X" \
  --close-source-branch                         # auto-close source
bb pr create --title "Fix X" \
  --reviewer "user1" --reviewer "user2"         # add reviewers
```

Defaults: `--src` = current branch, `--dst` = repo default branch.

#### `bb pr update`

```bash
bb pr update 467 --title "New title"
bb pr update 467 --description "New description"
bb pr update 467 --title "New" --description "New"
```

#### `bb pr comment`

```bash
bb pr comment 467 --body "Looks good!"
bb pr comment 467 --body-file review.md
bb pr comment 467 --reply-to 123 --body "Agreed"   # reply to a comment
```

#### `bb pr approve / decline / merge`

```bash
bb pr approve 467           # approve
bb pr unapprove 467         # remove approval
bb pr decline 467           # decline (close without merging)
bb pr merge 467             # merge with confirmation prompt
```

#### Review data subcommands

All default to the current branch's PR when ID is omitted.

```bash
bb pr comments [<id>] [--limit 50]
bb pr tasks [<id>] [--limit 50]
bb pr commits [<id>] [--limit 50]
bb pr statuses [<id>] [--limit 50]    # commit build statuses
bb pr conflicts [<id>]                # merge conflict info
```

Change requests:

```bash
bb pr request-changes 467
bb pr unrequest-changes 467
```

---

### `bb ci`

Pipeline / CI operations.

#### `bb ci list`

```bash
bb ci list                          # latest pipelines for current branch
bb ci list --branch main            # pipelines for a specific branch
bb ci list --json
```

Output uses a table with columns: #, State, Step, Duration. Each step is its own row.

#### `bb ci status`

```bash
bb ci status                        # latest pipeline for current branch
bb ci status --branch main
bb ci status --json
```

#### `bb ci steps`

```bash
bb ci steps                         # steps for latest pipeline (current branch)
bb ci steps <uuid>                  # steps for a specific pipeline
bb ci steps --json
```

Output uses a table with columns: Step, State, Duration.

#### `bb ci watch`

Live-tail a running pipeline. Exits non-zero on failure.

```bash
bb ci watch                         # current branch
bb ci watch --branch main
bb ci watch --logs                  # print failing log on failure
bb ci watch --interval-secs 10      # poll interval (default 5)
```

#### `bb ci logs`

```bash
bb ci logs                          # smart default: failed step, else latest
bb ci logs --failed                 # require a failed step
bb ci logs --latest                 # latest step from latest pipeline
bb ci logs <uuid>                   # first step's log for a pipeline
bb ci logs <uuid> --failed          # failed step for a pipeline
bb ci logs <uuid> --step <step-uuid> # specific step UUID
bb ci logs <uuid> --step "Run Tests" # specific step name
bb ci logs --output ./pipeline.log  # write log to file (not stdout)
```

---

### `bb repo`

Repository information.

```bash
bb repo info                        # workspace, slug, language, url, etc.
bb repo branches [--limit 50]       # remote branches (table)
bb repo tags [--limit 50]           # remote tags (table)
bb repo commits [--branch main] [--limit 50]  # commits (table)
```

All support `--json`.

---

### `bb commit`

Create or update a build status on a commit. Defaults to HEAD when commit is omitted.

```bash
bb commit status set [<commit>] \
  --key lint \
  --state successful \
  --name "Lint" \
  --url "$CI_JOB_URL" \
  --description "All checks passed" \
  --refname "$BITBUCKET_BRANCH"
```

Accepted states: `successful`, `failed`, `inprogress`, `stopped`.

---

### `bb open`

Open Bitbucket pages in your browser. With `--json`, prints the URL and does not launch a browser.

```bash
bb open                           # repository page
bb open repo                      # same
bb open pr-list                   # PR list
bb open pr                        # current branch's open PR
bb open pr 467                    # PR by ID
bb open pipelines                 # pipelines list
bb open ci                        # latest pipeline for current branch
bb open ci --branch main
```

---

### `bb auth`

Credential management.

```bash
bb auth setup                     # interactive credential setup
bb auth test                      # validate credentials against /user
bb auth status                    # show current auth method
bb auth logout                    # remove stored credentials
```

`bb auth test` output:
```
✓ Authenticated as Your Name (you@example.com)
```

---

### `bb completion`

```bash
bb completion bash > /etc/bash_completion.d/bb
bb completion zsh > "${fpath[1]}/_bb"
bb completion fish > ~/.config/fish/completions/bb.fish
```

---

## Authentication

`bbr` checks credential sources in order:

### 1. Environment variables (CI / scripts)

```bash
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="..."            # PAT (preferred)
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

macOS: `~/Library/Application Support/bb/credentials.toml`.
Windows: `%APPDATA%\bb\credentials.toml`.

### PAT scopes

| Scope | Access |
|-------|--------|
| `account:read` | Read user info |
| `repository:read` | Read repos and branches |
| `repository:write` | Create PRs and create/update commit statuses |
| `pullrequest:read` | Read PRs |
| `pullrequest:write` | Create PRs/comments and request changes |
| `pipeline:read` | Read pipeline status |

---

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | success |
| 1 | generic error |
| 2 | auth failure |
| 3 | not found |
| 4 | rate limited |
| 5 | pipeline failed (`bb ci watch`) |

---

## JSON Schema

All data commands accept `--json`. Schema is documented in [`docs/output-schema.md`](docs/output-schema.md).

### Common patterns

```bash
bb status --json       # { branch, commit, pr?, pipeline?, ... }
bb pr list --json      # { workspace, slug, state, pull_requests: [...] }
bb ci status --json    # { branch, pipeline: { uuid, state, steps, ... } }
```

---

## Scripting Patterns

```bash
# Check PR state
bb status --json | jq -r '.pr.state'

# Get PR URL
bb pr view --json | jq -r '.url'

# Create PR and get URL
bb pr create --title "Fix" --body-file body.md --json | jq -r '.url'

# Wait for CI
bb ci watch --branch "$BRANCH" --interval-secs 10

# List PRs with details
bb pr list --state open --json | jq -c '.pull_requests[] | {id, title}'

# Check commit status
bb pr statuses --json | jq -r '.statuses[] | select(.state == "FAILED") | .key'
```

---

## Error Handling

All errors go to stderr with a clear message:

```bash
$ bb status
bb: no Bitbucket credentials found; run `bb auth setup` or set BITBUCKET_USERNAME + BITBUCKET_TOKEN
```

Exit codes are stable — scripts can branch on `$?`.

---

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `BITBUCKET_USERNAME` | Bitbucket username (email) | — |
| `BITBUCKET_TOKEN` | Personal Access Token | — |
| `BITBUCKET_APP_PASSWORD` | Legacy app password | — |
| `BITBUCKET_API_BASE` | API base URL | `https://api.bitbucket.org/2.0` |
| `NO_COLOR` | Disable color output | — |
| `XDG_CONFIG_HOME` | Config directory (Linux) | `~/.config` |
