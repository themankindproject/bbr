# bbr — BitBucket Remote CLI

[![CI](https://img.shields.io/github/actions/workflow/status/themankindproject/bbr/ci.yml?branch=main&label=CI)](https://github.com/themankindproject/bbr/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/release/themankindproject/bbr)](https://github.com/themankindproject/bbr/releases/latest)
![Rust Version](https://img.shields.io/badge/rust-1.88%2B-blue)
[![License](https://img.shields.io/crates/l/bbr)](LICENSE)
![Tests](https://img.shields.io/badge/tests-433%20passing-brightgreen)

A fast, single-binary Bitbucket Cloud CLI. Agent-first (`--json` everywhere, stable schemas and exit codes, env auth) with pretty human output.

PR lifecycle · CI/pipelines · status dashboard · batch ops · stacked PRs · repo admin · code search · deployments · raw API passthrough · self-update.

Full command reference: **[USAGE.md](USAGE.md)** · JSON schemas: **[docs/output-schema.md](docs/output-schema.md)** · Changelog: **[CHANGELOG.md](CHANGELOG.md)**

## Install

```bash
# One-liner (Linux x86_64/aarch64, macOS Intel/ARM)
curl -fsSL https://github.com/themankindproject/bbr/raw/main/install.sh | bash

# Or from source
cargo install --locked --git https://github.com/themankindproject/bbr

bbr completion --install    # shell completions (bash/zsh/fish/powershell)
```

Pre-built archives: [Releases](https://github.com/themankindproject/bbr/releases/latest).

## Auth

HTTP Basic with an [Atlassian API token](https://id.atlassian.com/manage-profile/security/api-tokens):

```bash
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="<api-token>"

# Or interactive file (~/.config/bbr/credentials.toml, mode 0600)
bbr auth setup && bbr auth test
```

Required scopes: `account:read`, `repository:read`, `repository:write`, `pullrequest:read`, `pullrequest:write`, `pipeline:read`, `pipeline:write`. Env vars take precedence over the credentials file.

## Quick Start

```bash
cd my-bitbucket-repo

bbr                           # overview: PRs, approvals, recent CI
bbr status                    # full PR + CI for current branch
bbr pr create --title "Fix" --body "..."
bbr pr diff --file 3 --wrap   # inspect specific files, wrap long lines
bbr ci watch --logs           # live-tail, failing log on failure
bbr batch merge-approved      # merge all fully-approved PRs (plan/apply)
bbr doctor                    # self-check: git, creds, API, quota, version
```

Every data command supports `--json`. See [USAGE.md](USAGE.md) for all flags and scripting patterns.

## Exit Codes

Stable public contract — scripts can branch on `$?`.

| Code | Meaning |
|------|---------|
| 0 | success |
| 1 | generic error |
| 2 | auth failure |
| 3 | not found |
| 4 | rate limited |
| 5 | pipeline failed (`bbr ci watch`) |
| 64 | usage error (invalid flags/arguments) |

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
| `BBR_NO_INTERACTIVE` | Never prompt, even on a TTY | — |
| `NO_COLOR` | Disable color output | — |

## Develop

```bash
cargo build --release --locked
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

MSRV **1.88**. No OpenSSL (`rustls`). Tests use `wiremock` (no network). Release: bump `Cargo.toml`, update `CHANGELOG.md`, tag `vX.Y.Z` — GitHub Actions cross-compiles and publishes.

## License

MIT — see [LICENSE](LICENSE).
