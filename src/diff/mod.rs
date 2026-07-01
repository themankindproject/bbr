//! Pretty diff rendering for `bbr pr diff`.
//!
//! This module parses raw unified diff text into structured types,
//! computes intra-line word-level changes, and renders a beautiful
//! terminal output with box-drawing, line numbers, and ANSI colors.

pub mod parser;
pub mod renderer;
pub mod word_diff;

pub use parser::{DiffFile, DiffHunk, DiffLine, DiffLineKind, FileStatus};
pub use renderer::{render, DiffRenderOptions, RenderMode};
