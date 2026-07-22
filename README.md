# bbr — BitBucket Remote CLI

[![CI](https://img.shields.io/github/actions/workflow/status/themankindproject/bbr/ci.yml?branch=main&label=CI)](https://github.com/themankindproject/bbr/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/release/themankindproject/bbr)](https://github.com/themankindproject/bbr/releases/latest)
![Rust Version](https://img.shields.io/badge/rust-1.88%2B-blue)
[![License](https://img.shields.io/crates/l/bbr)](LICENSE)
![Tests](https://img.shields.io/badge/tests-262%20passing-brightgreen)

A fast, single-binary Bitbucket Cloud CLI. **Agent-first** (`--json` everywhere, stable schemas, env auth) with pretty human output.

Full command reference: **[USAGE.md](USAGE.md)** · JSON schemas: **[docs/output-schema.md](docs/output-schema.md)** · Changelog: **[CHANGELOG.md](CHANGELOG.md)**

---

## Why

Bitbucket Cloud lacked a solid CLI. Coding agents and developers needed something like GitHub's `gh` — scriptable, `--json`-friendly, zero-config auth — without living in `curl` or the web UI.

`bbr` gives you that: status/overview in one shot, full PR + CI lifecycle, batch ops, stacks, and stable exit codes for automation.

---

## Features at a Glance

| Area | What bbr does |
|------|---------------|
| **PR Lifecycle** | List, view, create, update, merge, decline, approve, comment, diff, patch, checkout, tasks, conflicts, stacks |
| **CI / Pipelines** | Status, list, watch, trigger, rerun, stop, logs, tests, compare, schedules, variables |
| **Status Dashboard** | One-command branch status, workspace-wide overview, Slack/Markdown export |
| **Batch Operations** | Merge all approved PRs, rerun failed pipelines, clean up merged branches |
| **Repository** | Info, branches, tags, commits, permissions, create, delete, fork, audit |
| **Code Search** | Search across all workspace repos |
| **Deployments** | Environments, environment variables, deploy keys |
| **Auth** | Interactive setup, credential validation, rate-limit tracking |
| **Raw API** | Passthrough to any Bitbucket REST endpoint |
| **Self-Update** | Auto-update from GitHub releases with SHA256 verification |
| **Pretty Diffs** | Built-in diff renderer with word-level highlighting, side-by-side, line numbers |

---

## Install

```bash
# One-liner (recommended — supports Linux x86_64/aarch64, macOS Intel/ARM)
curl -fsSL https://github.com/themankindproject/bbr/raw/main/install.sh | bash

# Or from source
cargo install --locked --git https://github.com/themankindproject/bbr
```

Pre-built archives: [Releases](https://github.com/themankindproject/bbr/releases/latest) (Linux x86_64, macOS Intel/ARM, Windows).

```bash
bbr completion --install    # auto-detects shell, wires RC file sourcing
bbr completion bash          # or print to stdout: zsh / fish / powershell also supported
```

---

## Auth

HTTP Basic with an [Atlassian API token](https://id.atlassian.com/manage-profile/security/api-tokens):

```bash
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="<api-token>"

# Or interactive file (~/.config/bbr/credentials.toml, mode 0600)
bbr auth setup
bbr auth test
bbr auth status    # also shows API rate-limit remaining
bbr auth logout
```

### Required API Token Scopes

| Scope | Required for |
|-------|-------------|
| `account:read` | Read user info (`bbr auth test`, `bbr pr dashboard`) |
| `repository:read` | Read repos, branches, commits |
| `repository:write` | Create repos, create/update commit statuses |
| `pullrequest:read` | Read PRs, comments, tasks |
| `pullrequest:write` | Create/merge/decline PRs, post comments |
| `pipeline:read` | Read pipelines and test reports |
| `pipeline:write` | Rerun/stop pipelines (`bbr batch rerun-failed`, `bbr ci rerun/stop`) |

### Credential Resolution

Priority order:
1. **Environment variables** — `BITBUCKET_USERNAME` + `BITBUCKET_TOKEN`
2. **Config file** — `~/.config/bbr/credentials.toml` (Linux), `~/Library/Application Support/bbr/credentials.toml` (macOS)

The credentials file is created with Unix `0600` permissions and the secret is stored as a `SecretString` that is zeroized on drop.

---

## Quick Start

```bash
cd my-bitbucket-repo

bbr                           # overview: PRs, approvals, recent CI
bbr status                    # full PR + CI for current branch
bbr pr list
bbr pr create --title "Fix" --body "..."
bbr ci watch --logs
bbr ci list --no-steps        # fast pipeline list
bbr batch merge-approved --max 10
bbr update
```

| Exit | Meaning |
|------|---------|
| 0 | success |
| 1 | generic error |
| 2 | auth failure |
| 3 | not found |
| 4 | rate limited |
| 5 | pipeline failed |

Every data command supports `--json`. See [USAGE.md](USAGE.md) for flags, subcommands, and scripting patterns.

---

## Commands Reference

### Status & Overview

```bash
bbr                                 # overview (current branch + recent PRs/CI)
bbr status                          # full PR + CI view for current branch
bbr status --short                  # compact single-line output
bbr status --watch [--interval N]   # live refresh every N seconds (default 5)
bbr status --json                   # machine-readable JSON
bbr status --export slack           # Slack mrkdwn standup snippet
bbr status --export markdown        # GitHub-flavored Markdown snippet
bbr status --branch feat/x          # override branch detection
```

### Pull Requests

```bash
# List & View
bbr pr list                                      # open PRs (default)
bbr pr list --state merged --limit 50            # merged PRs
bbr pr list --author "John" --reviewer "Jane"    # filter by author/reviewer
bbr pr list --search "cache"                     # search in PR titles/descriptions
bbr pr view                                      # current branch's open PR
bbr pr view 467                                  # by ID
bbr pr view --diff --comments                    # with diff and comments
bbr pr view --side-by-side --context 5           # inline side-by-side diff
bbr pr checkout 467                              # fetch + switch to source branch

# Create & Update
bbr pr create --title "Fix X" --body "Description"
bbr pr create --title "Fix X" --body-file pr.md
bbr pr create --title "Fix X" --draft
bbr pr create --title "Fix X" --close-source-branch
bbr pr create --title "Fix X" --reviewer "user1" --reviewer "user2"
bbr pr update 467 --title "New title" --description "New description"

# Merge & Approve
bbr pr merge 467                                 # merge with confirmation prompt
bbr pr merge 467 --strategy squash               # squash | merge_commit | fast_forward
bbr pr merge 467 --close-source-branch
bbr pr approve 467 --message "LGTM!"
bbr pr unapprove 467
bbr pr decline 467

# Comments & Reviews
bbr pr comment 467 --body "Looks good!"
bbr pr comment 467 --reply-to 123 --body "Agreed"
bbr pr request-changes 467
bbr pr unrequest-changes 467

# Submissions
bbr pr tasks [<id>]         # review tasks
bbr pr conflicts [<id>]     # merge conflict info
bbr pr comments [<id>]      # comments list
bbr pr commits [<id>]       # commits in PR
bbr pr statuses [<id>]      # commit build statuses

# Diff & Patch
bbr pr diff 467                          # pretty diff (word-level, syntax, line numbers)
bbr pr diff                              # open PR for the current branch
bbr pr diff 467 --raw                    # bypass renderer, use bat/less
bbr pr diff 467 --json                   # structured JSON with file/hunk/line data
bbr pr diff 467 --side-by-side           # side-by-side view
bbr pr diff 467 --context 5              # more context lines
bbr pr diff 467 --no-word-diff           # disable intra-line word highlighting
bbr pr diff 467 --no-syntax              # disable syntect syntax highlighting
bbr pr diff 467 --name-only              # paths only
bbr pr diff 467 --name-status            # status + path
bbr pr diff 467 -- src/                  # filter by pathspec
bbr pr diffstat 467                      # file changes summary table
bbr pr patch 467                         # unified patch to stdout
bbr pr patch 467 --output fix.patch      # unified patch to file

# Dashboard & Stacks
bbr pr dashboard                         # workspace-wide PR dashboard
bbr pr dashboard --repos 50 --filter "api"
bbr pr stack init my-stack               # start a stacked PR chain (becomes active)
bbr pr stack use my-stack                # select active stack when several exist
bbr pr stack add feat/step-1             # add branch to stack (auto-creates PR)
bbr pr stack list                        # show active stack with PR states
bbr pr stack rebase                      # rebase all branches bottom-up
bbr pr stack land --strategy squash      # merge all PRs in stack
bbr pr stack abort                       # decline all PRs, delete branches
```

### Batch Operations

Safe bulk operations with **Plan/Apply** pattern — always shows what will happen before executing.

```bash
bbr batch merge-approved                         # merge all fully-approved PRs
bbr batch merge-approved --strategy squash       # merge strategy
bbr batch merge-approved --dry-run               # plan only, no changes
bbr batch merge-approved --max 10                # cap at 10 PRs

bbr batch rerun-failed                           # rerun latest failed pipeline per branch
bbr batch rerun-failed --branch "feat/x"         # single branch filter

bbr batch cleanup-merged-branches               # delete merged local branches
bbr batch cleanup-merged-branches --remote      # also delete remote branches
```

Protected branches (`main`, `master`, `develop`, `production`, `release/*`, `hotfix/*`) are never deleted.

### CI / Pipelines

```bash
# Status & Listing
bbr ci status                        # latest pipeline for current branch
bbr ci list --no-steps               # fast pipeline-only listing
bbr ci list --branch main --limit 20
bbr ci steps <uuid>                  # steps for a specific pipeline

# Live Monitoring
bbr ci watch                         # live-tail, exits with code 5 on failure
bbr ci watch --logs                  # print failing step log on failure
bbr ci logs                          # smart default: failed step, else latest
bbr ci logs <uuid> --step "Run Tests"  # specific step
bbr ci logs --output ./pipeline.log    # write to file

# Trigger & Rerun
bbr ci trigger                       # trigger pipeline for current branch
bbr ci trigger --var DEPLOY_ENV=staging --var KEY=val
bbr ci trigger --var SECRET=x --secured SECRET
bbr ci rerun                         # rerun latest pipeline
bbr ci stop                          # stop a running pipeline

# Reports & Comparison
bbr ci tests                         # test reports for latest pipeline
bbr ci tests --step <step-uuid> --limit 100
bbr ci compare last 42               # compare latest vs build #42
bbr ci compare 42 57 --json          # machine-readable deltas

# Schedules & Variables
bbr ci schedules list
bbr ci schedules create --cron "0 2 * * *" --branch main
bbr ci schedules update <uuid> [--enabled true|false]
bbr ci schedules delete <uuid>

bbr variable list                    # alias for bbr ci vars
bbr variable set KEY value [--secured]
bbr variable delete KEY
```

### Repository

```bash
bbr repo info                        # workspace, slug, language, url, etc.
bbr repo branches [--limit 50]       # remote branches
bbr repo tags [--limit 50]           # remote tags
bbr repo commits [--branch main] [--limit 50]
bbr repo permissions                 # user and group permissions

bbr repo create my-new-repo --private --language rust
bbr repo delete my-old-repo --yes
bbr repo fork --target-workspace other-ws
bbr repo create-branch feature/new --from abc123
bbr repo create-tag v1.0.0 --message "Release"

bbr repo audit                       # SOC2-readiness compliance check
bbr repo audit my-repo               # audit a specific repo
bbr repo audit --json
```

Audit checks: branch restrictions, approval requirements (>= 2 approvers), push protection on main, force-push restrictions, default reviewers.

### Deployments

```bash
bbr deploy list                            # list deployments
bbr deploy env list                        # list environments
bbr deploy env create staging --env-type staging
bbr deploy trigger <env-uuid> --commit <hash>

bbr deploy env vars list <env-uuid>        # environment variables
bbr deploy env vars set <env-uuid> KEY value --secured
bbr deploy env vars delete <env-uuid> KEY
```

Environment types: `test`, `staging`, `production`.

### Deploy Keys

```bash
bbr deploy-keys list
bbr deploy-keys add --key "ssh-rsa AAAA..." --label "ci-runner"
bbr deploy-keys view <key_id>
bbr deploy-keys delete <key_id> --yes
```

### Code Search

```bash
bbr search "TODO:"                     # search across all workspace repos
bbr search "fn main" --limit 50
bbr search "error" --repo my-service   # search within a specific repo
bbr search "class Repository" --json
```

### Issues (Deprecated)

> Bitbucket's issue tracker is not available on workspaces created after ~2024. Consider using Jira.

```bash
bbr issue list --status open --kind bug --priority major
bbr issue view 1 --comments
bbr issue create --title "Bug" --body "Description" --kind bug
bbr issue update 1 --status resolved
bbr issue comment 1 --body "Working on it"
```

### Commit Statuses

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

### Open in Browser

```bash
bbr open                           # repository page
bbr open pr                        # current branch's open PR
bbr open pr 467                    # PR by ID
bbr open pipelines                 # pipelines list
bbr open ci                        # latest pipeline for current branch
```

With `--json`, prints the URL without launching a browser.

### Webhooks

```bash
bbr webhook list
bbr webhook get <uuid>
bbr webhook create --url "https://..." --events "repo:push,pr:updated"
bbr webhook update <uuid> --active
bbr webhook delete <uuid>
```

### Raw API

Passthrough to any Bitbucket REST API endpoint. Always outputs JSON.

```bash
bbr api GET /user
bbr api GET /repositories/myws/myrepo
bbr api POST /repositories/myws/myrepo/issues --data '{"title":"Bug","kind":"bug"}'
bbr api GET /repositories/myws/myrepo/pullrequests --paginate   # follow all pages
```

Pairs well with `jq`:

```bash
bbr api GET /repositories/myws/myrepo/pullrequests \
  --paginate | jq '[.[] | {id, title, state}]'
```

### Workspaces

```bash
bbr workspace list                           # list accessible workspaces
bbr workspace list --role admin              # filter by role
bbr workspace list --json
```

### Configuration

```bash
bbr config path                    # print config and credentials file paths
bbr config show                    # show current config (username, workspace, etc.)
bbr config set workspace my-ws     # persist a default workspace
```

### Self-Update

```bash
bbr update                            # check + auto-install if newer
bbr update --check                    # check only, no install
bbr update --json                     # machine-readable version info
```

Background version check: running `bbr status` automatically checks for updates once per 24 hours. The check is silently skipped in CI environments. SHA256 checksum verification is performed when available.

---

## Global Flags

| Flag | Short | Description |
|------|-------|-------------|
| `--json` | | Emit stable JSON instead of human output |
| `--verbose` | `-v` | Increase verbosity (`-v` = info, `-vv` = debug) |
| `--workspace <WS>` | | Override workspace (env: `BB_WORKSPACE`) |
| `--slug <SLUG>` | | Override repo slug (env: `BB_SLUG`) |
| `--api-base <URL>` | | Override API base URL (env: `BITBUCKET_API_BASE`) |
| `--no-pager` | | Disable output paging |
| `--quiet` | `-q` | Suppress spinners and non-essential output (env: `BBR_QUIET`) |
| `--color <WHEN>` | | Color output: `auto` (default), `always`, or `never` |
| `--no-color` | | Disable color (same as `--color never`; wins over `--color`) |
| `--no-unicode` | | Use ASCII instead of Unicode |
| `--timeout <SECS>` | | HTTP request timeout in seconds (env: `BBR_TIMEOUT`, default: 30) |

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

Exit codes are a stable public contract — scripts can branch on `$?`.

---

## JSON Output

Every data command accepts `--json`. Output is stable across v0.1.x and suitable for scripting.

```bash
bbr status --json         # { branch, commit, repo, pr?, open_prs, pipeline?, commit_statuses }
bbr auth status --json    # { authenticated, username, source, rate_limit_remaining? }
bbr pr list --json        # { workspace, slug, state, pull_requests: [...] }
bbr ci status --json      # { branch, pipeline: { uuid, state, steps, ... } }
bbr ci list --json        # { branch, pipelines: [...] }
bbr pr dashboard --json   # { workspace, user, needs_review, my_prs, recent_activity, repo_count }
bbr batch merge-approved --dry-run --json  # { dry_run, action_count, actions: [...] }
bbr ci compare 42 57 --json  # { a, b, step_deltas, test_deltas }
bbr repo audit --json     # { workspace, total_repos, repos: [...], summary }
bbr pr stack list --json  # { name, base_branch, stacks, prs: [...] }
bbr search "TODO" --json  # { query, total, results: [{ file, content_matches }] }
bbr update --json         # { current_version, latest_version, up_to_date, downloaded? }
```

Field names are `snake_case`. Full schema documentation: [docs/output-schema.md](docs/output-schema.md).

---

## Scripting Patterns

```bash
# Check PR state
bbr status --json | jq -r '.pr.state'

# Create PR and get URL
bbr pr create --title "Fix" --body-file body.md --json | jq -r '.url'

# Wait for CI, fail the script if pipeline fails
bbr ci watch --branch "$BRANCH" --interval-secs 10

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

## Output & Theme

bbr renders human output with a theme system that respects your terminal:

- **Colors**: Precedence is `--no-color` > `--color` > `CLICOLOR_FORCE` > `NO_COLOR` > `CLICOLOR=0` > TTY detection. Force with `--color always`.
- **Unicode**: Semantic glyphs (`[ok]`/`[X]`/`[!]`/`[~]`/`[.]`/`[?]`). Use `--no-unicode` for ASCII-only terminals.
- **Pager**: Long output paged through `less -F -R -X`. Disable with `--no-pager`.
- **Spinners**: `indicatif` spinners during network operations. Hidden in `--json` mode and `--quiet` mode.
- **Tables**: `comfy-table` with UTF-8 preset, right-aligned IDs, 60-char text column cap (CJK-safe via `unicode-width`).

---

## Architecture

```
src/
  main.rs           entry point -> cli::run()
  cli.rs            clap Cmd definition + all enums (28+ commands)
  dispatch.rs       command routing to handlers
  error.rs          BitbucketError, exit codes, ExitCode mapping
  auth.rs           credential resolution (env -> config file); ApiToken + Basic auth only
  config.rs         XDG config dir, credentials.toml parsing
  git.rs            detect workspace/slug + current branch via `git` shell-out
  api/              BitbucketClient + endpoint modules (pr, pipeline, repo, status, deploy, webhook, source, issue)
  commands/         one file per subcommand group (28 modules)
  output/           Formatter trait; table + json + theme
  diff/             structured diff parser, renderer with box-drawing/line-numbers/word-diff, word_diff module
```

---

## Develop

```bash
cargo build --release --locked
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

MSRV **1.88**. No OpenSSL (`rustls`). Tests use `wiremock` (no network). 262 tests across unit, integration, and smoke suites.

### Conventions

- Never add comments unless asked.
- Follow existing style; run `cargo fmt` before committing.
- All API types derive `serde::Serialize` + `Deserialize`; `--json` output uses `serde_json::to_string_pretty`.
- Human output respects `NO_COLOR` and TTY detection via the `output::theme` module.
- Exit codes are centralized in `error.rs` (`ExitCode` enum).
- Tests mock the API with `wiremock`; do not hit real Bitbucket in CI.

### Release

Bump `Cargo.toml`, update `CHANGELOG.md`, tag `vX.Y.Z` → GitHub Actions cross-compiles and publishes to [Releases](https://github.com/themankindproject/bbr/releases).

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
| `BBR_TIMEOUT` | HTTP request timeout in seconds | 30 |
| `NO_COLOR` | Disable color output | — |
| `XDG_CONFIG_HOME` | Config directory (Linux) | `~/.config` |
| `RUST_LOG` | Tracing log filter (overrides `--verbose`) | — |

---

## License

MIT — see [LICENSE](LICENSE).
