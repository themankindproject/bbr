# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
