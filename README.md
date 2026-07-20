# bbr — BitBucket Remote CLI

[![CI](https://img.shields.io/github/actions/workflow/status/themankindproject/bbr/ci.yml?branch=main&label=CI)](https://github.com/themankindproject/bbr/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/release/themankindproject/bbr)](https://github.com/themankindproject/bbr/releases/latest)
![Rust Version](https://img.shields.io/badge/rust-1.88%2B-blue)
[![License](https://img.shields.io/crates/l/bbr)](LICENSE)

A fast, single-binary Bitbucket Cloud CLI. **Agent-first** (`--json` everywhere, stable schemas, env auth) with pretty human output.

Full command reference: **[USAGE.md](USAGE.md)** · JSON schemas: **[docs/output-schema.md](docs/output-schema.md)** · Changelog: **[CHANGELOG.md](CHANGELOG.md)**

---

## Why

Bitbucket Cloud lacked a solid CLI. Coding agents and developers needed something like GitHub’s `gh` — scriptable, `--json`-friendly, zero-config auth — without living in `curl` or the web UI.

`bbr` gives you that: status/overview in one shot, full PR + CI lifecycle, batch ops, stacks, and stable exit codes for automation.

---

## Install

```bash
# One-liner (recommended)
curl -fsSL https://github.com/themankindproject/bbr/raw/main/install.sh | bash

# Or from source
cargo install --locked --git https://github.com/themankindproject/bbr
```

Pre-built archives: [Releases](https://github.com/themankindproject/bbr/releases/latest) (Linux x86_64, macOS Intel/ARM, Windows).

```bash
bbr completion bash --install   # zsh / fish / powershell also supported
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
```

---

## Quick start

```bash
cd my-bitbucket-repo

bbr                           # overview: PRs, approvals, recent CI
bbr status                    # full status for current branch
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
| 2 | auth |
| 3 | not found |
| 4 | rate limited |
| 5 | pipeline failed |

Every data command supports `--json`. See [USAGE.md](USAGE.md) for flags, subcommands, and scripting patterns.

---

## Develop

```bash
cargo build --release --locked
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

MSRV **1.88**. No OpenSSL (`rustls`). Tests use `wiremock` (no network).

Release: bump `Cargo.toml`, update `CHANGELOG.md`, tag `vX.Y.Z` → GitHub Actions cross-compiles and publishes.

---

## License

MIT — see [LICENSE](LICENSE).
