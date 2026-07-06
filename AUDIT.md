# bbr Codebase Audit Report

**Date:** 2026-07-07  
**Scope:** Full codebase — security, performance, correctness, UX, architecture, test coverage  
**Version audited:** 0.1.6

---

## Executive Summary

bbr is a well-architected Rust CLI with solid foundations: clean error handling, stable JSON output, good async patterns, and resilient API deserialization. However, the audit identified **3 security issues**, **4 correctness bugs**, **5 performance concerns**, and several UX/architecture improvements worth addressing.

**Priority breakdown:**
- 🔴 Critical (fix before next release): 3
- 🟠 High (fix soon): 6  
- 🟡 Medium (next sprint): 8
- ⚪ Low (backlog): 7

---

## 🔴 Critical Issues

### C1. Self-Update Has No Integrity Verification

**File:** `src/commands/update.rs` (lines 340-430)  
**Risk:** Supply-chain attack / binary tampering

The `download_and_install()` function downloads a `.tar.gz` from GitHub Releases over HTTPS and directly replaces the running binary. There is:
- No checksum verification (SHA256)
- No GPG/Sigstore signature verification
- No pinned TLS certificate for GitHub

If GitHub is compromised, a CDN is MITM'd, or a DNS hijack occurs, an attacker can deliver a malicious binary that auto-installs with user permissions.

**Recommendation:**
1. Publish SHA256 checksums as a release asset (e.g., `checksums.txt`)
2. Verify downloaded archive hash before extraction
3. Consider cosign/minisign signatures for the checksum file

```rust
// After downloading bytes:
let expected_hash = fetch_checksum_for_asset(&asset.name).await?;
let actual_hash = sha256_hex(&bytes);
if actual_hash != expected_hash {
    return Err(BitbucketError::Other("checksum mismatch — download may be corrupted".into()));
}
```

---

### C2. Git Argument Injection via Branch Names

**File:** `src/git.rs` (all write functions: lines 148-210)  
**Risk:** Arbitrary git flag injection

Branch names starting with `-` are passed directly as arguments to git commands without a `--` separator:

```rust
pub fn push_branch(branch: &str) -> Result<()> {
    git_with_timeout(&["push", "origin", branch], GIT_WRITE_TIMEOUT)?;  // branch = "--force" ?
    Ok(())
}

pub fn rebase_branch(branch: &str, onto: &str) -> Result<()> {
    git(&["switch", branch])?;             // branch = "--detach" ?
    git_with_timeout(&["rebase", onto], GIT_WRITE_TIMEOUT)?;  // onto = "--exec=malicious" ?
    Ok(())
}
```

A Bitbucket repository could have a branch named `--force`, `--exec=cmd`, or `--upload-pack=cmd` which would be interpreted as git flags.

**Recommendation:** Add `--` before any user-supplied ref in every git command:

```rust
pub fn push_branch(branch: &str) -> Result<()> {
    git_with_timeout(&["push", "origin", "--", branch], GIT_WRITE_TIMEOUT)?;
    Ok(())
}

pub fn checkout_branch(branch: &str) -> Result<()> {
    git(&["switch", "--", branch]).map(|_| ())
}
```

Affected functions: `fetch_branch`, `checkout_branch`, `push_branch`, `push_force_with_lease`, `delete_branch_local`, `delete_branch_local_safe`, `delete_branch_remote`, `rebase_branch`.

---

### C3. Pagination Infinite Loop

**File:** `src/api/mod.rs` (sequential pagination fallback, lines ~129-148)  
**Risk:** CLI hangs indefinitely, consuming CPU

If the Bitbucket API returns `next: Some(url)` with `values: []` (empty page), the pagination loop spins forever:

```rust
loop {
    let page: Paginated<T> = self.send(Method::GET, &next_path, None).await?;
    all.extend(page.values.into_iter().take(remaining));
    if all.len() >= limit { break; }
    match page.next {
        Some(next_url) => { next_path = strip_base(&next_url, &self.base_url)?; }
        None => break,
    }
    // BUG: if values is empty but next is Some, this loops forever
}
```

**Fix:**
```rust
let page: Paginated<T> = self.send(Method::GET, &next_path, None).await?;
if page.values.is_empty() { break; }  // <-- add this guard
all.extend(page.values.into_iter().take(remaining));
```

---

## 🟠 High Issues

### H1. Blocking Git I/O on Async Runtime

**File:** `src/git.rs` (all functions), called from `src/commands/stack.rs`, `src/commands/batch.rs`  
**Impact:** Freezes the tokio runtime thread for up to 120 seconds

All git functions are synchronous and spawn OS threads internally. When called from async command handlers (`stack rebase`, `batch cleanup-merged-branches`), they block the tokio worker thread. If multiple tokio tasks exist (e.g., spinners, background update checks), they'll all stall.

**Recommendation:** Wrap all git calls in `tokio::task::spawn_blocking`:
```rust
pub async fn push_branch_async(branch: &str) -> Result<()> {
    let branch = branch.to_string();
    tokio::task::spawn_blocking(move || push_branch(&branch))
        .await
        .map_err(|e| BitbucketError::Other(format!("git task panicked: {e}")))?
}
```

---

### H2. Unbounded Parallel Page Fetch (OOM + Rate Limit Bombardment)

**File:** `src/api/mod.rs` (parallel pagination, lines ~154-170)  
**Impact:** With large `limit` values, spawns thousands of concurrent requests

```rust
let num_pages = total_needed.div_ceil(pagelen);
let mut futures = Vec::new();
for p in 2..=num_pages {
    futures.push(async move { self.send::<Paginated<T>>(...).await });
}
let results = futures::future::try_join_all(futures).await?;
```

With `limit=5000, pagelen=25`, this fires **200 concurrent requests**, likely triggering Bitbucket's rate limiter and holding 200 response bodies in memory simultaneously.

**Fix:** Use `buffer_unordered` with a concurrency cap:
```rust
use futures::StreamExt;
let results: Vec<_> = futures::stream::iter(futures)
    .buffer_unordered(10)  // max 10 concurrent
    .try_collect()
    .await?;
```

---

### H3. Diff Parser Drops Intermediate Hunks (Bug)

**File:** `src/diff/parser.rs`  
**Impact:** Multi-hunk files display only the last hunk

The parser's state machine correctly counts additions/deletions for all hunks, but only pushes the *last* hunk's `DiffHunk` struct into `DiffFile.hunks`. Intermediate hunks are only counted (via `add_hunk_counts`), not stored.

This means `bbr pr diff` for a file with 3 hunks will only render the last hunk. Tests don't catch this because they all use single-hunk diffs.

**Recommendation:** Store each completed hunk in the hunks vector when a new `@@ ` header is encountered, not just in `finish()`.

---

### H4. `list_prs` Pagination Double-URL Bug

**File:** `src/api/pr.rs` (lines ~255-270)  
**Impact:** Silent failure when paginating PR lists > 100

```rust
let next_path = url.strip_prefix(&self.base_url).unwrap_or(url).to_string();
```

If `strip_prefix` fails (e.g., the API returns a URL with a different scheme or trailing slash), `unwrap_or(url)` passes the full absolute URL to `send()`, which prepends `base_url` again, producing `https://api.bitbucket.org/2.0/https://api.bitbucket.org/2.0/...`.

**Fix:** Use the `strip_base()` helper (which returns an error on mismatch) or reuse `fetch_all_pages`.

---

### H5. Rate Limit Retry Ignores `Retry-After` Header

**File:** `src/api/mod.rs` (lines ~100-111)  
**Impact:** Suboptimal retry timing; may retry too early or wait too long

The retry logic uses fixed linear backoff (5s, 10s, 15s + jitter) instead of honoring Bitbucket's `Retry-After` header. The header is discarded because `decode()` consumes the response body before reading headers.

**Fix:** Extract `Retry-After` from response headers before consuming the body:
```rust
let retry_after = resp.headers().get("retry-after")
    .and_then(|v| v.to_str().ok())
    .and_then(|v| v.parse::<u64>().ok());
```

---

### H6. CLICOLOR/CLICOLOR_FORCE Not Implemented

**File:** `src/output/theme.rs`  
**Impact:** README documents `CLICOLOR` support but it doesn't work

The README and `--help` output state that `CLICOLOR=0` and `CLICOLOR_FORCE=1` are respected, but `theme.rs` only checks `NO_COLOR`. Users who rely on `CLICOLOR` conventions get unexpected colored output.

**Fix:**
```rust
let no_color = std::env::var_os("NO_COLOR").is_some()
    || std::env::var("CLICOLOR").ok().as_deref() == Some("0");
let force_color = std::env::var("CLICOLOR_FORCE").ok()
    .map(|v| v != "0")
    .unwrap_or(false);
let colors = force_color || (!no_color && is_tty);
```

---

## 🟡 Medium Issues

### M1. `status.rs` Code Duplication (~100 lines)

**File:** `src/commands/status.rs`  
**Impact:** Maintenance burden, divergence risk

`run_inner()` and `run_overview()` both independently:
1. Resolve repo + head
2. Fetch PR + pipeline + commit statuses via `tokio::try_join!`
3. Construct `pr_summary` with diffstat/conflicts
4. Construct `pipeline_summary`
5. Build suggested commands

This is ~100 lines of identical logic. Extract a shared `fetch_branch_status()` helper.

---

### M2. `send_empty` Deserializes Then Discards

**File:** `src/api/mod.rs` (lines 116-119)  
**Impact:** Unnecessary allocation; potential failure on 204 No Content with empty body

```rust
pub async fn send_empty(&self, method: Method, path: &str, body: Option<&str>) -> Result<()> {
    let _: serde_json::Value = self.send(method, path, body).await?;
    Ok(())
}
```

For DELETE endpoints returning 204, this works only because `decode()` special-cases empty bodies as `"null"`. But it allocates a `serde_json::Value` that's immediately dropped.

**Fix:** Add a dedicated void-response path that only checks the status code.

---

### M3. Stack Config Uses Relative Path

**File:** `src/stack.rs` (`config_path()`)  
**Impact:** Stack commands break when run from a subdirectory

```rust
pub fn config_path() -> PathBuf {
    Path::new(".bbr").join("stack.toml")
}
```

This resolves relative to CWD. Running `bbr pr stack list` from `src/` won't find the stack config at the repo root.

**Fix:** Use `git rev-parse --show-toplevel` to anchor to repo root:
```rust
pub fn config_path() -> Result<PathBuf> {
    let root = crate::git::repo_root()?;
    Ok(PathBuf::from(root).join(".bbr").join("stack.toml"))
}
```

---

### M4. No Credential File Permission Check

**File:** `src/config.rs`  
**Impact:** User's token may be world-readable without warning

If a user accidentally does `chmod 644 ~/.config/bbr/credentials.toml`, bbr reads it silently. Should warn when permissions are too open (like SSH does).

---

### M5. Custom Base64 Implementation

**File:** `src/api/mod.rs` (lines ~280-300)  
**Impact:** Low maintenance risk for auth-critical code path

A hand-rolled base64 encoder is used for HTTP Basic auth. While correct (has tests), any future edge-case bug would break all authentication. The `base64` crate adds ~10KB to binary.

---

### M6. Theme Override Race Condition

**File:** `src/output/theme.rs`  
**Impact:** `--color always` silently ignored if Theme accessed before CLI parsing

`set_color_override()` uses `OnceLock::set()` which silently fails if already initialized. If any code path (e.g., tracing subscriber formatting) accesses `Theme::current()` before arg parsing, the override is permanently lost with no warning.

---

### M7. `truncate_mid` UTF-8 Panic

**File:** `src/diff/renderer.rs` (line ~408)  
**Impact:** Panic on multi-byte filenames in diff headers

```rust
// Byte-level slicing without char boundary check
&s[end_start..]
```

If `end_start` lands in the middle of a multi-byte UTF-8 character, this panics.

**Fix:** Use `.char_indices()` from the end or `s.floor_char_boundary(end_start)` (nightly) / manual scan.

---

### M8. Issue Query Injection (BBQL)

**File:** `src/api/issue.rs` (`build_issue_query`)  
**Impact:** Bitbucket query manipulation via crafted filter values

```rust
parts.push(format!("state=\"{s}\""));  // s is user input
```

A status like `open" OR priority="critical` breaks out of the intended query. Low severity (only affects the user's own view), but should sanitize.

---

## ⚪ Low Issues

### L1. `detect_repo()` Error Message Is Misleading

**File:** `src/git.rs` (line ~131)  
Says "no bitbucket.org remote found" but the code actually parses any git remote (GitHub, GitLab, etc.).

### L2. `active_stack()` Always Returns First Stack

**File:** `src/stack.rs` (line 73)  
No way to select between multiple stacks. Design debt.

### L3. Whitespace-Only Credentials Not Rejected

**File:** `src/auth.rs`  
Empty-string check exists but `"   "` passes validation.

### L4. `PipelineFailed` Error Display Uninformative

**File:** `src/error.rs`  
`#[error("pipeline failed")]` doesn't include build number or branch. User sees generic message before hints.

### L5. Spawned Timeout Threads Never Joined

**File:** `src/git.rs` (`git_with_timeout`)  
If timeout fires, the spawned thread leaks until process exit. Acceptable for CLI but technically a resource leak.

### L6. `Credentials` Coupled to `BitbucketClient`

**File:** `src/auth.rs`  
`into_client()` on `Credentials` creates a coupling between auth and HTTP layers. Should be a constructor on `BitbucketClient`.

### L7. Deploy/Webhook List Methods Don't Paginate

**File:** `src/api/deploy.rs`, `src/api/webhook.rs`  
Unlike other list methods, these silently truncate at 100 results without calling `fetch_all_pages`.

---

## Performance Optimization Opportunities

| Area | Current | Proposed | Impact |
|------|---------|----------|--------|
| Git operations | Sync + thread per call | `tokio::process::Command` | Eliminates thread spawn overhead + runtime blocking |
| Pagination | Unbounded `try_join_all` | `buffer_unordered(10)` | Prevents OOM + rate limiting on large fetches |
| CI list | Sequential step fetch per pipeline | Already uses `buffer_unordered(5)` ✓ | Good |
| Status command | 2 sequential API calls after initial join | Batch into initial `try_join!` | Saves 1-2 round trips |
| Body clone in retry loop | `b.to_owned()` on each attempt | Clone once before loop | Minor allocation saving |
| Formatter | Always builds human string even for --json | Lazy evaluation via `Fn() -> String` | Avoids formatting work in JSON mode |
| Table builder | `add_row` takes self + returns Self | `&mut self` pattern | Eliminates N struct moves for N rows |

---

## Test Coverage Assessment

### What's Tested (~25% of commands):
- ✅ PR list, get, comments, tasks, commits, statuses, conflicts
- ✅ Pipeline latest + steps
- ✅ Repo tags
- ✅ Commit build status creation
- ✅ Rate-limit retry, pagination, send_raw
- ✅ Deployments, webhooks, issues, source browser
- ✅ CLI help, version, completion, schema smoke tests

### Critical Gaps (NOT tested):
- ❌ `bbr status` / `bbr` (overview) — the main user-facing command
- ❌ `bbr pr create` / `pr merge` / `pr approve` — PR lifecycle
- ❌ `bbr batch` — all batch operations
- ❌ `bbr ci watch` — live-tail polling
- ❌ `bbr pr stack` — all stacked PR operations
- ❌ `bbr update` — self-update (at minimum: version comparison logic)
- ❌ `bbr pr diff` — diff rendering with multi-hunk files
- ❌ `--json` output schema correctness — no test validates output matches documented schemas
- ❌ Git remote parsing edge cases (SSH aliases, non-standard URLs)
- ❌ Error paths: malformed API responses, partial JSON, unexpected nulls
- ❌ End-to-end command pipeline (arg parsing → API → formatting → output)

### Recommendations:
1. Add E2E tests with `assert_cmd` + `wiremock` for the top 5 commands
2. Add property tests for `parse_remote_url` with fuzzing
3. Add multi-hunk diff test to catch the parser bug
4. Add JSON schema validation tests (parse `--json` output and verify structure)

---

## UX/DX Improvements

1. **`--no-color` vs `--color never` redundancy** — Document precedence or remove one
2. **Empty results messaging** — Several commands print nothing on empty results in human mode (should always print "No X found")
3. **Spinner cleanup** — `SpinnerGuard` RAII is well done ✓ 
4. **Error hints** — The `report()` function provides excellent contextual hints ✓
5. **`bbr` with no args overview** — Excellent UX feature ✓
6. **Suggested commands** — Smart contextual suggestions ✓
7. **Watch mode** — Good refresh UX with TTY detection ✓

---

## Architecture Assessment

### Strengths:
- Clean layered architecture (CLI → Commands → API → HTTP)
- Excellent serde resilience (`#[serde(default)]` everywhere)
- `Formatter` enum cleanly separates human/JSON output
- `SpinnerGuard` RAII prevents dangling spinners
- Stable exit codes well-mapped via `thiserror`
- `OnceLock` caching for repo/head (process-level singleton, correct for CLI)

### Concerns:
- **cli.rs is 1800 lines** — routing table will grow; consider co-locating dispatch with commands
- **status.rs is 48KB** — significant duplication between run/overview; needs refactoring
- **No abstraction over git** — makes testing impossible without real git repos; consider a trait
- **26 command files** with inconsistent patterns — some use `make_formatter(g)`, others use `Formatter::from_json_flag(g.json)` directly

---

## Dependency Health

| Dependency | Version | Pinned? | Notes |
|-----------|---------|---------|-------|
| clap | =4.5.6 | ✅ Exact | Good |
| clap_complete | =4.5.4 | ✅ Exact | Good |
| reqwest | 0.12 | ❌ Range | Pin for reproducibility |
| tokio | 1 | ❌ Range | Acceptable (semver stable) |
| comfy-table | =7.1.4 | ✅ Exact | Good |
| rpassword | =7.3.1 | ✅ Exact | Good |
| others | ranges | ❌ | Consider `cargo update --locked` |

**Positive:** No OpenSSL, `rustls` only. Single binary. Minimal dependency tree for the feature set.

---

## Summary of Recommended Actions

### Immediate (before next release):
1. Add `--` separator in all git commands before user-supplied refs
2. Add empty-page guard in pagination loop
3. Add SHA256 checksum verification to self-update

### Short-term (next 1-2 sprints):
4. Wrap git calls in `spawn_blocking`
5. Cap parallel page fetches with `buffer_unordered(10)`
6. Implement `CLICOLOR`/`CLICOLOR_FORCE` support
7. Fix diff parser multi-hunk bug
8. Fix `truncate_mid` UTF-8 panic
9. Extract shared `fetch_branch_status()` from status.rs

### Medium-term (backlog):
10. Add E2E integration tests for top 5 commands
11. Refactor cli.rs routing
12. Add credential file permission warning
13. Anchor stack config to git repo root
14. Replace custom base64 with crate
