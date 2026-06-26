# bbr — a Bitbucket Cloud CLI for coding agents and humans

A fast, single-binary Bitbucket Cloud CLI written in Rust. Designed for
coding agents first (machine-readable `--json`, zero-config env auth) and
humans second (pretty tables, color, progress bars). The `gh`-equivalent
Bitbucket never had.

```
$ bb status
On branch: feat/av1-ffprobe-timeout  (commit 765d8bec)

PR #467 — open
  feat/av1-ffprobe-timeout -> main
  Title: create frame_utils_1_2 with ffprobe-based AV1 detection
  Author: bravo1goingdark
  URL:   https://bitbucket.org/sdadev/bvrm-backend/pull-requests/467

CI - last pipeline
  SUCCESSFUL (3 minutes ago, 172s)
  Branch: test-ci  /  Commit: 4644ec4b
  Steps:
    [ok] Run Tests        172s
```

## Install

```bash
# from source
cargo install --git https://github.com/sdadev/bbr

# pre-built binary (releases page)
curl -sSf https://github.com/sdadev/bbr/releases/latest/download/bbr-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv bbr /usr/local/bin/bb
```

The binary is installed as `bb`.

## Quick start

```bash
# 1. Get a Personal Access Token (PAT):
#    https://bitbucket.org/account/settings/api-tokens
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
bb ci status         # last pipeline for current branch
bb ci watch          # live-tail a running pipeline
```

## Commands (v0.1)

```
bb status                              # PR + CI for current branch
bb pr list   [--state open|merged|declined|all]
bb pr view   [<id>]                    # defaults to current branch's PR
bb pr create --title T --body B [--src S --dst D]
bb pr comment <id> --body B
bb ci status [--branch B]
bb ci watch  [--branch B]              # live tail, exits non-zero on failure
bb ci logs   <pipeline-uuid>           # step logs
bb auth setup                          # interactive credential setup
bb auth status                         # verify auth works
bb auth logout                         # remove stored creds
bb repo info                           # show workspace/slug for current dir
bb --version
bb --help
bb completion bash|zsh|fish            # emit completions
```

Add `--json` to any data command for stable, predictable JSON output.

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

## Output format

- **Humans:** pretty tables, color, emoji. Respects `NO_COLOR` and auto-disables
  decoration when stdout is not a TTY.
- **Agents:** `bb <cmd> --json` emits stable JSON. Schema documented in
  [`docs/output-schema.md`](docs/output-schema.md).

### Exit codes

| Code | Meaning                                  |
|------|------------------------------------------|
| 0    | success                                  |
| 1    | generic error                            |
| 2    | auth error (no creds / bad creds)        |
| 3    | not found (no PR / no pipeline)          |
| 4    | API rate limit                           |
| 5    | pipeline failed (for `bb ci watch`)      |

## Shell completions

```bash
bb completion bash > /etc/bash_completion.d/bb   # or ~/.local/share/bash-completion/completions/bb
bb completion zsh > "${fpath[1]}/_bb"
bb completion fish > ~/.config/fish/completions/bb.fish
```

## Development

```bash
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Tests use [`wiremock`](https://crates.io/crates/wiremock) to mock the Bitbucket
API; no network access is required.

## License

MIT — see [LICENSE](LICENSE).
