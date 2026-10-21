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
///
/// Numeric selectors are validated: `0`, reversed ranges, and numbers that
/// overflow `usize` are usage errors. Silently dropping them used to widen
/// the output to the whole diff.
pub fn parse_file_selectors(values: &[String]) -> crate::error::Result<Vec<FileSelector>> {
    let mut out = Vec::new();
    for value in values {
        let v = value.trim();
        if v.is_empty() {
            continue;
        }
        if v.chars().all(|c| c.is_ascii_digit()) {
            let n = v.parse::<usize>().map_err(|_| {
                crate::error::BitbucketError::Usage(format!(
                    "invalid --file index {v:?}: must be a positive integer"
                ))
            })?;
            if n == 0 {
                return Err(crate::error::BitbucketError::Usage(
                    "invalid --file index 0: indices are 1-based".into(),
                ));
            }
            out.push(FileSelector::Index(n));
            continue;
        }
        if let Some((lo, hi)) = v.split_once('-') {
            if let (Ok(lo), Ok(hi)) = (lo.trim().parse::<usize>(), hi.trim().parse::<usize>()) {
                if lo >= 1 && hi >= lo {
                    out.push(FileSelector::Range(lo, hi));
                    continue;
                }
                return Err(crate::error::BitbucketError::Usage(format!(
                    "invalid --file range {v:?}: expected START-END with 1 <= START <= END"
                )));
            }
        }
        out.push(FileSelector::Glob(v.to_string()));
    }
    Ok(out)
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
        // The `diff --git` line is required: downstream `--` pathspec
        // filtering keys off it. Without it, selecting a file and then
        // filtering produced an empty diff.
        let (header_old, header_new) = match f.status {
            FileStatus::Added => (new.as_str(), new.as_str()),
            FileStatus::Deleted => (old.as_str(), old.as_str()),
            _ => (old.as_str(), new.as_str()),
        };
        out.push_str(&format!(
            "diff --git {} {}\n",
            quote_git_path(&format!("a/{header_old}")),
            quote_git_path(&format!("b/{header_new}"))
        ));
        // Preserve status metadata so re-parsing the rebuilt text (the
        // pretty/JSON paths do) yields the same rename/binary classification.
        match f.status {
            FileStatus::Added => out.push_str("new file mode 100644\n"),
            FileStatus::Deleted => out.push_str("deleted file mode 100644\n"),
            FileStatus::Renamed => out.push_str(&format!(
                "similarity index 100%\nrename from {}\nrename to {}\n",
                quote_git_path(old),
                quote_git_path(new)
            )),
            FileStatus::Modified => {}
        }
        if f.binary {
            out.push_str(&format!(
                "Binary files {} and {} differ\n",
                quote_git_path(&format!("a/{old}")),
                quote_git_path(&format!("b/{new}"))
            ));
            continue;
        }
        match f.status {
            FileStatus::Added => {
                out.push_str(&format!(
                    "--- /dev/null\n+++ {}\n",
                    quote_git_path(&format!("b/{new}"))
                ));
            }
            FileStatus::Deleted => {
                out.push_str(&format!(
                    "--- {}\n+++ /dev/null\n",
                    quote_git_path(&format!("a/{old}"))
                ));
            }
            _ => {
                out.push_str(&format!(
                    "--- {}\n+++ {}\n",
                    quote_git_path(&format!("a/{old}")),
                    quote_git_path(&format!("b/{new}"))
                ));
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
                if l.no_newline {
                    out.push_str("\\ No newline at end of file\n");
                }
            }
        }
    }
    out
}

/// Quote a path the way Git does when it contains characters that would
/// otherwise break the `diff --git`/`---`/`+++` lines.
fn quote_git_path(path: &str) -> String {
    let needs_quotes = path
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || !c.is_ascii() || matches!(c, '"' | '\\'));
    if !needs_quotes {
        return path.to_string();
    }
    let mut out = String::from("\"");
    for &byte in path.as_bytes() {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            0x20..=0x7e => out.push(byte as char),
            other => out.push_str(&format!("\\{other:03o}")),
        }
    }
    out.push('"');
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
        let sels = parse_file_selectors(&["3".into(), "1-5".into()]).unwrap();
        assert_eq!(
            sels,
            vec![FileSelector::Index(3), FileSelector::Range(1, 5)]
        );
    }

    #[test]
    fn parse_selectors_globs_pass_through() {
        let sels = parse_file_selectors(&["src/api/*.rs".into()]).unwrap();
        assert_eq!(sels, vec![FileSelector::Glob("src/api/*.rs".into())]);
    }

    #[test]
    fn parse_selectors_invalid_numeric_tokens_are_errors() {
        assert!(parse_file_selectors(&["10-2".into()]).is_err());
        assert!(parse_file_selectors(&["0".into()]).is_err());
        assert!(parse_file_selectors(&["999999999999999999999999".into()]).is_err());
        // A non-numeric token containing a dash is still a valid glob.
        assert_eq!(
            parse_file_selectors(&["src/foo-bar.rs".into()]).unwrap(),
            vec![FileSelector::Glob("src/foo-bar.rs".into())]
        );
    }

    #[test]
    fn select_by_index_matches_original_numbering() {
        let files = [file("a", "a"), file("b", "b"), file("c", "c")];
        let picked = select_files(&files, &parse_file_selectors(&["2".into()]).unwrap());
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
        let picked = select_files(&files, &parse_file_selectors(&["1-3".into()]).unwrap());
        assert_eq!(picked.len(), 3);
        assert_eq!(picked[0].new_path, "a");
        assert_eq!(picked[2].new_path, "c");
    }

    #[test]
    fn select_no_match_yields_empty() {
        let files = [file("a", "a")];
        let picked = select_files(&files, &parse_file_selectors(&["9".into()]).unwrap());
        assert!(picked.is_empty());
    }

    #[test]
    fn raw_rebuild_keeps_headers_for_pathspec_filtering() {
        let files = [file("a.rs", "a.rs"), file("b.rs", "b.rs")];
        let picked = select_files(&files, &parse_file_selectors(&["2".into()]).unwrap());
        let raw = render_raw_from_files(&picked);
        assert!(raw.starts_with("diff --git a/b.rs b/b.rs\n"), "got:\n{raw}");
        let filtered = filter_raw_diff(&raw, &["b.rs".into()]);
        assert!(
            filtered.contains("+line"),
            "filtered diff was empty:\n{filtered}"
        );
        assert!(!filtered.contains("a.rs"));
    }

    #[test]
    fn raw_rebuild_quotes_paths_with_spaces() {
        let files = [file("my file.rs", "my file.rs")];
        let raw = render_raw_from_files(&files);
        assert!(raw.contains("diff --git \"a/my file.rs\" \"b/my file.rs\""));
        let filtered = filter_raw_diff(&raw, &["my file.rs".into()]);
        assert!(filtered.contains("+line"));
    }

    #[test]
    fn raw_rebuild_preserves_rename_status() {
        let mut renamed = file("old.rs", "new.rs");
        renamed.status = FileStatus::Renamed;
        let raw = render_raw_from_files(&[renamed]);
        assert!(raw.contains("rename from old.rs"), "{raw}");
        assert!(raw.contains("rename to new.rs"), "{raw}");
        let reparsed = parse(&raw);
        assert_eq!(reparsed[0].status, FileStatus::Renamed);
        assert_eq!(reparsed[0].old_path, "old.rs");
        assert_eq!(reparsed[0].new_path, "new.rs");
    }

    #[test]
    fn raw_rebuild_preserves_binary_marker() {
        let mut binary = file("blob.bin", "blob.bin");
        binary.binary = true;
        let raw = render_raw_from_files(&[binary]);
        assert!(raw.contains("Binary files a/blob.bin and b/blob.bin differ"));
        assert!(parse(&raw)[0].binary);
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
