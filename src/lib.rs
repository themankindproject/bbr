//! `bbr` — a Bitbucket Cloud CLI library + binary.
//!
//! The library is exposed so integration tests can exercise the API client
//! directly. The `bb` binary (`src/main.rs`) is a thin wrapper around
//! [`cli::run`].

pub mod api;
pub mod auth;
pub mod cli;
pub mod commands;
pub mod config;
pub mod error;
pub mod git;
pub mod output;
pub mod stack;

pub use error::{BitbucketError, ExitCode, Result};
