# bbr — BitBucket Remote CLI

[![CI](https://img.shields.io/github/actions/workflow/status/themankindproject/bbr/ci.yml?branch=main&label=CI)](https://github.com/themankindproject/bbr/actions/workflows/ci.yml)
![Rust Version](https://img.shields.io/badge/rust-1.74%2B-blue)
[![License](https://img.shields.io/crates/l/bbr)](LICENSE)

A fast, single-binary Bitbucket Cloud CLI. Agent-first (`--json` everywhere, zero-config env auth) with pretty human output.

```bash
cargo install --locked --git https://github.com/themankindproject/bbr
```

## Quick Start

```bash
export BITBUCKET_USERNAME="you@example.com"
export BITBUCKET_TOKEN="<pat-from-id.atlassian.com>"

bb status              # PR + CI for current branch
bb pr list             # open PRs
bb pr create --title T --body B
bb ci list             # pipelines for this branch
bb open pr             # open current PR in browser
```

## Commands

### PR
```bash
bb pr list [--state open|merged|declined|all]
bb pr view [<id>] [--diff] [--comments]
bb pr create --title T --body B [--src S --dst D]
bb pr update <id> --title T --description D
bb pr comment <id> --body B [--reply-to <id>]
bb pr approve|unapprove|decline|merge <id>
bb pr comments|tasks|commits|statuses|conflicts [<id>]
bb pr request-changes|unrequest-changes <id>
```

### CI
```bash
bb ci status [--branch B]
bb ci list [--branch B]
bb ci steps [<uuid>]
bb ci watch [--branch B] [--logs]
bb ci logs [<uuid>] [--failed] [--step <name>] [--output <file>]
```

### Repo
```bash
bb repo info
bb repo branches
bb repo tags
bb repo commits [--branch B] [--limit N]
```

### Auth
```bash
bb auth setup
bb auth test
bb auth status
bb auth logout
```

### Other
```bash
bb open [repo|pr|ci|pipelines]
bb status --watch [--interval N]   # live refresh
bb status --short                  # compact single-line
bb commit status set --key K --state successful --url U
bb completion bash|zsh|fish
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

Sources checked in order: `BITBUCKET_USERNAME` + `BITBUCKET_TOKEN` env vars → `bb auth setup` config file (`~/.config/bb/credentials.toml`, mode 0600).

Requires an [Atlassian PAT](https://id.atlassian.com/manage-profile/security/api-tokens) with scopes: `account:read`, `repository:read`, `repository:write`, `pullrequest:read`, `pullrequest:write`, `pipeline:read`.

## Development

```bash
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Tests use `wiremock` — no network access required.

## License

MIT
