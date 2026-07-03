# bbr — BitBucket Remote CLI

[![CI](https://img.shields.io/github/actions/workflow/status/themankindproject/bbr/ci.yml?branch=main&label=CI)](https://github.com/themankindproject/bbr/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/release/themankindproject/bbr)](https://github.com/themankindproject/bbr/releases/latest)
![Rust Version](https://img.shields.io/badge/rust-1.75%2B-blue)
[![License](https://img.shields.io/crates/l/bbr)](LICENSE)

A fast, single-binary Bitbucket Cloud CLI. **Agent-first** (`--json` everywhere, stable schemas, zero-config env auth) with pretty human output.

---

## Table of Contents

- [Why bbr Exists](#why-bbr-exists)
- [Key Features](#key-features)
- [Tech Stack](#tech-stack)
- [Quick Start](#quick-start)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Authentication](#authentication)
- [Usage](#usage)
- [Command Reference](#command-reference)
- [Exit Codes](#exit-codes)
- [Environment Variables](#environment-variables)
- [JSON Output](#json-output)
- [Architecture](#architecture)
- [Development](#development)
- [Testing](#testing)
- [Deployment & Releases](#deployment--releases)
- [Contributing](#contributing)
- [License](#license)

---

## Why bbr Exists

Bitbucket Cloud had no good CLI. Developers were stuck between:

- **`curl`** — verbose, error-prone, no auth management, no pretty output
- **Web UI** — context-switching away from the terminal, not scriptable
- **GitHub's `gh`** — excellent tool, but doesn't work with Bitbucket

The breaking point: **coding agents** (Claude, Cursor, Copilot) need a reliable, scriptable way to interact with Bitbucket — create PRs, check CI status, merge approved PRs — without human intervention. `curl` in a loop doesn't cut it.

`bbr` solves three problems:

1. **Agent-first** — `--json` on every command, stable schemas, exit codes for CI, zero-config env auth. An agent can run `bbr status --json` and parse the result without guessing.

2. **Developer UX** — `bbr` with no arguments shows PR + CI + commit statuses + suggested next commands. No more `bbr ci status`, `bbr pr list`, `bbr commit status` separately.

3. **Power features** — stacked PRs, pipeline comparison, batch operations, SOC2 audit, cross-repo dashboard. Things that require 20 clicks in the web UI become one command.

---

## Key Features

- **Zero-config auth** — `BITBUCKET_USERNAME` + `BITBUCKET_TOKEN` env vars or a single `credentials.toml` file
- **`--json` on every command** — stable, documented JSON schemas for all outputs
- **Stable exit codes** — `0` success, `1` generic, `2` auth, `3` not found, `4` rate limited, `5` pipeline failed
- **100% Bitbucket Cloud API** — PRs, pipelines, repos, deployments, webhooks, source browsing, code search, issues
- **No OpenSSL dependency** — uses `rustls` for TLS, cross-compiles cleanly
- **Single ~10MB binary** — no runtime dependencies, no system keyring
- **Wiremock-tested** — all integration tests use mocked HTTP; no network needed to run tests
- **Parallel fetch** — concurrent API calls via `tokio::join!` / `futures::join_all` / `buffer_unordered`

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| **Language** | Rust 2021 Edition, MSRV 1.75+ |
| **CLI Framework** | `clap` 4.5 (derive macros, env overrides, `wrap_help`) |
| **Async Runtime** | `tokio` 1 (multi-threaded) + `futures` 0.3 |
| **HTTP Client** | `reqwest` 0.12 with `rustls-tls` |
| **Serialization** | `serde` 1 + `serde_json` 1 + `toml` 0.8 |
| **Table Output** | `comfy-table` 7.1 with custom styling |
| **Spinners** | `indicatif` 0.17 |
| **Colors** | `colored` 2 + inline ANSI (NO_COLOR-aware) |
| **Self-Update** | `flate2` + `tar` (GitHub Releases API) |
| **Config Dirs** | `dirs` 5 + `xdg` 2 |
| **Error Handling** | `thiserror` 1 |
| **Logging** | `tracing` 0.1 + `tracing-subscriber` 0.3 (env-filter) |
| **Testing** | `wiremock` 0.6, `assert_cmd` 2, `predicates` 3, `tempfile` 3 |

---

## Quick Start

```bash
# 1. Set your credentials (or they go to ~/.config/bbr/credentials.toml)
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="<api-token-from-id.atlassian.com>"

# 2. Clone a repo and cd in
cd my-bitbucket-repo

# 3. Run it
bbr                           # PR + CI + statuses for current branch
bbr pr list                   # open PRs for this repo
bbr ci watch --logs           # live-tail pipeline, auto-fetch failing step
bbr pr dashboard              # workspace-wide PR dashboard
bbr batch merge-approved      # merge all fully-approved PRs
```

---

## Prerequisites

- **A Bitbucket Cloud account** (Bitbucket Server/Data Center is not supported)
- **An Atlassian API token** — generate one at `https://id.atlassian.com/manage-profile/security/api-tokens`
- **A git repository** (cloned locally) — `bbr` auto-detects workspace, repo slug, and current branch from git remotes
- **Rust 1.75+** (only if building from source)

---

## Installation

### Option 1: One-Line Installer (Recommended)

```bash
curl -fsSL https://github.com/themankindproject/bbr/raw/main/install.sh | bash
```

Detects your platform and architecture, downloads the latest release from GitHub, and installs to `~/.local/bin/bbr`. Optionally generates shell completions.

### Option 2: Build from Source

```bash
# Install via cargo
cargo install --locked --git https://github.com/themankindproject/bbr

# Or clone and build yourself
git clone https://github.com/themankindproject/bbr.git
cd bbr
cargo build --release --locked
# Binary at ./target/release/bbr
```

The `--release --locked` profile uses LTO, single codegen unit, `panic=abort`, and stripped symbols for a minimal ~10MB binary.

### Option 3: Download a Release

Grab the pre-built archive for your platform from the [Releases page](https://github.com/themankindproject/bbr/releases/latest):

| Platform | Archive |
|----------|---------|
| Linux (x86_64) | `bbr-x86_64-linux.tar.gz` |
| macOS (x86_64) | `bbr-x86_64-macos.tar.gz` |
| macOS (Apple Silicon) | `bbr-aarch64-macos.tar.gz` |
| Windows (x86_64) | `bbr-x86_64-windows.zip` |

```bash
# Example: Linux
curl -LO https://github.com/themankindproject/bbr/releases/latest/download/bbr-x86_64-linux.tar.gz
tar xzf bbr-x86_64-linux.tar.gz
sudo mv bbr /usr/local/bin/
```

### Shell Completions

```bash
bbr completion bash --install     # zsh / fish / powershell also supported
```

Completions are also bundled in release archives under `completions/`.

---

## Authentication

`bbr` uses **HTTP Basic authentication** with Bitbucket usernames and Atlassian API tokens.

### Credential Resolution Order

`bbr` checks these sources in order, using the first one found:

1. **Environment variables** — `BITBUCKET_USERNAME` + `BITBUCKET_TOKEN`
2. **Config file** — `~/.config/bbr/credentials.toml` (Unix) or `%APPDATA%/bbr/credentials.toml` (Windows)

### Using Environment Variables

```bash
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="<api-token-from-id.atlassian.com>"
bbr status
```

This is the recommended approach for CI/CD pipelines and coding agents.

### Using the Config File

Generate your config file:

```bash
bbr auth setup
# You'll be prompted for username and token
# Written to ~/.config/bbr/credentials.toml with 0600 permissions
```

Or write it manually:

```bash
mkdir -p ~/.config/bbr
cat > ~/.config/bbr/credentials.toml << 'EOF'
username = "you@example.com"
token = "<api-token-from-id.atlassian.com>"
EOF
chmod 600 ~/.config/bbr/credentials.toml
```

### Verify Authentication

```bash
bbr auth test
# Tests the configured credentials against the Bitbucket API
```

### Logout

```bash
bbr auth logout
# Removes the credentials file
```

> **Note:** `bbr` uses file permissions (`0600`) rather than the system keyring to keep the binary self-contained (~10MB vs 671MB with something like `libsecret`).

---

## Usage

### The Main Event: `bbr` With No Arguments

Running `bbr` bare (from inside a git repo) shows an **overview dashboard**:

```
bbr — BitBucket Remote

  my-workspace / my-repo  ─  my-branch  (#abc1234)

  PR #42: "Fix login bug"  ─  Needs work  (3 comments, 1 task)
  CI: Build (failed) ── Lint (passed) ── Test (running)

  Suggested:
    bbr pr view 42          view the PR
    bbr ci watch 123        watch the pipeline
    bbr status              full status
```

### Common Workflows

#### Development Loop

```bash
# Make changes, commit, push
git push origin my-branch

# Check everything
bbr status

# Create a PR
bbr pr create --title "Fix login bug" --body "Detailed description" --reviewers @team-lead

# Watch CI
bbr ci watch --logs

# Approve and merge
bbr pr approve 42
bbr pr merge 42
```

#### Pipeline Debugging

```bash
# List recent pipelines
bbr ci list

# Watch a running pipeline (live-tail)
bbr ci watch 123 --logs

# Compare two pipelines
bbr ci compare 123 124

# View test results
bbr ci tests 123

# Rerun a failed pipeline
bbr ci rerun 123
```

#### Batch Operations

```bash
# Preview what would happen
bbr batch merge-approved --plan

# Then execute
bbr batch merge-approved

# Rerun all failed pipelines across repos
bbr batch rerun-failed

# Clean up branches for merged PRs
bbr batch cleanup-merged-branches
```

#### Cross-Repo PR Dashboard

```bash
bbr pr dashboard
# Shows PRs needing review, your PRs, recent activity across the workspace
```

#### SOC2 Compliance Audit

```bash
bbr repo audit
# Checks branch restrictions, required approvals, push protection
# --json for CI integration
```

#### Stacked PRs

```bash
bbr pr stack init         # Initialize stack configuration
bbr pr stack add          # Add current branch to stack
bbr pr stack list         # List stacked branches
bbr pr stack rebase       # Rebase the stack
bbr pr stack land         # Land the bottom-most PR
bbr pr stack abort        # Abort stack operation
```

#### Self-Update

```bash
bbr update                # Check and apply updates
bbr update --check        # Check without updating
```

---

## Command Reference

### Top-Level Subcommands

| Command | Description | JSON |
|---------|-------------|------|
| `bbr` | Workspace overview (PR + CI + statuses + suggestions) | ✓ |
| `bbr status` | PR + CI + commit statuses for current branch | ✓ |
| `bbr pr` | Pull request operations | ✓ |
| `bbr ci` | Pipeline (CI/CD) operations | ✓ |
| `bbr batch` | Bulk operations (merge-approved, rerun-failed, cleanup) | ✓ |
| `bbr repo` | Repository management | ✓ |
| `bbr commit` | Commit build statuses | ✓ |
| `bbr auth` | Credential management | ✓ |
| `bbr config` | Configuration management | ✗ |
| `bbr api` | Raw Bitbucket API passthrough | ✓ |
| `bbr search` | Code search across workspace | ✓ |
| `bbr open` | Open Bitbucket in browser | ✗ |
| `bbr completion` | Generate shell completions | ✗ |
| `bbr update` | Self-update | ✓ |
| `bbr schema` | Print JSON schema for command output | ✓ |
| `bbr workspace` | List workspaces | ✓ |
| `bbr deploy` | Deployment and environment management | ✓ |
| `bbr webhook` | Webhook CRUD | ✓ |
| `bbr issue` | Issue tracker (deprecated — Bitbucket removed it) | ✓ |
| `bbr src` | Remote source browsing | ✓ |

### PR Subcommands (`bbr pr`)

| Subcommand | Description |
|------------|-------------|
| `bbr pr list` | List open PRs |
| `bbr pr view <id>` | View PR details |
| `bbr pr create` | Create a PR |
| `bbr pr update <id>` | Update PR title/description |
| `bbr pr comment <id>` | Comment on a PR |
| `bbr pr approve <id>` | Approve a PR |
| `bbr pr merge <id>` | Merge a PR |
| `bbr pr decline <id>` | Decline a PR |
| `bbr pr checkout <id>` | Checkout PR branch locally |
| `bbr pr diff <id>` | View PR diff (pretty or raw) |
| `bbr pr patch <id>` | Download PR as patch file |
| `bbr pr diffstat <id>` | PR file change summary |
| `bbr pr dashboard` | Cross-repo PR dashboard |
| `bbr pr stack init|add|list|rebase|land|abort` | Stacked PR management |

### CI Subcommands (`bbr ci`)

| Subcommand | Description |
|------------|-------------|
| `bbr ci list` | List pipelines |
| `bbr ci status <id>` | Pipeline status summary |
| `bbr ci watch <id>` | Live-tail pipeline (auto-fetch failing step logs) |
| `bbr ci logs <id>` | Fetch pipeline logs |
| `bbr ci tests <id>` | Test results |
| `bbr ci steps <id>` | Individual step statuses |
| `bbr ci rerun <id>` | Rerun a pipeline |
| `bbr ci stop <id>` | Stop a running pipeline |
| `bbr ci trigger` | Trigger a new pipeline |
| `bbr ci compare <a> <b>` | Compare two pipelines (step durations, test diffs) |
| `bbr ci vars` | List pipeline variables |

### Global Flags

Available on every command:

| Flag | Description |
|------|-------------|
| `--json` | Output as JSON (stable schema, documented) |
| `--verbose`, `-v` | Enable debug logging (repeat for trace) |
| `--workspace <name>` | Override auto-detected workspace |
| `--slug <name>` | Override auto-detected repo slug |
| `--no-pager` | Disable pager for long output |
| `--quiet`, `-q` | Suppress non-essential output |
| `--color <when>` | When to use color: `auto`, `always`, `never` |
| `--no-color` | Disable colored output (also respects `NO_COLOR`) |
| `--no-unicode` | Use ASCII-only glyphs (no emoji/box-drawing) |
| `--timeout <seconds>` | HTTP request timeout (default: 30) |
| `--api-base <url>` | API base URL override |

---

## Exit Codes

| Code | Meaning | When |
|------|---------|------|
| `0` | Success | Command completed successfully |
| `1` | Generic error | API error, network failure, bad input, etc. |
| `2` | Auth failure | Invalid or missing credentials |
| `3` | Not found | Resource (PR, pipeline, repo) not found |
| `4` | Rate limited | Bitbucket API rate limit hit |
| `5` | Pipeline failed | CI pipeline completed with failure status |

All exit codes are stable. Use them in CI/CD scripts and agent workflows.

---

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `BITBUCKET_USERNAME` | — | Bitbucket account email or username (required) |
| `BITBUCKET_TOKEN` | — | Atlassian API token (required) |
| `NO_COLOR` | — | Disable all ANSI color output (respected) |
| `CLICOLOR` / `CLICOLOR_FORCE` | — | Color overrides (`0` = disable, `1` = enable) |
| `PAGER` | `less` | Pager for long output |
| `BAT_THEME` | `Monokai Extended` | Theme for syntax-highlighted output (via `bat`) |
| `XDG_CONFIG_HOME` | `~/.config` | Config directory for `credentials.toml` |
| `RUST_LOG` | `info` | Log level override (debug, trace) |

---

## JSON Output

Every command that produces output supports `--json` with a **stable, documented schema**.

```bash
bbr status --json
```

```json
{
  "workspace": "my-workspace",
  "repo": "my-repo",
  "branch": "my-branch",
  "commit": "abc1234",
  "pr": { "id": 42, "title": "Fix login bug", "state": "OPEN" },
  "ci": { "state": "IN_PROGRESS", "build_number": 123 }
}
```

Full JSON schemas are documented in [`docs/output-schema.md`](docs/output-schema.md), covering:

- `bbr` (root) — workspace overview
- `bbr status` — full status
- `bbr pr list` / `bbr pr view` — PR details
- `bbr pr create` / `bbr pr comment` / `bbr pr approve` — PR actions
- `bbr ci status` / `bbr ci list` — pipeline details
- And more

To print the schema for any command:

```bash
bbr schema status              # JSON schema for "bbr status --json"
bbr schema pr view             # JSON schema for "bbr pr view --json"
```

---

## Architecture

### Layered Design

```
main.rs → cli::run() → dispatch() → commands::*
                              ↓
                         api::BitbucketClient
                              ↓
                         reqwest (REST API)
                         https://api.bitbucket.org/2.0/
```

**Layer 1 — Entry (`src/main.rs` + `src/lib.rs`)**

`main.rs` is a 9-line `#[tokio::main]` async entry point that calls `bbr::cli::run().await`. `lib.rs` re-exports all modules so integration tests can import them directly.

**Layer 2 — CLI Definition (`src/cli.rs` — 1760 lines)**

Uses `clap` derive macros to define all 17 top-level subcommands, nested actions, and 13 global flags. The `dispatch()` function is a routing table mapping each command to its handler.

**Layer 3 — Commands (`src/commands/` — 26 files)**

Each subcommand group lives in its own file. They use shared helpers from `commands/mod.rs`:

- `client()` — builds the authenticated `BitbucketClient`
- `resolve_repo()` — parses git remotes for workspace + slug
- `make_formatter()` — creates human or JSON formatter
- `make_spinner()` — creates an `indicatif` spinner
- `human_duration()` — formats durations readably
- `confirm()` — interactive confirmation prompt

**Layer 4 — API Client (`src/api/` — 9 files)**

`BitbucketClient` provides:

- `send()` — GET with JSON deserialization + rate-limit retry
- `send_raw()` — GET returning raw text
- `post()` — POST/PUT/DELETE with JSON body
- `fetch_all_pages()` — auto-paginate through `next` URLs
- `map_error()` — parse Bitbucket API error envelopes

Endpoint files (`pr.rs`, `pipeline.rs`, `repo.rs`, `status.rs`, `deploy.rs`, `issue.rs`, `source.rs`, `webhook.rs`) define typed request/response structs with `serde`.

**Layer 5 — Output (`src/output/` — 4 files)**

- `Formatter` enum (`Human`/`Json`) — `print()`, `print_paginated()`, `print_diff()` (bat + less)
- `json.rs` — `serde_json::to_writer_pretty`
- `table.rs` — `comfy-table` wrapper (ID right-aligned, State centered)
- `theme.rs` — Singleton `Theme` with `NO_COLOR` + TTY detection, providing `success()`, `warn()`, `error()`, `dim()`, `bold()`, `status_glyph()`, `checkmark()`, etc.

### Cross-Cutting Modules

- **`error.rs`** — `BitbucketError` enum (9 variants) with stable exit code mapping and user-friendly hints
- **`auth.rs`** — Credential resolution: env vars → config file
- **`config.rs`** — XDG platform paths, `credentials.toml` parsing, `0600` permissions
- **`git.rs`** — Shell-out git integration: repo detection (parses HTTPS/SSH remotes), branch/commit lookup, branch fetch/checkout/rebase/push with 30s/120s timeouts
- **`stack.rs`** — `.bbr/stack.toml` config model for stacked PR chains

### Directory Structure

```
bbr/
├── src/
│   ├── main.rs                  # Entry: #[tokio::main] -> cli::run()
│   ├── lib.rs                   # Library root (re-exports all modules)
│   ├── cli.rs                   # Clap CLI definition + dispatch()
│   ├── error.rs                 # BitbucketError + exit code mapping
│   ├── auth.rs                  # Credential resolution
│   ├── config.rs                # XDG config paths + TOML parsing
│   ├── git.rs                   # Shell-out git (repo detection, branch ops)
│   ├── stack.rs                 # Stacked PRs config model
│   ├── api/                     # Bitbucket REST API client
│   │   ├── mod.rs              # BitbucketClient (send, pagination, retry)
│   │   ├── pr.rs               # PR endpoints
│   │   ├── pipeline.rs         # Pipeline endpoints
│   │   ├── repo.rs             # Repository endpoints
│   │   ├── status.rs           # Commit status endpoints
│   │   ├── deploy.rs           # Deployment endpoints
│   │   ├── issue.rs            # Issue tracker (deprecated)
│   │   ├── source.rs           # Source browsing endpoints
│   │   └── webhook.rs          # Webhook endpoints
│   ├── commands/                # Command implementations (26 files)
│   │   ├── mod.rs              # Shared helpers (client, resolve_repo, spinner...)
│   │   ├── status.rs           # bbr status
│   │   ├── pr.rs               # bbr pr
│   │   ├── ci.rs               # bbr ci
│   │   ├── ci_compare.rs       # bbr ci compare
│   │   ├── ci_vars.rs          # bbr ci vars
│   │   ├── batch.rs            # bbr batch
│   │   ├── repo.rs             # bbr repo
│   │   ├── audit.rs            # bbr repo audit
│   │   ├── auth.rs             # bbr auth
│   │   ├── update.rs           # bbr update
│   │   ├── open.rs             # bbr open
│   │   ├── config.rs           # bbr config
│   │   ├── api.rs              # bbr api (raw passthrough)
│   │   ├── commit.rs           # bbr commit
│   │   ├── completion.rs       # bbr completion
│   │   ├── dashboard.rs        # bbr pr dashboard
│   │   ├── deploy.rs           # bbr deploy
│   │   ├── export.rs           # Slack/markdown export formatters
│   │   ├── issue.rs            # bbr issue (deprecated)
│   │   ├── schema.rs           # bbr schema
│   │   ├── search.rs           # bbr search
│   │   ├── src_cmd.rs          # bbr src
│   │   ├── stack.rs            # bbr pr stack
│   │   ├── webhook.rs          # bbr webhook
│   │   └── workspace.rs        # bbr workspace
│   └── output/                  # Output formatting
│       ├── mod.rs              # Formatter enum (Human/Json) + paging
│       ├── json.rs             # JSON pretty-printer
│       ├── table.rs            # comfy-table wrapper
│       └── theme.rs            # Colors, glyphs, NO_COLOR support
├── tests/                       # Integration tests (wiremock-mocked)
│   ├── api_pr.rs
│   ├── api_pipeline.rs
│   ├── api_repo.rs
│   ├── api_status.rs
│   ├── api_retry.rs
│   ├── api_new_features.rs
│   └── cli_smoke.rs
├── docs/
│   └── output-schema.md        # Stable JSON schemas
├── .github/workflows/
│   ├── ci.yml                  # CI pipeline (fmt, clippy, test, msrv, nextest, audit)
│   └── release.yml             # Release pipeline (cross-compile 4 targets)
├── Cargo.toml                  # Package metadata + dependencies
├── Cargo.lock
├── README.md
├── USAGE.md                    # 1061-line complete command reference
├── CHANGELOG.md                # Detailed changelog
├── CLAUDE.md                   # AI agent guide
├── LICENSE                     # MIT
├── install.sh                  # One-line curl-pipe-bash installer
├── clippy.toml                 # too-many-arguments-threshold = 8
└── rustfmt.toml                # edition = 2021, max_width = 100
```

---

## Development

### Prerequisites

- Rust 1.75+ (`rustup` recommended)
- No system dependencies — `rustls` means no OpenSSL needed
- No database, no Docker, no services

### Build

```bash
cargo build                          # Debug build
cargo build --release --locked        # Release build (LTO, stripped, ~10MB)
```

### Run

```bash
cargo run -- status                   # Run bbr status
cargo run -- pr list --json           # Run bbr pr list with JSON output
cargo run -- --help                   # Full help
```

### Lint

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

### Watch (Development Loop)

```bash
cargo watch -x 'run -- status'        # Rebuild + run on changes
cargo watch -x 'clippy'               # Re-lint on changes
cargo watch -x 'test'                 # Re-test on changes
```

### Code Conventions

- **MSRV 1.75** — avoid features requiring newer Rust
- **Max width 100** — enforced by `rustfmt.toml`
- **`too-many-arguments` threshold 8** — enforced by `clippy.toml`
- **`-D warnings`** in CI — clippy warnings are errors
- **`--json` everywhere** — every output command must support JSON
- **No OpenSSL** — use `rustls` for cross-compilation simplicity
- **No system keyring** — file permissions (`0600`) for credentials

---

## Testing

### Run All Tests

```bash
cargo test --all-features             # ~200 tests, no network needed
```

All HTTP interactions are mocked with `wiremock`. No Bitbucket credentials or network access required.

### Test Structure

```
tests/
├── api_pr.rs                  # PR endpoint tests (mocked)
├── api_pipeline.rs            # Pipeline endpoint tests (mocked)
├── api_repo.rs                # Repository endpoint tests (mocked)
├── api_status.rs              # Commit status tests (mocked)
├── api_retry.rs               # Rate-limit retry, pagination, send_raw
├── api_new_features.rs        # New feature integration tests
└── cli_smoke.rs               # Binary smoke tests (assert_cmd)
```

**Unit tests** are embedded inline in each source file via `#[cfg(test)] mod tests`.

### Run Specific Tests

```bash
cargo test --test api_pr                 # Only PR integration tests
cargo test --test cli_smoke              # Only CLI smoke tests
cargo test -- status_tests               # Inline unit test by name
```

### Faster Test Runner

```bash
cargo install cargo-nextest
cargo nextest run --all-features         # Parallel, faster, better output
# (used in CI)
```

### Security Audit

```bash
cargo install cargo-audit
cargo audit                              # Check dependencies for CVEs
# (run in CI)
```

---

## Deployment & Releases

### CI/CD Pipeline

The project uses GitHub Actions with two workflows:

**CI (`ci.yml`)** — runs on push/PR to `main`:

| Job | What it does |
|-----|-------------|
| `fmt` | `cargo fmt --check` |
| `clippy` | `cargo clippy --all-targets --all-features -- -D warnings` |
| `test` | `cargo test --all-features` on Linux, macOS, Windows |
| `msrv` | Verifies minimum supported Rust version (1.75) |
| `nextest` | `cargo nextest run --all-features` |
| `audit` | `cargo audit` for security vulnerabilities |

**Release (`release.yml`)** — triggered by `v*.*.*` tags:

1. Cross-compiles binaries for 4 targets: x86_64-linux, x86_64-macos, aarch64-macos, x86_64-windows
2. Packages each as `.tar.gz` (`.zip` for Windows) with README + LICENSE
3. Generates shell completions (bash, zsh, fish, powershell)
4. Creates a GitHub Release with all archives

### How to Release

```bash
# 1. Update version in Cargo.toml
# 2. Commit and push
git add Cargo.toml
git commit -m "release: v0.2.0"
git push

# 3. Tag and push
git tag v0.2.0
git push origin v0.2.0   # triggers release.yml
```

### Self-Update

`bbr update` checks GitHub Releases for the latest version, downloads the matching platform archive, and replaces the current binary. It caches version info and runs a background check on invocation.

```bash
bbr update                     # Update to latest
bbr update --check             # Check for update without applying
```

### Distribution

- **Pre-built binaries** — GitHub Releases (4 platforms)
- **`cargo install`** — from git
- **`install.sh`** — one-line curl-pipe-bash (auto-detects platform)
- **No Docker, no npm, no runtime** — single ~10MB ELF/Mach-O/PE binary

---

## Troubleshooting

### "No repository found"

`bbr` needs to be run from inside a git clone of a Bitbucket repository.

```bash
# Check your git remote
git remote -v
# Should show something like:
# origin  git@bitbucket.org:workspace/repo.git (fetch)
# origin  git@bitbucket.org:workspace/repo.git (push)
```

Supported remote formats:
- `https://username@bitbucket.org/workspace/slug.git`
- `https://username:token@bitbucket.org/workspace/slug.git`
- `git@bitbucket.org:workspace/slug.git`
- SSH aliases: `origin	git@github.com-org:workspace/slug.git`

### Authentication Errors "2"

```bash
# Verify your credentials
bbr auth test

# Check env vars
echo $BITBUCKET_USERNAME
echo $BITBUCKET_TOKEN        # Should be non-empty

# Check config file
bbr config path
cat "$(bbr config path)"
```

API tokens can be generated at `https://id.atlassian.com/manage-profile/security/api-tokens`.

### "Not found" (exit code 3)

The resource doesn't exist or you don't have access:

```bash
# Verify the resource exists
bbr pr view 42               # Replace 42 with the actual PR number
bbr ci status 123            # Replace 123 with the actual pipeline number

# Check you're looking at the right repo
bbr                            # Shows workspace/repo/branch
```

### Rate Limited (exit code 4)

Bitbucket API rate limits apply. `bbr` automatically retries on `429 Too Many Requests` with exponential backoff.

### Colors Not Working

`bbr` respects:

- `--no-color` flag
- `NO_COLOR` environment variable (https://no-color.org/)
- `CLICOLOR=0` / `CLICOLOR_FORCE=0`
- Non-TTY output (piped commands auto-disable color)

```bash
# Force color even when piping
bbr status --color always

# Disable color
bbr status --no-color
NO_COLOR=1 bbr status
```

### "Command not found" After Install

If you used the one-line installer:

```bash
# Ensure ~/.local/bin is in your PATH
export PATH="$HOME/.local/bin:$PATH"
```

Add that line to your shell config (`~/.bashrc`, `~/.zshrc`, etc.).

### Slow Performance

`bbr` fetches data concurrently where possible, but some commands make multiple API calls:

```bash
# Use --json to avoid formatting overhead
bbr status --json
```

---

## Comparison With Alternatives

| Feature | bbr | curl + jq | gh (GitHub) | Bitbucket UI |
|---------|-----|-----------|-------------|--------------|
| Zero-config auth | ✓ | ✗ | ✓ | N/A |
| `--json` everywhere | ✓ | N/A | Partial | N/A |
| PR lifecycle | Full | Manual | Full | Click-heavy |
| CI watch (live-tail) | ✓ | Manual loop | ✓ | Polling |
| Pipeline comparison | ✓ | Manual | ✗ | ✗ |
| Batch operations | ✓ | Script | ✓ | ✗ |
| Stacked PRs | ✓ | Script | ✓ | ✗ |
| SOC2 audit | ✓ | Script | ✗ | Manual |
| Cross-repo dashboard | ✓ | Script | ✓ | ✗ |
| Single binary, no deps | ✓ | Requires curl+jq | ✓ | N/A |
| Exit codes for CI | ✓ | Manual | ✓ | N/A |
| Self-update | ✓ | N/A | ✓ | N/A |
| Shell completions | ✓ | N/A | ✓ | N/A |
| Slack/markdown export | ✓ | Script | ✗ | ✗ |
| Works on Windows | ✓ | Partial | ✓ | N/A |

---

## Contributing

### Reporting Issues

Open a [GitHub Issue](https://github.com/themankindproject/bbr/issues/new/choose) using the bug report or feature request template.

### Submitting Changes

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/my-feature`
3. Make your changes
4. Run the test suite: `cargo test --all-features`
5. Run lints: `cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check`
6. Commit with descriptive messages
7. Push and open a PR

### Design Principles

- **Agent-first** — every command must support `--json` with documented schemas
- **Stable exit codes** — scripts and agents depend on them
- **No new dependencies** unless absolutely necessary (binary size matters)
- **`rustls` only** — no OpenSSL dependency
- **Test with mocks** — `wiremock` for all HTTP tests, no network required

---

## License

MIT — see [LICENSE](LICENSE).

---

*Built for coding agents, shipped for developers.*
