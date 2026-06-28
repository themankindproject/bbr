# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-06-28

### Added

#### Pull Requests
- `bb pr list` with `--state`, `--author`, `--source-branch`, `--reviewer` filters.
- `bb pr view [<id>]` with `--diff` and `--comments` flags.
- `bb pr create` — reviewers, body resolution (direct/file/stdin), auto-default branch.
- `bb pr update <id> --title --description`.
- `bb pr comment <id> --body` with `--reply-to <id>`.
- `bb pr comments|tasks|commits|statuses|conflicts [<id>]`.
- `bb pr approve|unapprove|decline|merge|request-changes|unrequest-changes`.
- `bb pr checkout <id>` — fetch + switch to source branch.
- `bb pr diff <id>` — raw diff output.

#### CI / Pipelines
- `bb ci list [--branch] [--limit]` with parallel step fetching.
- `bb ci status [--branch]` — latest pipeline with steps.
- `bb ci watch [--branch] [--interval-secs] [--logs]` — live poll, non-zero exit on failure.
- `bb ci logs [<uuid>] [--step] [--failed] [--latest] [--output <file>]` — smart step selection.
- `bb ci steps [<uuid>]` — list pipeline steps.
- `bb ci tests [<uuid>] [--step] [--limit]` — test reports and test cases.
- `bb ci rerun [--branch]` — with confirmation prompt.
- `bb ci stop [<uuid>] [--branch]`.

#### Repository
- `bb repo info` — workspace, slug, SCM type, language, description, URL.
- `bb repo branches [--limit]` — remote branches.
- `bb repo tags [--limit]` — remote tags with target hash and date.
- `bb repo commits [--branch] [--limit]` — recent commits.

#### Auth
- `bb auth setup` — interactive prompts (username, credential type, secret), writes `0600` credentials.toml.
- `bb auth test` — validates credentials via `GET /user`.
- `bb auth status` — shows auth source (env/config/none), display name, account ID.
- `bb auth logout` — removes stored credentials.
- Credential resolution chain: env vars (`BITBUCKET_USERNAME` + `BITBUCKET_TOKEN`/`BITBUCKET_APP_PASSWORD`) → config file → (keyring reserved).
- PAT (`CredentialKind::Pat`), AppPassword, and Atlassian API Token (`ATATT`-prefix auto-detection) support.
- Credential file written with Unix `0o600` mode, atomic write.

#### Status / Overview
- `bb status` — PR + CI + commit statuses for current branch.
- `bb status --short` — compact single-line output.
- `bb status --watch [--interval N]` — live-refresh with ANSI clear.
- `bb` (no subcommand) — repo overview with recent PRs and CI.
- Concurrent fetch of PR, pipeline, and commit statuses via `tokio::try_join!`.
- Smart suggested commands (`bb open pr`, `bb ci logs --failed`, `bb ci watch --logs`).

#### Commit Statuses
- `bb commit status set <commit> --key --state --name --url --description --refname`.
- State normalization accepts aliases (e.g., `in-progress`, `running`, `cancelled`).

#### Open in Browser
- `bb open repo|pr-list|pr [<id>]|pipelines|ci [--branch]`.
- Platform-specific opener: `open` (macOS), `xdg-open` (Linux), `cmd /C start` (Windows).

#### Output
- Dual output mode: `--json` (stable `serde_json::to_string_pretty`) and human (themed tables).
- `Formatter` enum routes between JSON and human paths.
- `comfy-table` rendering with `UTF8_FULL` preset, right-aligned ID, centered State columns.
- `Theme` singleton respects `NO_COLOR` env var and TTY detection, returns `Cow<'_, str>` to avoid allocations.
- Semantic status glyphs: `[ok]`/`[X]`/`[!]`/`[~]`/`[.]`/`[?]` with appropriate colors.
- Spinner (`indicatif`) during network operations, hidden in JSON mode.
- Pagination support for long output (`$PAGER` or `less -F -R -X`).

#### Shell Completions
- `bb completion bash|zsh|fish|powershell` via `clap_complete`.

#### Error Handling
- Centralized `BitbucketError` enum (11 variants) with `thiserror`.
- Stable exit codes: 0 success, 1 generic, 2 auth, 3 not found, 4 rate-limited, 5 pipeline failed.
- API error envelope parsing with scope/permission table rendering for auth failures.
- Rate-limit retry (up to 2 attempts, linear 5s/10s backoff + sub-second jitter).

#### Git Integration
- `RepoIdentity` (workspace + slug) from `origin` remote URL — supports HTTPS (with embedded credentials), SSH, and SSH alias formats.
- Cached `OnceLock` for repo identity and HEAD (branch + commit) — avoids repeated subprocess spawns.
- Branch fetch (`git fetch origin <branch>`) and checkout (`git switch -c <branch> origin/<branch>`).

#### Testing
- 147 unit tests + 16 integration tests = 163 total.
- Wiremock-based HTTP mocking for all API integration tests (no network).
- CLI smoke tests via `assert_cmd`.
- Tests cover: all error mappings, pagination, URL parsing, credential resolution, state normalization, step selection, rendering, serialization, UUID normalization.

#### Build & CI
- Single-binary, `cargo install`-able (`bbr` crate, installed as `bb`).
- Rustls-only TLS (no OpenSSL dependency).
- Release profile: LTO, single codegen unit, panic=abort, stripped symbols.
- GitHub Actions CI: fmt, clippy, test, MSRV check on Linux/macOS/Windows.
- Cross-platform release workflow: Linux (x86_64 + aarch64), macOS (x86_64 + aarch64), Windows.

### Changed
- Theme methods return `Cow<'_, str>` instead of `String` — avoids allocation in no-color mode.
- `send_raw()` added to `BitbucketClient` for text/plain endpoints (diff, logs) with rate-limit + error handling.
- N+1 query parallelized in `ci list` — steps for all pipelines fetched concurrently via `futures::future::join_all`.
- N+1 query parallelized in `status` — commit statuses fetched concurrently with PR/pipeline.
- `Cargo.toml` cleaned up — unused `tokio` features (`io-util`, `fs`, `signal`) and `reqwest` feature (`stream`) removed.
- `confirm()` uses `eq_ignore_ascii_case` instead of allocating `to_ascii_lowercase()`.
- `stdout_is_tty()` (dead code) removed.
- `PullRequest`, `BranchRef`, `Participant` now derive `Default` for test ergonomics.
- `PullRequest.state` is now `#[serde(default)]` for resilience.

### Fixed
- `bb auth setup` silently discarded Atlassian API token secrets — stored in neither `token` nor `app_password` field.
- Missing `use crate::error` imports in `commands/auth`.
- `bb repo info --json` emitted `"scim"` instead of `"scm"`.
- `bb status` swallowed 401/403 API errors and returned empty output instead of exiting non-zero.
- HTTP response body-read failures were hidden behind a misleading JSON parse error.
- `detect_repo` now explicitly queries `origin` first before scanning all remotes.
- Pipeline/step UUID braces kept in API URLs — Bitbucket requires `%7B`/`%7D` encoding.
- `bb ci logs` 400 error — `Accept: text/plain` → `Accept: */*`.

### Security
- Credentials file opened with mode `0o600` at creation time on Unix, closing TOCTOU window.
- No system keyring dependency (avoids 671 MB texlive pull).

[Unreleased]: https://github.com/themankindproject/bbr/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/themankindproject/bbr/releases/tag/v0.1.0
