# bbr — BitBucket Remote

[![CI](https://img.shields.io/github/actions/workflow/status/themankindproject/bbr/ci.yml?branch=main&label=CI)](https://github.com/themankindproject/bbr/actions/workflows/ci.yml)
![Rust Version](https://img.shields.io/badge/rust-1.74%2B-blue)
[![License](https://img.shields.io/crates/l/bbr)](LICENSE)

A fast, single-binary Bitbucket Cloud CLI written in Rust. Designed for
**coding agents first** (machine-readable `--json`, zero-config env auth) and
**humans second** (pretty tables, color, progress bars). The `gh`-equivalent
Bitbucket never had.

```text
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

## Overview

| Feature | Status |
|---------|--------|
| **PR lifecycle** | `list`, `view`, `create`, `comment`, review data |
| **CI / pipelines** | `status`, `watch --logs`, `logs --failed` |
| **Commit statuses** | create/update build statuses for CI integrations |
| **Auth** | PAT + legacy app password, env / config-file |
| **Output** | `--json` stable schema for agents, pretty tables for humans |
| **Browser shortcuts** | `bb open`, `bb open pr`, `bb open ci` |
| **Shell** | bash / zsh / fish completions via `bb completion` |
| **Binary** | single static binary, < 5 MB stripped, zero runtime deps |

Perfect for:
- Coding agents that need to check PR status, create PRs, or watch CI from the terminal
- Human devs who want a fast CLI alternative to the browser
- CI scripts that pipe `--json` output to other tools

## Installation

```bash
# from source
cargo install --locked --git https://github.com/themankindproject/bbr

# pre-built binary (releases page)
curl -sSf https://github.com/themankindproject/bbr/releases/latest/download/bbr-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv bb /usr/local/bin/bb
```

The binary is installed as `bb`.

## Quick Start

```bash
# 1. Get a Personal Access Token (PAT):
#    https://id.atlassian.com/manage-profile/security/api-tokens
#    Required scopes: account:read, repository:read, repository:write,
#                     pullrequest:read, pullrequest:write, pipeline:read

# 2a. Env vars (CI / scripts):
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="..."

# 2b. Or interactive setup (local dev):
bb auth setup

# 3. Use it:
bb status            # PR + CI for current branch
bb pr list           # open PRs in this repo
bb pr create --title "Fix X" --body-file pr.md
bb pr comments       # comments for current branch's PR
bb pr tasks          # tasks for current branch's PR
bb ci status         # last pipeline for current branch
bb ci logs --failed  # failed step log from latest pipeline
bb commit status set --key lint --state successful --url "$CI_JOB_URL"
bb ci watch --logs   # live-tail and print failing logs on failure
bb open pr           # open current branch's PR
```

## Features

- **Zero-config auth** — env vars work out of the box (`BITBUCKET_USERNAME` + `BITBUCKET_TOKEN`)
- **Agent-first output** — `bb <cmd> --json` emits stable, predictable JSON
- **Human-first output** — pretty tables, color, progress bars (respects `NO_COLOR`)
- **Fast** — cold start < 50 ms, commands complete in < 1 s for typical operations
- **Cross-platform** — Linux x86_64, macOS x86_64 + aarch64, Windows x86_64
- **Single binary** — no runtime deps, statically linked with `rustls`

## Commands

### PR operations

```bash
bb pr list [--state open|merged|declined|all]
bb pr view [<id>]                    # defaults to current branch's PR
bb pr create --title T --body B [--src S --dst D]
bb pr comment <id> --body B
bb pr comments [<id>]
bb pr tasks [<id>]
bb pr commits [<id>]
bb pr statuses [<id>]
bb pr conflicts [<id>]
bb pr request-changes <id>
bb pr unrequest-changes <id>
```

### CI / pipeline operations

```bash
bb ci status [--branch B]
bb ci watch  [--branch B] [--logs]   # exits non-zero on failure
bb ci logs   [pipeline-uuid]         # defaults to latest pipeline/current branch
bb ci logs   --failed                # fetch failed step log automatically
bb ci logs   --step "Run Tests"      # step UUID or step name
```

### Repository and auth

```bash
bb repo info                         # show workspace/slug for current dir
bb repo tags                         # list remote tags
bb commit status set --key K --state successful [commit]
bb open [repo|pr|ci]                 # open Bitbucket in your browser
bb auth setup                        # interactive credential setup
bb auth status                       # verify stored credentials work
bb auth logout                       # remove stored credentials
```

### The killer feature

```bash
bb status                            # PR + CI for current branch (merged view)
```

### Shell completions

```bash
bb completion bash > /etc/bash_completion.d/bb
bb completion zsh > "${fpath[1]}/_bb"
bb completion fish > ~/.config/fish/completions/bb.fish
```

## Authentication

`bbr` tries three credential sources, in order:

1. **Environment variables** (preferred for CI/scripts):
   ```bash
   export BITBUCKET_USERNAME="you@example.com"
   export BITBUCKET_TOKEN="..."              # PAT (preferred)
   # or legacy app password:
   export BITBUCKET_APP_PASSWORD="..."
   ```

2. **Config file** (created by `bb auth setup`, mode 0600):
   ```toml
   # ~/.config/bb/credentials.toml
   [default]
   username = "you@example.com"
   token = "..."
   ```
   On macOS: `~/Library/Application Support/bb/credentials.toml`.
   On Windows: `%APPDATA%\bb\credentials.toml`.

3. **System keyring** (planned for v0.3).

> **Note:** Bitbucket Cloud is deprecating **app passwords** in favor of
> **Personal Access Tokens (PATs)**. `bbr` supports both today; PATs are
> recommended for new setups.

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

## Roadmap

`bbr` intentionally prioritizes workflows that help agents and developers make
decisions quickly from a terminal. The current surface covers daily PR, CI, and
repo inspection. The next useful Bitbucket API areas are:

| Priority | Area | Why it matters |
|----------|------|----------------|
| 1 | Pipeline test reports | Show failed test summaries and cases without downloading full logs |
| 2 | Branch restrictions | Audit and apply repository protection rules from scripts |
| 3 | Reports and annotations | Publish lint/test findings back to Bitbucket from CI or agents |
| 4 | Source browsing | `cat`, `ls`, and history for files at a commit without cloning |
| 5 | Webhooks | Manage repository integrations reproducibly |
| 6 | Downloads | Upload and manage release artifacts |

Near-term command ideas:

```bash
bb ci tests [pipeline-uuid] --step "Run Tests"
bb branch restrictions
bb branch protect main --require-approvals 2 --require-builds 1
bb report publish --file report.json
bb source cat path/to/file.rs --rev main
bb webhooks list
bb downloads upload ./artifact.tar.gz
```

## Output Format

- **Humans:** pretty tables, color, emoji. Respects `NO_COLOR` and auto-disables
  decoration when stdout is not a TTY.
- **Agents:** `bb <cmd> --json` emits stable JSON. Schema documented in
  [`docs/output-schema.md`](docs/output-schema.md).

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | success |
| 1 | generic error |
| 2 | auth error (no creds / bad creds) |
| 3 | not found (no PR / no pipeline) |
| 4 | API rate limit |
| 5 | pipeline failed (for `bb ci watch`) |

## Architecture

```text
src/
├── main.rs           # thin binary entry point → bbr::cli::run()
├── lib.rs            # library root (enables integration tests)
├── cli.rs            # clap derive + async dispatch
├── error.rs          # BitbucketError + stable ExitCode enum
├── auth.rs           # credential resolution (env → config file)
├── config.rs         # XDG config dir, credentials.toml (mode 0600)
├── git.rs            # detect repo/branch via git shell-out
├── api/
│   ├── mod.rs        # BitbucketClient (reqwest + rustls)
│   ├── pr.rs         # pull request endpoints + serde types
│   ├── pipeline.rs   # pipeline + step endpoints
│   ├── repo.rs       # repository metadata
│   └── status.rs     # commit build-status endpoints
├── commands/
│   ├── mod.rs        # shared helpers (client(), current_repo(), resolve_body())
│   ├── status.rs     # `bb status` — merged PR + CI view
│   ├── pr.rs         # `bb pr` subcommands
│   ├── ci.rs         # `bb ci` subcommands
│   ├── open.rs       # `bb open` browser shortcuts
│   ├── auth.rs       # `bb auth` subcommands
│   └── repo.rs       # `bb repo info`
└── output/
    ├── mod.rs        # Formatter trait (Human | Json)
    ├── table.rs      # comfy-table pretty output
    ├── json.rs       # serde_json stable output
    └── theme.rs      # color/style, NO_COLOR detection
```

### Dependency choices

| Concern | Crate | Why |
|---------|-------|-----|
| CLI parsing | `clap` v4 (derive) | Industry standard, auto-generates completions |
| HTTP | `reqwest` + `rustls` | No system OpenSSL dep, statically linked |
| Async | `tokio` | Makes `bb ci watch` clean |
| JSON | `serde` + `serde_json` | De facto standard |
| Errors | `thiserror` | Ergonomic, zero-cost |
| Tables | `comfy-table` | Pretty output with auto-sizing |
| Progress | `indicatif` | Spinner for `bb ci watch` |
| Color | `colored` | Respects `NO_COLOR` |

## Comparison with Alternatives

| Feature | bbr | `gh` | `glab` | `bbb` |
|---------|-----|------|--------|-------|
| **Bitbucket Cloud** | ✅ | ❌ | ❌ | ❌ |
| **GitHub** | ❌ | ✅ | ❌ | ❌ |
| **GitLab** | ❌ | ❌ | ✅ | ❌ |
| **Machine-readable** | `--json` | `--json` | `--json` | ❌ |
| **Pretty output** | ✅ | ✅ | ✅ | ❌ |
| **Live CI watch** | ✅ | ✅ | ✅ | ❌ |
| **Single binary** | ✅ | ✅ | ✅ | ❌ |
| **Agent-first design** | ✅ | ❌ | ❌ | ❌ |

## Development

```bash
cargo build
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Tests use [`wiremock`](https://crates.io/crates/wiremock) to mock the Bitbucket
API; no network access is required.

## Examples

The `tests/` directory contains integration tests against a mock server:

- `tests/api_pr.rs` — mock Bitbucket PR endpoints
- `tests/api_pipeline.rs` — mock pipeline endpoints
- `tests/cli_smoke.rs` — CLI binary smoke tests (`--help`, `--version`, exit codes)

The doctests across the public API and [USAGE.md](USAGE.md) cover the full
surface for users wiring `bbr` into their own scripts.

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/your-feature`)
3. Run tests: `cargo test --all-features`
4. Run clippy: `cargo clippy --all-targets --all-features -- -D warnings`
5. Run formatter: `cargo fmt --all -- --check`
6. Commit your changes
7. Push the branch and open a Pull Request

### Development Setup

```bash
git clone https://github.com/themankindproject/bbr
cd bbr
cargo test --all-features
```

CI (`.github/workflows/ci.yml`) runs `fmt`, `clippy`, `test`, and `msrv` jobs
in parallel on every push and PR.

## License

MIT — see [LICENSE](LICENSE).
