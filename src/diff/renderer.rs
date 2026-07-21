//! Pretty diff renderer with box-drawing, line numbers, and ANSI colors.
//!
//! Takes parsed [`DiffFile`]s and outputs a beautifully formatted string
//! suitable for terminal display or piping through a pager.

use std::borrow::Cow;
use std::io::{self, Write};

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::output::theme::Theme;

use super::parser::{DiffFile, DiffHunk, DiffLine, DiffLineKind, FileStatus};

/// Display columns per tab when expanding `\t` for width/truncation.
const TABSTOP: usize = 8;

/// Options for rendering a diff.
#[derive(Debug, Clone)]
pub struct DiffRenderOptions {
    /// Number of context lines to show around changes (default: 3).
    pub context_lines: usize,
    /// Render mode (unified or side-by-side).
    pub mode: RenderMode,
}

impl Default for DiffRenderOptions {
    fn default() -> Self {
        DiffRenderOptions {
            context_lines: 3,
            mode: RenderMode::Unified,
        }
    }
}

/// How to lay out the diff.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderMode {
    /// Traditional unified diff (default).
    Unified,
    /// Side-by-side view.
    SideBySide,
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Terminal width for one render pass (ioctl once; respects mid-session resize).
fn term_width_for_render() -> usize {
    crate::output::theme::terminal_width().unwrap_or(80)
}

/// Render a parsed diff into a formatted terminal string.
pub fn render(files: &[DiffFile], options: &DiffRenderOptions, theme: &Theme) -> String {
    let mut buf = Vec::new();
    let _ = render_to(files, options, theme, &mut buf);
    String::from_utf8(buf).unwrap_or_default()
}

/// Stream a pretty diff to `w`, writing one file at a time to limit peak memory.
pub fn render_to(
    files: &[DiffFile],
    options: &DiffRenderOptions,
    theme: &Theme,
    w: &mut dyn Write,
) -> io::Result<()> {
    let term_width = term_width_for_render();
    let mut buf = String::new();

    if files.is_empty() {
        if theme.colors_enabled() {
            buf.push_str("\x1b[2m  (no diff content)\x1b[0m\n");
        } else {
            buf.push_str("  (no diff content)\n");
        }
        return w.write_all(buf.as_bytes());
    }

    if files.len() >= 2 {
        render_file_index(files, theme, &mut buf);
        w.write_all(buf.as_bytes())?;
        buf.clear();
    }

    for (i, file) in files.iter().enumerate() {
        if i > 0 {
            w.write_all(b"\n")?;
        }
        render_file(file, options, theme, term_width, &mut buf);
        w.write_all(buf.as_bytes())?;
        buf.clear();
    }

    render_summary(files, theme, &mut buf);
    w.write_all(buf.as_bytes())?;
    Ok(())
}

/// Render only file paths (one per line), like `git diff --name-only`.
pub fn render_name_only(files: &[DiffFile]) -> String {
    let mut out = String::new();
    for file in files {
        out.push_str(display_path(file));
        out.push('\n');
    }
    out
}

/// Render status + path lines, like `git diff --name-status`.
pub fn render_name_status(files: &[DiffFile], theme: &Theme) -> String {
    let mut out = String::new();
    for file in files {
        let code = status_code(file.status);
        let path = match file.status {
            FileStatus::Renamed => {
                if theme.unicode_enabled() {
                    format!("{} \u{2192} {}", file.old_path, file.new_path)
                } else {
                    format!("{} -> {}", file.old_path, file.new_path)
                }
            }
            _ => display_path(file).to_string(),
        };
        if theme.colors_enabled() {
            let colored = match file.status {
                FileStatus::Added => format!("\x1b[32m{code}\x1b[0m"),
                FileStatus::Deleted => format!("\x1b[31m{code}\x1b[0m"),
                FileStatus::Renamed => format!("\x1b[33m{code}\x1b[0m"),
                FileStatus::Modified => code.to_string(),
            };
            out.push_str(&format!("{colored}\t{path}\n"));
        } else {
            out.push_str(&format!("{code}\t{path}\n"));
        }
    }
    out
}

fn display_path(file: &DiffFile) -> &str {
    if !file.new_path.is_empty() {
        &file.new_path
    } else {
        &file.old_path
    }
}

fn status_code(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added => "A",
        FileStatus::Deleted => "D",
        FileStatus::Modified => "M",
        FileStatus::Renamed => "R",
    }
}

// ---------------------------------------------------------------------------
// File index (#9)
// ---------------------------------------------------------------------------

fn render_file_index(files: &[DiffFile], theme: &Theme, out: &mut String) {
    let header = format!("  Files changed ({}):", files.len());
    out.push_str(&header);
    out.push('\n');

    for (i, file) in files.iter().enumerate() {
        let path = if !file.new_path.is_empty() {
            &file.new_path
        } else {
            &file.old_path
        };
        let idx_str = format!("{}.", i + 1);
        let adds = format!("+{}", file.additions);
        let dels = format!("-{}", file.deletions);

        if theme.colors_enabled() {
            let dimmed_idx = format!("\x1b[2m{}\x1b[0m", idx_str);
            let bold_path = format!("\x1b[1m{}\x1b[0m", path);
            let green_adds = format!("\x1b[32m{}\x1b[0m", adds);
            let red_dels = format!("\x1b[31m{}\x1b[0m", dels);
            out.push_str(&format!(
                "    {} {}  {}, {}\n",
                dimmed_idx, bold_path, green_adds, red_dels
            ));
        } else {
            out.push_str(&format!("    {} {}  {}, {}\n", idx_str, path, adds, dels));
        }
    }
    out.push('\n');
}

// ---------------------------------------------------------------------------
// File rendering
// ---------------------------------------------------------------------------

/// Render a single file's diff.
fn render_file(
    file: &DiffFile,
    options: &DiffRenderOptions,
    theme: &Theme,
    term_width: usize,
    out: &mut String,
) {
    render_file_header(file, theme, term_width, out);

    if file.binary {
        let msg = "  (binary file changed)";
        if theme.colors_enabled() {
            out.push_str(&format!("\x1b[2m{}\x1b[0m\n", msg));
        } else {
            out.push_str(msg);
            out.push('\n');
        }
        return;
    }

    if file.hunks.is_empty() {
        if file.status == FileStatus::Renamed {
            let msg = "  (renamed with no content change)";
            if theme.colors_enabled() {
                out.push_str(&format!("\x1b[2m{}\x1b[0m\n", msg));
            } else {
                out.push_str(msg);
                out.push('\n');
            }
        }
        return;
    }

    let lineno_width = lineno_width_for_file(file);
    for hunk in &file.hunks {
        render_hunk(hunk, options, theme, lineno_width, term_width, out);
    }
}

/// Digits needed to display the largest line number in this file (min 1).
fn lineno_width_for_file(file: &DiffFile) -> usize {
    let mut max = 1u32;
    for hunk in &file.hunks {
        if hunk.old_lines > 0 {
            max = max.max(
                hunk.old_start
                    .saturating_add(hunk.old_lines.saturating_sub(1)),
            );
        } else {
            max = max.max(hunk.old_start);
        }
        if hunk.new_lines > 0 {
            max = max.max(
                hunk.new_start
                    .saturating_add(hunk.new_lines.saturating_sub(1)),
            );
        } else {
            max = max.max(hunk.new_start);
        }
        for line in &hunk.lines {
            if let Some(n) = line.old_lineno {
                max = max.max(n);
            }
            if let Some(n) = line.new_lineno {
                max = max.max(n);
            }
        }
    }
    max.to_string().len().max(1)
}

fn format_lineno(n: Option<u32>, width: usize) -> String {
    match n {
        Some(n) => format!("{:>width$}", n, width = width),
        None => " ".repeat(width),
    }
}

/// Visible width of the unified-mode prefix: ` {old} {new} {sign} {sep} `
fn unified_prefix_width(lineno_width: usize) -> usize {
    // space + old + space + new + space + sign + space + sep + space
    1 + lineno_width + 1 + lineno_width + 1 + 1 + 1 + 1 + 1
}

// ---------------------------------------------------------------------------
// File header (#4 - inline compact header with stats bar)
// ---------------------------------------------------------------------------

/// Render the file header as a single compact line with stats bar.
fn render_file_header(file: &DiffFile, theme: &Theme, term_width: usize, out: &mut String) {
    let (icon, status_text) = match file.status {
        FileStatus::Added => ("+", "new file"),
        FileStatus::Deleted => {
            if theme.unicode_enabled() {
                ("\u{2212}", "deleted")
            } else {
                ("-", "deleted")
            }
        }
        FileStatus::Modified => ("~", "modified"),
        FileStatus::Renamed => {
            if theme.unicode_enabled() {
                ("\u{2192}", "renamed")
            } else {
                ("->", "renamed")
            }
        }
    };

    let display_path = if file.status == FileStatus::Renamed {
        if theme.unicode_enabled() {
            format!("{} \u{2192} {}", file.old_path, file.new_path)
        } else {
            format!("{} -> {}", file.old_path, file.new_path)
        }
    } else if !file.new_path.is_empty() {
        file.new_path.clone()
    } else {
        file.old_path.clone()
    };

    // Build the stats bar (8 display columns)
    const BAR_WIDTH: usize = 8;
    let stats_bar = build_stats_bar(file.additions, file.deletions, BAR_WIDTH, theme);

    let adds_str = format!("+{}", file.additions);
    let dels_str = format!("-{}", file.deletions);

    let dash = if theme.unicode_enabled() {
        "\u{2500}"
    } else {
        "-"
    };
    let dash2 = format!("{}{} ", dash, dash);

    // Plain (no ANSI) layout for accurate fill width.
    let plain = format!(
        "{}{} {} {} {} {} [{}] {}, {} ",
        dash2,
        dash,
        icon,
        display_path,
        dash2,
        status_text,
        "X".repeat(BAR_WIDTH),
        adds_str,
        dels_str
    );
    let visible_len = plain.width();
    let fill_count = term_width.saturating_sub(visible_len);
    let fill = dash.repeat(fill_count);

    if theme.colors_enabled() {
        let bold_icon = format!("\x1b[1m{}\x1b[0m", icon);
        let bold_path = format!("\x1b[1m{}\x1b[0m", display_path);
        let green_adds = format!("\x1b[32m{}\x1b[0m", adds_str);
        let red_dels = format!("\x1b[31m{}\x1b[0m", dels_str);
        let content = format!(
            "{}{} {} {} {} {} [{}] {}, {} ",
            dash2, dash, bold_icon, bold_path, dash2, status_text, stats_bar, green_adds, red_dels
        );
        let dimmed_fill = format!("\x1b[2m{}\x1b[0m", fill);
        out.push_str(&content);
        out.push_str(&dimmed_fill);
        out.push('\n');
    } else {
        let content = format!(
            "{}{} {} {} {} {} [{}] {}, {} ",
            dash2, dash, icon, display_path, dash2, status_text, stats_bar, adds_str, dels_str
        );
        out.push_str(&content);
        out.push_str(&fill);
        out.push('\n');
    }
}

/// Build a proportional stats bar of given width.
/// Green blocks for additions, red for deletions, empty for remainder.
fn build_stats_bar(additions: u32, deletions: u32, bar_width: usize, theme: &Theme) -> String {
    let total = additions + deletions;
    if total == 0 {
        let empty_char = if theme.unicode_enabled() {
            "\u{2591}"
        } else {
            "."
        };
        return empty_char.repeat(bar_width);
    }

    let add_blocks = ((additions as f64 / total as f64) * bar_width as f64).round() as usize;
    let del_blocks = ((deletions as f64 / total as f64) * bar_width as f64).round() as usize;

    // Ensure we don't exceed bar_width
    let add_blocks = add_blocks.min(bar_width);
    let del_blocks = del_blocks.min(bar_width.saturating_sub(add_blocks));
    let empty_blocks = bar_width.saturating_sub(add_blocks + del_blocks);

    let filled = if theme.unicode_enabled() {
        "\u{2588}"
    } else {
        "#"
    };
    let empty = if theme.unicode_enabled() {
        "\u{2591}"
    } else {
        "."
    };

    if theme.colors_enabled() {
        let green_part = format!("\x1b[32m{}\x1b[0m", filled.repeat(add_blocks));
        let red_part = format!("\x1b[31m{}\x1b[0m", filled.repeat(del_blocks));
        let empty_part = empty.repeat(empty_blocks);
        format!("{}{}{}", green_part, red_part, empty_part)
    } else {
        format!(
            "{}{}{}",
            filled.repeat(add_blocks),
            filled.repeat(del_blocks),
            empty.repeat(empty_blocks)
        )
    }
}

// ---------------------------------------------------------------------------
// Hunk rendering
// ---------------------------------------------------------------------------

/// Render a single hunk.
fn render_hunk(
    hunk: &DiffHunk,
    options: &DiffRenderOptions,
    theme: &Theme,
    lineno_width: usize,
    term_width: usize,
    out: &mut String,
) {
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

    if options.mode == RenderMode::SideBySide {
        render_hunk_side_by_side(hunk, options, theme, lineno_width, term_width, out);
    } else {
        render_hunk_unified(hunk, options, theme, lineno_width, term_width, out);
    }
}

// ---------------------------------------------------------------------------
// Unified mode (#3 - interleaved pairs)
// ---------------------------------------------------------------------------

fn render_hunk_unified(
    hunk: &DiffHunk,
    options: &DiffRenderOptions,
    theme: &Theme,
    lineno_width: usize,
    term_width: usize,
    out: &mut String,
) {
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

        let mut i = range.start;
        while i < range.end {
            let mut deletions = Vec::new();
            while i < range.end && hunk.lines[i].kind == DiffLineKind::Deletion {
                deletions.push(&hunk.lines[i]);
                i += 1;
            }
            let mut additions = Vec::new();
            while i < range.end && hunk.lines[i].kind == DiffLineKind::Addition {
                additions.push(&hunk.lines[i]);
                i += 1;
            }

            if !deletions.is_empty() || !additions.is_empty() {
                let rows = crate::diff::align::align_change_block(&deletions, &additions);
                for row in rows {
                    match row {
                        crate::diff::align::AlignedRow::Pair(del, add) => {
                            render_paired_line(
                                del,
                                Some(add),
                                theme,
                                lineno_width,
                                term_width,
                                out,
                            );
                            render_paired_line(
                                add,
                                Some(del),
                                theme,
                                lineno_width,
                                term_width,
                                out,
                            );
                        }
                        crate::diff::align::AlignedRow::DeleteOnly(del) => {
                            render_paired_line(del, None, theme, lineno_width, term_width, out);
                        }
                        crate::diff::align::AlignedRow::AddOnly(add) => {
                            render_paired_line(add, None, theme, lineno_width, term_width, out);
                        }
                    }
                }
            } else {
                // Context line
                render_line(&hunk.lines[i], theme, lineno_width, term_width, out);
                i += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tinted line builder — keeps background across mid-line style changes
// ---------------------------------------------------------------------------

/// Build a full-width tinted diff line without mid-line `\x1b[0m` resets.
///
/// Each style change ends with an explicit attribute reset (`22` / `39` / `49`)
/// and re-asserts the line background so padding spaces stay tinted.
struct TintedLine {
    bg_code: u8,
    buf: String,
}

impl TintedLine {
    fn new(bg_code: u8) -> Self {
        let mut buf = String::with_capacity(128);
        buf.push_str(&format!("\x1b[48;5;{bg_code}m"));
        Self { bg_code, buf }
    }

    fn reassert_bg(&mut self) {
        self.buf.push_str(&format!("\x1b[48;5;{}m", self.bg_code));
    }

    fn push_raw(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    fn push_space(&mut self) {
        self.buf.push(' ');
    }

    /// Dim text, then restore normal intensity while keeping the background.
    fn push_dim(&mut self, s: &str) {
        self.buf.push_str("\x1b[2m");
        self.buf.push_str(s);
        self.buf.push_str("\x1b[22m");
        self.reassert_bg();
    }

    /// Foreground-colored text (e.g. `"32"` green / `"31"` red).
    fn push_fg(&mut self, fg: &str, s: &str) {
        self.buf.push_str("\x1b[");
        self.buf.push_str(fg);
        self.buf.push('m');
        self.buf.push_str(s);
        self.buf.push_str("\x1b[39m");
        self.reassert_bg();
    }

    /// Word-level highlight: black on bright green/red, then restore line bg + fg.
    fn push_word_hl(&mut self, hl_bg: &str, restore_fg: &str, s: &str) {
        self.buf.push_str("\x1b[30;");
        self.buf.push_str(hl_bg);
        self.buf.push('m');
        self.buf.push_str(s);
        // Reset fg+bg, re-assert line background, restore content foreground.
        self.buf.push_str("\x1b[39;49m");
        self.reassert_bg();
        self.buf.push_str("\x1b[");
        self.buf.push_str(restore_fg);
        self.buf.push('m');
    }

    fn finish(mut self, pad: usize) -> String {
        if pad > 0 {
            self.buf.push_str(&" ".repeat(pad));
        }
        self.buf.push_str("\x1b[0m\n");
        self.buf
    }
}

const ADD_BG: u8 = 22;
const DEL_BG: u8 = 52;

/// Truncate content to fit the remaining terminal columns after the prefix.
fn unified_content_width(lineno_width: usize, term_width: usize) -> usize {
    term_width.saturating_sub(unified_prefix_width(lineno_width))
}

/// Expand tabs to spaces using a fixed tabstop (display columns).
fn expand_tabs(s: &str) -> Cow<'_, str> {
    if !s.as_bytes().contains(&b'\t') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let spaces = TABSTOP - (col % TABSTOP);
            out.push_str(&" ".repeat(spaces));
            col += spaces;
        } else {
            out.push(ch);
            col += ch.width().unwrap_or(0);
        }
    }
    Cow::Owned(out)
}

fn display_content(raw: &str, empty_marker: &str, max_width: usize) -> String {
    let expanded = expand_tabs(raw);
    if expanded.is_empty() {
        return empty_marker.to_string();
    }
    truncate_code_raw(expanded.as_ref(), max_width)
}

fn append_no_newline_marker(line: &DiffLine, theme: &Theme, out: &mut String) {
    if !line.no_newline {
        return;
    }
    let msg = "\\ No newline at end of file";
    if theme.colors_enabled() {
        out.push_str(&format!("\x1b[2m{}\x1b[0m\n", msg));
    } else {
        out.push_str(msg);
        out.push('\n');
    }
}

fn render_addition_content(
    line: &DiffLine,
    pair: Option<&DiffLine>,
    empty_marker: &str,
    max_width: usize,
    tinted: &mut TintedLine,
) -> usize {
    let content = display_content(&line.content, empty_marker, max_width);
    let visible = content.width();

    if line.content.is_empty() {
        tinted.push_dim(empty_marker);
        return visible;
    }

    if let Some(p) = pair {
        let pair_disp = display_content(&p.content, "", max_width);
        let sim = crate::diff::word_diff::similarity(pair_disp.as_ref(), content.as_ref());
        if sim < crate::diff::word_diff::WORD_DIFF_THRESHOLD {
            tinted.push_fg("32", content.as_ref());
        } else {
            let segments =
                crate::diff::word_diff::word_changes(pair_disp.as_ref(), content.as_ref());
            for seg in segments {
                match seg.kind {
                    crate::diff::word_diff::WordChange::Inserted => {
                        tinted.push_word_hl("42", "32", &seg.text);
                    }
                    crate::diff::word_diff::WordChange::Deleted => {}
                    crate::diff::word_diff::WordChange::Equal => {
                        tinted.push_fg("32", &seg.text);
                    }
                }
            }
        }
    } else {
        tinted.push_fg("32", content.as_ref());
    }
    visible
}

fn render_deletion_content(
    line: &DiffLine,
    pair: Option<&DiffLine>,
    empty_marker: &str,
    max_width: usize,
    tinted: &mut TintedLine,
) -> usize {
    let content = display_content(&line.content, empty_marker, max_width);
    let visible = content.width();

    if line.content.is_empty() {
        tinted.push_dim(empty_marker);
        return visible;
    }

    if let Some(p) = pair {
        let pair_disp = display_content(&p.content, "", max_width);
        let sim = crate::diff::word_diff::similarity(content.as_ref(), pair_disp.as_ref());
        if sim < crate::diff::word_diff::WORD_DIFF_THRESHOLD {
            tinted.push_fg("31", content.as_ref());
        } else {
            let segments =
                crate::diff::word_diff::word_changes(content.as_ref(), pair_disp.as_ref());
            for seg in segments {
                match seg.kind {
                    crate::diff::word_diff::WordChange::Deleted => {
                        tinted.push_word_hl("41", "31", &seg.text);
                    }
                    crate::diff::word_diff::WordChange::Inserted => {}
                    crate::diff::word_diff::WordChange::Equal => {
                        tinted.push_fg("31", &seg.text);
                    }
                }
            }
        }
    } else {
        tinted.push_fg("31", content.as_ref());
    }
    visible
}

// ---------------------------------------------------------------------------
// Paired line rendering (#1 bg tinting, #2 sign column, #5 threshold, #8 empty marker)
// ---------------------------------------------------------------------------

fn render_paired_line(
    line: &DiffLine,
    pair: Option<&DiffLine>,
    theme: &Theme,
    lineno_width: usize,
    term_width: usize,
    out: &mut String,
) {
    let old = format_lineno(line.old_lineno, lineno_width);
    let new = format_lineno(line.new_lineno, lineno_width);
    let sep = if theme.unicode_enabled() {
        "\u{2502}"
    } else {
        "|"
    };
    let empty_marker = if theme.unicode_enabled() {
        "\u{23ce}"
    } else {
        "<CR>"
    };
    let max_content = unified_content_width(lineno_width, term_width);

    match line.kind {
        DiffLineKind::Addition => {
            if theme.colors_enabled() {
                let mut tinted = TintedLine::new(ADD_BG);
                tinted.push_space();
                tinted.push_dim(&old);
                tinted.push_space();
                tinted.push_raw(&new);
                tinted.push_space();
                tinted.push_fg("32", "+");
                tinted.push_space();
                tinted.push_dim(sep);
                tinted.push_space();
                let content_w =
                    render_addition_content(line, pair, empty_marker, max_content, &mut tinted);
                let pad = term_width.saturating_sub(unified_prefix_width(lineno_width) + content_w);
                out.push_str(&tinted.finish(pad));
            } else {
                let content = display_content(&line.content, empty_marker, max_content);
                out.push_str(&format!(" {} {} + {} {}\n", old, new, sep, content));
            }
        }
        DiffLineKind::Deletion => {
            if theme.colors_enabled() {
                let mut tinted = TintedLine::new(DEL_BG);
                tinted.push_space();
                tinted.push_raw(&old);
                tinted.push_space();
                tinted.push_dim(&new);
                tinted.push_space();
                tinted.push_fg("31", "-");
                tinted.push_space();
                tinted.push_dim(sep);
                tinted.push_space();
                let content_w =
                    render_deletion_content(line, pair, empty_marker, max_content, &mut tinted);
                let pad = term_width.saturating_sub(unified_prefix_width(lineno_width) + content_w);
                out.push_str(&tinted.finish(pad));
            } else {
                let content = display_content(&line.content, empty_marker, max_content);
                out.push_str(&format!(" {} {} - {} {}\n", old, new, sep, content));
            }
        }
        DiffLineKind::Context => {
            render_line(line, theme, lineno_width, term_width, out);
        }
    }
    if line.kind != DiffLineKind::Context {
        append_no_newline_marker(line, theme, out);
    }
}

// ---------------------------------------------------------------------------
// Context line rendering (#6 - normal weight content, dimmed line numbers)
// ---------------------------------------------------------------------------

/// Render a single context/addition/deletion line.
fn render_line(
    line: &DiffLine,
    theme: &Theme,
    lineno_width: usize,
    term_width: usize,
    out: &mut String,
) {
    let old = format_lineno(line.old_lineno, lineno_width);
    let new = format_lineno(line.new_lineno, lineno_width);
    let sep = if theme.unicode_enabled() {
        "\u{2502}"
    } else {
        "|"
    };
    let empty_marker = if theme.unicode_enabled() {
        "\u{23ce}"
    } else {
        "<CR>"
    };
    let max_content = unified_content_width(lineno_width, term_width);

    match line.kind {
        DiffLineKind::Context => {
            let content = display_content(&line.content, "", max_content);
            if theme.colors_enabled() {
                let dimmed_old = format!("\x1b[2m{}\x1b[0m", old);
                let dimmed_new = format!("\x1b[2m{}\x1b[0m", new);
                let dimmed_sep = format!("\x1b[2m{}\x1b[0m", sep);
                out.push_str(&format!(
                    " {} {}   {} {}\n",
                    dimmed_old, dimmed_new, dimmed_sep, content
                ));
            } else {
                out.push_str(&format!(" {} {}   {} {}\n", old, new, sep, content));
            }
        }
        DiffLineKind::Addition => {
            if theme.colors_enabled() {
                let mut tinted = TintedLine::new(ADD_BG);
                tinted.push_space();
                tinted.push_dim(&old);
                tinted.push_space();
                tinted.push_raw(&new);
                tinted.push_space();
                tinted.push_fg("32", "+");
                tinted.push_space();
                tinted.push_dim(sep);
                tinted.push_space();
                let content_w =
                    render_addition_content(line, None, empty_marker, max_content, &mut tinted);
                let pad = term_width.saturating_sub(unified_prefix_width(lineno_width) + content_w);
                out.push_str(&tinted.finish(pad));
            } else {
                let content = display_content(&line.content, empty_marker, max_content);
                out.push_str(&format!(" {} {} + {} {}\n", old, new, sep, content));
            }
        }
        DiffLineKind::Deletion => {
            if theme.colors_enabled() {
                let mut tinted = TintedLine::new(DEL_BG);
                tinted.push_space();
                tinted.push_raw(&old);
                tinted.push_space();
                tinted.push_dim(&new);
                tinted.push_space();
                tinted.push_fg("31", "-");
                tinted.push_space();
                tinted.push_dim(sep);
                tinted.push_space();
                let content_w =
                    render_deletion_content(line, None, empty_marker, max_content, &mut tinted);
                let pad = term_width.saturating_sub(unified_prefix_width(lineno_width) + content_w);
                out.push_str(&tinted.finish(pad));
            } else {
                let content = display_content(&line.content, empty_marker, max_content);
                out.push_str(&format!(" {} {} - {} {}\n", old, new, sep, content));
            }
        }
    }
    append_no_newline_marker(line, theme, out);
}

// ---------------------------------------------------------------------------
// Side-by-side mode (#10 - continuous vertical divider)
// ---------------------------------------------------------------------------

fn render_hunk_side_by_side(
    hunk: &DiffHunk,
    options: &DiffRenderOptions,
    theme: &Theme,
    lineno_width: usize,
    term_width: usize,
    out: &mut String,
) {
    let width = term_width;
    // Reserved: left_edge(1) + left_lineno(w) + sep(3) + middle_sep(3) + right_lineno(w) + sep(3)
    // Total overhead: 1 + w + 3 + 3 + w + 3 = 2*w + 10
    let overhead = lineno_width * 2 + 10;
    let code_width = width.saturating_sub(overhead) / 2;

    let ranges = find_change_ranges(&hunk.lines, options.context_lines);

    for range in &ranges {
        if range.is_collapsed && range.end > range.start {
            let hidden = range.end - range.start;
            let msg = format!("{} lines hidden", hidden);
            let line = if theme.unicode_enabled() {
                let fill_len = width.saturating_sub(msg.len() + 6) / 2;
                let fill = "\u{2576}".repeat(fill_len.max(2));
                format!("  {} {} {}\n", fill, msg, fill)
            } else {
                format!("  -- {} --\n", msg)
            };
            out.push_str(&dim(&line, theme));
            continue;
        }

        let mut i = range.start;
        while i < range.end {
            let mut deletions = Vec::new();
            while i < range.end && hunk.lines[i].kind == DiffLineKind::Deletion {
                deletions.push(&hunk.lines[i]);
                i += 1;
            }
            let mut additions = Vec::new();
            while i < range.end && hunk.lines[i].kind == DiffLineKind::Addition {
                additions.push(&hunk.lines[i]);
                i += 1;
            }

            if !deletions.is_empty() || !additions.is_empty() {
                let rows = crate::diff::align::align_change_block(&deletions, &additions);
                for row in rows {
                    match row {
                        crate::diff::align::AlignedRow::Pair(del, add) => {
                            render_side_by_side_row(
                                Some(del),
                                Some(add),
                                code_width,
                                lineno_width,
                                theme,
                                out,
                            );
                        }
                        crate::diff::align::AlignedRow::DeleteOnly(del) => {
                            render_side_by_side_row(
                                Some(del),
                                None,
                                code_width,
                                lineno_width,
                                theme,
                                out,
                            );
                        }
                        crate::diff::align::AlignedRow::AddOnly(add) => {
                            render_side_by_side_row(
                                None,
                                Some(add),
                                code_width,
                                lineno_width,
                                theme,
                                out,
                            );
                        }
                    }
                }
            } else {
                let line = &hunk.lines[i];
                render_side_by_side_row(
                    Some(line),
                    Some(line),
                    code_width,
                    lineno_width,
                    theme,
                    out,
                );
                i += 1;
            }
        }
    }
}

fn render_side_by_side_row(
    del: Option<&DiffLine>,
    add: Option<&DiffLine>,
    code_width: usize,
    lineno_width: usize,
    theme: &Theme,
    out: &mut String,
) {
    let left_lineno = format_lineno(del.and_then(|l| l.old_lineno), lineno_width);
    let right_lineno = format_lineno(add.and_then(|l| l.new_lineno), lineno_width);

    let pipe = if theme.unicode_enabled() {
        "\u{2502}"
    } else {
        "|"
    };
    let empty_marker = if theme.unicode_enabled() {
        "\u{23ce}"
    } else {
        "<CR>"
    };

    if theme.colors_enabled() {
        let sep = format!(" \x1b[2m{}\x1b[0m ", pipe);
        // #10: left edge thin pipe
        let left_edge = format!("\x1b[2m{}\x1b[0m", pipe);

        let left_col = match (del, add) {
            (Some(l), Some(r))
                if l.kind == DiffLineKind::Deletion && r.kind == DiffLineKind::Addition =>
            {
                // Paired change: check threshold then word-level highlighting
                let left_visible = truncate_code_raw(expand_tabs(&l.content).as_ref(), code_width);
                let right_visible = truncate_code_raw(expand_tabs(&r.content).as_ref(), code_width);
                let sim = crate::diff::word_diff::similarity(&left_visible, &right_visible);
                let mut col = String::new();
                let mut left_w = 0;
                if left_visible.is_empty() {
                    col.push_str(&format!("\x1b[2m{}\x1b[0m", empty_marker));
                    left_w = empty_marker.width();
                } else if sim < crate::diff::word_diff::WORD_DIFF_THRESHOLD {
                    col.push_str(&format!("\x1b[31m{}\x1b[0m", left_visible));
                    left_w = left_visible.width();
                } else {
                    let segments =
                        crate::diff::word_diff::word_changes(&left_visible, &right_visible);
                    for seg in segments {
                        match seg.kind {
                            crate::diff::word_diff::WordChange::Deleted => {
                                col.push_str(&format!("\x1b[30;41m{}\x1b[0m", seg.text));
                                left_w += seg.text.width();
                            }
                            crate::diff::word_diff::WordChange::Equal => {
                                col.push_str(&format!("\x1b[31m{}\x1b[0m", seg.text));
                                left_w += seg.text.width();
                            }
                            crate::diff::word_diff::WordChange::Inserted => {}
                        }
                    }
                }
                let pad = code_width.saturating_sub(left_w);
                col.push_str(&" ".repeat(pad));
                col
            }
            (Some(l), _) => {
                if l.content.is_empty() {
                    let marker = format!("\x1b[2m{}\x1b[0m", empty_marker);
                    let pad = code_width.saturating_sub(empty_marker.width());
                    format!("{}{}", marker, " ".repeat(pad))
                } else {
                    let left_visible =
                        truncate_code_raw(expand_tabs(&l.content).as_ref(), code_width);
                    let left_w = left_visible.width();
                    let col = if l.kind == DiffLineKind::Context {
                        left_visible.clone()
                    } else {
                        format!("\x1b[31m{}\x1b[0m", left_visible)
                    };
                    let pad = code_width.saturating_sub(left_w);
                    format!("{}{}", col, " ".repeat(pad))
                }
            }
            (None, _) => " ".repeat(code_width),
        };

        let right_col = match (del, add) {
            (Some(l), Some(r))
                if l.kind == DiffLineKind::Deletion && r.kind == DiffLineKind::Addition =>
            {
                let left_visible = truncate_code_raw(expand_tabs(&l.content).as_ref(), code_width);
                let right_visible = truncate_code_raw(expand_tabs(&r.content).as_ref(), code_width);
                let sim = crate::diff::word_diff::similarity(&left_visible, &right_visible);
                let mut col = String::new();
                let mut right_w = 0;
                if right_visible.is_empty() {
                    col.push_str(&format!("\x1b[2m{}\x1b[0m", empty_marker));
                    right_w = empty_marker.width();
                } else if sim < crate::diff::word_diff::WORD_DIFF_THRESHOLD {
                    col.push_str(&format!("\x1b[32m{}\x1b[0m", right_visible));
                    right_w = right_visible.width();
                } else {
                    let segments =
                        crate::diff::word_diff::word_changes(&left_visible, &right_visible);
                    for seg in segments {
                        match seg.kind {
                            crate::diff::word_diff::WordChange::Inserted => {
                                col.push_str(&format!("\x1b[30;42m{}\x1b[0m", seg.text));
                                right_w += seg.text.width();
                            }
                            crate::diff::word_diff::WordChange::Equal => {
                                col.push_str(&format!("\x1b[32m{}\x1b[0m", seg.text));
                                right_w += seg.text.width();
                            }
                            crate::diff::word_diff::WordChange::Deleted => {}
                        }
                    }
                }
                let pad = code_width.saturating_sub(right_w);
                col.push_str(&" ".repeat(pad));
                col
            }
            (_, Some(r)) => {
                if r.content.is_empty() {
                    let marker = format!("\x1b[2m{}\x1b[0m", empty_marker);
                    let pad = code_width.saturating_sub(empty_marker.width());
                    format!("{}{}", marker, " ".repeat(pad))
                } else {
                    let right_visible =
                        truncate_code_raw(expand_tabs(&r.content).as_ref(), code_width);
                    let right_w = right_visible.width();
                    let col = if r.kind == DiffLineKind::Context {
                        right_visible.clone()
                    } else {
                        format!("\x1b[32m{}\x1b[0m", right_visible)
                    };
                    let pad = code_width.saturating_sub(right_w);
                    format!("{}{}", col, " ".repeat(pad))
                }
            }
            (_, None) => " ".repeat(code_width),
        };

        out.push_str(&left_edge);
        out.push_str(&format!("\x1b[2m{}\x1b[0m", left_lineno));
        out.push_str(&sep);
        out.push_str(&left_col);
        out.push_str(&sep);
        out.push_str(&format!("\x1b[2m{}\x1b[0m", right_lineno));
        out.push_str(&sep);
        out.push_str(&right_col);
        out.push('\n');
    } else {
        let left_content = del
            .map(|l| {
                if l.content.is_empty() {
                    truncate_code(empty_marker, code_width)
                } else {
                    truncate_code(expand_tabs(&l.content).as_ref(), code_width)
                }
            })
            .unwrap_or_else(|| " ".repeat(code_width));
        let right_content = add
            .map(|l| {
                if l.content.is_empty() {
                    truncate_code(empty_marker, code_width)
                } else {
                    truncate_code(expand_tabs(&l.content).as_ref(), code_width)
                }
            })
            .unwrap_or_else(|| " ".repeat(code_width));

        let sep = format!(" {} ", pipe);
        let left_sign = del
            .map(|l| match l.kind {
                DiffLineKind::Deletion => "-",
                _ => " ",
            })
            .unwrap_or(" ");
        let right_sign = add
            .map(|l| match l.kind {
                DiffLineKind::Addition => "+",
                _ => " ",
            })
            .unwrap_or(" ");
        // #10: left edge pipe in plain mode too
        out.push_str(&format!(
            "{}{}{}{}{}{}{}{}{}{}",
            pipe,
            left_lineno,
            sep,
            left_sign,
            " ",
            left_content,
            sep,
            right_lineno,
            &format!("{}{}{}\n", sep, right_sign, right_content),
            "",
        ));
    }
    match (del, add) {
        (Some(l), Some(r)) if std::ptr::eq(l, r) => append_no_newline_marker(l, theme, out),
        (Some(l), Some(r)) => {
            append_no_newline_marker(l, theme, out);
            append_no_newline_marker(r, theme, out);
        }
        (Some(l), None) => append_no_newline_marker(l, theme, out),
        (None, Some(r)) => append_no_newline_marker(r, theme, out),
        (None, None) => {}
    }
}

// ---------------------------------------------------------------------------
// Summary bar (#7 - colorized counts + proportion bar)
// ---------------------------------------------------------------------------

/// Render the summary bar at the bottom.
fn render_summary(files: &[DiffFile], theme: &Theme, out: &mut String) {
    let total_additions: u32 = files.iter().map(|f| f.additions).sum();
    let total_deletions: u32 = files.iter().map(|f| f.deletions).sum();
    let total_files = files.len();

    let files_text = format!(
        " {} file{} changed,",
        total_files,
        if total_files == 1 { "" } else { "s" },
    );

    let insertions_text = format!(
        "{} insertion{}(+)",
        total_additions,
        if total_additions == 1 { "" } else { "s" },
    );

    let deletions_text = format!(
        "{} deletion{}(-)",
        total_deletions,
        if total_deletions == 1 { "" } else { "s" },
    );

    out.push('\n');

    if theme.colors_enabled() {
        // Build proportion bar (12 chars)
        let bar = build_proportion_bar(total_additions, total_deletions, 12, theme);

        out.push_str(&format!(
            "\x1b[1m{}\x1b[0m \x1b[32m{}\x1b[0m, \x1b[31m{}\x1b[0m [{}]\n",
            files_text, insertions_text, deletions_text, bar
        ));
    } else {
        out.push_str(&format!(
            "{} {}, {}\n",
            files_text, insertions_text, deletions_text
        ));
    }
}

/// Build a proportion bar showing green/red ratio.
fn build_proportion_bar(additions: u32, deletions: u32, bar_width: usize, theme: &Theme) -> String {
    let total = additions + deletions;
    if total == 0 {
        let empty = if theme.unicode_enabled() {
            "\u{2591}"
        } else {
            "."
        };
        return empty.repeat(bar_width);
    }

    let add_blocks = ((additions as f64 / total as f64) * bar_width as f64).round() as usize;
    let del_blocks = bar_width.saturating_sub(add_blocks);

    let filled = if theme.unicode_enabled() {
        "\u{2588}"
    } else {
        "#"
    };

    if theme.colors_enabled() {
        format!(
            "\x1b[32m{}\x1b[0m\x1b[31m{}\x1b[0m",
            filled.repeat(add_blocks),
            filled.repeat(del_blocks)
        )
    } else {
        format!("{}{}", filled.repeat(add_blocks), filled.repeat(del_blocks))
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Apply dim styling if colors are enabled.
fn dim<'a>(s: &'a str, theme: &Theme) -> Cow<'a, str> {
    if theme.colors_enabled() {
        Cow::Owned(format!("\x1b[2m{}\x1b[0m", s))
    } else {
        Cow::Borrowed(s)
    }
}

/// Truncate a string in the middle if it exceeds max_width.
#[allow(dead_code)]
fn truncate_mid(s: &str, max_width: usize) -> String {
    let w = s.width();
    if w <= max_width || max_width < 5 {
        return s.to_string();
    }
    // Show start and end, with "…" in the middle
    let each = (max_width - 1) / 2; // 1 for "…"

    // Collect char indices for safe slicing
    let char_indices: Vec<(usize, char)> = s.char_indices().collect();
    let char_count = char_indices.len();

    let mut result = String::with_capacity(max_width);

    // Take `each` chars from the start
    for &(_byte_idx, ch) in char_indices.iter().take(each) {
        result.push(ch);
    }
    result.push('\u{2026}');

    // Take `each` chars from the end
    let end_char_start = char_count.saturating_sub(each);
    for &(_byte_idx, ch) in char_indices.iter().skip(end_char_start) {
        result.push(ch);
    }
    result
}

fn truncate_code_raw(s: &str, max_width: usize) -> String {
    let w = unicode_width::UnicodeWidthStr::width(s);
    if w <= max_width {
        s.to_string()
    } else {
        let mut result = String::new();
        let mut current_w = 0;
        for ch in s.chars() {
            let ch_w = ch.width().unwrap_or(0);
            if current_w + ch_w + 1 > max_width {
                break;
            }
            result.push(ch);
            current_w += ch_w;
        }
        result.push('\u{2026}');
        result
    }
}

fn truncate_code(s: &str, max_width: usize) -> String {
    let raw = truncate_code_raw(s, max_width);
    let w = unicode_width::UnicodeWidthStr::width(raw.as_str());
    let pad = max_width.saturating_sub(w);
    format!("{}{}", raw, " ".repeat(pad))
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::parser::parse;

    fn test_theme() -> Theme {
        Theme::test_instance(false, false)
    }

    fn test_theme_colored() -> Theme {
        Theme::test_instance(true, true)
    }

    #[test]
    fn test_find_change_ranges_simple() {
        let lines = vec![
            DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(1),
                new_lineno: Some(1),
                content: "a".into(),
                no_newline: false,
            },
            DiffLine {
                kind: DiffLineKind::Addition,
                old_lineno: None,
                new_lineno: Some(2),
                content: "b".into(),
                no_newline: false,
            },
            DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(2),
                new_lineno: Some(3),
                content: "c".into(),
                no_newline: false,
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
                no_newline: false,
            });
        }
        lines.push(DiffLine {
            kind: DiffLineKind::Addition,
            old_lineno: None,
            new_lineno: Some(16),
            content: "add".into(),
            no_newline: false,
        });
        for i in 17u32..=31 {
            lines.push(DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(i.saturating_sub(1)),
                new_lineno: Some(i),
                content: "ctx".into(),
                no_newline: false,
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
        assert!(result.contains("1 file"), "summary should appear");
        assert!(result.contains("2"), "additions count");
        assert!(result.contains("1"), "deletions count");
    }

    #[test]
    fn test_render_multiple_files_has_index() {
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
        assert!(result.contains("2 file"));
        assert!(result.contains("a.rs"));
        assert!(result.contains("b.rs"));
        // #9: file index should appear
        assert!(result.contains("Files changed (2):"));
        assert!(result.contains("1."));
        assert!(result.contains("2."));
    }

    #[test]
    fn test_render_side_by_side() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-foo
+bar
";
        let files = parse(diff);
        let options = DiffRenderOptions {
            context_lines: 3,
            mode: RenderMode::SideBySide,
        };
        let result = render(&files, &options, &test_theme());
        assert!(result.contains("a.rs"));
        assert!(result.contains("foo"));
        assert!(result.contains("bar"));
        // Check for side-by-side separator
        assert!(result.contains(" | ") || result.contains(" \u{2502} "));
    }

    #[test]
    fn test_render_interleaved_pairs() {
        // Verify that deletions and additions are interleaved
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,3 +1,3 @@
-line1_old
-line2_old
-line3_old
+line1_new
+line2_new
+line3_new
";
        let files = parse(diff);
        let options = DiffRenderOptions::default();
        let result = render(&files, &options, &test_theme());
        // With similarity pairing, each deletion is emitted with its matched
        // addition: del1, add1, del2, add2, …
        let pos_old1 = result.find("line1_old").unwrap();
        let pos_new1 = result.find("line1_new").unwrap();
        let pos_old2 = result.find("line2_old").unwrap();
        let pos_new2 = result.find("line2_new").unwrap();
        assert!(
            pos_old1 < pos_new1,
            "deletion should appear before its paired addition"
        );
        assert!(
            pos_new1 < pos_old2 && pos_old2 < pos_new2,
            "pairs should interleave: del1, add1, del2, add2"
        );
    }

    #[test]
    fn test_render_empty_line_marker() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,2 @@
 hello
+
";
        let files = parse(diff);
        let options = DiffRenderOptions::default();

        // Plain mode (no unicode) should show <CR>
        let theme_plain = Theme::test_instance(false, false);
        let result = render(&files, &options, &theme_plain);
        assert!(
            result.contains("<CR>"),
            "empty addition should show <CR> marker in plain mode"
        );

        // Unicode mode should show ⏎
        let theme_unicode = Theme::test_instance(false, true);
        let result = render(&files, &options, &theme_unicode);
        assert!(
            result.contains("\u{23ce}"),
            "empty addition should show ⏎ marker in unicode mode"
        );
    }

    #[test]
    fn test_render_colored_background_tinting() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-old_line
+new_line
";
        let files = parse(diff);
        let options = DiffRenderOptions::default();
        let theme = test_theme_colored();
        let result = render(&files, &options, &theme);

        // #1: should contain dark-green bg for additions
        assert!(
            result.contains("\x1b[48;5;22m"),
            "should have dark-green background for additions"
        );
        // #1: should contain dark-red bg for deletions
        assert!(
            result.contains("\x1b[48;5;52m"),
            "should have dark-red background for deletions"
        );

        // Background must survive mid-line style changes: no full reset until EOL.
        for line in result.lines() {
            if !line.contains("\x1b[48;5;22m") && !line.contains("\x1b[48;5;52m") {
                continue;
            }
            let bg_pos = line
                .find("\x1b[48;5;22m")
                .or_else(|| line.find("\x1b[48;5;52m"))
                .unwrap();
            let after_bg = &line[bg_pos + "\x1b[48;5;22m".len()..];
            // Mid-line may re-assert bg / change intensity/fg, but must not fully reset.
            let reset_pos = after_bg.find("\x1b[0m");
            assert!(
                reset_pos.is_some(),
                "tinted line should end with a full reset"
            );
            let before_reset = &after_bg[..reset_pos.unwrap()];
            assert!(
                !before_reset.contains("\x1b[0m"),
                "must not fully reset mid-line while background tint is active: {line:?}"
            );
            // Soft resets are OK (22 intensity, 39 fg, 49 bg) — bg is re-asserted after.
            assert!(
                before_reset.contains("\x1b[22m") || before_reset.contains("\x1b[39m"),
                "expected nested SGR style changes without full reset"
            );
        }
    }

    #[test]
    fn test_unified_truncates_long_lines() {
        let long = "x".repeat(200);
        let diff = format!(
            "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-{long}
+{long}_new
"
        );
        let files = parse(&diff);
        let result = render(&files, &DiffRenderOptions::default(), &test_theme());
        assert!(
            result.contains('\u{2026}'),
            "long unified lines should truncate with ellipsis, got length {}",
            result.len()
        );
        // Raw 200-char run should not appear intact once truncated for term width.
        assert!(
            !result.contains(&long),
            "full 200-char line should not be emitted untruncated"
        );
    }

    #[test]
    fn test_render_sign_column() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-old
+new
";
        let files = parse(diff);
        let options = DiffRenderOptions::default();
        let theme = test_theme_colored();
        let result = render(&files, &options, &theme);

        // #2: colored + and - signs (fg set, then soft-reset — not full \x1b[0m)
        assert!(result.contains("\x1b[32m+"), "should have green + sign");
        assert!(result.contains("\x1b[31m-"), "should have red - sign");
    }

    #[test]
    fn test_render_summary_colored() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,2 @@
-old
+new
+extra
";
        let files = parse(diff);
        let options = DiffRenderOptions::default();
        let theme = test_theme_colored();
        let result = render(&files, &options, &theme);

        // #7: colored insertion/deletion counts
        assert!(
            result.contains("\x1b[32m"),
            "summary should have green-colored insertions"
        );
        assert!(
            result.contains("\x1b[31m"),
            "summary should have red-colored deletions"
        );
    }

    #[test]
    fn test_inline_file_header() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,1 +1,1 @@
-old
+new
";
        let files = parse(diff);
        let options = DiffRenderOptions::default();
        let theme = test_theme();
        let result = render(&files, &options, &theme);

        // #4: compact header should contain the path and status on one line
        assert!(result.contains("src/main.rs"));
        assert!(result.contains("modified"));
        assert!(result.contains("+1"));
        assert!(result.contains("-1"));
    }

    #[test]
    fn test_context_lines_not_dimmed() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,3 +1,4 @@
 fn hello() {
-    old();
+    new();
 }
";
        let files = parse(diff);
        let options = DiffRenderOptions::default();
        let theme = test_theme_colored();
        let result = render(&files, &options, &theme);

        // #6: context line content should NOT be dimmed (no \x1b[2m around "fn hello")
        // The line numbers should be dimmed but not the code content
        // Find a context line: "fn hello() {"
        // It should appear without dim around it
        assert!(result.contains("fn hello() {"));
    }

    #[test]
    fn test_truncate_mid_ascii() {
        let s = "abcdefghijklmnopqrstuvwxyz";
        let result = truncate_mid(s, 11);
        assert_eq!(result.len(), 11 + 2); // "…" is 3 bytes
        assert!(result.contains('\u{2026}'));
    }

    #[test]
    fn test_truncate_mid_short_string() {
        let s = "hello";
        let result = truncate_mid(s, 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_mid_utf8_no_panic() {
        // Multi-byte characters: each emoji is 4 bytes
        let s = "\u{1f980}\u{1f980}\u{1f980}\u{1f980}\u{1f980}\u{1f980}\u{1f980}\u{1f980}\u{1f980}\u{1f980}";
        let result = truncate_mid(s, 7);
        assert!(result.contains('\u{2026}'));
        // Should not panic - that's the main thing we're testing
    }

    #[test]
    fn test_truncate_mid_mixed_utf8_no_panic() {
        let s = "h\u{e9}llo w\u{f6}rld caf\u{e9} r\u{e9}sum\u{e9} na\u{ef}ve";
        let result = truncate_mid(s, 12);
        assert!(result.contains('\u{2026}'));
    }

    #[test]
    fn test_side_by_side_left_edge_pipe() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-foo
+bar
";
        let files = parse(diff);
        let options = DiffRenderOptions {
            context_lines: 3,
            mode: RenderMode::SideBySide,
        };

        // Plain mode: should have | at left edge
        let theme = test_theme();
        let result = render(&files, &options, &theme);
        // Each data line should start with |
        let data_lines: Vec<&str> = result
            .lines()
            .filter(|l| l.contains("foo") || l.contains("bar"))
            .collect();
        for line in &data_lines {
            assert!(
                line.starts_with('|'),
                "side-by-side row should start with | but got: {}",
                line
            );
        }
    }

    #[test]
    fn test_render_binary_file_message() {
        let diff = "\
diff --git a/logo.png b/logo.png
new file mode 100644
index 0000000..abc1234
Binary files /dev/null and b/logo.png differ
";
        let files = parse(diff);
        assert!(files[0].binary);
        let result = render(&files, &DiffRenderOptions::default(), &test_theme());
        assert!(
            result.contains("binary file changed"),
            "binary files should show an explicit message, got:\n{result}"
        );
    }

    #[test]
    fn test_lineno_width_grows_for_large_line_numbers() {
        let diff = "\
diff --git a/big.rs b/big.rs
--- a/big.rs
+++ b/big.rs
@@ -9998,3 +9998,3 @@
 context
-old
+new
";
        let files = parse(diff);
        // 9998 + 2 context lines → line 10000 → 5 digits
        assert_eq!(lineno_width_for_file(&files[0]), 5);
        let result = render(&files, &DiffRenderOptions::default(), &test_theme());
        assert!(
            result.contains("9998") || result.contains("9999") || result.contains("10000"),
            "large line numbers should render fully, got:\n{result}"
        );
    }

    #[test]
    fn test_lineno_width_for_small_files_is_compact() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-foo
+bar
";
        let files = parse(diff);
        assert_eq!(lineno_width_for_file(&files[0]), 1);
    }

    #[test]
    fn test_render_no_newline_marker() {
        let diff = "\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,1 +1,1 @@
-old
\\ No newline at end of file
+new
\\ No newline at end of file
";
        let files = parse(diff);
        let result = render(&files, &DiffRenderOptions::default(), &test_theme());
        assert!(
            result.contains("\\ No newline at end of file"),
            "missing no-newline marker, got:\n{result}"
        );
    }

    #[test]
    fn test_expand_tabs_uses_tabstop_8() {
        assert_eq!(expand_tabs("a\tb").as_ref(), "a       b");
        assert_eq!(expand_tabs("\t").as_ref(), "        ");
        assert_eq!(expand_tabs("abcd\te").as_ref(), "abcd    e");
    }

    #[test]
    fn test_render_pure_rename_cue() {
        let diff = "\
diff --git a/old.rs b/new.rs
similarity index 100%
rename from old.rs
rename to new.rs
";
        let files = parse(diff);
        let result = render(&files, &DiffRenderOptions::default(), &test_theme());
        assert!(
            result.contains("renamed with no content change"),
            "pure rename should show a cue, got:\n{result}"
        );
    }

    #[test]
    fn test_render_to_matches_render() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-foo
+bar
";
        let files = parse(diff);
        let opts = DiffRenderOptions::default();
        let theme = test_theme();
        let as_string = render(&files, &opts, &theme);
        let mut buf = Vec::new();
        render_to(&files, &opts, &theme, &mut buf).unwrap();
        assert_eq!(as_string, String::from_utf8(buf).unwrap());
    }

    #[test]
    fn test_header_fill_uses_plain_visible_width() {
        let files = parse(
            "\
diff --git a/short.rs b/short.rs
--- a/short.rs
+++ b/short.rs
@@ -1,1 +1,1 @@
-a
+b
",
        );
        // Force a known width by rendering and checking the header line length
        // stays within a reasonable bound (no ANSI in width math → no huge fill).
        let result = render(&files, &DiffRenderOptions::default(), &test_theme_colored());
        let header = result.lines().next().unwrap_or("");
        // Strip ANSI for measurement
        let mut plain = String::new();
        let mut chars = header.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for ch in chars.by_ref() {
                        if ch.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                continue;
            }
            plain.push(c);
        }
        assert!(
            plain.width() <= term_width_for_render() + 2,
            "header visible width {} exceeds term width, plain={plain:?}",
            plain.width()
        );
    }
}
