//! Test-only support utilities shared across unit and integration tests.
//!
//! Not compiled into release builds.

#![cfg(test)]

use std::sync::Mutex;

/// Process-wide lock guarding process-global environment mutations in tests.
///
/// Tests that call `std::env::set_var` / `remove_var` (e.g. pointing
/// `XDG_CONFIG_HOME` at a tempdir) must hold this lock for the whole
/// mutation-and-assert section. A per-module mutex is not enough: env vars
/// are process-global, so tests in *different* modules running in parallel
/// would otherwise race each other.
pub static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Convenience wrapper: acquire [`ENV_LOCK`], recovering from a poisoned
/// mutex (a panicking earlier test must not wedge the whole suite).
pub fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
