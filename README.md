# bbr — BitBucket Remote CLI

[![CI](https://img.shields.io/github/actions/workflow/status/themankindproject/bbr/ci.yml?branch=main&label=CI)](https://github.com/themankindproject/bbr/actions/workflows/ci.yml)
![Rust Version](https://img.shields.io/badge/rust-1.74%2B-blue)
[![License](https://img.shields.io/crates/l/bbr)](LICENSE)

A fast, single-binary Bitbucket Cloud CLI. Agent-first (`--json` everywhere, zero-config env auth) with pretty human output.

| Area | Others | bbr |
|------|--------|-----|
| Auth | PAT, OAuth, complex setup | env vars or single `bbr auth setup` prompt |
| Output | human-only or requires `jq` | `--json` everywhere, stable schema, no scraping |
| PRs | basic list/view | create, update, merge, comment, approve, request-changes, stacked PRs, dashboard |
| CI | web UI only | `bbr ci watch --logs` with live tail, step-by-step breakdown, compare runs, test reports |
| Batch | manual | `bbr batch merge-approved`, `rerun-failed`, `cleanup-merged-branches` |
| Status | piecemeal | `bbr status` = PR + CI + commit statuses + suggested next commands in one shot |
| Config | YAML/JSON files | zero-config — `BITBUCKET_USERNAME` + `BITBUCKET_TOKEN` or interactive setup |
| Extras | — | webhooks CRUD, issue tracker, source browser, deployments, completions, repo audit |

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
