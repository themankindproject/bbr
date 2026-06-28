# bbr Usage Guide

> Complete reference for `bbr`, the Bitbucket Cloud CLI.

---

- [Quick Start](#quick-start)
- [Commands](#commands)
  - [`bbr status`](#bbr-status)
  - [`bbr pr`](#bbr-pr)
  - [`bbr ci`](#bbr-ci)
  - [`bbr repo`](#bbr-repo)
  - [`bbr commit`](#bbr-commit)
  - [`bbr open`](#bbr-open)
  - [`bbr auth`](#bbr-auth)
  - [`bbr completion`](#bbr-completion)
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

bbr status              # PR + CI for current branch
bbr pr list             # open PRs
bbr pr create --title T --body B
bbr ci list             # pipelines for this branch
bbr open pr             # open current PR in browser
```

---

## Commands

### `bbr status`

PR + CI for the current branch in one view. The killer feature.

```bash
bbr status                        # full overview
bbr status --short                # compact single-line
bbr status --watch [--interval N] # live refresh every N seconds (default 5)
bbr status --json                 # machine-readable
```

#### `--short`

```
sdadev/bvrm-backend  feat/av1-ffprobe-timeout  cedc6b27d5  #467 OPEN | SUCCESSFUL  7m 48s
```

#### `--watch`

```
bbr status --watch (refreshing every 5s — Ctrl+C to stop)

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

### `bbr pr`

Pull request operations.

#### `bbr pr list`

```bash
bbr pr list                                  # open PRs (default)
bbr pr list --state merged                   # merged PRs
bbr pr list --state all --limit 50           # all states, more results
bbr pr list --author "John"                  # filter by author display name
bbr pr list --reviewer "Jane"                # filter by reviewer display name
bbr pr list --source-branch "feat/x"         # filter by source branch
bbr pr list --json                           # JSON array
```

Output uses a table with columns: ID, State, Title, Source, Destination, Author.

#### `bbr pr view`

```bash
bbr pr view                          # current branch's open PR
bbr pr view 467                      # by ID
bbr pr view --diff                   # append diff to output
bbr pr view --comments               # show comments inline
bbr pr view --json
```

#### `bbr pr create`

```bash
bbr pr create --title "Fix X" --body "Description"
bbr pr create --title "Fix X" --body-file pr.md
bbr pr create --title "Fix X" --body-stdin       # body from stdin
bbr pr create --title "Fix X" \
  --src feat/x --dst main                       # explicit branches
bbr pr create --title "Fix X" \
  --close-source-branch                         # auto-close source
bbr pr create --title "Fix X" \
  --reviewer "user1" --reviewer "user2"         # add reviewers
```

Defaults: `--src` = current branch, `--dst` = repo default branch.

#### `bbr pr update`

```bash
bbr pr update 467 --title "New title"
bbr pr update 467 --description "New description"
bbr pr update 467 --title "New" --description "New"
```

#### `bbr pr comment`

```bash
bbr pr comment 467 --body "Looks good!"
bbr pr comment 467 --body-file review.md
bbr pr comment 467 --reply-to 123 --body "Agreed"   # reply to a comment
```

#### `bbr pr approve / decline / merge`

```bash
bbr pr approve 467                              # approve
bbr pr unapprove 467                            # remove approval
bbr pr decline 467                              # decline (close without merging)
bbr pr merge 467                                # merge with confirmation prompt
bbr pr merge 467 --close-source-branch          # close source branch after merge
bbr pr merge 467 --strategy squash              # merge strategy (merge_commit|squash|fast_forward)
bbr pr merge 467 --message "closes #123"        # custom merge commit message
```

#### Review data subcommands

All default to the current branch's PR when ID is omitted.

```bash
bbr pr comments [<id>] [--limit 50]
bbr pr tasks [<id>] [--limit 50]
bbr pr commits [<id>] [--limit 50]
bbr pr statuses [<id>] [--limit 50]    # commit build statuses
bbr pr conflicts [<id>]                # merge conflict info
```

Change requests:

```bash
bbr pr request-changes 467
bbr pr unrequest-changes 467
```

---

### `bbr ci`

Pipeline / CI operations.

#### `bbr ci list`

```bash
bbr ci list                          # latest pipelines for current branch
bbr ci list --branch main            # pipelines for a specific branch
bbr ci list --json
```

Output uses a table with columns: #, State, Step, Duration. Each step is its own row.

#### `bbr ci status`

```bash
bbr ci status                        # latest pipeline for current branch
bbr ci status --branch main
bbr ci status --json
```

#### `bbr ci steps`

```bash
bbr ci steps                         # steps for latest pipeline (current branch)
bbr ci steps <uuid>                  # steps for a specific pipeline
bbr ci steps --json
```

Output uses a table with columns: Step, State, Duration.

#### `bbr ci watch`

Live-tail a running pipeline. Exits non-zero on failure.

```bash
bbr ci watch                         # current branch
bbr ci watch --branch main
bbr ci watch --logs                  # print failing log on failure
bbr ci watch --interval-secs 10      # poll interval (default 5)
```

#### `bbr ci tests`

Pipeline test reports from Bitbucket's test reporting API. Shows pass/fail/skip/error totals and individual test cases.

```bash
bbr ci tests                         # latest pipeline (current branch), first failed/latest step
bbr ci tests <uuid>                  # specific pipeline
bbr ci tests --step <step-uuid>      # specific step UUID or name
bbr ci tests --limit 100             # max test cases (default 50)
bbr ci tests --json
```

Output:
```
Test report for Run Tests / {abc-123}
─────────────────────────────────────
  [ok]  [failed]  [skip]  [err]  Total
     38        2       1       0     41

Test cases:
  Status  │ Name                  │ Duration
  ────────┼───────────────────────┼─────────
  [ok]    │ test_foo              │ 1.23s
  [fail]  │ test_bar              │ 0.45s
  [skip]  │ test_baz              │ -
```

#### `bbr ci logs`

```bash
bbr ci logs                          # smart default: failed step, else latest
bbr ci logs --failed                 # require a failed step
bbr ci logs --latest                 # latest step from latest pipeline
bbr ci logs <uuid>                   # first step's log for a pipeline
bbr ci logs <uuid> --failed          # failed step for a pipeline
bbr ci logs <uuid> --step <step-uuid> # specific step UUID
bbr ci logs <uuid> --step "Run Tests" # specific step name
bbr ci logs --output ./pipeline.log  # write log to file (not stdout)
```

---

### `bbr repo`

Repository information.

```bash
bbr repo info                        # workspace, slug, language, url, etc.
bbr repo branches [--limit 50]       # remote branches (table)
bbr repo tags [--limit 50]           # remote tags (table)
bbr repo commits [--branch main] [--limit 50]  # commits (table)
```

All support `--json`.

---

### `bbr commit`

Create or update a build status on a commit. Defaults to HEAD when commit is omitted.

```bash
bbr commit status set [<commit>] \
  --key lint \
  --state successful \
  --name "Lint" \
  --url "$CI_JOB_URL" \
  --description "All checks passed" \
  --refname "$BITBUCKET_BRANCH"
```

Accepted states: `successful`, `failed`, `inprogress`, `stopped`.

---

### `bbr open`

Open Bitbucket pages in your browser. With `--json`, prints the URL and does not launch a browser.

```bash
bbr open                           # repository page
bbr open repo                      # same
bbr open pr-list                   # PR list
bbr open pr                        # current branch's open PR
bbr open pr 467                    # PR by ID
bbr open pipelines                 # pipelines list
bbr open ci                        # latest pipeline for current branch
bbr open ci --branch main
```

---

### `bbr auth`

Credential management.

```bash
bbr auth setup                     # interactive credential setup
bbr auth test                      # validate credentials against /user
bbr auth status                    # show current auth method
bbr auth logout                    # remove stored credentials
```

`bbr auth test` output:
```
✓ Authenticated as Your Name (you@example.com)
```

---

### `bbr completion`

```bash
bbr completion bash > /etc/bash_completion.d/bb
bbr completion zsh > "${fpath[1]}/_bb"
bbr completion fish > ~/.config/fish/completions/bb.fish
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

Created by `bbr auth setup`:

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
| 5 | pipeline failed (`bbr ci watch`) |

---

## JSON Schema

All data commands accept `--json`. Schema is documented in [`docs/output-schema.md`](docs/output-schema.md).

### Common patterns

```bash
bbr status --json       # { branch, commit, pr?, pipeline?, ... }
bbr pr list --json      # { workspace, slug, state, pull_requests: [...] }
bbr ci status --json    # { branch, pipeline: { uuid, state, steps, ... } }
```

---

## Scripting Patterns

```bash
# Check PR state
bbr status --json | jq -r '.pr.state'

# Get PR URL
bbr pr view --json | jq -r '.url'

# Create PR and get URL
bbr pr create --title "Fix" --body-file body.md --json | jq -r '.url'

# Wait for CI
bbr ci watch --branch "$BRANCH" --interval-secs 10

# List PRs with details
bbr pr list --state open --json | jq -c '.pull_requests[] | {id, title}'

# Check commit status
bbr pr statuses --json | jq -r '.statuses[] | select(.state == "FAILED") | .key'
```

---

## Error Handling

All errors go to stderr with a clear message:

```bash
$ bbr status
bb: no Bitbucket credentials found; run `bbr auth setup` or set BITBUCKET_USERNAME + BITBUCKET_TOKEN
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
