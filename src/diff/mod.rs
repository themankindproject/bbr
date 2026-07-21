//! Pretty diff rendering for `bbr pr diff`.
//!
//! This module parses raw unified diff text into structured types,
//! computes intra-line word-level changes, and renders a beautiful
//! terminal output with box-drawing, line numbers, and ANSI colors.

pub mod align;
pub mod parser;
pub mod pathspec;
pub mod renderer;
pub mod word_diff;

pub use parser::{
    filter_raw_diff, parse, parse_diff_git_paths, DiffFile, DiffHunk, DiffLine, DiffLineKind,
    FileStatus,
};
pub use pathspec::{matches_any as pathspec_matches_any, matches_one as pathspec_matches};
pub use renderer::{render, render_name_only, render_name_status, DiffRenderOptions, RenderMode};

/// Keep files whose old or new path matches any pathspec.
pub fn filter_files(mut files: Vec<DiffFile>, pathspecs: &[String]) -> Vec<DiffFile> {
    if pathspecs.is_empty() {
        return files;
    }
    files.retain(|f| {
        pathspec::matches_any(pathspecs, &f.old_path)
            || pathspec::matches_any(pathspecs, &f.new_path)
    });
    files
}
