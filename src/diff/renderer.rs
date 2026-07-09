//! Pretty diff renderer with box-drawing, line numbers, and ANSI colors.
//!
//! Takes parsed [`DiffFile`]s and outputs a beautifully formatted string
//! suitable for terminal display or piping through a pager.

use std::borrow::Cow;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
    /// Side-by-side view.
    SideBySide,
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Cached terminal width to avoid repeated ioctl syscalls per rendered line.
/// The width is determined once per process and reused for all subsequent renders.
fn cached_term_width() -> usize {
    use std::sync::OnceLock;
    static WIDTH: OnceLock<usize> = OnceLock::new();
    *WIDTH.get_or_init(|| crate::output::theme::terminal_width().unwrap_or(80))
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

    // Improvement #9: File index at top of multi-file diffs
    if files.len() >= 2 {
        render_file_index(files, theme, &mut out);
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
fn render_file(file: &DiffFile, options: &DiffRenderOptions, theme: &Theme, out: &mut String) {
    render_file_header(file, theme, out);

    if !file.hunks.is_empty() {
        for hunk in &file.hunks {
            render_hunk(hunk, options, theme, out);
        }
    }
}

// ---------------------------------------------------------------------------
// File header (#4 - inline compact header with stats bar)
// ---------------------------------------------------------------------------

/// Render the file header as a single compact line with stats bar.
fn render_file_header(file: &DiffFile, theme: &Theme, out: &mut String) {
    let width = cached_term_width();

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

    // Build the stats bar (8 chars)
    let stats_bar = build_stats_bar(file.additions, file.deletions, 8, theme);

    let adds_str = format!("+{}", file.additions);
    let dels_str = format!("-{}", file.deletions);

    let dash = if theme.unicode_enabled() {
        "\u{2500}"
    } else {
        "-"
    };
    let dash2 = format!("{}{} ", dash, dash);

    if theme.colors_enabled() {
        // Build: ── {icon} {path} ── {status} ── [stats_bar] +X, -Y ──
        let bold_icon = format!("\x1b[1m{}\x1b[0m", icon);
        let bold_path = format!("\x1b[1m{}\x1b[0m", display_path);
        let green_adds = format!("\x1b[32m{}\x1b[0m", adds_str);
        let red_dels = format!("\x1b[31m{}\x1b[0m", dels_str);

        let content = format!(
            "{}{} {} {} {} {} [{}] {}, {} ",
            dash2, dash, bold_icon, bold_path, dash2, status_text, stats_bar, green_adds, red_dels
        );

        // Calculate visible width for fill (approximate since ANSI codes are invisible)
        // visible: "── ─ {icon} {path} ── {status} ── [{bar}] +X, -Y "
        let visible_len = 3
            + 2
            + icon.width()
            + 1
            + display_path.width()
            + 1
            + 3
            + status_text.len()
            + 1
            + 3
            + 8
            + 2
            + adds_str.len()
            + 2
            + dels_str.len()
            + 1;
        let fill_count = width.saturating_sub(visible_len);
        let fill = dash.repeat(fill_count);
        let dimmed_fill = format!("\x1b[2m{}\x1b[0m", fill);

        out.push_str(&content);
        out.push_str(&dimmed_fill);
        out.push('\n');
    } else {
        let content = format!(
            "{}{} {} {} {} {} [{}] {}, {} ",
            dash2, dash, icon, display_path, dash2, status_text, stats_bar, adds_str, dels_str
        );
        let visible_len = content.width();
        let fill_count = width.saturating_sub(visible_len);
        let fill = dash.repeat(fill_count);
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

    if options.mode == RenderMode::SideBySide {
        render_hunk_side_by_side(hunk, options, theme, out);
    } else {
        render_hunk_unified(hunk, options, theme, out);
    }
}

// ---------------------------------------------------------------------------
// Unified mode (#3 - interleaved pairs)
// ---------------------------------------------------------------------------

fn render_hunk_unified(
    hunk: &DiffHunk,
    options: &DiffRenderOptions,
    theme: &Theme,
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
                // Improvement #3: Interleave paired lines
                let max_pairs = deletions.len().max(additions.len());
                for j in 0..max_pairs {
                    // Output deletion line j (paired with addition j for word-diff)
                    if let Some(del) = deletions.get(j) {
                        let pair = additions.get(j).copied();
                        render_paired_line(del, pair, theme, out);
                    }
                    // Output addition line j (paired with deletion j for word-diff)
                    if let Some(add) = additions.get(j) {
                        let pair = deletions.get(j).copied();
                        render_paired_line(add, pair, theme, out);
                    }
                }
            } else {
                // Context line
                render_line(&hunk.lines[i], theme, out);
                i += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Paired line rendering (#1 bg tinting, #2 sign column, #5 threshold, #8 empty marker)
// ---------------------------------------------------------------------------

fn render_paired_line(line: &DiffLine, pair: Option<&DiffLine>, theme: &Theme, out: &mut String) {
    let term_width = cached_term_width();

    let old = line
        .old_lineno
        .map(|n| format!("{:>4}", n))
        .unwrap_or_else(|| "    ".to_string());
    let new = line
        .new_lineno
        .map(|n| format!("{:>4}", n))
        .unwrap_or_else(|| "    ".to_string());

    let sep = if theme.unicode_enabled() {
        "\u{2502}"
    } else {
        "|"
    };

    // Improvement #8: empty line marker
    let empty_marker = if theme.unicode_enabled() {
        "\u{23ce}"
    } else {
        "<CR>"
    };

    match line.kind {
        DiffLineKind::Addition => {
            if theme.colors_enabled() {
                // #1: full-line dark-green background
                let bg = "\x1b[48;5;22m";
                let reset = "\x1b[0m";
                // #2: green + sign
                let sign = "\x1b[32m+\x1b[0m";
                let dimmed_old = format!("\x1b[2m{}\x1b[0m", old);
                let dimmed_sep = format!("\x1b[2m{}\x1b[0m", sep);

                // Build content with word-diff or plain
                let content = if line.content.is_empty() {
                    format!("\x1b[2m{}\x1b[0m", empty_marker)
                } else if let Some(p) = pair {
                    // #5: check similarity threshold before word diff
                    let sim = crate::diff::word_diff::similarity(&p.content, &line.content);
                    if sim < crate::diff::word_diff::WORD_DIFF_THRESHOLD {
                        format!("\x1b[32m{}\x1b[0m", line.content)
                    } else {
                        let segments =
                            crate::diff::word_diff::word_changes(&p.content, &line.content);
                        let mut s = String::new();
                        for seg in segments {
                            match seg.kind {
                                crate::diff::word_diff::WordChange::Inserted => {
                                    s.push_str(&format!("\x1b[30;42m{}\x1b[0m", seg.text));
                                }
                                crate::diff::word_diff::WordChange::Deleted => {}
                                crate::diff::word_diff::WordChange::Equal => {
                                    s.push_str(&format!("\x1b[32m{}\x1b[0m", seg.text));
                                }
                            }
                        }
                        s
                    }
                } else {
                    format!("\x1b[32m{}\x1b[0m", line.content)
                };

                // Calculate visible width of prefix: " {old} {new} {sign} {sep} "
                let prefix_visible_width = 1 + 4 + 1 + 4 + 1 + 1 + 1 + 1 + 1; // " XXXX XXXX + │ "
                let content_visible_width = if line.content.is_empty() {
                    empty_marker.width()
                } else {
                    line.content.width()
                };
                let used = prefix_visible_width + content_visible_width;
                let pad = term_width.saturating_sub(used);

                out.push_str(&format!(
                    "{} {} {} {} {} {} {}{}{}",
                    bg,
                    dimmed_old,
                    new,
                    sign,
                    dimmed_sep,
                    content,
                    " ".repeat(pad),
                    reset,
                    "\n"
                ));
            } else {
                let content = if line.content.is_empty() {
                    empty_marker.to_string()
                } else {
                    line.content.clone()
                };
                out.push_str(&format!(" {} {} + {} {}\n", old, new, sep, content));
            }
        }
        DiffLineKind::Deletion => {
            if theme.colors_enabled() {
                // #1: full-line dark-red background
                let bg = "\x1b[48;5;52m";
                let reset = "\x1b[0m";
                // #2: red - sign
                let sign = "\x1b[31m-\x1b[0m";
                let dimmed_new = format!("\x1b[2m{}\x1b[0m", new);
                let dimmed_sep = format!("\x1b[2m{}\x1b[0m", sep);

                // Build content with word-diff or plain
                let content = if line.content.is_empty() {
                    format!("\x1b[2m{}\x1b[0m", empty_marker)
                } else if let Some(p) = pair {
                    // #5: check similarity threshold before word diff
                    let sim = crate::diff::word_diff::similarity(&line.content, &p.content);
                    if sim < crate::diff::word_diff::WORD_DIFF_THRESHOLD {
                        format!("\x1b[31m{}\x1b[0m", line.content)
                    } else {
                        let segments =
                            crate::diff::word_diff::word_changes(&line.content, &p.content);
                        let mut s = String::new();
                        for seg in segments {
                            match seg.kind {
                                crate::diff::word_diff::WordChange::Deleted => {
                                    s.push_str(&format!("\x1b[30;41m{}\x1b[0m", seg.text));
                                }
                                crate::diff::word_diff::WordChange::Inserted => {}
                                crate::diff::word_diff::WordChange::Equal => {
                                    s.push_str(&format!("\x1b[31m{}\x1b[0m", seg.text));
                                }
                            }
                        }
                        s
                    }
                } else {
                    format!("\x1b[31m{}\x1b[0m", line.content)
                };

                let prefix_visible_width = 1 + 4 + 1 + 4 + 1 + 1 + 1 + 1 + 1;
                let content_visible_width = if line.content.is_empty() {
                    empty_marker.width()
                } else {
                    line.content.width()
                };
                let used = prefix_visible_width + content_visible_width;
                let pad = term_width.saturating_sub(used);

                out.push_str(&format!(
                    "{} {} {} {} {} {} {}{}{}",
                    bg,
                    old,
                    dimmed_new,
                    sign,
                    dimmed_sep,
                    content,
                    " ".repeat(pad),
                    reset,
                    "\n"
                ));
            } else {
                let content = if line.content.is_empty() {
                    empty_marker.to_string()
                } else {
                    line.content.clone()
                };
                out.push_str(&format!(" {} {} - {} {}\n", old, new, sep, content));
            }
        }
        DiffLineKind::Context => {
            render_line(line, theme, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Context line rendering (#6 - normal weight content, dimmed line numbers)
// ---------------------------------------------------------------------------

/// Render a single context/addition/deletion line.
fn render_line(line: &DiffLine, theme: &Theme, out: &mut String) {
    let term_width = cached_term_width();

    let old = line
        .old_lineno
        .map(|n| format!("{:>4}", n))
        .unwrap_or_else(|| "    ".to_string());
    let new = line
        .new_lineno
        .map(|n| format!("{:>4}", n))
        .unwrap_or_else(|| "    ".to_string());

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

    match line.kind {
        DiffLineKind::Context => {
            if theme.colors_enabled() {
                // #6: line numbers dimmed, separator dimmed, content normal
                let dimmed_old = format!("\x1b[2m{}\x1b[0m", old);
                let dimmed_new = format!("\x1b[2m{}\x1b[0m", new);
                let dimmed_sep = format!("\x1b[2m{}\x1b[0m", sep);
                out.push_str(&format!(
                    " {} {}   {} {}\n",
                    dimmed_old, dimmed_new, dimmed_sep, line.content
                ));
            } else {
                out.push_str(&format!(" {} {}   {} {}\n", old, new, sep, line.content));
            }
        }
        DiffLineKind::Addition => {
            if theme.colors_enabled() {
                let bg = "\x1b[48;5;22m";
                let reset = "\x1b[0m";
                let sign = "\x1b[32m+\x1b[0m";
                let dimmed_old = format!("\x1b[2m{}\x1b[0m", old);
                let dimmed_sep = format!("\x1b[2m{}\x1b[0m", sep);
                let content = if line.content.is_empty() {
                    format!("\x1b[2m{}\x1b[0m", empty_marker)
                } else {
                    format!("\x1b[32m{}\x1b[0m", line.content)
                };
                let prefix_visible_width = 1 + 4 + 1 + 4 + 1 + 1 + 1 + 1 + 1;
                let content_visible_width = if line.content.is_empty() {
                    empty_marker.width()
                } else {
                    line.content.width()
                };
                let used = prefix_visible_width + content_visible_width;
                let pad = term_width.saturating_sub(used);
                out.push_str(&format!(
                    "{} {} {} {} {} {}{}{}{}",
                    bg,
                    dimmed_old,
                    new,
                    sign,
                    dimmed_sep,
                    content,
                    " ".repeat(pad),
                    reset,
                    "\n"
                ));
            } else {
                let content = if line.content.is_empty() {
                    empty_marker.to_string()
                } else {
                    line.content.clone()
                };
                out.push_str(&format!(" {} {} + {} {}\n", old, new, sep, content));
            }
        }
        DiffLineKind::Deletion => {
            if theme.colors_enabled() {
                let bg = "\x1b[48;5;52m";
                let reset = "\x1b[0m";
                let sign = "\x1b[31m-\x1b[0m";
                let dimmed_new = format!("\x1b[2m{}\x1b[0m", new);
                let dimmed_sep = format!("\x1b[2m{}\x1b[0m", sep);
                let content = if line.content.is_empty() {
                    format!("\x1b[2m{}\x1b[0m", empty_marker)
                } else {
                    format!("\x1b[31m{}\x1b[0m", line.content)
                };
                let prefix_visible_width = 1 + 4 + 1 + 4 + 1 + 1 + 1 + 1 + 1;
                let content_visible_width = if line.content.is_empty() {
                    empty_marker.width()
                } else {
                    line.content.width()
                };
                let used = prefix_visible_width + content_visible_width;
                let pad = term_width.saturating_sub(used);
                out.push_str(&format!(
                    "{} {} {} {} {} {}{}{}{}",
                    bg,
                    old,
                    dimmed_new,
                    sign,
                    dimmed_sep,
                    content,
                    " ".repeat(pad),
                    reset,
                    "\n"
                ));
            } else {
                let content = if line.content.is_empty() {
                    empty_marker.to_string()
                } else {
                    line.content.clone()
                };
                out.push_str(&format!(" {} {} - {} {}\n", old, new, sep, content));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Side-by-side mode (#10 - continuous vertical divider)
// ---------------------------------------------------------------------------

fn render_hunk_side_by_side(
    hunk: &DiffHunk,
    options: &DiffRenderOptions,
    theme: &Theme,
    out: &mut String,
) {
    let width = cached_term_width();
    // Reserved: left_edge(2) + left_lineno(4) + sep(3) + middle_sep(3) + right_lineno(4) + sep(3) + right_edge(0)
    // Total overhead: 2 + 4 + 3 + 3 + 4 + 3 = 19
    let code_width = width.saturating_sub(19) / 2;

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
                let max_len = deletions.len().max(additions.len());
                for j in 0..max_len {
                    let del = deletions.get(j).copied();
                    let add = additions.get(j).copied();
                    render_side_by_side_row(del, add, code_width, theme, out);
                }
            } else {
                let line = &hunk.lines[i];
                render_side_by_side_row(Some(line), Some(line), code_width, theme, out);
                i += 1;
            }
        }
    }
}

fn render_side_by_side_row(
    del: Option<&DiffLine>,
    add: Option<&DiffLine>,
    code_width: usize,
    theme: &Theme,
    out: &mut String,
) {
    let left_lineno = del
        .and_then(|l| l.old_lineno)
        .map(|n| format!("{:>4}", n))
        .unwrap_or_else(|| "    ".to_string());

    let right_lineno = add
        .and_then(|l| l.new_lineno)
        .map(|n| format!("{:>4}", n))
        .unwrap_or_else(|| "    ".to_string());

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
                let left_visible = truncate_code_raw(&l.content, code_width);
                let right_visible = truncate_code_raw(&r.content, code_width);
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
                    let left_visible = truncate_code_raw(&l.content, code_width);
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
                let left_visible = truncate_code_raw(&l.content, code_width);
                let right_visible = truncate_code_raw(&r.content, code_width);
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
                    let right_visible = truncate_code_raw(&r.content, code_width);
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
                    truncate_code(&l.content, code_width)
                }
            })
            .unwrap_or_else(|| " ".repeat(code_width));
        let right_content = add
            .map(|l| {
                if l.content.is_empty() {
                    truncate_code(empty_marker, code_width)
                } else {
                    truncate_code(&l.content, code_width)
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
            &sep,
            left_sign,
            " ",
            left_content,
            &sep,
            right_lineno,
            &format!("{}{}{}\n", &sep, right_sign, right_content),
            "",
        ));
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
            syntax_highlight: false,
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
        // In interleaved mode, line1_old should appear before line1_new
        let pos_old1 = result.find("line1_old").unwrap();
        let pos_new1 = result.find("line1_new").unwrap();
        assert!(
            pos_old1 < pos_new1,
            "deletion should appear before its paired addition"
        );

        // line2_old should appear after line1_new
        let pos_old2 = result.find("line2_old").unwrap();
        assert!(
            pos_old2 > pos_new1,
            "second deletion should follow first addition"
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

        // #2: colored + and - signs
        assert!(
            result.contains("\x1b[32m+\x1b[0m"),
            "should have green + sign"
        );
        assert!(
            result.contains("\x1b[31m-\x1b[0m"),
            "should have red - sign"
        );
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
            syntax_highlight: false,
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
}
