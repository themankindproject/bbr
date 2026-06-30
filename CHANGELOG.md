# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- CI now runs `cargo test` and `cargo nextest` on macOS and Windows in addition to Linux.
- **`bbr update`** — new subcommand that checks for and automatically installs newer
  releases from GitHub. If a newer version exists, it downloads, extracts, and
  replaces the current binary in one step. Version check results are cached in
  `~/.config/bbr/update-check.json` (24h TTL).
- **`bbr update --check`** — check for updates without installing.
- **Auto-update notification** — running `bbr` (no subcommand) or `bbr status` now
  performs a lightweight background check against GitHub and prints a hint if a
  newer version is available.
- **Auth scope guidance** — `bbr auth setup` now lists all required API token scopes
  (`read:user:bitbucket`, `read:repository:bitbucket`, `read:pullrequest:bitbucket`,
  `write:pullrequest:bitbucket`, `read:pipeline:bitbucket`, `write:pipeline:bitbucket`,
  plus optional issue/webhook scopes) instead of the old OAuth-style scope names.
- **Better error hints** — `bbr` now shows scoped guidance on auth failures
  (API token URL, minimum required scopes), rate-limit hints, and timeout hints.
- `bbr repo commits` table now includes an Author column.

### Performance

- Replaced `s.push_str(&format!(...))` with `write!()` in the CI comparison renderer
  (`src/commands/ci_compare.rs`) — avoids 11 temporary `String` allocations per render.
- Removed `async` from three purely-synchronous functions (`schema::run`,
  `stack::init`, `stack::rebase`) — eliminates unnecessary future/task overhead.
- Eliminated unnecessary `.clone()` on `Option<String>` render fields in
  `pr.rs` and `status.rs` (6 call sites) — uses `as_deref().unwrap_or("-").to_string()` instead.
- Rate-limit jitter improved from 0–2s (`subsec_nanos % 3`) to 0–4s counter-based spread.

### Fixed

- PowerShell `bbr completion --install` wrote to `.config/powershell/` instead of `Documents/PowerShell/` on Windows.
- `bbr auth setup` now prints a confirmation line (`✓ Token read (N characters)`) after pasting
  the API token, preventing users from pasting multiple times due to lack of visual feedback.
- Nested or-pattern in `api/pipeline.rs:33` flattened (`Some("SUCCESSFUL" | "FAILED" | "STOPPED" | "ERROR")`).
- Unnecessary raw-string hashes in `commands/completion.rs:79` (`r#"..."#` → `r"..."`).
- `map(|h| h.branch).unwrap_or_else(|| "main".to_string())` replaced with
  `map_or_else` in `ci_compare.rs`.
- **JSON output corruption** — `deploy set-env-var`, `deploy delete-env-var`, `ci vars set`,
  `ci vars delete`, `batch`, and `issue view --comments` all used `println!` directly,
  which corrupted `--json` output. All now route through `Formatter` or `eprintln!`.
- **Schema definitions** — `bbr schema status`, `bbr schema pr`, and `bbr schema ci` were
  out of sync with actual output structs. Rewritten to match.
- **Dashboard double-counted approvals** — counted both `participants` and `reviewers`,
  but reviewers appear in both arrays. Now counts `participants` only.
- **`bbr auth setup` lost workspace override** — re-running setup set `workspace: None`,
  discarding a previously saved workspace. Now preserves existing value.
- **Issue URL encoding** — `bbr issue list --query` only encoded spaces and double quotes.
  Now uses proper percent-encoding for all special characters.
- **`git checkout_branch` swallowed errors** — retried `git switch -c` for any failure
  (including dirty tree, merge conflicts). Now only retries when branch doesn't exist.
- **Config directory permissions** — `~/.config/bbr/` created with `0700` on Unix
  (was `0755`, leaking metadata).
- **`Debug` leaked credentials** — `Credentials` and `BitbucketClient` derived `Debug`,
  printing raw tokens in trace output. Now uses `[REDACTED]`.
- **`PipelineFailed` error** now carries `build_number` and `branch` context.
- **Timeout errors** now show a hint about the 30s default and suggest checking the network.
- `bbr batch` results use ASCII-safe `[ok]`/`[X]` glyphs (was using Unicode `✓`/`✗`).
- `bbr repo info` now uses themed labels for field names.
- `bbr issue view --comments` in JSON mode now includes comments in the JSON object
  instead of dumping raw text after it.
- Removed dead `empty` variable in `api/issue.rs` `update_issue`.
- Dashboard activity format removed spurious `#{:<3}` alignment.
- All `bb` references in docs (`output-schema.md`, `USAGE.md`, issue templates, `README.md`
  MSRV badge) corrected to `bbr`.
- `Cargo.toml` repository/homepage URLs updated to `themankindproject/bbr`.

## [0.1.1] - 2026-06-29

### Fixed

- Pasting API tokens in `bbr auth setup` no longer corrupts them with bracketed-paste
  escape sequences (`\x1b[200~` … `\x1b[201~`) from modern terminals.
- Pasted API tokens are now trimmed of leading/trailing whitespace, matching the
  behaviour of the username prompt.

### Changed

- All internal command references and suggested commands unified to `bbr`
  (was inconsistently mixing `bb` and `bbr`).

## [0.1.0] - 2026-06-29

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
- `bb auth setup` — interactive prompts (username, API token), writes `0600` credentials.toml.
- `bb auth test` — validates credentials via `GET /user`.
- `bb auth status` — shows auth source (env/config/none), display name, account ID.
- `bb auth logout` — removes stored credentials.
- Credential resolution chain: env vars (`BITBUCKET_USERNAME` + `BITBUCKET_TOKEN`) → config file.
- Only `ApiToken` with HTTP Basic auth — Bitbucket Cloud API does not accept PAT/Bearer or AppPassword.
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
- **Credential system simplified**: removed `Pat` (Bearer) and `AppPassword` credential kinds.
  Only `ApiToken` with HTTP Basic auth remains — the only method Bitbucket Cloud accepts.
- `CredentialProfile` only stores `username` + `token` + `workspace`; `app_password` field removed.
- `bb auth setup` no longer asks for credential type — only username + API token.
- `bb auth status`/`bb auth test` report `"atlassian_api_token"` as credential kind.
- Documentation updated: PAT/Bearer/PAT-scopes → Atlassian API token, `BITBUCKET_APP_PASSWORD` removed.

### Fixed
- `bb auth setup` silently discarded Atlassian API token secrets — stored in neither `token` nor `app_password` field.
- Missing `use crate::error` imports in `commands/auth`.
- `bb repo info --json` emitted `"scim"` instead of `"scm"`.
- `bb status` swallowed 401/403 API errors and returned empty output instead of exiting non-zero.
- HTTP response body-read failures were hidden behind a misleading JSON parse error.
- `detect_repo` now explicitly queries `origin` first before scanning all remotes.
- Pipeline/step UUID braces kept in API URLs — Bitbucket requires `%7B`/`%7D` encoding.
- `bb ci logs` 400 error — `Accept: text/plain` → `Accept: */*`.
- CLI smoke test no longer cleans up `BITBUCKET_APP_PASSWORD` env var.

### Security
- Credentials file opened with mode `0o600` at creation time on Unix, closing TOCTOU window.
- No system keyring dependency (avoids 671 MB texlive pull).

[Unreleased]: https://github.com/themankindproject/bbr/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/themankindproject/bbr/releases/tag/v0.1.1
[0.1.0]: https://github.com/themankindproject/bbr/releases/tag/v0.1.0
