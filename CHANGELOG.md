# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`bbr pr create --push`** — push the source branch to `origin` before
  creating the PR, so `git push && bbr pr create` collapses into a single
  command. A new branch is pushed; an up-to-date or fast-forwardable branch is
  pushed normally; a branch that has diverged from the remote is refused with a
  clear message. Pass `--force` to overwrite a diverged remote branch via
  `git push --force-with-lease` (which still refuses if the remote gained new
  commits since). The `--force` flag requires `--push`.
- **`bbr pr comment delete <id> <comment-id>`** — delete a comment on a pull
  request (`DELETE .../comments/{comment_id}`). Destructive operations confirm
  on a TTY; skip the prompt with `--yes` or run with `--json`. Note that
  Bitbucket marks a comment with visible replies as deleted rather than
  removing it, to preserve the comment tree.

## [0.2.4] - 2026-08-26

### Added

- **`bbr api --paginate --limit <N>`** — cap on items fetched when following
  pagination (default 10000). Previously `--paginate` buffered every page of
  a huge repo in memory before emitting anything.

### Fixed

- **Quitting the pager no longer prints an error** — pressing `q` in
  `less`/`bat` while `bbr pr diff` (or any paged output) was still streaming
  closed the pipe mid-write and surfaced `IO error: Broken pipe (os error
  32)`. EPIPE from an early-exiting pager or downstream consumer
  (`bbr status | head`) is now treated as a clean exit 0 with no message.
- **stderr broken pipe no longer aborts with a core dump** — `bbr status
  --watch 2>&1 | head -1` panicked on the first stderr write after the
  consumer closed the pipe, and with `panic = "abort"` that became SIGABRT +
  core dump. `main()` now resets `SIGPIPE` to its default disposition, so the
  kernel terminates the process cleanly like standard Unix tools.
- **`status --watch --interval 0` no longer busy-loops** — the poll interval
  is clamped to at least 1s (matching the `ci` watch loops), so a zero
  interval can't hammer the API and burn the hourly quota.
- **Windows self-update now works** — the updater looked for a `.tar.gz`
  asset and a `bbr` binary, but the release pipeline ships Windows as a
  `.zip` containing `bbr.exe`. It now selects the right archive format and
  binary name per platform, extracts zip assets, and renames the running
  (locked) executable out of the way before installing. Install-dir fallback
  uses a portable home lookup (`$HOME` is unset on Windows).
- **POST requests are no longer retried on 5xx** — a 5xx response to a
  mutation (merge, approve, comment, create) can mean the server applied it
  before failing, so replaying could double-apply. 429 is still retried for
  all methods (the server rejects it before processing); 5xx is retried only
  for idempotent methods.
- **`Retry-After` is capped at 60s** — a misbehaving proxy sending an
  enormous `Retry-After` could stall the CLI for hours mid-command.
- **`--limit` is honored on single-page fetches** — `fetch_paginated` now
  truncates to the requested limit (parity with the all-pages path) and
  returns empty for `--limit 0` instead of fetching a default page.
- **Credentials file is written atomically** — `credentials.toml` is written
  to a 0600 temp file and renamed into place, so a crash mid-write can no
  longer leave a truncated credential file.
- **`pr stack` no longer hangs on a wedged git** — the repo-root lookup now
  goes through the shared timeout-guarded git runner instead of an unbounded
  `git rev-parse`.
- **`BBR_SKIP_CHECKSUM` requires a truthy value** — `=0` or empty no longer
  skips checksum verification; only `1`/`true`/`yes`/`on` do.
- **Checksum matching uses the asset basename** — a sibling asset whose name
  merely ends with ours can no longer match by accident.
- **Prerelease tags no longer compare as updates** — version parsing strips
  prerelease/build metadata, so `v1.2.3-beta.1` isn't offered over `v1.2.3`.
- **`$PAGER` values with quotes or spaces are parsed correctly** — e.g.
  `PAGER="'my pager' -X"` now splits into the right argv instead of passing
  literal quote characters.
- **Truncated fields never exceed their column budget** — the ellipsis is
  reserved out of the width, so truncated table cells can't widen columns.
- **A credentials file with a token but empty username now warns** instead of
  silently failing with "no credentials".

## [0.2.3] - 2026-08-26

### Added

- **`bbr doctor`** — environment self-check: git, repo identity, credentials,
  credential file permissions, pager tooling, API reachability, rate-limit
  headroom, and version currency, each reported as ok/warn/fail. Never
  aborts on individual failures; `--json` emits the structured check array;
  `--strict` exits non-zero when anything fails (for CI/scripts).
- **Light terminal theme support** — `bbr config set ui.theme dark|light|auto`
  (default `auto`). Light presets switch syntax highlighting to the
  InspiredGitHub palette and diff line tints to pale green/red so diffs stay
  readable on white backgrounds. Auto-detection uses `COLORFGBG` where
  available.
- **Interactive PR disambiguation** — when a branch has multiple open PRs,
  `bbr pr view` (no id) shows a numbered picker on TTYs. Non-interactive
  contexts (pipes/agents) keep the previous first-PR behavior;
  `BBR_NO_INTERACTIVE=1` forces it.
- **`status --watch` footer** — stable dim footer with refresh cadence and
  remaining API quota (TTY only), plus gentler per-tick redraw.
- **Help examples** — root `--help` gains a "Common workflows" block; `pr`,
  `ci`, and `batch` help pages show copy-pasteable recipe examples.
- **`--min-approvals <N>` on `batch merge-approved`** — require at least N
  approvals (reviewers first, falling back to participants) before a PR
  qualifies for auto-merge. Default remains 1; raise it to avoid merging
  on a single approval.
- **Secret-safe stdin input** — `variable set --stdin`, `ci vars set
  --stdin`, `webhook create --secret-stdin`, and `deploy-keys add --stdin`
  read the secret/key material from stdin so it never appears in `ps`
  output or shell history. Mutually exclusive with the corresponding
  inline flags (`--value`, `--secret`, `--key`), which are now optional.
- **`pr list --sort/--order` validated at the CLI layer** — invalid values
  now fail fast as usage errors (exit 64) before any network access.
- **`pr diff --file <SEL>`** — restrict the pretty diff to specific files:
  1-based indices from the file list (`--file 3`), ranges (`--file 1-5`),
  and/or pathspec globs (`--file "src/api/*.rs"`), comma-separated and
  repeatable. Indices match the numbering shown in the file index; a
  selection that matches nothing fails fast with a clear error.
- **`pr diff --wrap`** — wrap long lines at the terminal width instead of
  truncating them with `…`. Continuation rows carry a dimmed gutter with a
  wrap marker so nothing is discarded; works in both colored and plain
  output.

### Changed

- **Binary files are labeled, not counted** — the file index and per-file
  header show `(binary)` instead of a misleading `+0, -0`, and the body
  renders an explicit "(binary file changed)" note. Detection no longer
  relies on the `Binary files … differ` marker, which Bitbucket's diff
  endpoint omits: a changed file with no hunks and no line counts is now
  recognized as binary (pure renames excluded).
- **Side-by-side diff auto-collapses one-sided files** — a file that only
  adds or only deletes lines renders in unified layout instead of wasting
  half the terminal on an empty column.
- `bbr status --watch` no longer full-screen-clears each tick (cursor-home +
  erase-to-EOL), reducing visible flicker.
- **Rate-limit-aware request pacing** — parallel pagination fan-out scales
  down automatically when remaining API quota is low (10 → 5 → 2 concurrent
  page fetches below 300 / 100 remaining requests).
- **`status --watch` request diet** — PR diffstat and conflict results are
  cached across watch ticks, cutting per-tick usage from 2 extra requests
  per open PR to zero for unchanged PRs (previously ~1,400 requests/hour
  for a branch with two open PRs at the default 5s interval).
- **Failing-step log tail is bounded** — on pipeline failure, `ci watch`
  fetches only the last ~64KB of the failing step's log via an HTTP range
  request instead of downloading the entire multi-MB log.
- **Syntax-highlight engine warms in the background** — the syntect syntax
  set loads during startup instead of stalling the first diff render.
- Retry back-off jitter mixes a process-local counter with clock nanos, so
  concurrent retries no longer produce identical delays.

### Fixed

- **Approvals from the web UI now count** — Bitbucket records an approval
  from someone who isn't a listed reviewer as a `PARTICIPANT` participant,
  not as a reviewer. `bbr status`, `bbr pr view`, and
  `batch merge-approved --min-approvals` only counted formal reviewers, so
  PRs kept showing "unapproved" (or were skipped by batch merges) after
  being approved. Reviewers and approving/requesting participants are now
  rolled up per person (UUID → nickname → name matching), so approvals from
  either surface are reflected immediately.
- **Git subprocess deadlock** — `git` child processes are now drained
  concurrently with the wait; previously, output larger than the OS pipe
  buffer (~64KB) blocked until the command timed out and was killed,
  surfacing as spurious "git command timed out" errors on large
  fetch/rebase/clone operations.
- **`ci watch` survives transient poll errors** — a single network blip or
  5xx during polling now logs a warning and retries next tick instead of
  aborting hours-long watches; auth failures remain fatal.
- **Range responses that servers ignore no longer replay log content** —
  streaming commands distinguish HTTP 206 from a full-body 200 and reset
  their byte offset in the latter case, preventing duplicate output when an
  intermediary strips Range support. The same holdback now applies to the
  final flush of `ci tail --follow`, fixing split-line double renders.
- **Retryability is structural** — 5xx errors carry their status code in a
  dedicated error variant (`server` kind in `--json` output) rather than
  relying on string-matching error message text.
- **Error scope tables are plain ASCII** — permission-denied messages no
  longer embed theme-dependent Unicode glyphs, keeping `--json` stderr
  stable across environments.
- **Stored API tokens are zeroized on drop** — `credentials.toml` parsing
  holds the token in a `SecretString`; the value is wiped from memory when
  the profile is released instead of lingering in freed heap memory. Disk
  format is unchanged.
- **Test isolation** — environment-mutating tests share one process-wide
  lock, removing cross-module flakiness from parallel test runs.

## [0.2.2] - 2026-08-16

### Added

- **`bbr pr merge --yes`** — skip the interactive confirmation prompt for
  scripted merges (matches `batch merge-approved --yes` and `pr stack land`).
- **Pre-flight credential check** — API commands now fail fast with a clear
  auth error (exit 2) before git detection, so running e.g. `bbr repo info`
  outside a git repo reports "no credentials" instead of a confusing
  "not a git repository" error.
- **`bbr ci watch --notify` / `bbr ci tail --notify`** — ring the terminal
  bell when the pipeline/step reaches a terminal state, so a long build
  finishing in a background pane grabs attention.
- **Smart failure extraction** — on pipeline failure, `bbr ci watch` now
  locates the first error line and shows it with surrounding context (10
  lines before, 30 after, plus a short tail with an omission marker) instead
  of a blind last-120-lines dump. Falls back to the tail when nothing in the
  log classifies as an error.
- **`bbr ci tail --all`** — auto-advance: when the tailed step finishes and
  the pipeline is still running, tail switches to the next step (preferring
  one already running, else the next pending one) and prints a fresh header,
  following the whole pipeline in one command.
- **`--line-numbers` on `ci watch --logs` and `ci tail`** — dim line-number
  gutter on streamed output; numbering continues across chunk boundaries and
  resets per step when auto-advancing.
- **`--from-offset <BYTES>` on `ci watch` and `ci tail`** — resume streaming
  from a byte offset after a dropped connection instead of re-fetching the
  whole log.

### Fixed

- **`bbr auth status` truthfulness** — the command no longer claims "no
  Bitbucket credentials found" (and exits 0) when credentials actually exist
  but the API call failed. No-credentials is now reported as a status (exit 0);
  real API failures surface the actual error with the correct non-zero exit
  code so scripts can detect them.
- **`bbr pr diff --raw --json` corrupted stdout** — `--json` now wins over
  `--raw` / `--name-only` / `--name-status` and always emits the single
  structured JSON document. Previously raw diff text was appended after the
  JSON, producing invalid output.
- **`bbr status --export slack --json` dropped `--json`** — the export path
  now honors `--json` and emits the machine-readable shape instead of always
  printing the Slack text.
- **Usage errors exit 64, not 2** — clap usage errors (unknown flags, invalid
  values) previously exited with `2`, colliding with the documented "auth
  failure" exit code. They now exit `64` (BSD sysexits) so scripts can tell
  "you typed it wrong" from "the operation failed".
- **`bbr completion` panicked on broken pipe** — `bbr completion fish | head`
  panicked inside `clap_complete` on EPIPE. Completions are now generated into
  a buffer and written with graceful EPIPE handling (silent exit 0).
- **`--no-unicode` leaks** — hardcoded Unicode glyphs in `ci` formatters
  (arrows, box-drawing, step-transition corners), `ci_schedules` spinner
  messages, audit/stack icons, update glyphs (incl. the no-checksum warning),
  dashboard arrows, status branch arrows, export bullets/arrows, auth setup
  checkmarks, the API error-envelope scope table (`✓`/`─`), and truncation
  ellipses now respect the theme's unicode setting.
- **Errors under `--json` were human text on stdout** — a failed command with
  `--json` now emits a structured error object
  (`{"error": {"kind", "message", "exit_code"}}`) on stderr with stable
  machine-readable `kind` values, and nothing on stdout.
- **`bbr issue view --comments` interleaved stderr with JSON** — human
  comments are now folded into the single stdout block; `--json` emits exactly
  one combined `{"issue", "comments"}` document (previously the issue JSON was
  printed first and the combined document second, producing invalid output for
  JSON parsers).
- **`bbr batch merge-approved` silently closed source branches** — source
  branches are no longer closed unless `--close-source-branch` is passed;
  drafts are skipped; fetch limit follows `--max`.
- **`--workspace`/`--slug` with unresolvable counterpart** — partial overrides
  now produce a clear error instead of a half-empty repo identity that
  generated confusing API 404s.
- **`bbr pr dashboard` silent 15-repo scan cap** — the cap is now surfaced to
  the user and `repo_count` reports the repos actually scanned.
- **`bbr update` installed to the wrong directory** — the install directory is
  now derived from the running binary's real path (covers `cargo install`),
  falling back to `~/.local/bin`.
- **`install.sh` used unconditional `sudo`** for completions — completions now
  prefer user-local dirs and only fall back to system dirs when writable.
- **`bbr workspace list --role` accepted arbitrary values** — the role filter
  is now validated to `member | contributor | admin`.
- **Parallel pagination scrambled result order** — `paginate_from` fetches
  pages 2..N concurrently, but appended them in *completion* order instead of
  page order, so any fetch spanning multiple pages (PR lists, CI lists,
  commits) could return rows in nondeterministic order. Pages are now tagged
  with their index and reassembled in order. Covered by a regression test that
  forces out-of-order completion.
- **`bbr pr reviewer add/remove` dropped all existing reviewers** — the
  `get_pr` field projection omitted `reviewers.uuid`, so the add/remove logic
  (which rebuilds the reviewer list from that response) silently discarded
  every existing reviewer on each PUT. `reviewers.uuid` is now requested.
- **`bbr api --paginate` emitted double-encoded JSON** — the paginated values
  were serialized to a string and then serialized *again* by the JSON
  formatter, producing a JSON string containing escaped JSON. The response is
  now kept as a `Value` end-to-end and printed once.
- **`bbr pr stack land` deleted other stacks' config** — on full success the
  command removed the entire `stack.toml`, wiping every other stack defined in
  the same file. It now removes only the landed stack (and clears `active` if
  it pointed at it), deleting the file only when no stacks remain. Covered by
  unit tests.
- **`bbr pr stack rebase` left the repo mid-rebase on conflict** — a failed
  rebase previously left the working tree on the switched branch with an
  unfinished rebase. The rebase is now aborted on failure and the original
  branch is restored, with the recovery noted in the error message.
- **`bbr status --watch` never refreshed branch/commit** — the process-cached
  HEAD meant new commits and branch switches were invisible until restart. The
  watch loop now re-reads HEAD from git on every tick.
- **`bbr repo fork` derived the workspace by splitting `full_name`** — it now
  prefers the API's `workspace.slug` field, falling back to the old parse.
- **`bbr issue list` indexed a parallel vec by position** — replaced fragile
  `out[i]` indexing with `zip`, so the two lists can't silently drift.
- **`--word-diff` was accepted but ignored** — `pr view` and `pr diff` bound
  the flag as `_` and only honored `--no-word-diff`. The explicit flag is now
  wired through (`word_diff || !no_word_diff`); word-diff remains on by
  default.

### Security

- **`bbr update` now fails closed on missing checksums** — a release without a
  usable `checksums.txt` entry for the target asset is refused instead of
  installed with only a warning. Opt out explicitly with `BBR_SKIP_CHECKSUM=1`.
- **`bbr update` probes install-dir writability before downloading** — a
  permission problem now fails fast with an actionable message (suggesting
  `sudo bbr update`) instead of erroring after a multi-MB download.

### Changed

- `-v` help text corrected: `-v` = debug, `-vv` = trace (the code already
  behaved this way; docs now match).
- **CI log watching UX** — `ci watch --logs` and `ci tail` now highlight
  error/warning lines (red/yellow) when colors are enabled; piped output is
  byte-identical. When parallel steps stream at once, `ci watch --logs` tags
  each line with its step name so interleaved output stays readable. The
  `ci watch` spinner now shows state · current step · elapsed time (anchored
  to the pipeline's `created_on`, correct even when attaching mid-run).
- **`bbr update` shows a download progress bar** — the binary download now
  streams with an indicatif progress bar (bytes, ETA) instead of looking like
  a hang; hidden under `--quiet`/`BBR_QUIET`.
- **`bbr status --watch` header shows a timestamp** — each refresh prints the
  current time so it's obvious the display is live.
- **Small diffs skip the pager** — `pr diff` output that fits on one screen is
  printed with `bat --paging=never` (still syntax-highlighted) instead of
  always entering the pager.
- **Bounded API fan-out** — `pr dashboard` (repo scan) and `ci compare`
  (test-report/test-case fetches) now cap concurrent requests instead of
  unbounded `join_all`, reducing rate-limit spikes.
- **`bbr ci compare <build-number>` uses a server-side query** — resolving a
  pipeline by build number now issues `?q=build_number=N` (one request)
  instead of listing up to 1000 pipelines and scanning client-side.
- **Default `bbr` runs the update check concurrently** — the background
  version check now overlaps with the overview fetch instead of adding its
  latency on top.
- **Stack config path is cached per process** — `stack.toml` location
  resolution no longer shells out to git on every load/save.
- **`bbr status --watch` error rendering** — watch-mode errors now render with
  the same hint/theme system as other commands (`display_error`) instead of a
  bare `bbr: {error}` line.
- **`bbr pr diff --raw`** — raw diff output is now printed directly to stdout
  (`print_block`) rather than routed through the bat/pager path, so it
  pipelines cleanly when stdout is not a TTY.

### Testing

- **12 new CLI smoke tests** covering: usage-error exit 64, help/version exit
  0, `pr merge --yes` help, structured `--json` errors on stderr, human error
  hints, auth-status truthfulness (no creds → exit 0, bad creds → non-zero,
  `--json` validity), `pr diff --raw --json` single-doc integrity,
  `status --export slack --json` JSON output, workspace role validation, and
  completion EPIPE non-panic.
- **Parallel-pagination ordering regression test** — forces out-of-order page
  completion (slow page 2, fast page 3) and asserts rows come back in page
  order.
- **`stack land` config-preservation unit tests** — landing one stack keeps
  sibling stacks, clears `active` only when it pointed at the landed stack,
  and partial failure retains unmerged PRs.
- **CI log-watching UX tests** — log-line classification (error/warn/normal,
  zero-count summaries ignored), chunk highlighting preserves bytes and
  trailing partial lines when colors are off, step-tag truncation is bounded,
  and pipeline elapsed time anchors to `created_on` with a local fallback.
- **Smart failure extraction tests** — first-error windowing with context,
  tail append + omission marker for early errors, fallback to `None` when no
  error lines exist, and window clamping at log start.
- **Line-numbered streaming tests** — per-line numbering, numbering
  continuity across chunk boundaries, and no spurious number on partial-line
  continuations.

### Docs

- `USAGE.md`, `README.md`, and `docs/output-schema.md` updated: exit code 64,
  corrected `--verbose` semantics, `pr diff --json` precedence, and the new
  `batch merge-approved --close-source-branch` flag.

## [0.2.1] - 2026-07-25

### Added

- **`bbr context` command group** — manage named workspace/repo profiles for quick switching:
  - `bbr context create <name> --set-workspace <ws> [--set-slug <repo>] [--set-active]`
  - `bbr context list` (active context marked with `*`)
  - `bbr context use <name>` (switch active context)
  - `bbr context delete <name>`
  - Active context supplies workspace/slug defaults to all commands when no `--workspace`/`--slug`
    flags are passed and no git remote is detected. Priority: CLI flags > active context > git remote.
  - All commands support `--json`.
  - Contexts stored in `~/.config/bbr/config.toml`.
- **`bbr ci tail`** — stream step logs in real time from a running pipeline using HTTP Range
  requests (`Range: bytes={N}-`). Defaults to the first in-progress step of the latest
  pipeline on the current branch. Supports `--pipeline <uuid>`, `--branch <name>`,
  `--interval <secs>` (default 3s), and `--follow` (keep polling after the step reaches
  a terminal state to catch flushing logs). Accepts step UUID or step name as positional arg.
- **`bbr ci watch --logs` live log streaming** — the `--logs` flag on `bbr ci watch` now
  streams logs from all running steps in real time alongside the pipeline-state polling loop,
  instead of only dumping the tail on failure. Each step's new output is printed with its
  name header and line count. Log tail dump on failure is suppressed when `--logs` is active
  (since everything was already streamed).
- **`BitbucketClient::send_raw_range`** — issues `Range: bytes={N}-` GET requests with full
  retry support (429/5xx), handling `416 Range Not Satisfiable` as an empty-body signal.
  ETag conditional GET is bypassed for range requests since content is append-only.
- **`PipelineStep::is_terminal`** — checks whether a step has reached a terminal state
  (SUCCESSFUL, FAILED, ERROR, STOPPED), used by both `tail` and `watch --logs` to know
  when to stop polling.
- **`step_log_range`** in `pipeline.rs` — thin API wrapper that delegates to
  `send_raw_range` for the step log endpoint.
- **Partial-line handling** — when a range response ends mid-line, the byte offset backs up
  to the last newline so the next poll fetches the complete line. This prevents garbled output
  from split ANSI sequences or multi-byte characters.
- **Rich UI output for log streaming** — extracted formatting functions with dedicated test coverage:
  - `render_tail_header` — `==> StepName :: #42 :: uuid :: IN_PROGRESS` header with build number
  - `render_step_transition` — inline state change display: `── [~] → [ok] SUCCESSFUL`
  - `render_watch_step_header` — box-drawn step header: `┌ StepName [~] IN_PROGRESS`
  - `render_watch_log_line` — gutter-indented log line: `│ cargo build --release`
  - `render_tail_exit_summary` — final summary with elapsed time: `── Build [ok] SUCCESSFUL (42.5s)`
  - `StepLogState` struct for stateful log tracking (offset, consumed, prev_state, header_printed)
  - State transitions printed inline without scrolling past user's visible area

### Testing

- **15 new unit tests** for API error handling, pagination, and raw body retrieval.
- **6 new integration tests** for `send_raw_range` (partial content, byte offsets, 416 empty, auth header, 401 error, 429 retry).
- **18 new UI formatting unit tests** — every visual rendering function is independently tested:
  - header formats, field separators, step name inclusion
  - transition glyph ordering (old → new), state-specific glyphs
  - box-drawing characters, vertical bar gutter indentation
  - elapsed time formatting (whole seconds, sub-second, exact 1.0s)
  - newline exclusion contract (all formatters return raw strings without `\n`)
- Total: 286 unit + 66 integration + 12 smoke = **364 tests, all passing**.

## [0.2.0] - 2026-07-22

### Fixed

- **`bbr status` / commit statuses** — HTTP 404 "Commit not found" on unpushed HEAD no longer aborts the command; treated as empty status list.
- **`bbr pr diff` docs** — USAGE/README no longer claim side-by-side is deferred or that pretty mode has syntax highlighting; `--no-syntax` removed (it was a no-op).
- **Pretty diff background tinting** — mid-line styles no longer emit `\x1b[0m` (which cleared the row background); nested SGR keeps the tint through padding.
- **`diff --git` paths with spaces** — quoted C-style paths (and `---/`+++` quotes) parse correctly instead of splitting on the first space.
- **`\ No newline at end of file`** — preserved on the preceding line and shown in pretty diffs (was dropped silently).
- **Pretty diff tab alignment** — tabs expand to tabstop 8 for width/truncation so columns stay aligned.
- **Pure rename cue** — 100% renames with no hunks show `(renamed with no content change)` instead of a bare header.
- **Diff header fill width** — trailing `─` fill uses plain (no-ANSI) Unicode width so colored headers don't over/under-fill.
- **Terminal width** — measured once per render pass (not process-lifetime `OnceLock`), so resize mid-session is respected.
- **`git rev-parse --verify --`** — branch existence checks pass `--` so names that look like options are safe.

### Added

- **Release `checksums.txt`** — release workflow publishes SHA256 sums for all archives; `install.sh` verifies before extract (warns and continues if the asset is missing on older releases).
- **`bbr pr merge-check`** — report mergeability (conflicts, draft/state, failing/pending statuses, change requests).
- **`bbr pr add-reviewer` / `remove-reviewer`** — update reviewers on an existing PR (username or UUID).
- **`bbr repo default-reviewers list|add|remove`** — manage repository default reviewers.
- **`BitbucketClient::from_credentials`** — preferred client factory; removed `Credentials::into_client` coupling.
- **`table_or_empty`** — consistent "No X found" human messages for empty list commands.
- **PR lifecycle + `prs_for_branch` API tests** — create/approve/merge/decline and status branch lookup covered with wiremock.
- **`bbr pr stack use <name>`** — select which stack `add`/`list`/`rebase`/`land`/`abort` operate on (stored as `active` in `.bbr/stack.toml`; legacy configs without `active` still use the first stack).
- **`Paginated<T>` now implements `Default`** — empty page helper without `T: Default` bound.
- **`bbr pr diff` optional ID** — omit the PR id to resolve the open PR for the current branch (same as `diffstat` / `patch`).
- **Binary file marker in pretty diffs** — `Binary files … differ` entries show `(binary file changed)` and expose `"binary": true` in `--json`.
- **`bbr pr diffstat` human table** — Status / Path / + / − table with totals instead of pretty-printed JSON.
- **`bbr pr view --side-by-side` / `--context`** — inline diff options (side-by-side implies `--diff`).
- **`bbr pr diff --name-only` / `--name-status`** — path listing modes (git-compatible).
- **`bbr pr diff -- PATH…`** — pathspec filters (exact, prefix, basename, `*`/`?` globs) after `--`.
- **Streaming pretty diffs** — `render_to` / `write_paginated` write file-by-file into the pager to cut peak memory on large PRs.
- **`--word-diff` / `--no-word-diff`** — toggle intra-line word highlighting on `pr diff` and `pr view` (on by default).
- **Syntect syntax highlighting** — pretty diffs colorize code by file type; disable with `--no-syntax`.
- **Streaming `pr view --diff`** — header/comments stream with the diff instead of buffering the full view string.

### Changed

- **Command formatters** — handlers use `make_formatter(g)` so `--no-pager` is honored consistently (not only `--json`).
- **Color flag docs** — documented precedence: `--no-color` > `--color` > env (`CLICOLOR_FORCE` / `NO_COLOR` / `CLICOLOR`) > TTY.
- **README** — comprehensive rewrite with full command reference, API scopes table, scripting patterns, output/theme docs, and conventions.
- **Pretty diff line numbers** — width scales to the largest line number in each file (no longer hard-capped at 4 columns).
- **`DiffRenderOptions`** — dropped unused `syntax_highlight` field.
- **Unified pretty diffs** — long lines truncate with `…` to the terminal width (same helper as side-by-side).
- **Word-diff line pairing** — deletions/additions are matched by similarity (not index zip), so reordered edits highlight the right counterparts.

## [0.1.9] - 2026-07-20

### Added

- **`bbr ci list --no-steps`** — skip per-pipeline step fetches for a fast listing path.
- **HTTP/2 + gzip** — reqwest client enables `http2` and `gzip` for lower latency and smaller payloads.
- **ETag conditional GETs** — in-process ETag + body cache with `If-None-Match` for watch/poll loops.
- **Rate-limit tracking** — parses `X-RateLimit-Remaining`; warns when low; exposed via `bbr auth status`.
- **`fetch_paginated` / `paginate_from` helpers** — shared pagination with parallel `page=N` only when the API `next` URL proves numeric paging (and `size` is known).
- **`prs_for_branch`** — fetch all open PRs for a branch (status/overview render each one).
- **Batch API pacing** — 200ms between batch mutations (1s when quota is low).

### Changed

- **Overview / status** — full reviewer payload (no light-PR path for overview); spinner covers the whole fetch; recent open PRs list up to 25.
- **Retry path** — shared `with_retries` helper; retries 502/503/504 as well as 429.
- **`auth_header` stored as `SecretString`** — zeroized on drop like the credential secret.
- **Git timeouts** — `wait-timeout` instead of a 50ms busy-wait poll loop.
- **`truncate`** — uses Unicode display width (CJK-safe) via `unicode-width`.
- **Watch clear-screen** — gated on stdout TTY, not `colors_enabled` / `CLICOLOR_FORCE`.

### Fixed

- **`ensure_uuid_braces`** — partial braces (`{abc` / `abc}`) are normalized instead of left malformed.
- **`merge_pr`** — sends no body when merge options are absent (no forced `{}`).
- **Empty JSON success bodies** — deserialize via `null` then `{}` instead of hard-failing.
- **Approvals display** — honors Bitbucket `state: "approved"` in addition to the `approved` bool; reviewer `state` included in API `fields`.
- **`list_prs` BadRequest fallback** — reuses the first fetched page (no double-fetch); pagination metadata kept in `fields=`.
- **Protected-branch cleanup logic** — `release/` / `hotfix/` checks are no longer incorrectly nested inside the named-branch `.any()`.
- **Permissive credentials file** — auto-fixes mode to `0600` after warning instead of only suggesting `chmod`.
- **Diff/patch Accept headers** — negotiate `application/x-diff` / `application/x-patch` with `text/plain` fallback.
- **Pipeline steps sort** — dropped non-standard `sort=order`.

## [0.1.8] - 2026-07-10

### Added

- **`bbr variable` top-level command** — manage repository pipeline variables with `bbr variable list|set|delete`. Provides a more discoverable entry point to the same operations available via `bbr ci vars`. (#38)
- **`bbr deploy-keys` command group** — full CRUD for repository SSH deploy keys: `list`, `add --key <pubkey> --label <name>`, `view <key_id>`, `delete <key_id> [--yes]`. All subcommands support `--json`. (#19)
- **`bbr ci schedules` command group** — manage pipeline schedules (cron-based triggers) with 6 subcommands: `list`, `create --cron <expr> --branch <name>`, `view <uuid>`, `update <uuid> [--cron] [--enabled]`, `delete <uuid> [--yes]`, `executions <uuid>`. (#12)
- **`--max` safety cap on batch operations** — all `bbr batch` subcommands (`merge-approved`, `rerun-failed`, `cleanup-merged-branches`) now accept `--max <n>` to limit the number of items processed, preventing accidental bulk operations in automation.
- **`secrecy` crate for credential storage** — `Credentials.secret` is now a `SecretString` that is automatically zeroized on drop, preventing credential leakage in memory dumps or core files.
- **Terminal escape sequence sanitization** — diff content from the API is now sanitized to strip ANSI/CSI/OSC escape sequences at parse time, preventing terminal escape injection attacks via malicious diffs.
- **Cached terminal width** — `terminal_width()` is now queried once per process via `OnceLock` instead of per-line during diff rendering, eliminating repeated `ioctl` syscalls.

### Changed

- **`cli.rs` split into `cli.rs` + `dispatch.rs`** — the 1802-line monolithic CLI file has been split into type definitions (1247 lines) and dispatch routing (600+ lines) for better maintainability.
- **Self-update integrity warning** — when no checksums asset is available in a GitHub release, `bbr update` now prints a visible warning to the user instead of silently skipping verification.

### Fixed

- **[SECURITY] Orphan git process leak** — `git_with_timeout` now uses a polling loop with explicit `child.kill()` on timeout instead of spawning a thread that could never reap the child process.
- **[SECURITY] URL path injection in source endpoints** — `git_ref` and `path` parameters in `get_file_raw` and `list_src` are now properly URL-encoded segment-by-segment.
- **Serialization error in `merge_pr`** — previously swallowed with `unwrap_or_else(|_| "{}".into())`; now propagates the error via `Result`.
- **Redundant `client()` construction in `run_overview`** — the API client is now reused from the initial `fetch_branch_status` call instead of being reconstructed.
- **Redundant `&` in `format!` arguments** — fixed 4 `clippy::useless_borrows_in_formatting` lint errors caught by Rust 1.97.

### Testing

- 3 new escape sanitization tests (`test_sanitizes_terminal_escape_sequences`, `test_sanitizes_osc_sequences`, `test_sanitize_preserves_clean_content`).
- 4 new pipeline variable tests (`list_repo_pipeline_variables`, `create_repo_pipeline_variable`, `update_repo_pipeline_variable`, `delete_repo_pipeline_variable`).
- 6 new deploy-keys tests (`list_deploy_keys`, `add_deploy_key`, `get_deploy_key`, `delete_deploy_key`, + 2 more).
- 4 new CI schedules tests (`list_schedules`, `create_schedule`, `get_schedule`, `delete_schedule`).
- Total: 221 unit + 41 integration = **262 tests, all passing**.

- **Pretty diff renderer UX overhaul** — 10 production-grade improvements to `bbr pr diff` output:
  - **Full-line background tinting** — addition lines get a subtle dark-green background (`48;5;22`), deletion lines get dark-red (`48;5;52`), spanning the full terminal width for instant scannability.
  - **Sign column** — colored `+`/`-` glyphs in the gutter between line numbers and separator, improving accessibility and skim-readability (especially for colorblind users).
  - **Interleaved paired lines** — unified mode now outputs deletion/addition pairs consecutively (del1→add1→del2→add2) instead of all deletions then all additions, making word-diff highlights spatially adjacent.
  - **Inline compact file header** — replaced the multi-line box-drawing header with a single-line format: `── ~ path ── modified ── [████░░░░] +X, -Y ──` with a proportional stats bar.
  - **Word-diff similarity threshold** — lines that differ by more than 70% (similarity < 0.30) skip word-level highlighting and render as plain colored, avoiding the "everything highlighted" noise on full rewrites.
  - **Context lines at normal weight** — context line content is now rendered at normal weight; only line numbers and the `│` separator are dimmed, improving code readability.
  - **Colorized summary with proportion bar** — insertion count is green, deletion count is red, with a 12-char `[████████░░░░]` bar showing the addition/deletion ratio.
  - **Empty line markers** — blank added/deleted lines now show a visible `⏎` (unicode) or `<CR>` (ASCII) marker instead of invisible whitespace.
  - **File index for multi-file diffs** — diffs with 2+ files now show a table-of-contents at the top with numbered paths and colored `+X, -Y` stats.
  - **Continuous vertical divider in side-by-side** — left-edge `│` pipe on every row plus consistent dim center separator for clear two-pane structure.
- **`word_diff::similarity()` function** — computes word-level similarity ratio (0.0–1.0) between two strings using the `similar` crate's ratio method.
- **`word_diff::WORD_DIFF_THRESHOLD` constant** — configurable threshold (0.30) below which word-level highlighting is skipped.

### Changed

- **Diff renderer `dim()` helper switched from `colored` crate to direct ANSI** — avoids `colored::Colorize` import in the renderer, using `\x1b[2m...\x1b[0m` directly for consistency with other escape sequences in the module.

### Testing

- 11 new renderer tests: `test_render_colored_background_tinting`, `test_render_sign_column`, `test_render_interleaved_pairs`, `test_inline_file_header`, `test_render_empty_line_marker`, `test_render_multiple_files_has_index`, `test_render_summary_colored`, `test_context_lines_not_dimmed`, `test_side_by_side_left_edge_pipe`, `test_theme_colored` helper.
- Total: 218 unit + 31 integration = **249 tests, all passing**.

## [0.1.7] - 2026-07-07

### Added

- **Word-level highlighting inside side-by-side diff mode** — paired change lines in side-by-side layout now display fine-grained red/green highlights for deleted/inserted words.
- **SHA256 checksum verification for self-update** — `bbr update` now downloads and verifies a `checksums.txt` release asset (if present) against the downloaded archive before installation, protecting against supply-chain attacks and corrupted downloads.
- **Async git wrappers** — all blocking git operations (`fetch_branch`, `checkout_branch`, `push_branch`, `push_force_with_lease`, `delete_branch_local`, `delete_branch_remote`, `rebase_branch`) now have `_async` variants using `tokio::task::spawn_blocking`, preventing tokio runtime thread stalls.
- **Credential file permission warning** — on Unix, `bbr` now warns to stderr if `credentials.toml` has group/other-readable permissions (mode > 0600) with a `chmod 600` suggestion.

### Fixed

- **`git branch -d` local deletion in `batch.rs` no longer blocks Tokio worker threads** — refactored manual `git` subprocess spawning to use a shared non-blocking `git::delete_branch_local_safe()` helper with built-in timeout protection.
- **Browser opening in `open.rs` no longer blocks Tokio worker threads** — wrapped blocking browser launch `opener_command().status()` in a `tokio::task::spawn_blocking` task.
- **[SECURITY] Git argument injection** — all git commands now use `--` separator before user-supplied branch/ref names, preventing branch names starting with `-` from being interpreted as git flags.
- **[SECURITY] Pagination infinite loop** — added empty-page guard (`if page.values.is_empty() { break }`) to both the generic `fetch_all_pages` sequential fallback and the `list_prs` manual pagination loop, preventing infinite loops when the API returns empty pages with `next` URLs.
- **[SECURITY] BBQL query injection** — issue and PR query parameters now reject values containing double-quotes with a clear error message, preventing Bitbucket Query Language injection.
- **Diff parser multi-hunk bug** — intermediate hunks in multi-hunk files are now correctly stored (previously only the last hunk per file was preserved; line counts were accurate but rendered output was incomplete).
- **`list_prs` pagination double-URL** — replaced `unwrap_or(url)` with `strip_base()` in the PR list pagination fallback, preventing corrupted URLs when `strip_prefix` fails.
- **`send_empty` unnecessary deserialization** — DELETE/PUT endpoints returning 204 No Content no longer attempt to deserialize the empty response body as JSON.
- **Stack config relative path** — `.bbr/stack.toml` is now anchored to the git repository root via `git rev-parse --show-toplevel`, fixing stack commands when run from subdirectories.
- **`truncate_mid` UTF-8 panic** — the diff renderer's mid-string truncation now uses character-boundary-safe iteration instead of byte-level slicing, preventing panics on multi-byte filenames.
- **`PipelineFailed` error display** — now shows `"pipeline #42 failed on main"` instead of the generic `"pipeline failed"`, including build number and branch when available.
- **Whitespace-only credentials** — username and token values are now trimmed before validation; whitespace-only strings are correctly rejected.
- **`detect_repo()` error message** — changed from the misleading "no bitbucket.org remote found" to "no git remote found" since the parser accepts any git hosting provider.
- **Deploy/webhook list truncation** — `list_deployments`, `list_environments`, and `list_webhooks` now paginate through all results instead of silently truncating at 100 items.
- **Theme override race condition** — `set_color_override()` and `set_unicode_override()` now use `AtomicU8` checked before `OnceLock` initialization, and log a `tracing::warn!` if called too late.

### Changed

- **MSRV bumped from 1.75 to 1.88** — enables use of latest dependency versions without pinning; aligns with the edition 2024 ecosystem.
- **`time` crate unpinned (0.3.36 → 0.3.53)** — resolves RUSTSEC-2026-0009 (DoS via stack exhaustion in time parsing).
- **`tempfile` crate unpinned (3.15 → 3.27)** — no longer blocked by `getrandom` 0.4 requiring edition 2024.
- **Clippy `is_none_or` lint fix** — replaced `map_or(true, …)` with idiomatic `is_none_or(…)` in API error field filtering.
- **`CLICOLOR` / `CLICOLOR_FORCE` support** — the theme color detection now implements the full CLICOLOR spec: `CLICOLOR_FORCE` (non-"0") forces colors on; `NO_COLOR` disables; `CLICOLOR=0` disables; otherwise colors are enabled only on TTY. Previously only `NO_COLOR` was checked.
- **Custom base64 replaced with `base64` crate** — HTTP Basic auth encoding now uses the well-tested `base64` 0.22 crate instead of a hand-rolled implementation, eliminating maintenance risk in auth-critical code.
- **Rate limit retry honors `Retry-After` header** — when Bitbucket returns a 429 with a `Retry-After` header, the retry delay now uses the server-specified duration instead of fixed linear backoff.
- **Parallel page fetches capped** — `fetch_all_pages` now uses `buffer_unordered(10)` instead of unbounded `try_join_all`, preventing OOM and rate limit bombardment on large paginated results.
- **Stacked PR commands use async git** — `bbr pr stack add/rebase/land/abort` and `bbr batch cleanup-merged-branches` now use non-blocking async git wrappers, no longer stalling the tokio runtime.
- **`status.rs` deduplicated** — extracted shared `fetch_branch_status()` helper, removing ~80 lines of duplicated logic between `run_inner()` and `run_overview()`.

### Performance

- **Bounded concurrency for `bbr pr stack list` status checks** — replaced the unbounded `join_all` concurrency loop with a capped `buffered(5)` stream to prevent rate limit spikes on large PR stacks, while keeping the parent-child ordering intact.
- **Parallel page fetch bounded to 10 concurrent requests** — prevents spawning thousands of simultaneous HTTP requests for large paginated results.

## [0.1.6] - 2026-07-04

### Added

- **Intra-line word-level highlighting** — integrated word-level tokenization and diffing (using the `similar` crate) into the pretty diff renderer. Changed lines now highlight modified words with distinct background colors (reverse video green/red).
- **Side-by-side diff rendering mode** — implemented the side-by-side layout option for diff displays, splitting the terminal width between old and new files with clean vertical boundary lines.

### Fixed

- **`--color` flag now accepts `auto|always|never`** — was a boolean `SetTrue` flag that accepted no
  value, disagreeing with the README, USAGE.md, and `git`-style conventions. Now a proper
  `ValueEnum` with three variants. `--color always` forces color even when piped; `--color never`
  disables it; `--color auto` (default) continues to detect TTY + `NO_COLOR`. `--no-color` is
  kept as a shorthand for `--color never`.
- **`-v` now logs at `debug`, not `info`** — `--verbose 0` (no flag) was logging at `warn`
  while the docs said `info`. Fixed: no flag → `info`, `-v` → `debug`, `-vv` → `trace`.
- **`make_spinner` used arg-scanner instead of `GlobalArgs.quiet`** — `is_quiet()` scanned
  `std::env::args()` for `--quiet`/`-q` literals, bypassing clap's normalization. It now reads
  the `quiet` field on `GlobalArgs` directly.
- **`resolve_body` blocked a Tokio worker thread on stdin** — `--body-stdin` called
  `std::io::stdin().read_to_string()` synchronously inside an async function. Now uses
  `tokio::task::spawn_blocking` so the runtime thread is not stalled.
- **`bbr status --watch` emitted raw ANSI escapes regardless of `--no-color`** — screen-clear
  sequences (`\x1B[H\x1B[J`, `\x1B[2J\x1B[H`) were hardcoded. Now gated on
  `Theme::colors_enabled()`; falls back to a plain separator line when color is off.
- **`terminal_width()` spawned a `stty` subprocess** — replaced with a `TIOCGWINSZ` ioctl on
  Unix (instant, no subprocess) and a `$COLUMNS` env-var fallback that works everywhere
  including Windows. Eliminated duplicate `stty` spawning in `src/diff/renderer.rs` by routing to the shared helper.
- **`confirm` prompt no longer blocks Tokio worker threads** — wrapped blocking stdin `read_line`
  in `tokio::task::spawn_blocking` across all subcommands.
- **Dynamic window resizing in `--watch` loops** — removed the static `OnceLock` caching from
  the shared `terminal_width()` helper, allowing terminal geometry changes to be captured dynamically.
- **`BITBUCKET_TOKEN` set-but-empty was silently ignored** — now emits a `tracing::warn` message
  pointing to the API token creation page, matching the existing warning for an empty username.

### Changed

- **`libc` added as an explicit Unix-only dependency** — used for the `TIOCGWINSZ` ioctl in
  `terminal_width()`. Was already a transitive dependency; now declared explicitly with
  `[target.'cfg(unix)'.dependencies]`.

### Performance

- **Bounded concurrency for `bbr ci list` pipeline steps** — replaced the unbounded `join_all`
  concurrency loop with a capped `buffer_unordered(5)` stream to prevent 429 Rate Limit spikes when listing large pipeline sets.

### UX

- **`bbr pr view --diff` pretty rendering** — now formats the pull request diff with the custom
  box-drawn and line-numbered renderer, matching the layout of `bbr pr diff`.
- **Spinner always cleared on error paths** — introduced `SpinnerGuard`, a RAII wrapper around
  `indicatif::ProgressBar`. All spinner locals across every command file are now wrapped; the
  `Drop` impl calls `finish_and_clear()` automatically, so early `?`-returns on errors no longer
  leave a dangling spinner on the terminal.
- **Table text columns capped at 60 characters** — PR titles, descriptions, names, and similar
  free-text columns now have an `UpperBoundary(Width::Fixed(60))` constraint applied by the
  `Table` wrapper. A 200-character PR title no longer blows out the terminal width.
- **`bbr pr diff` hints when `bat` is not installed** — previously fell back silently to `less`.
  Now prints a single `hint:` line pointing to the bat install page when the binary is not found,
  then continues with the plain pager.

### Testing

- New tests: `make_spinner_hidden_in_quiet_mode`, `title_column_constraint_applied`,
  `resolve_body_direct` / `resolve_body_errors_without_source` converted to async
  `#[tokio::test]`.
- Total: 196 unit + 22 integration + 9 smoke = **237 tests, all passing**.

## [0.1.5] - 2026-07-03

### Changed

- **Unified polling interval flags** — changed `--interval-secs` to `--interval` in `bbr ci watch` to match `bbr status --watch --interval`.
- **Enriched PR status output** — enriched `bbr status` output with diffstats (`+N, -N`), relative time "opened N days ago", description first line excerpt, reviewer approval annotations (✅ / ⏳ / ❌), and a merge readiness row showing approvals, CI status, and conflicts.
- **Smarter suggested commands** — status output suggestions now dynamically adapt to PR state (open, merged, declined, unapproved, changes requested) and pipeline state (failing, running, successful, none), ordered by urgency (merge > fix CI > approve > view).
- **Optimized batch merge approvals** — retrieved reviewers and participants data directly in `bbr batch merge-approved` listing, completely eliminating N+1 `get_pr()` calls.
- **Concurrent stack listing** — parallelized pull request status fetching in `bbr pr stack list` using asynchronous joins, resolving sequential fetch bottlenecks.
- **Concurrent page fetching** — parallelized multi-page fetches in `fetch_all_pages` using asynchronous page requests based on total counts.
- **Cached default branch inference** — cached repository default branch results per process lifetime in `bbr pr create`.
- **Targeted fields fallback** — list fallback query retries are now restricted to 400 Bad Request responses, preventing duplicate requests on transient/rate-limiting errors.

### Fixed

- **Spinner respects CLI `--quiet` flag** — global `--quiet` / `-q` CLI flags now properly suppress the steady tick spinner.
- **Table rendering respects `NO_COLOR`** — pretty table outputs now respect the `NO_COLOR` env var via the global `Theme` singleton, and fallback to `presets::ASCII_FULL` when Unicode is disabled.
- **Fixed workspace list deprecation** — migrated `bbr workspace list` from the deprecated cross-workspace `/workspaces` endpoint to the supported `/user/permissions/workspaces` endpoint and updated the deserialization schema.

## [0.1.4] - 2026-07-02

### Added

- **Pretty diff renderer for `bbr pr diff`** — structured diff parser, intra-line word diffing, and a terminal renderer with box-drawing, line numbers, ANSI colors, collapsed context sections, and summary bar.
  - New flags: `--raw` (legacy bat/less), `--context N` (lines around changes), `--no-syntax`, `--side-by-side`.
  - `--json` now emits structured `files[]` / `hunks[]` / `lines[]` data instead of raw diff text.
  - New dependencies: `similar` (word-level diffing), `unicode-width` (CJK-safe column width).
- **`bbr workspace list`** — list workspaces with `--role` filter and `--limit`.
- **`bbr deploy trigger <env_uuid> --commit <hash>`** — trigger a deployment.
- **`bbr repo permissions`** — list user and group permissions for the repository.
- **`--no-unicode` global flag** — use ASCII characters instead of Unicode for terminals that don't support UTF-8.
- **`--timeout` global flag** — configurable HTTP request timeout (env: `BBR_TIMEOUT`).
- **`--var` and `--secured` flags on `bbr ci trigger`** — pass pipeline variables (repeatable `--var KEY=VALUE`).
- **Git subprocess timeouts** — all `git` commands now time out (30s reads, 120s writes) via thread-safe `recv_timeout`.
- **`Theme::empty()`, `Theme::checkmark()`, `Theme::cross()`** — standardized empty state and indicator helpers.
- **`with_timeout()` constructor on `BitbucketClient`** — programmatic timeout configuration.

### Performance

- **Parallelized N+1 API calls in `ci_compare`** — four sequential HTTP loops merged into two concurrent batches, reducing wall-clock time for multi-step pipelines.
- **Parallelized repo audit loop** — `bbr repo audit` now audits all repos concurrently instead of sequentially.
- **Parallelized per-repo PR fetches in dashboard** — open and merged PR fetches now run concurrently via `tokio::join!`.
- **Reused HTTP client for update checks** — `fetch_latest_release()` now uses a `OnceLock`-cached `reqwest::Client` instead of creating a new one per call.
- **ASCII fast-path in `truncate()`** — avoids O(n) Unicode char scan for ASCII strings.

### Fixed

- **`bbr update` no longer panics on TLS misconfiguration** — `update_client()` now returns `Result`
  instead of `expect()`, so update checks gracefully degrade when the HTTP client fails to build.
- **`git checkout_branch` locale-independent** — uses `git rev-parse --verify` exit code
  instead of matching against locale-dependent error strings, fixing failures on non-English systems.
- **`bbr batch merge-approved` overly restrictive approval check** — now accepts PRs with at least
  one approval instead of requiring all assigned reviewers to approve.
- **`bbr pr stack rebase` typo** — success message now reads "Successfully rebased" instead of "Successfully rebase".
- **`bbr ci compare` build number lookup only checked first 100 pipelines** — increased limit to
  1000 to trigger automatic pagination, correctly finding older builds.
- **Auth test mutex poison recovery** — test mutex now recovers from poisoned state via
  `unwrap_or_else(|e| e.into_inner())` instead of panicking on subsequent test failures.
- **`bbr config path/show/set` ignored `--json`** — all config commands now route through
  `Formatter::from_json_flag(g.json)` instead of being hardcoded to human mode.
- **`bbr status --export slack|markdown` used `println!`** — switched to `print_block()` to
  avoid corrupting `--json` output.
- **`bbr update` used hardcoded `✓` glyph** — now uses `Theme::checkmark()` to respect
  `--no-unicode`.
- **`commit_statuses` paginated with `pagelen=25`** — increased to `pagelen=100` for fewer API calls.

### Changed

- **Improved HTTP error messages** — 401, 403, and 404 errors now show human-readable descriptions when the API response lacks detail.
- **Body serialization optimized** — `.to_string()` replaced with `.to_owned()` in `send()` to avoid unnecessary allocation.
- `Theme` now tracks `unicode` field alongside `colors`.

### Testing

- All 222 tests pass. Clippy clean with `-D warnings`.

## [0.1.3] - 2026-06-30

### Added

- **`bbr deploy env create`** — create deployment environments via CLI.
- **`--slug` global flag** — override repo slug inferred from git remote (env: `BB_SLUG`).
  Enables `bbr status --workspace ws --slug repo` without a matching local git repo.
- **`--enable-issues` flag on `bbr repo create`** — enable issue tracker on repository creation.

### Fixed

- **`bbr batch merge-approved` failed with "Invalid pagelen"** — the `fields=` query parameter
  conflicted with `pagelen=100` on some Bitbucket account types. Now falls back to no `fields=`
  and `pagelen=50` on 400 error.
- **`bbr batch merge-approved` missed self-approvals** — only checked `role=REVIEWER`, but
  self-approvals have `role=PARTICIPANT`. Now checks `approved=true` regardless of role.

### Deprecated

- **`bbr issue`** — Bitbucket's issue tracker is not available on workspaces created after ~2024.
  All `bbr issue` commands now print a deprecation warning. Consider using Jira.

## [0.1.2] - 2026-06-30

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
- **`--no-pager` global flag** — disables output paging through `less`.
- **`--quiet` global flag** — suppresses spinners and non-essential output for scripting.
  Also respects `BBR_QUIET` env var.
- **`--color` / `--no-color` global flags** — force ANSI color output on or off.
- `Formatter::from_args()` constructor accepts `no_pager` flag.
- `make_formatter()` helper in `commands/mod.rs` for consistent flag propagation.

### Performance

- Replaced `s.push_str(&format!(...))` with `write!()` in the CI comparison renderer
  (`src/commands/ci_compare.rs`) — avoids 11 temporary `String` allocations per render.
- Removed `async` from three purely-synchronous functions (`schema::run`,
  `stack::init`, `stack::rebase`) — eliminates unnecessary future/task overhead.
- Eliminated unnecessary `.clone()` on `Option<String>` render fields in
  `pr.rs` and `status.rs` (6 call sites) — uses `as_deref().unwrap_or("-").to_string()` instead.
- Rate-limit jitter improved from 0–2s (`subsec_nanos % 3`) to 0–4s counter-based spread.
- **`bbr batch merge-approved`** — per-PR fetches parallelized with bounded concurrency
  (`buffer_unordered(10)`). Reduces wall-clock time from ~100 sequential calls to ~10 batches.

### Changed

- **`dispatch()` refactored** — the 463-line function is now a thin routing table calling
  `dispatch_status`, `dispatch_pr`, `dispatch_ci`, `dispatch_repo`, `dispatch_batch`,
  `dispatch_auth`, `dispatch_webhook`, `dispatch_deploy`, `dispatch_issue`.
- **`url_encode` consolidated** — single implementation in `api/mod.rs`, removed duplicates
  from `api/pr.rs`, `api/issue.rs`, and `commands/search.rs`.
- **Status rendering deduplicated** — `render_human` and `render_overview_human` now share
  `render_pr_section`, `render_pipeline_section`, `render_build_statuses`,
  `render_suggested_commands` helpers.
- **Race condition documented** — `update_webhook` and `update_issue` now log
  `tracing::debug` noting the inherent GET-then-PUT pattern (Bitbucket has no ETag/PATCH).
- `Theme::set_color_override()` for CLI flag integration (must be called before first
  `Theme::current()` access).

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
- `from_env()` now warns when `BITBUCKET_USERNAME` is empty but `BITBUCKET_TOKEN` is set.

### Testing

- **18 new unit tests** for `update.rs` (parse_version, is_newer, render_update),
  `ci_compare.rs` (compute_step_deltas, render_compare), and `export.rs`
  (format_slack, format_markdown).
- **9 new integration tests** (`tests/api_retry.rs`) covering:
  - Rate-limit retry: 429→200 success, exhaustion after 3×429, send_raw retry.
  - Pagination: multi-page follow, limit enforcement, single-page.
  - send_raw: success body, error mapping.
  - Error envelope: scope table parsing.
- Total test count: 153 → 203 (across unit, integration, and smoke suites).

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

[Unreleased]: https://github.com/themankindproject/bbr/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/themankindproject/bbr/compare/v0.1.9...v0.2.0
[0.1.9]: https://github.com/themankindproject/bbr/releases/tag/v0.1.9
[0.1.8]: https://github.com/themankindproject/bbr/releases/tag/v0.1.8
[0.1.7]: https://github.com/themankindproject/bbr/releases/tag/v0.1.7
[0.1.6]: https://github.com/themankindproject/bbr/releases/tag/v0.1.6
[0.1.5]: https://github.com/themankindproject/bbr/releases/tag/v0.1.5
[0.1.4]: https://github.com/themankindproject/bbr/releases/tag/v0.1.4
[0.1.3]: https://github.com/themankindproject/bbr/releases/tag/v0.1.3
[0.1.2]: https://github.com/themankindproject/bbr/releases/tag/v0.1.2
[0.1.1]: https://github.com/themankindproject/bbr/releases/tag/v0.1.1
[0.1.0]: https://github.com/themankindproject/bbr/releases/tag/v0.1.0
