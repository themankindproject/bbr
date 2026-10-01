# bbr — BitBucket Remote CLI

[![CI](https://img.shields.io/github/actions/workflow/status/themankindproject/bbr/ci.yml?branch=main&label=CI)](https://github.com/themankindproject/bbr/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/release/themankindproject/bbr)](https://github.com/themankindproject/bbr/releases/latest)
![Rust Version](https://img.shields.io/badge/rust-1.88%2B-blue)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

A fast, single-binary Bitbucket Cloud CLI. Agent-first (`--json` everywhere, stable schemas and exit codes, env auth) with pretty human output.

PR lifecycle · CI/pipelines · status dashboard · batch ops · stacked PRs · repo admin · code search · deployments · raw API passthrough · self-update.

Full command reference: **[USAGE.md](USAGE.md)** · JSON schemas: **[docs/output-schema.md](docs/output-schema.md)** · Changelog: **[CHANGELOG.md](CHANGELOG.md)**

## Install

```bash
# Recommended: verified download from this repository's releases
curl -fsSL https://github.com/themankindproject/bbr/raw/main/install.sh | bash

# From source
cargo install --locked --git https://github.com/themankindproject/bbr

bbr completion --install  # shell completions (bash/zsh/fish/powershell)
```

**This project is not on crates.io.** The `bbr` name there belongs to an
unrelated congestion-control crate, so `cargo install bbr`,
`cargo binstall bbr`, and any plain registry lookup install the wrong program
with no relation to this CLI. Always fetch from
[this repository](https://github.com/themankindproject/bbr) — either the
installer above, `cargo install --git`, or a pre-built archive from
[Releases](https://github.com/themankindproject/bbr/releases/latest).

The install script verifies the download against the release's `checksums.txt`
and **fails closed** — a missing or mismatched checksum aborts the install, with
no silent "verification skipped" path. Set `BBR_SKIP_CHECKSUM=1` only if you must
bypass it. Pin a version by passing a tag (`... | bash -s v0.2.5`), and set
`GITHUB_TOKEN` to avoid GitHub API rate limits in CI. See
[docs/distribution.md](docs/distribution.md) for the full channel list, published
targets, and how to enable the optional package registries.

If you installed through a package manager (Homebrew, Scoop, Nix, apt), upgrade
through that same channel — `bbr update` detects a package-managed install and
refuses to overwrite it.

The installer and `bbr update` verify `checksums.txt`; the latest published
release provides Linux x86_64 (musl and glibc), macOS Intel and Apple Silicon,
and Windows x86_64 (MSVC). Additional targets, including aarch64 Linux, are
configured in the release matrix for future releases. Homebrew, Scoop, and winget
manifests are generated per release; those registries are optional and are not
published yet.

## Auth

HTTP Basic with an [Atlassian API token](https://id.atlassian.com/manage-profile/security/api-tokens)
(as opposed to the older app passwords):

```bash
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="<api-token>"

# Or interactive file (~/.config/bbr/credentials.toml, mode 0600)
bbr auth setup && bbr auth test

# Store a token from a protected file without putting it in process arguments
bbr auth setup --username you@example.com --token-stdin < /secure/path/token.txt
```

### Token scopes

Create the token with the scopes for the commands you use — see Atlassian's
[API token permissions](https://support.atlassian.com/bitbucket-cloud/docs/api-token-permissions/)
reference for the authoritative list.

| Scope | Enables |
|-------|---------|
| `read:user:bitbucket` | `auth test`, `auth status`, `pr dashboard` |
| `read:repository:bitbucket` | repo info, branches, commits, `src`, code search |
| `write:repository:bitbucket` | commit statuses |
| `read:pullrequest:bitbucket` | listing/viewing PRs, comments, tasks, diffs |
| `write:pullrequest:bitbucket` | create, update, approve, decline, merge PRs |
| `read:pipeline:bitbucket` | pipelines, steps, logs, test reports |
| `write:pipeline:bitbucket` | trigger, rerun, stop pipelines |
| `read:issue:bitbucket` / `write:issue:bitbucket` | `bbr issue` (optional) |
| `read:webhook:bitbucket` / `write:webhook:bitbucket` | `bbr webhook` (optional) |
| `read:ssh-key:bitbucket` / `write:ssh-key:bitbucket` / `delete:ssh-key:bitbucket` | `bbr deploy-keys` (optional) |
| `read:workspace:bitbucket` | `bbr workspace list` (optional) |
| `delete:repository:bitbucket` | `bbr repo delete` (optional, destructive) |

`write:` scopes do not imply their `read:` counterparts, so request both where
you need them. Environment variables take precedence over the credentials file.

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
| 5 | pipeline failed (`bbr ci watch`) or deployment failed (`bbr deploy view --wait` / `bbr deploy trigger --wait`) |
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
| `BBR_TIMEOUT` | HTTP request timeout in seconds (1–3600) | 30 |
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
