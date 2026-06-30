# bbr — BitBucket Remote CLI

[![CI](https://img.shields.io/github/actions/workflow/status/themankindproject/bbr/ci.yml?branch=main&label=CI)](https://github.com/themankindproject/bbr/actions/workflows/ci.yml)
![Rust Version](https://img.shields.io/badge/rust-1.75%2B-blue)
[![License](https://img.shields.io/crates/l/bbr)](LICENSE)

A fast, single-binary Bitbucket Cloud CLI. Agent-first (`--json` everywhere, zero-config env auth) with pretty human output.

| Area | Other tools | bbr |
|------|-------------|-----|
| Auth | OAuth flows, profile systems, multi-cred | `BITBUCKET_USERNAME` + `BITBUCKET_TOKEN` env vars or single `bbr auth setup` |
| Output | human-only, JSON ad-hoc | `--json` on **every** command with stable schema + `bbr schema` introspection |
| Status | per-resource commands | `bbr` = PR + CI + commit statuses + suggested next commands in one shot |
| PRs | list / view / approve / merge | same + stacked PRs (`init`→`add`→`rebase`→`land`→`abort`), cross-repo dashboard |
| CI | web UI, basic CLI logs | `bbr ci watch --logs` live-tail with auto-fetch of failing steps, `bbr ci compare` (step/test deltas), test reports |
| Batch | manual per-repo | `bbr batch merge-approved`, `rerun-failed`, `cleanup-merged-branches` with `--dry-run` |
| Repo | info, branches, tags | same + `bbr repo audit` (compliance: branch restrictions, default reviewers) |
| Extras | webhooks, issues, source browsing | same + deployments, environment variables, raw API passthrough with pagination |

**bbr is the only Bitbucket CLI with:** stacked PRs, pipeline comparison, batch operations, repo audit, live CI watch with auto-log-fetch, `--json` on every command + schema introspection, and a one-shot status overview — all in a single Rust binary with no runtime dependencies.

```bash
curl -fsSL https://github.com/themankindproject/bbr/raw/main/install.sh | bash
# or: cargo install --locked --git https://github.com/themankindproject/bbr --tag $(curl -fsSL https://api.github.com/repos/themankindproject/bbr/releases/latest | grep tag_name | cut -d'"' -f4)
```

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

## Commands

### PR
```bash
bbr pr list [--state open|merged|declined|all]
bbr pr view [<id>] [--diff] [--comments]
bbr pr create --title T --body B [--src S --dst D]
bbr pr update <id> --title T --description D
bbr pr comment <id> --body B [--reply-to <id>]
bbr pr approve|unapprove|decline|merge <id>
bbr pr comments|tasks|commits|statuses|conflicts [<id>]
bbr pr request-changes|unrequest-changes <id>
```

### CI
```bash
bbr ci status [--branch B]
bbr ci list [--branch B]
bbr ci steps [<uuid>]
bbr ci watch [--branch B] [--logs]
bbr ci logs [<uuid>] [--failed] [--step <name>] [--output <file>]
```

### Repo
```bash
bbr repo info
bbr repo branches
bbr repo tags
bbr repo commits [--branch B] [--limit N]
```

### Auth
```bash
bbr auth setup
bbr auth test
bbr auth status
bbr auth logout
```

### Other
```bash
bbr open [repo|pr|ci|pipelines]
bbr status --watch [--interval N]   # live refresh
bbr status --short                  # compact single-line
bbr commit status set --key K --state successful --url U
bbr completion bash|zsh|fish
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | success |
| 1 | generic error |
| 2 | auth failure |
| 3 | not found |
| 4 | rate limited |
| 5 | pipeline failed |

## Authentication

Sources checked in order: `BITBUCKET_USERNAME` + `BITBUCKET_TOKEN` env vars → `bbr auth setup` config file (`~/.config/bbr/credentials.toml`, mode 0600).

Requires an [Atlassian API token](https://id.atlassian.com/manage-profile/security/api-tokens) with scopes: `account:read`, `repository:read`, `repository:write`, `pullrequest:read`, `pullrequest:write`, `pipeline:read`.

## Development

```bash
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Tests use `wiremock` — no network access required.

## License

MIT
