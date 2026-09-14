# Production-readiness audit worklist

Status: in progress. This is an evidence ledger, not a production certification.
Review began from commit `170d5d0` plus an existing uncommitted `rpassword`
update in Cargo.toml/Cargo.lock, which this round leaves unchanged.

## Verified fixes (goal round 4)

- **Credential parser disclosure:** `read_credentials_file` formatted the TOML
  parser error into `BitbucketError::Config`, exposing source lines containing
  tokens through both human and JSON errors. It now returns only a path,
  numeric line location, and recovery instructions. It does not use even
  `Error::message()`, because parser messages can contain user-controlled keys.
- **Live config truncation:** `save_config` used `fs::write` on `config.toml`.
  It now reuses the same-directory private temporary-file replacement helper.
  This avoids partial live-file writes; it does not serialize concurrent
  read-modify-write operations or guarantee directory durability after power loss.

Evidence: `tests/config_safety.rs` failed on both defects before the implementation
change and passed afterward. Its three malformed-file cases cover syntax errors,
inline-table type errors, and duplicate secret-shaped keys, in human and JSON
modes. On Unix, an already-open reader must retain the original config after a
save. Additional unit checks cover failed-persist cleanup, replacement of an
existing file, and cache invalidation after a second save.

Verification on Linux after these changes: **536 tests passed**, zero failed,
one documentation test ignored (baseline: 531 passing). Full all-feature tests,
Clippy for all targets/features with warnings denied, `cargo fmt --check`, and
`git diff --check` passed. No new dependency was introduced.

The CLI tests use disposable home/config directories and no real credentials.
Windows replacement behavior has helper tests but has not been run on Windows
in this session. The old-reader regression is Unix-only.

## Authentication hardening (goal round 5)

- Empty noninteractive username/token inputs now fail with usage exit 64 before
  changing the credentials file.
- `auth setup --token-stdin` accepts bounded UTF-8 input (64 KiB before trimming)
  without putting the token in command arguments, and preserves the stored
  workspace. Invalid input leaves the existing file unchanged.
- Setup takes global formatting flags and emits a secret-free JSON storage
  receipt, not a claim that authentication was validated. The receipt schema is
  available through `bbr schema auth-setup`.
- Bare setup refuses prompting with JSON, BBR_NO_INTERACTIVE, or nonterminal
  stdin/stderr. Explicit token input still works noninteractively.
- `auth status` preserves the established missing-credentials result but now
  propagates parse/IO errors rather than disguising them as missing credentials.

Evidence: `tests/auth_setup.rs` initially had seven failing tests; after the core
changes all nine initial tests passed. Additional boundary and schema tests were
added; the schema test failed before its implementation. Final verification on
Linux: **548 tests passed**, zero failed, one doctest ignored; Clippy with warnings
denied, formatting, and diff checks passed. An additional Linux PTY probe verified
hidden-token interactive setup, BBR_NO_INTERACTIVE suppression, JSON suppression,
and refusal of --token-stdin on a terminal. Tests use per-child environment
overrides and throwaway homes, not real credentials or a Bitbucket account.

## Context targeting safety (goal round 6)

Repository resolution now loads the active context through one fallible helper.
Malformed/unreadable configuration, stale active names, and empty context identity
fields are errors rather than permission to select a Git-derived repository.
Providing both explicit fields (flags or environment) still bypasses configuration.
A missing file or no selected context still falls back to Git, and an omitted
context slug can still be filled from Git without replacing the context workspace.

Evidence: five of the initial eleven `tests/context_resolution.rs` tests failed
before the change. The failures included an authenticated `repo info` request
reaching the mock server instead of rejecting a stale context. All fourteen final
focused tests passed, including zero requests for a read and a mutation under
invalid context, flag/env precedence, absence of Git, and repair through context
commands. Fixture repositories and homes are temporary and use only fake tokens.
Final Linux verification: **562 tests passed**, zero failures, one ignored doctest;
all-target/all-feature Clippy with warnings denied, formatting, and diff checks
passed. Rust 1.88 is installed locally; MSRV and cross-target checks remain to run.

## Git remote identity hardening (goal round 7)

The remote parser now validates the host and original path before deriving an
identity. HTTP(S) must use bitbucket.org. SSH supports bitbucket.org,
altssh.bitbucket.org, and single-label local aliases, including explicit ssh://
URLs. Dotted non-Bitbucket hosts require explicit identity overrides. Paths must
contain exactly two safe ASCII segments; dot segments, extra slashes, percent
escapes, query/fragment data, whitespace and control characters are rejected.
Only one `.git` suffix is removed so a repository itself ending in `.git` keeps
its name. No new dependency was added (the URL parser is exposed by reqwest).

Evidence: all six `tests/git_remote.rs` tests failed before the implementation
and passed after it. The CLI fixture checks no-match errors, HTTPS and SSH
GitHub origins falling back to Bitbucket upstream, and valid-origin precedence.
The full run exposed four existing contract tests depending on this checkout's
GitHub remote being accepted; they now set explicit fake BB_WORKSPACE/BB_SLUG
values, keeping consent/transport checks independent from repository inference.

Limitation: bbr trusts single-label SSH aliases without evaluating ssh_config.
An alias pointing elsewhere can still provide the wrong identity; use an explicit
workspace/slug or a context when its destination is uncertain. This compatibility
boundary is documented in USAGE.md; there is no network alias verification.

Verification: **568 tests passed**, zero failed, one doctest ignored; Clippy
(all targets/features, warnings denied), formatting, and diff checks passed.
`cargo +1.88.0 check --locked` also passed, verifying the declared MSRV against
the current lockfile. Cross-platform runtime checks remain outstanding.

## Pagination correctness (goal round 8)

The shared paginator follows server-provided absolute next links sequentially.
It no longer infers numeric pages from a `page=` substring, duplicates an existing
page parameter, trusts size metadata over links, or stops prematurely on short/
empty pages. Limits <=100 use the same traversal. A HashSet detects repeated
normalized next paths, and a 10,000-page cap bounds unique endless links even
for internal callers using usize::MAX. HTTP retries remain bounded separately.
Next links require the same URL origin and an API path-boundary match; fragments,
userinfo, controls and escaped-dot path escapes outside the base are rejected.
Link-validation errors do not print remote query strings or embedded credentials.

Evidence: nine of eleven initial `tests/api_pagination.rs` regressions failed
before implementation; all thirteen final cases passed, including an actual
10,000-empty-page traversal (1.63s for the focused suite locally). The existing
page-order fixture now supplies the missing next link on its middle page and
still verifies stable row order with differing response latency.

Tradeoff: large numeric listings lose speculative parallel page fetches, avoiding
invented cursors and excess requests. No throughput improvement is claimed. This
also removes the extra futures/result-page collection and sorting. Returned rows
are still buffered (bounded by the requested item limit or page cap); aggregate
byte budgeting and endpoints bypassing the shared paginator remain follow-ups.

Verification: **581 tests passed**, zero failed, one ignored doctest. Full
all-feature tests, Clippy (warnings denied), formatting, diff checks, and Rust
1.88 `cargo check --locked` passed on Linux. No Bitbucket account was used.

## HTTP cache consistency and diagnostic privacy (goal round 9)

Conditional GETs now snapshot the ETag and Arc-backed body together, keyed by
path and Accept. GETs with bodies bypass the cache; mutations never populate it
and invalidate entries before/after request completion (including failure).
A shared generation prevents pre-mutation GET responses from repopulating cache.
In-flight 304s retain the exact body they validated even if entries were evicted.
Mutation 304s and unsolicited GET 304s fail rather than fabricating success.
Full responses with no validator, no-store, or unsupported Vary fields discard
old entries. Cache replacement now obeys the byte cap as well as insertions.

Verbose JSON decode logs no longer include the first 200 response characters or
query values; they retain only path, length, and numeric error position.

Evidence: eight of nine initial tests in `tests/api_cache.rs` failed before the
implementation. They include a POST succeeding from a cached GET on a 304, a
text request reusing a JSON validator, ignored no-store, stale validators after
writes, and leaked fake response secrets. All nine passed after the changes.
Further tests cover actual delayed-304/mutation overlap, unexpected 304s, entry
replacement byte caps, eviction snapshots, and generation invalidation.

Limits: already in-flight reads may still finish with a pre-write snapshot;
this cache does not serialize operations or promise transactional isolation.
The 8 MiB cache cap excludes bodies retained by in-flight requests. Full HTTP
cache semantics (e.g. metadata updates on 304) and sanitization of all other
API error/retry messages remain separate audit work, not claimed fixed here.

Verification: **596 tests passed**, zero failures, one ignored doctest; Clippy
with warnings denied, formatting, diff checks, and Rust 1.88 check all passed.

## Endpoint pagination adoption (goal round 10)

Issues, issue comments, schedule executions, and workspace lists now send a
capped pagelen but pass the original item limit to the shared paginator. Source
directories, pipeline/environment variables, and schedules follow every next
link (under the global page cap) instead of returning only page one. Output DTOs
remain unchanged. Pipeline variable set/delete callers now see later-page keys;
a failed continuation propagates before any write based on an incomplete lookup.

Evidence: ten endpoint tests failed before implementation (the workspace fixture
first required correcting an unsupported role from owner to admin). All ten then
passed, covering server over-return, short pages, limits above page-size caps,
zero-limit no-request behavior, and a CLI variable update choosing PUT for a
page-two key rather than POST. A further regression verifies no mutation when
fetching the continuation fails. All use local wiremock servers and fake tokens.

Scope: search, pipeline steps, and branch-specific PR lookup retain separate
response/limit contracts and remain follow-ups; this round does not claim every
list in the CLI is fully paginated. Aggregate result-byte limits remain open.

Verification: **607 tests passed**, zero failed, one ignored doctest. Full tests,
Clippy (all targets/features, warnings denied), formatting, diff checks, and Rust
1.88 check passed on Linux.

## Remaining list consumers (goal round 11)

Search now consumes a typed paginated response and preserves the initial server
size independently of the result limit. Full branch PR lookup separates item
limit from page size (all results versus the light lookup's single result).
Pipeline list field projections now include next/size/page/pagelen; previously
the server could omit next even though the client called the shared paginator.

`list_steps` remains a single-page library method with continuation metadata.
A `list_all_steps` helper is used by CI step/log/test selectors, tail/watch loops,
status summaries, and pipeline comparison. This avoids a fake combined page
object and preserves command JSON schemas while completing the lists.

Evidence: six of eight initial `tests/list_completion.rs` tests failed before
implementation, including a field-projection-aware mock that hides next unless
requested. All eight passed after the changes. Two further CLI regressions check
page-two failed-log and named-test-step selection. The raw single-page library
contract and lightweight PR request count are explicitly covered.

Known limit: existing status/watch paths still downgrade some API failures to
empty lists via unwrap_or_default; their diagnostic/retry semantics remain to
review. This round fixes pagination, not error reporting for every polling loop.

Verification: **617 tests passed**, zero failures, one ignored doctest; full
all-feature tests, all-target Clippy with warnings denied, formatting, diff checks,
and Rust 1.88 check passed on Linux.

## Stack local-state safety (goal round 12)

Stack saves now reuse the private atomic replacement helper instead of truncating
the state file, and report directory-creation failures. Loading treats only a
missing file as empty; init no longer swallows read/parse failures. Explicit stale
active-stack names are rejected before stack operations, while legacy files with
no selection retain first-stack fallback. `stack use` can repair a stale name.

Evidence: five of eight initial `tests/stack_safety.rs` cases failed before the
change; all eight passed afterward. Tests cover corrupt init preservation, an
open reader retaining the old inode contents after save, stale abort rejection,
legacy selection, state from repo subdirectories, and invalid filesystem paths.
Further unit checks exercise failed replacement cleanup, parent creation errors,
and complete replacement roundtrips. Tests touch only temporary repositories.

Scope warning: atomic writes do NOT solve remote-operation recovery. Inspection
of `src/commands/stack.rs` found land ignores final save/remove failures and returns
success with a nonempty failed list; rebase also reports failed steps with exit 0.
Abort continues deleting branches after decline failures, then removes the stack
regardless of partial failures. Both land and abort reload with unwrap_or_default
at completion. These require dedicated partial-failure/checkpoint tests and fixes
before stack operations are production-ready. Usage docs now disclose this risk.

Verification: **628 tests passed**, zero failures, one ignored doctest; full
all-feature tests, Clippy with warnings denied, formatting, diff checks, and Rust
1.88 check passed on Linux.

## Stack landing checkpoints (goal round 13)

Landing validates every PR ID before remote work, verifies current PR identity/
state, merges only OPEN PRs, and reconciles already-MERGED PRs without replaying
POST. It requires an explicitly MERGED response before counting success. Each
confirmed merge updates remaining local state atomically before the next operation.
The final save removes only that stack and retains a valid empty file if needed.
Errors stop the loop, print the partial receipt, and propagate nonzero status;
API errors retain their mapped exit codes. Checkpoint errors identify the remote
merge that already happened. Local branch deletion is safe (`-d`) and best-effort
only after checkpoint persistence, retaining local-only commits with a warning.

Optimistic state comparisons before requests/checkpoints detect intervening edits
without overwriting them. They are not locks and cannot eliminate the race between
comparison and replacement. Remote merge plus local persistence is not atomic;
a retry reconciles MERGED state for the interruption gap. This does not solve
multi-writer safety, rebasing/retargeting stacked destinations, or all abort cases.

Evidence: all eight initial `tests/stack_land.rs` regressions failed before the
change. Eleven focused tests passed after changes: partial failure with a checkpoint
visible to the next mock request, invalid IDs, injected checkpoint failure, no
replayed POST for MERGED, declined/unconfirmed states, siblings/final empty config,
intervening config edits, retry of remaining entries only, and local-only commit
preservation. Fixtures use isolated Git repositories and local mock API servers.

Verification: **639 tests passed**, zero failed, one ignored doctest; full tests,
Clippy with warnings denied, formatting, diff checks, and Rust 1.88 check passed
on Linux. No real PRs or user branches were modified.

## Historical draft PR snapshot (commit 1c2851d)

The following is the original draft state, superseded by the continuation below.

This snapshot includes unfinished abort/rebase hardening, at the user's request to
open a PR containing all accumulated repository changes. It is **not merge-ready**.

Fresh full-suite result at PR preparation: **641 passing, 10 failing**, one ignored
doctest. All ten failures are in `tests/stack_abort.rs`. The two rebase failure-exit
tests in that suite now pass: rebase/force-push failures preserve their receipt
and return the underlying error instead of exit 0.

Abort implementation is still the old best-effort loop. The new failing tests
specify the intended guarantees, not behavior delivered by this draft: stop after
failed decline, validate PR identity/state, use the resolved repository for remote
branch cleanup, preserve unmerged local commits, checkpoint completed entries,
handle already-declined/missing branches on retry, and reject unsafe stack entries.
The branch-validation and idempotent local-cleanup helpers in `src/git.rs` are
preparatory and not yet wired into abort. PR reads now request source-repository
identity fields for the pending validation.

The preceding **639-passing** snapshot refers to completed round-13 work, not the
current all-changes draft. Do not disable the ten regression tests to make CI green.
Complete the implementation or split the unfinished slice before merging.

## PR #54 continuation: abort completion and CI failures

Abort now validates IDs and literal branch names, protects the current/base/shared
branches, verifies the PR source branch/repository and state, and stops after any
unconfirmed decline. It deletes remote branches through the resolved API repository,
then safely deletes a local exact ref if present. Completed entries are checkpointed
one at a time; unfinished entries survive failures. Retries reconcile DECLINED PRs
and missing branches. Receipts retain their existing shape and errors propagate
with nonzero exits. State comparisons remain optimistic rather than locked.

All twelve existing abort/rebase regressions passed after implementation. Added
success/empty-stack coverage plus shared-branch/zero-ID cases. Independent review
then found three additional hazards, reproduced and fixed: tracked upstream refs
allowing `branch -d` to delete commits absent from HEAD, sibling parent dependencies,
and duplicate stack names. Safe deletion now explicitly checks ancestry to HEAD,
independent of cached upstream refs. A nonroot Unix test also verifies an actual
checkpoint-write denial after remote/local cleanup, separately from unreadable
state failures. All eighteen abort/rebase tests pass. Local cleanup may
fail after remote deletion (e.g. unmerged local commits); there is no transactional
rollback, and recreated remote branches still present a race. Review these limits
before claiming production readiness.

CI run 34887721080 identified four non-abort failures, now addressed locally:
- rustls 0.23.41 was affected by RUSTSEC-2026-0285. A focused lockfile update selects
  rustls 0.23.45 and rustls-webpki 0.103.15; cargo audit now passes, retaining two
  allowed unmaintained-crate warnings (bincode and number_prefix).
- Installer ShellCheck SC2088 flagged intentional display-only tilde RC hints.
  A scoped explanatory suppression preserves the displayed command. ShellCheck
  v0.10.0 runs successfully in an isolated, no-network container against installer
  and packaging scripts; shell syntax checks also pass.
- `ci tail --pipeline` eagerly resolved Git HEAD even though UUID mode does not
  need it, failing all CI checkout platforms. A temporary non-Git cwd reproduces
  the failure; resolution is now deferred until pipeline inference is needed.
- A Windows test expected a mixed-separator credentials path; it now joins each
  path component natively. Windows runtime confirmation awaits the updated CI run.

Final local verification: **657 passing tests**, zero failures, one ignored doctest;
Clippy warnings-denied, formatting, diff checks, Rust 1.88 check, and release build
passed. Cargo audit passes with the two maintenance warnings above. Cargo deny
licenses/bans/sources passes with duplicate-version warnings. Rebuilt release-binary
installer smoke: **7 passed, 0 failed** against localhost; synthetic Homebrew/Scoop/
winget generation passed Ruby syntax and JSON/YAML/digest checks. Nothing published.

## Review coverage and next actions

Core architecture: `cli.rs` parses; `dispatch.rs` routes; `commands/` resolves
identity/auth and formats output; `api/` implements endpoints over a shared HTTP
client; `git.rs` owns git subprocesses; `config.rs` and `stack.rs` persist state;
`output/` and `diff/` render output. Existing tests exercise many API and CLI
contracts. Large command/diff modules still need a complete correctness review;
file size alone is not a reason to refactor them.

Prioritized remaining items (source-inspected, not yet regression-verified):

1. **Authentication follow-ups** — setup/status defects above are fixed.
   `status`/`doctor` infer the credential source from token-env presence, which
   can mislabel a fallback to file credentials when environment input is invalid.
   Review this alongside source resolution and in-memory secret lifetimes.
   Linux PTY behavior was verified this round; macOS/Windows still need native
   validation.
2. **Repository identity follow-ups** — context-error fallback and remote-host
   parsing are fixed above. Assess path-segment validation for explicit/configured
   identity fields. Local SSH aliases remain trusted. Commands that infer API
   identity from an upstream remote but push/fetch `origin` need consistency review.
3. **Stack remote-operation safety (high priority)** — local persistence,
   landing/abort checkpoints, rebase exits, and abort source validation are fixed.
   Review destination retargeting/rebase order when parent branches are closed,
   landing source validation, branch recreation races, and stack-add save failure.
   Concurrent writers still need a locking strategy (optimistic checks are not locks).
4. **Pagination follow-ups** — shared traversal, search, step consumers, and
   full branch lookup are fixed above. Review remaining low-level page-returning
   methods (commit statuses, diffstat) and their consumers, plus aggregate byte
   budgets for large results, API-base validation, and redirect boundaries.
   Status/watch still swallow some failed fetches as empty data; audit that next.
5. **HTTP follow-ups** — cache consistency and JSON debug previews are fixed
   above. Audit URL/query exposure in retry/error messages, structured serde
   errors that may include offending string values, and cache metadata on 304.
   Transport timeout/redirect/cancellation behavior needs a separate review.
6. **Installation and documentation truthfulness** — README warns the crates.io
   name is unrelated but recommends bare `cargo binstall bbr`; verify registry
   lookup behavior before recommending it. README scope names also disagree with
   `auth setup`'s `read:*:bitbucket` names. Validate against authoritative docs.
   Review release-generated manifests, installer fallback/checksum behavior, and
   actual artifact support. Do not publish releases without explicit approval.
7. **Test isolation and platform gates** — some existing smoke tests mutate the
   parent environment or reuse fixed `/tmp` paths. Move to per-child environment
   overrides and unique temporary directories as tests are touched. Verify MSRV,
   dependency policy, and Windows/macOS behavior; a Linux pass is not all-platform
   evidence. Replace hardcoded test-count badges with a non-stale signal.

## Acceptance gates for the overall objective

- Reproduced high-impact safety/correctness defects fixed with regression tests.
- Default and JSON/noninteractive paths tested without real-account side effects.
- Fresh full tests, formatting, Clippy, MSRV, and dependency checks pass, or
  specific environment limitations are reported.
- Installation instructions select this project rather than an unrelated crate;
  packaged artifacts and upgrade paths are tested on their advertised targets.
- Documentation matches actual flags, auth scopes, output schemas, and exit codes.
- Performance claims use measured before/after evidence rather than estimates.
- Remaining audit coverage and risks are explicit; no blanket security claim.

## Local verification recipe

Cargo is available under `$HOME/.cargo/bin`. A user-level sccache configuration
previously failed due to a literal tilde cache path; use a command-local wrapper
override rather than modifying global configuration or restarting shared services:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTC_WRAPPER=""
cargo test --locked --all-features --no-fail-fast
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo fmt --check
git diff --check
```
