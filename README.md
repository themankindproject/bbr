# bbr — BitBucket Remote CLI

[![CI](https://img.shields.io/github/actions/workflow/status/themankindproject/bbr/ci.yml?branch=main&label=CI)](https://github.com/themankindproject/bbr/actions/workflows/ci.yml)
![Rust Version](https://img.shields.io/badge/rust-1.75%2B-blue)
[![License](https://img.shields.io/crates/l/bbr)](LICENSE)

A fast, single-binary Bitbucket Cloud CLI. Agent-first (`--json` everywhere, zero-config env auth) with pretty human output.

## Install

```bash
curl -fsSL https://github.com/themankindproject/bbr/raw/main/install.sh | bash
```

Or build from source:

```bash
cargo install --locked --git https://github.com/themankindproject/bbr
```

## Quick Start

```bash
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="<api-token-from-id.atlassian.com>"

bbr                          # PR + CI + statuses for current branch
bbr pr list                  # open PRs
bbr ci watch --logs          # live-tail pipeline, auto-fetch failing step
bbr pr dashboard             # workspace-wide PR dashboard
bbr batch merge-approved     # merge all fully-approved PRs
```

## What's Different

| Feature | bbr | Other CLIs |
|---------|-----|------------|
| `bbr` (no args) | PR + CI + commit statuses + suggested commands | N/A |
| `--json` | On **every** command, stable schema | Ad-hoc or missing |
| Stacked PRs | `init` → `add` → `rebase` → `land` → `abort` | Not available |
| Pipeline comparison | `bbr ci compare` with step/test deltas | Not available |
| Batch operations | `merge-approved`, `rerun-failed`, `cleanup-merged-branches` | Manual per-repo |
| Repo audit | SOC2-readiness: branch restrictions, approvals, push protection | Not available |
| CI watch | Live-tail with auto-fetch of failing step logs | Basic logs only |
| Output | `--export slack/markdown`, `--short`, `--quiet`, `--no-pager` | Human-only |
| Single binary | Rust, no runtime deps, ~10MB | Varies |

## Full Documentation

**[USAGE.md](USAGE.md)** — complete command reference, global flags, scripting patterns, authentication, exit codes, environment variables.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | success |
| 1 | generic error |
| 2 | auth failure |
| 3 | not found |
| 4 | rate limited |
| 5 | pipeline failed |

## Development

```bash
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Tests use `wiremock` — no network access required.

## License

MIT
