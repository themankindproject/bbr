# bbr Usage Guide

> Complete reference for `bbr`, the Bitbucket Cloud CLI.

---

- [Quick Start](#quick-start)
- [Global Flags](#global-flags)
- [Commands](#commands)
  - [`bbr status`](#bbr-status)
  - [`bbr pr`](#bbr-pr)
  - [`bbr batch`](#bbr-batch)
  - [`bbr ci`](#bbr-ci)
  - [`bbr search`](#bbr-search)
  - [`bbr repo`](#bbr-repo)
  - [`bbr commit`](#bbr-commit)
  - [`bbr open`](#bbr-open)
  - [`bbr auth`](#bbr-auth)
  - [`bbr config`](#bbr-config)
  - [`bbr api`](#bbr-api)
  - [`bbr completion`](#bbr-completion)
  - [`bbr update`](#bbr-update)
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
export BITBUCKET_TOKEN="<api-token>"

bbr status                       # PR + CI for current branch
bbr pr list                      # open PRs
bbr pr create --title T --body B
bbr ci trigger                   # trigger a pipeline for current branch
bbr open pr                      # open current PR in browser

# Power features
bbr status --export slack        # Slack-ready standup snippet
bbr pr dashboard                 # workspace-wide PR dashboard
bbr batch merge-approved         # merge all fully-approved PRs
bbr ci compare last 42           # compare latest vs build #42
bbr ci trigger --branch main     # trigger pipeline on main
bbr search "TODO:"               # code search across workspace
bbr repo audit                   # SOC2-readiness compliance check
bbr pr stack init my-stack       # start a stacked PR chain
bbr update                       # self-update to latest release
```

---

## Global Flags

These flags are available on **every** subcommand:

| Flag | Short | Description |
|------|-------|-------------|
| `--json` | | Emit stable JSON instead of human output |
| `--verbose` | `-v` | Increase verbosity (`-v` = info, `-vv` = debug) |
| `--workspace <WS>` | | Override workspace inferred from git remote (env: `BB_WORKSPACE`) |
| `--slug <SLUG>` | | Override repo slug inferred from git remote (env: `BB_SLUG`) |
| `--api-base <URL>` | | Override the Bitbucket API base URL (env: `BITBUCKET_API_BASE`) |
| `--no-pager` | | Disable output paging (don't pipe through `less`) |
| `--quiet` | `-q` | Suppress spinners and non-essential output (env: `BBR_QUIET`) |
| `--color` | | Force ANSI color output |
| `--no-color` | | Disable ANSI color output |

---

## Commands

### `bbr status`

PR + CI overview for the current branch — the killer feature. Running `bbr` with no subcommand shows a workspace-level overview.

```bash
bbr status                          # full PR + CI view
bbr status --short                  # compact single-line
bbr status --watch [--interval N]   # live refresh every N seconds (default 5)
bbr status --json                   # machine-readable JSON
bbr status --export slack           # Slack mrkdwn standup snippet
bbr status --export markdown        # GitHub-flavored Markdown snippet
```

#### `--short`

One-line summary ideal for scripts and status bars:

```
sdadev/bvrm-backend  feat/av1-ffprobe-timeout  cedc6b27d5  #467 OPEN | SUCCESSFUL  7m 48s
```

#### `--watch`

Live-tail mode that refreshes the terminal every N seconds (Ctrl+C to stop):

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

#### `--export slack`

Copies a Slack mrkdwn block ready to paste into a standup thread:

```
*Status for `feat/my-branch` (`myws/myrepo`)*
• PR #467 "Fix AV1 detection" — *OPEN*
  → main | by @alice | 2 comments, 0 tasks
  Reviewers: @bob (approved), @carol
• Pipeline — *SUCCESSFUL*
  Duration: 7m 48s
```

#### `--export markdown`

Produces GitHub-flavored Markdown for PR descriptions, wikis, or Notion:

```markdown
## Status for `feat/my-branch` (`myws/myrepo`)

### Pull Request
- **#467** "Fix AV1 detection" — OPEN → main (by @alice)
  - Comments: 2 | Tasks: 0
  - Reviewers: @bob ✅, @carol

### Pipeline
- **SUCCESSFUL** — Duration: 7m 48s
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
bbr pr list --sort created_on --order asc    # sort by creation date, ascending
bbr pr list --json                           # JSON array
```

Sort fields: `created_on`, `updated_on` (default), `title`. Order: `desc` (default), `asc`.

Output: table with columns `ID  State  Title  Source  Destination  Author  URL`.

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
bbr pr create --title "Fix X" --draft            # create as draft PR
bbr pr create --title "Fix X" \
  --src feat/x --dst main                        # explicit branches
bbr pr create --title "Fix X" \
  --close-source-branch                          # auto-close source
bbr pr create --title "Fix X" \
  --reviewer "user1" --reviewer "user2"          # add reviewers (repeatable)
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

#### `bbr pr approve / unapprove / decline / merge`

```bash
bbr pr approve 467                               # approve
bbr pr approve 467 --message "LGTM!"             # approve with comment
bbr pr unapprove 467                             # remove approval
bbr pr decline 467                               # decline (close without merging)
bbr pr merge 467                                 # merge with confirmation prompt
bbr pr merge 467 --close-source-branch           # close source branch after merge
bbr pr merge 467 --strategy squash               # merge strategy: merge_commit | squash | fast_forward
bbr pr merge 467 --message "closes #123"         # custom merge commit message
```

#### `bbr pr checkout`

Check out a PR's source branch locally:

```bash
bbr pr checkout 467
```

#### `bbr pr diff`

Print the diff for a PR with syntax highlighting and paging. Uses `bat` if installed, falls back to `less`/`$PAGER`, and writes raw to stdout when piped.

```bash
bbr pr diff 467
```

#### `bbr pr diffstat`

Show a JSON summary of file changes (files changed, insertions, deletions).

```bash
bbr pr diffstat 467                            # by ID
bbr pr diffstat                                # current branch's open PR
bbr pr diffstat 467 --json                     # machine-readable
```

#### `bbr pr patch`

Download the unified patch for a PR. Uses `bat` for syntax highlighting when available.

```bash
bbr pr patch 467                               # print to stdout
bbr pr patch 467 --output fix.patch            # write to file
bbr pr patch                                   # current branch's open PR
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

#### `bbr pr dashboard`

Cross-workspace PR dashboard — shows all PRs awaiting your review, all your open PRs, and recent merged activity. Scans up to 15 most recently updated repos concurrently. Repo list is cached for 24 hours at `~/.config/bbr/cache/dashboard-repos-{workspace}.json`.

```bash
bbr pr dashboard                         # full workspace dashboard
bbr pr dashboard --repos 50              # limit to 50 repos scanned
bbr pr dashboard --filter "api"          # only repos matching "api" in name/slug
bbr pr dashboard --json                  # machine-readable JSON
```

Output sections:

```
● PR Dashboard — myworkspace (@alice)
────────────────────────────────────────────

Needs Your Review (2)
  PR #83   myrepo         "Add caching layer" → main  by @bob
  PR #101  payments-api   "Fix idempotency" → main    by @carol

Your Open PRs (1)
  PR #92   myrepo         "Refactor auth" → main  OPEN  1 approvals

Recent Activity
  merged  myrepo         PR #88 "Bump deps" by @alice
  merged  payments-api   PR #97 "Hotfix null" by @bob
```

#### `bbr pr stack`

Manage stacked PR chains — a sequence of dependent branches/PRs where each targets the one below it. State is stored in `.bbr/stack.toml` in the repo root.

```bash
# Initialise a new stack on current branch
bbr pr stack init my-stack

# Initialise on an explicit base branch
bbr pr stack init my-stack --base main

# Add a branch to the active stack (creates PR automatically)
bbr pr stack add feat/step-1
bbr pr stack add feat/step-2

# Show stack status with live PR states
bbr pr stack list

# Rebase all branches bottom-up onto their parents
bbr pr stack rebase

# Rebase and immediately force-push (--force-with-lease)
bbr pr stack rebase --push

# Merge all PRs in the stack bottom-up (with confirmation)
bbr pr stack land
bbr pr stack land --strategy squash   # custom merge strategy
bbr pr stack land --yes               # skip confirmation

# Decline all PRs and delete local + remote branches
bbr pr stack abort
bbr pr stack abort --yes              # skip confirmation
```

`stack list` output:

```
● Stack: my-stack (base: main)
────────────────────────────────
  1. feat/step-1    PR #110  OPEN    → main
  2. feat/step-2    PR #111  OPEN    → feat/step-1
```

> **Note:** `rebase` and `land` require a clean working tree. Rebase stops at the first conflict and reports which branch failed. `land` stops at the first merge failure and preserves remaining stack config for retry.

---

### `bbr batch`

Safe bulk operations with a **Plan/Apply** pattern — always shows a table of what will happen before executing. Supports `--dry-run` to stop after the plan, and `--yes` to skip the confirmation prompt.

#### `bbr batch merge-approved`

Merge all open PRs where every reviewer has approved.

```bash
bbr batch merge-approved                         # current repo
bbr batch merge-approved --repo other-slug       # specific repo slug
bbr batch merge-approved --strategy squash       # merge strategy
bbr batch merge-approved --dry-run               # plan only, no changes
bbr batch merge-approved --yes                   # skip confirmation
bbr batch merge-approved --json                  # machine-readable plan/result
```

Plan output:

```
Proposed Merge Plan:
  PR ID  Title                Source      Destination  Approvals
  ─────────────────────────────────────────────────────────────
  467    Fix AV1 detection    feat/av1    main         2
  472    Update deps          chore/deps  main         1
```

#### `bbr batch rerun-failed`

Rerun the latest failed pipeline per branch (deduplicates by branch, keeps only most recent).

```bash
bbr batch rerun-failed                           # all branches
bbr batch rerun-failed --branch "feat/x"         # single branch filter
bbr batch rerun-failed --repo other-slug
bbr batch rerun-failed --dry-run
bbr batch rerun-failed --yes
```

#### `bbr batch cleanup-merged-branches`

Delete merged branches. Protects `main`, `master`, `develop`, `production`, `release/*`, and `hotfix/*` automatically.

```bash
bbr batch cleanup-merged-branches               # local branches only
bbr batch cleanup-merged-branches --remote      # also delete remote branches
bbr batch cleanup-merged-branches --dry-run
bbr batch cleanup-merged-branches --yes
```

> **Note:** Local deletion uses `git branch -d` (safe delete). Remote deletion calls the Bitbucket API.

---

### `bbr ci`

Pipeline / CI operations.

#### `bbr ci list`

```bash
bbr ci list                          # latest pipelines for current branch
bbr ci list --branch main            # specific branch
bbr ci list --limit 20               # max results (default 10)
bbr ci list --json
```

Output: table with columns `#  State  Step  Duration`. Each step is its own row.

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

Output: table with columns `Step  State  Duration`.

#### `bbr ci watch`

Live-tail a running pipeline. Exits with code `5` on pipeline failure.

```bash
bbr ci watch                         # current branch
bbr ci watch --branch main
bbr ci watch --logs                  # print failing step log on failure
bbr ci watch --interval-secs 10      # poll interval (default 5)
```

#### `bbr ci trigger`

Trigger a new pipeline for a branch.

```bash
bbr ci trigger                       # current branch
bbr ci trigger --branch main
```

#### `bbr ci rerun`

Rerun the latest pipeline for a branch.

```bash
bbr ci rerun                         # current branch
bbr ci rerun --branch main
```

#### `bbr ci stop`

Stop a running pipeline.

```bash
bbr ci stop                          # latest running pipeline on current branch
bbr ci stop <uuid>                   # specific pipeline UUID
bbr ci stop --branch main
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

#### `bbr ci compare`

Compare two pipeline runs side by side. Resolves pipeline references flexibly: UUID, build number, or `last`/`latest` for the most recent run on the current branch.

```bash
bbr ci compare last 42               # latest vs build #42
bbr ci compare 42 57                 # build #42 vs build #57
bbr ci compare <uuid-a> <uuid-b>     # by UUID
bbr ci compare last last             # compare two copies of latest (edge case)
bbr ci compare 42 57 --json          # machine-readable deltas
```

Output:

```
Pipeline Comparison
  A: #42 (feat/av1) — [ok] — 7m 48s
  B: #57 (feat/av1) — [fail] — 9m 12s

Step Duration Deltas
  Step              A       B       Δ
  ──────────────────────────────────────────────────────
  Install           23s     25s     +2s
  Build             180s    220s    +40s  ←
  Run Tests         245s    307s    +62s

Test Results
              A         B
  Passed      38        35
  Failed      2         5
  New failures: test_timeout_handling, test_retry_logic
  Fixed:       test_connection_pool
```

The step with the largest absolute duration delta is highlighted with `←`. If no test reports exist for either pipeline the test section is omitted.

---

### `bbr search`

Search code across all repos in the workspace via the Bitbucket code search API.

```bash
bbr search "TODO:"                     # search for TODOs
bbr search "fn main" --limit 50        # max results (default 20)
bbr search "class Repository" --json   # machine-readable
bbr search "error" --repo my-service   # search within specific repo
```

Output:
```
4 result(s) for 'TODO:'
  src/api/pr.rs
    src/api/pr.rs:142    // TODO: handle pagination
  src/commands/pr.rs
    src/commands/pr.rs:89    // TODO: extract helper
```

---

### `bbr issue`

> **Deprecated:** Bitbucket's issue tracker is not available on workspaces created after ~2024. Consider using [Jira](https://www.atlassian.com/software/jira) for issue tracking. These commands will work on older workspaces that have the issue tracker enabled.

```bash
bbr issue list                          # list issues (--status, --kind, --priority, --assignee, --query)
bbr issue view 1                        # view issue details
bbr issue view 1 --comments             # view with comments
bbr issue create --title "Bug" --body "Description" --kind bug --priority major
bbr issue update 1 --status resolved    # update issue
bbr issue comment 1 --body "Working on it"
bbr issue comments 1                    # list comments
```

---

### `bbr repo`

Repository metadata.

```bash
bbr repo info                        # workspace, slug, language, url, etc.
bbr repo branches [--limit 50]       # remote branches (table)
bbr repo tags [--limit 50]           # remote tags (table)
bbr repo commits [--branch main] [--limit 50]  # commits (table)
```

All support `--json`.

#### `bbr repo create`

Create a new repository in the current workspace:

```bash
bbr repo create my-new-repo
bbr repo create my-new-repo --private
bbr repo create my-new-repo --description "A new service" --language rust
bbr repo create my-new-repo --enable-issues    # enable issue tracker
bbr repo create my-new-repo --json
```

#### `bbr repo delete`

Delete a repository (permanent, requires confirmation):

```bash
bbr repo delete my-old-repo                    # with confirmation prompt
bbr repo delete my-old-repo --yes              # skip confirmation
```

#### `bbr repo fork`

Fork a repository:

```bash
bbr repo fork                                  # fork current repo
bbr repo fork --name my-fork                   # custom fork name
bbr repo fork --target-workspace other-ws      # fork to different workspace
```

#### `bbr repo create-branch`

Create a remote branch:

```bash
bbr repo create-branch feature/new             # from current HEAD
bbr repo create-branch feature/new --from abc123  # from specific commit
```

#### `bbr repo create-tag`

Create a remote tag:

```bash
bbr repo create-tag v1.0.0                     # lightweight tag on current HEAD
bbr repo create-tag v1.0.0 --message "Release" # annotated tag
bbr repo create-tag v1.0.0 --target abc123     # tag specific commit
```

#### `bbr repo audit`

Audit repository compliance and SOC2-readiness. Checks branch restrictions, approval requirements, push protection on main, force-push restrictions, and default reviewer configuration.

```bash
bbr repo audit                       # audit all repos in the workspace
bbr repo audit my-repo               # audit a specific repo slug
bbr repo audit --json                # machine-readable full audit report
```

Severity levels:
| Level | Icon | Meaning |
|-------|------|---------|
| `error` | `✖` | Serious compliance gap — must fix |
| `warning` | `⚠` | Recommended practice not followed |
| `info` | `ℹ` | Informational finding |

Checks performed per repository:

| Check | Severity |
|-------|----------|
| No branch restrictions at all | warning |
| No approval requirement for PRs | warning |
| Fewer than 2 required approvers | error |
| Direct pushes allowed to main/master | error |
| Force push / rewrite history allowed | warning |
| Branch deletion allowed | info |
| No default reviewers configured | info |

Output:

```
● Audit — myworkspace — 3 repos
────────────────────────────────

myrepo (3 issues)
  ✖ Direct pushes allowed to main/master branch
  ⚠ Force pushing/rewriting history allowed
  ℹ No default reviewers configured

payments-api ✓ (0 issues)

legacy-service (1 issues)
  ✖ Only 1 required approver (recommend ≥ 2)

Summary: 4 issues (2 errors, 1 warnings, 1 info)
```

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

### `bbr deploy`

Deployment and environment management.

```bash
bbr deploy list                            # list deployments
bbr deploy env list                        # list environments
bbr deploy env create staging --env-type staging   # create environment
bbr deploy env create prod --env-type production
```

Environment types: `test`, `staging`, `production`.

#### Environment variables

```bash
bbr deploy env vars list <env-uuid>        # list env variables
bbr deploy env vars set <env-uuid> KEY value   # set variable
bbr deploy env vars set <env-uuid> KEY value --secured  # encrypted
bbr deploy env vars delete <env-uuid> KEY  # delete variable
```

---

### `bbr open`

Open Bitbucket pages in your browser. With `--json`, prints the URL without launching a browser.

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
bbr auth setup --username u --token t  # non-interactive (for CI scripts)
bbr auth test                      # validate credentials against /user
bbr auth status                    # show current auth method
bbr auth logout                    # remove stored credentials
```

`bbr auth test` output:
```
✓ Authenticated as Your Name (you@example.com)
```

---

### `bbr config`

View and manage local bbr configuration.

```bash
bbr config path                    # print config and credentials file paths
bbr config show                    # show current config (username, workspace, etc.)
bbr config set workspace my-ws     # persist a default workspace
```

`bbr config show` output:
```
config_path:      /home/user/.config/bbr/config.toml
credentials_path: /home/user/.config/bbr/credentials.toml
workspace:        my-workspace
username:         alice@example.com
has_token:        true
```

The only settable key is `workspace`. Once set, `--workspace` overrides and the git remote inference is skipped.

---

### `bbr api`

Raw authenticated passthrough to any Bitbucket REST API endpoint. Always outputs JSON.

```bash
bbr api GET /user
bbr api GET /repositories/myws/myrepo
bbr api POST /repositories/myws/myrepo/issues --data '{"title":"Bug","kind":"bug"}'
bbr api GET /repositories/myws/myrepo/pullrequests --paginate   # follow all pages
```

Pairs well with `jq` for exploration:

```bash
bbr api GET /repositories/myws/myrepo/pullrequests \
  --paginate | jq '[.[] | {id, title, state}]'
```

---

### `bbr completion`

```bash
# Print completion script to stdout
bbr completion bash > /etc/bash_completion.d/bbr
bbr completion zsh  > "${fpath[1]}/_bbr"
bbr completion fish > ~/.config/fish/completions/bbr.fish

# Auto-install for the detected shell ($SHELL)
bbr completion --install
```

---

### `bbr update`

Self-update `bbr` to the latest GitHub release. Downloads the correct binary for your platform from GitHub Releases and replaces the current binary.

```bash
bbr update                            # check + auto-install if newer
bbr update --check                    # check only, no install
bbr update --json                     # machine-readable version info
```

Background version check: running `bbr status` (or bare `bbr`) automatically checks for updates once per 24 hours and prints a notice if a newer version is available. The check is silently skipped in CI environments.

---

## Authentication

`bbr` checks credential sources in priority order:

### 1. Environment variables (CI / scripts)

```bash
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="..."            # Atlassian API token
```

### 2. Config file (local dev)

Created by `bbr auth setup`:

```toml
# ~/.config/bbr/credentials.toml
[default]
username = "you@example.com"
token = "..."
```

Platform paths:
- **Linux**: `~/.config/bbr/credentials.toml`
- **macOS**: `~/Library/Application Support/bbr/credentials.toml`
- **Windows**: `%APPDATA%\bbr\credentials.toml`

### Required scopes

| Scope | Required for |
|-------|-------------|
| `account:read` | Read user info (`bbr auth test`, `bbr pr dashboard`) |
| `repository:read` | Read repos, branches, commits |
| `repository:write` | Create repos, create/update commit statuses |
| `pullrequest:read` | Read PRs, comments, tasks |
| `pullrequest:write` | Create/merge/decline PRs, post comments |
| `pipeline:read` | Read pipelines and test reports |
| `pipeline:write` | Rerun/stop pipelines (`bbr batch rerun-failed`, `bbr ci rerun/stop`) |

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

All data commands accept `--json`. Output is stable and suitable for scripting.

### Common patterns

```bash
bbr status --json         # { branch, commit, repo, pr?, pipeline?, commit_statuses }
bbr pr list --json        # { workspace, slug, state, pull_requests: [...] }
bbr ci status --json      # { branch, pipeline: { uuid, state, steps, ... } }
bbr pr dashboard --json   # { workspace, user, needs_review, my_prs, recent_activity, repo_count }
bbr batch merge-approved --dry-run --json  # { dry_run, action_count, actions: [...] }
bbr ci compare 42 57 --json  # { a, b, step_deltas, test_deltas }
bbr repo audit --json     # { workspace, total_repos, repos: [...], summary }
bbr pr stack list --json  # { name, base_branch, prs: [...] }
bbr search "TODO" --json  # { query, total, results: [{ file, content_matches }] }
bbr update --json         # { current_version, latest_version, up_to_date, downloaded? }
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

# Wait for CI, fail the script if pipeline fails
bbr ci watch --branch "$BRANCH" --interval-secs 10

# List PRs with details
bbr pr list --state open --json | jq -c '.pull_requests[] | {id, title}'

# Check commit status
bbr pr statuses --json | jq -r '.statuses[] | select(.state == "FAILED") | .key'

# Post standup to Slack (via curl + incoming webhook)
bbr status --export slack | curl -s -X POST \
  -H 'Content-type: application/json' \
  --data "{\"text\": \"$(cat -)\"}" \
  "$SLACK_WEBHOOK_URL"

# Audit all repos and fail CI if any errors found
ERRORS=$(bbr repo audit --json | jq '.summary.errors')
if [ "$ERRORS" -gt 0 ]; then
  echo "Compliance audit failed: $ERRORS errors"
  exit 1
fi

# Find which step got slower between two builds
bbr ci compare 50 60 --json | jq '.step_deltas | max_by(.duration_delta) | {name, duration_delta}'

# Batch cleanup dry-run then apply
bbr batch cleanup-merged-branches --dry-run
bbr batch cleanup-merged-branches --remote --yes
```

---

## Error Handling

All errors go to stderr with a clear message:

```bash
$ bbr status
bbr: no Bitbucket credentials found; run `bbr auth setup` or set BITBUCKET_USERNAME + BITBUCKET_TOKEN

$ bbr pr stack rebase
bbr: Working directory is dirty. Please commit or stash changes before rebasing.
```

Exit codes are stable — scripts can branch on `$?`.

---

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `BITBUCKET_USERNAME` | Bitbucket username (email) | — |
| `BITBUCKET_TOKEN` | Atlassian API token | — |
| `BITBUCKET_API_BASE` | API base URL | `https://api.bitbucket.org/2.0` |
| `BB_WORKSPACE` | Default workspace override | — |
| `BB_SLUG` | Default repo slug override | — |
| `BBR_QUIET` | Suppress spinners and non-essential output | — |
| `NO_COLOR` | Disable color output | — |
| `XDG_CONFIG_HOME` | Config directory (Linux) | `~/.config` |
| `RUST_LOG` | Tracing log filter (overrides `--verbose`) | — |
