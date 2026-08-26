//! Pretty diff rendering for `bbr pr diff`.
//!
//! This module parses raw unified diff text into structured types,
//! computes intra-line word-level changes, and renders a beautiful
//! terminal output with box-drawing, line numbers, and ANSI colors.

pub mod align;
pub mod parser;
pub mod pathspec;
pub mod renderer;
pub mod syntax;
pub mod word_diff;

pub use parser::{
    filter_raw_diff, parse, parse_diff_git_paths, DiffFile, DiffHunk, DiffLine, DiffLineKind,
    FileStatus,
};
pub use pathspec::matches_any as pathspec_matches_any;
pub use renderer::{
    render, render_name_only, render_name_status, render_to, DiffRenderOptions, RenderMode,
};

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

/// A single `--file` selector: either a 1-based index/range or a pathspec glob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSelector {
    Index(usize),
    Range(usize, usize),
    Glob(String),
}

/// Parse `--file` values like `"3"`, `"1-5"`, or `"src/api/*.rs"` into
/// selectors. Tokens that parse as an index or range become indices;
/// anything else is treated as a glob.
pub fn parse_file_selectors(values: &[String]) -> Vec<FileSelector> {
    let mut out = Vec::new();
    for value in values {
        let v = value.trim();
        if v.is_empty() {
            continue;
        }
        if let Some((lo, hi)) = v.split_once('-') {
            if let (Ok(lo), Ok(hi)) = (lo.trim().parse::<usize>(), hi.trim().parse::<usize>()) {
                if lo >= 1 && hi >= lo {
                    out.push(FileSelector::Range(lo, hi));
                    continue;
                }
                // e.g. "10-2": treat as reversed range? Keep it simple — invalid.
                continue;
            }
        }
        match v.parse::<usize>() {
            Ok(n) if n >= 1 => out.push(FileSelector::Index(n)),
            // Bare numbers outside the valid range are ignored; only
            // non-numeric tokens become globs.
            Ok(_) => {}
            Err(_) => out.push(FileSelector::Glob(v.to_string())),
        }
    }
    out
}

/// Filter files by 1-based index / range selectors against the *original*
/// file-list numbering (the numbers shown in the file index), plus globs
/// matched against old/new paths.
///
/// Returns the filtered list; when nothing matches, returns an empty vec so
/// callers can report "no file matched".
pub fn select_files(files: &[DiffFile], selectors: &[FileSelector]) -> Vec<DiffFile> {
    if selectors.is_empty() {
        return files.to_vec();
    }
    let mut kept: Vec<usize> = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let idx = i + 1;
        let hit = selectors.iter().any(|sel| match sel {
            FileSelector::Index(n) => *n == idx,
            FileSelector::Range(lo, hi) => idx >= *lo && idx <= *hi,
            FileSelector::Glob(g) => {
                crate::diff::pathspec::matches_any(std::slice::from_ref(g), &f.old_path)
                    || crate::diff::pathspec::matches_any(std::slice::from_ref(g), &f.new_path)
            }
        });
        if hit {
            kept.push(i);
        }
    }
    kept.into_iter().map(|i| files[i].clone()).collect()
}

/// Rebuild a unified-diff text from parsed [`DiffFile`]s.
///
/// Used after index-based file selection so downstream stages (raw output,
/// `--` pathspec filtering, JSON summary) operate on the selected subset
/// exactly as if the server had sent only those files.
pub fn render_raw_from_files(files: &[DiffFile]) -> String {
    let mut out = String::new();
    for f in files {
        let old = &f.old_path;
        let new = &f.new_path;
        match f.status {
            FileStatus::Added => {
                out.push_str(&format!("--- /dev/null\n+++ b/{new}\n"));
            }
            FileStatus::Deleted => {
                out.push_str(&format!("--- a/{old}\n+++ /dev/null\n"));
            }
            _ => {
                out.push_str(&format!("--- a/{old}\n+++ b/{new}\n"));
            }
        }
        for h in &f.hunks {
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@ {}\n",
                h.old_start, h.old_lines, h.new_start, h.new_lines, h.header
            ));
            for l in &h.lines {
                let sign = match l.kind {
                    DiffLineKind::Addition => '+',
                    DiffLineKind::Deletion => '-',
                    DiffLineKind::Context => ' ',
                };
                out.push(sign);
                out.push_str(&l.content);
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod selector_tests {
    use super::*;

    fn file(old: &str, new: &str) -> DiffFile {
        crate::diff::parser::parse(&format!("--- a/{old}\n+++ b/{new}\n@@ -0,0 +1 @@\n+line\n"))
            .into_iter()
            .next()
            .expect("one file")
    }

    #[test]
    fn parse_selectors_indices_and_ranges() {
        let sels = parse_file_selectors(&["3".into(), "1-5".into()]);
        assert_eq!(
            sels,
            vec![FileSelector::Index(3), FileSelector::Range(1, 5)]
        );
    }

    #[test]
    fn parse_selectors_globs_pass_through() {
        let sels = parse_file_selectors(&["src/api/*.rs".into()]);
        assert_eq!(sels, vec![FileSelector::Glob("src/api/*.rs".into())]);
    }

    #[test]
    fn parse_selectors_invalid_ranges_dropped() {
        // "10-2" reversed and "0" below range are ignored, not errors.
        let sels = parse_file_selectors(&["10-2".into(), "0".into(), "7".into()]);
        assert_eq!(sels, vec![FileSelector::Index(7)]);
    }

    #[test]
    fn select_by_index_matches_original_numbering() {
        let files = [file("a", "a"), file("b", "b"), file("c", "c")];
        let picked = select_files(&files, &parse_file_selectors(&["2".into()]));
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].new_path, "b");
    }

    #[test]
    fn select_by_range_keeps_order() {
        let files = [
            file("a", "a"),
            file("b", "b"),
            file("c", "c"),
            file("d", "d"),
        ];
        let picked = select_files(&files, &parse_file_selectors(&["1-3".into()]));
        assert_eq!(picked.len(), 3);
        assert_eq!(picked[0].new_path, "a");
        assert_eq!(picked[2].new_path, "c");
    }

    #[test]
    fn select_no_match_yields_empty() {
        let files = [file("a", "a")];
        let picked = select_files(&files, &parse_file_selectors(&["9".into()]));
        assert!(picked.is_empty());
    }

    #[test]
    fn raw_rebuild_roundtrips_signs() {
        let f = crate::diff::parser::parse(
            "--- a/f.rs\n+++ b/f.rs\n@@ -1,2 +1,2 @@\n context\n-old\n+new\n",
        );
        assert_eq!(f.len(), 1);
        let raw = render_raw_from_files(&f);
        assert!(raw.contains("--- a/f.rs"));
        assert!(raw.contains("+++ b/f.rs"));
        assert!(raw.contains("-old"));
        assert!(raw.contains("+new"));
        // Re-parsing the rebuilt text yields the same line kinds.
        let reparsed = crate::diff::parser::parse(&raw);
        let kinds: Vec<_> = reparsed[0].hunks[0].lines.iter().map(|l| l.kind).collect();
        assert_eq!(
            kinds,
            vec![
                DiffLineKind::Context,
                DiffLineKind::Deletion,
                DiffLineKind::Addition
            ]
        );
    }
}
