# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-06-27

### Added
- `ApiToken` credential kind for Atlassian API tokens (auto-detected by `ATATT` prefix in env and config file).
- Exponential back-off retry (5 s then 10 s) on HTTP 429 rate-limit responses.
- `rpassword` integration: secret input is now hidden during `bb auth setup`.

### Fixed
- `bb auth setup` silently discarded Atlassian API token secrets — stored in neither `token` nor `app_password` field.
- Missing `use crate::error` imports in `commands/auth` caused a compile error introduced in the previous patch.
- `bb repo info --json` emitted `"scim"` instead of the correct `"scm"` field.
- `bb status` swallowed 401/403 API errors and returned empty output instead of exiting non-zero.
- HTTP response body-read failures were hidden behind a misleading JSON parse error.
- `detect_repo` now explicitly queries `origin` first before scanning all remotes.

### Security
- Credentials file is now opened with mode `0600` at creation time on Unix, closing a TOCTOU window where the file was briefly world-readable between write and chmod.

### Refactored
- `BitbucketClient::auth_header` promoted to `pub(crate)`; duplicate `auth_header_value` implementation in `pipeline.rs` removed.

### Added
- Initial project scaffold.
- `bb auth setup|status|logout` (env + config file credential sources).
- `bb repo info` for the current working directory.
- `bb pr list|view|create` against Bitbucket Cloud.
- `bb ci status` for the latest pipeline on a branch.
- `bb status` merged PR + CI view for the current branch.
- `--json` machine-readable output for all data commands.
- Pretty table output for humans (respects `NO_COLOR` and non-TTY).
- Shell completions via `bb completion bash|zsh|fish`.
- Stable exit codes (see README).
- CI workflow (fmt, clippy, test, msrv).
- Cross-platform release workflow (Linux, macOS x86_64 + aarch64, Windows).

[Unreleased]: https://github.com/sdadev/bbr/compare/v0.0.0...HEAD
