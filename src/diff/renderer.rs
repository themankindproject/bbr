//! Pretty diff renderer with box-drawing, line numbers, and ANSI colors.
//!
//! Takes parsed [`DiffFile`]s and outputs a beautifully formatted string
//! suitable for terminal display or piping through a pager.

use unicode_width::UnicodeWidthStr;

use crate::output::theme::Theme;

use super::parser::{DiffFile, DiffHunk, DiffLine, DiffLineKind, FileStatus};

/// Options for rendering a diff.
#[derive(Debug, Clone)]
pub struct DiffRenderOptions {
    /// Number of context lines to show around changes (default: 3).
    pub context_lines: usize,
    /// Render mode (unified or side-by-side).
    pub mode: RenderMode,
    /// Whether to apply syntax highlighting (deferred — currently a no-op).
    pub syntax_highlight: bool,
}

impl Default for DiffRenderOptions {
    fn default() -> Self {
        DiffRenderOptions {
            context_lines: 3,
            mode: RenderMode::Unified,
            syntax_highlight: false,
        }
    }
}

/// How to lay out the diff.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderMode {
    /// Traditional unified diff (default).
    Unified,
    /// Side-by-side view (deferred).
    #[allow(dead_code)]
    SideBySide,
}

/// Render a parsed diff into a formatted terminal string.
pub fn render(files: &[DiffFile], options: &DiffRenderOptions, theme: &Theme) -> String {
    let mut out = String::new();

    if files.is_empty() {
        if theme.colors_enabled() {
            out.push_str("\x1b[2m  (no diff content)\x1b[0m\n");
        } else {
            out.push_str("  (no diff content)\n");
        }
        return out;
    }

    for (i, file) in files.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        render_file(file, options, theme, &mut out);
    }

    // Summary bar
    render_summary(files, theme, &mut out);

    out
}

/// Render a single file's diff.
fn render_file(file: &DiffFile, options: &DiffRenderOptions, theme: &Theme, out: &mut String) {
    render_file_header(file, theme, out);

    if !file.hunks.is_empty() {
        for hunk in &file.hunks {
            render_hunk(hunk, options, theme, out);
        }
    }
}

/// Render the file header with status icon and path.
fn render_file_header(file: &DiffFile, theme: &Theme, out: &mut String) {
    let (icon, status_text) = match file.status {
        FileStatus::Added => ("+", "new file"),
        FileStatus::Deleted => ("\u{2212}", "deleted"),
        FileStatus::Modified => ("~", "modified"),
        FileStatus::Renamed => ("\u{2192}", "renamed"),
    };

    let change_count = if file.additions > 0 || file.deletions > 0 {
        format!(" \u{b7} +{}, -{}", file.additions, file.deletions)
    } else {
        String::new()
    };

    let display_path = if file.status == FileStatus::Renamed {
        format!("{} \u{2192} {}", file.old_path, file.new_path)
    } else if !file.new_path.is_empty() {
        file.new_path.clone()
    } else {
        file.old_path.clone()
    };

    let width = terminal_width().unwrap_or(80);
    let box_inner = width.saturating_sub(4); // margin 2 + box sides 2

    let header_text = format!(" {}  {}{}", icon, display_path, change_count);
    let header_text = truncate_mid(&header_text, box_inner.saturating_sub(2));

    if theme.unicode_enabled() {
        out.push_str(&dim("  \u{256d}\u{2500}", theme));
        out.push(' ');
        out.push_str(&header_text);
        out.push(' ');
        let fill = box_inner.saturating_sub(header_text.width() + 2);
        for _ in 0..fill {
            out.push_str(&dim("\u{2500}", theme));
        }
        out.push_str(&dim("\u{256e}\n", theme));

        let status_line = format!("  \u{2502} {} \u{2502}\n", status_text);
        out.push_str(&dim(&status_line, theme));

        out.push_str(&dim("  \u{251c}", theme));
        for _ in 2..box_inner {
            out.push_str(&dim("\u{2500}", theme));
        }
        out.push_str(&dim("\u{2524}\n", theme));
    } else {
        out.push_str("  +- ");
        out.push_str(&header_text);
        let fill = box_inner.saturating_sub(header_text.len() + 2);
        for _ in 0..fill {
            out.push('-');
        }
        out.push_str(" -+\n");

        out.push_str(&format!("  | {} |\n", status_text));

        out.push(' ');
        for _ in 0..=box_inner {
            out.push('-');
        }
        out.push('\n');
    }
}

/// Render a single hunk.
fn render_hunk(hunk: &DiffHunk, options: &DiffRenderOptions, theme: &Theme, out: &mut String) {
    // Hunk header (dimmed)
    if theme.colors_enabled() {
        out.push_str(&format!(
            "\x1b[2m@@ -{},{} +{},{} @@ {}\x1b[0m\n",
            hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines, hunk.header
        ));
    } else {
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@ {}\n",
            hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines, hunk.header
        ));
    };

    let ranges = find_change_ranges(&hunk.lines, options.context_lines);

    for range in &ranges {
        if range.is_collapsed && range.end > range.start {
            let hidden = range.end - range.start;
            let msg = format!("{} lines hidden", hidden);
            if theme.unicode_enabled() {
                out.push_str(&dim(&format!("  \u{2576} {} \u{2574}\n", msg), theme));
            } else {
                out.push_str(&dim(&format!("  > {} <\n", msg), theme));
            }
            continue;
        }

        for i in range.start..range.end {
            let line = &hunk.lines[i];
            render_line(line, theme, out);
        }
    }
}

/// A range of lines to render (either visible or collapsed).
struct LineRange {
    start: usize,
    end: usize,
    is_collapsed: bool,
}

/// Find which line ranges to render, collapsing context-only sections.
fn find_change_ranges(lines: &[DiffLine], context_lines: usize) -> Vec<LineRange> {
    if lines.is_empty() {
        return Vec::new();
    }

    // Find indices of addition/deletion lines
    let change_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind != DiffLineKind::Context)
        .map(|(i, _)| i)
        .collect();

    if change_indices.is_empty() {
        return vec![LineRange {
            start: 0,
            end: lines.len(),
            is_collapsed: true,
        }];
    }

    // Build visible ranges: for each change, show context_lines before and after
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for idx in &change_indices {
        let start = idx.saturating_sub(context_lines);
        let end = (idx + 1 + context_lines).min(lines.len());
        ranges.push((start, end));
    }

    // Merge overlapping ranges
    ranges.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    // Build LineRanges with collapsed sections between gaps
    let mut result = Vec::new();
    let mut cursor = 0;

    for (start, end) in &merged {
        if *start > cursor {
            result.push(LineRange {
                start: cursor,
                end: *start,
                is_collapsed: true,
            });
        }
        result.push(LineRange {
            start: *start,
            end: *end,
            is_collapsed: false,
        });
        cursor = *end;
    }

    if cursor < lines.len() {
        result.push(LineRange {
            start: cursor,
            end: lines.len(),
            is_collapsed: true,
        });
    }

    result
}

/// Render a single diff line with line numbers and colors.
fn render_line(line: &DiffLine, theme: &Theme, out: &mut String) {
    let old = line
        .old_lineno
        .map(|n| format!("{:>4}", n))
        .unwrap_or_else(|| "    ".to_string());
    let new = line
        .new_lineno
        .map(|n| format!("{:>4}", n))
        .unwrap_or_else(|| "    ".to_string());

    match line.kind {
        DiffLineKind::Context => {
            let dimmed_old = dim(&old, theme);
            let dimmed_new = dim(&new, theme);
            let dimmed_content = dim(&line.content, theme);
            if theme.colors_enabled() {
                out.push_str(&format!(
                    " {} {} \u{2502} {}\n",
                    dimmed_old, dimmed_new, dimmed_content
                ));
            } else {
                out.push_str(&format!(" {} {} | {}\n", old, new, line.content));
            }
        }
        DiffLineKind::Addition => {
            let dimmed_old = dim(&old, theme);
            if theme.colors_enabled() {
                out.push_str(&format!(
                    " {} {} \u{2502} \x1b[32m{}\x1b[0m\n",
                    dimmed_old, new, line.content
                ));
            } else {
                out.push_str(&format!(" {} {} + {}\n", old, new, line.content));
            }
        }
        DiffLineKind::Deletion => {
            let dimmed_new = dim(&new, theme);
            if theme.colors_enabled() {
                out.push_str(&format!(
                    " {} {} \u{2502} \x1b[31m{}\x1b[0m\n",
                    old, dimmed_new, line.content
                ));
            } else {
                out.push_str(&format!(" {} {} - {}\n", old, new, line.content));
            }
        }
    }
}

/// Render the summary bar at the bottom.
fn render_summary(files: &[DiffFile], theme: &Theme, out: &mut String) {
    let total_additions: u32 = files.iter().map(|f| f.additions).sum();
    let total_deletions: u32 = files.iter().map(|f| f.deletions).sum();
    let total_files = files.len();

    let summary = format!(
        " {} file{} changed, {} insertion{}(+), {} deletion{}(-)",
        total_files,
        if total_files == 1 { "" } else { "s" },
        total_additions,
        if total_additions == 1 { "" } else { "s" },
        total_deletions,
        if total_deletions == 1 { "" } else { "s" },
    );

    out.push('\n');
    if theme.colors_enabled() {
        out.push_str(&format!("\x1b[1m{}\x1b[0m\n", summary));
    } else {
        out.push_str(&format!("{}\n", summary));
    }
}

/// Apply dim styling if colors are enabled.
fn dim<'a>(s: &'a str, theme: &Theme) -> std::borrow::Cow<'a, str> {
    if theme.colors_enabled() {
        use colored::Colorize;
        std::borrow::Cow::Owned(s.dimmed().to_string())
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// Truncate a string in the middle if it exceeds max_width.
fn truncate_mid(s: &str, max_width: usize) -> String {
    let w = s.width();
    if w <= max_width || max_width < 5 {
        return s.to_string();
    }
    // Show start and end, with "…" in the middle
    let each = (max_width - 1) / 2; // 1 for "…"
    let mut result = String::with_capacity(max_width);
    for (i, ch) in s.chars().enumerate() {
        if i < each {
            result.push(ch);
        } else {
            break;
        }
    }
    result.push('\u{2026}');
    let end_start = s.len().saturating_sub(each);
    for (i, ch) in s[end_start..].char_indices() {
        if i + result.len() < max_width {
            result.push(ch);
        } else {
            break;
        }
    }
    result
}

/// Get terminal width.
fn terminal_width() -> Option<usize> {
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(n) = cols.parse::<usize>() {
            if n > 0 {
                return Some(n);
            }
        }
    }
    let output = std::process::Command::new("stty")
        .arg("size")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let fields: Vec<&str> = std::str::from_utf8(&output.stdout)
        .ok()?
        .split_whitespace()
        .collect();
    fields.get(1)?.parse::<usize>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::parser::parse;

    fn test_theme() -> Theme {
        Theme::test_instance(false, false)
    }

    #[test]
    fn test_find_change_ranges_simple() {
        let lines = vec![
            DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(1),
                new_lineno: Some(1),
                content: "a".into(),
            },
            DiffLine {
                kind: DiffLineKind::Addition,
                old_lineno: None,
                new_lineno: Some(2),
                content: "b".into(),
            },
            DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(2),
                new_lineno: Some(3),
                content: "c".into(),
            },
        ];
        let ranges = find_change_ranges(&lines, 3);
        assert!(!ranges.is_empty());
        assert!(!ranges[0].is_collapsed);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, 3);
    }

    #[test]
    fn test_find_change_ranges_collapsed() {
        let mut lines = Vec::new();
        for i in 1..=15 {
            lines.push(DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(i),
                new_lineno: Some(i),
                content: "ctx".into(),
            });
        }
        lines.push(DiffLine {
            kind: DiffLineKind::Addition,
            old_lineno: None,
            new_lineno: Some(16),
            content: "add".into(),
        });
        for i in 17u32..=31 {
            lines.push(DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(i.saturating_sub(1)),
                new_lineno: Some(i),
                content: "ctx".into(),
            });
        }

        let ranges = find_change_ranges(&lines, 3);
        assert!(ranges[0].is_collapsed);
        assert!(!ranges[1].is_collapsed);
        assert!(ranges[2].is_collapsed);
    }

    #[test]
    fn test_render_empty_diff() {
        let files = parse("");
        let options = DiffRenderOptions::default();
        let theme = test_theme();
        let result = render(&files, &options, &theme);
        assert!(!result.is_empty());
        assert!(result.contains("no diff content"));
    }

    #[test]
    fn test_render_simple_diff() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn hello() {
-    println!(\"goodbye\");
+    println!(\"hello\");
+    println!(\"world\");
 }
";
        let files = parse(diff);
        assert_eq!(files.len(), 1);
        let options = DiffRenderOptions::default();
        let result = render(&files, &options, &test_theme());
        assert!(
            result.contains("src/main.rs"),
            "path should appear in output"
        );
        assert!(result.contains("1 file changed"), "summary should appear");
        assert!(result.contains("2"), "additions count");
        assert!(result.contains("1"), "deletions count");
    }

    #[test]
    fn test_render_multiple_files() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-foo
+bar
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -1,1 +1,1 @@
-old
+new
";
        let files = parse(diff);
        assert_eq!(files.len(), 2);
        let options = DiffRenderOptions::default();
        let result = render(&files, &options, &test_theme());
        assert!(result.contains("2 files changed"));
        assert!(result.contains("a.rs"));
        assert!(result.contains("b.rs"));
    }
}
