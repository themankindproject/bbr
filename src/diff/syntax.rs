//! Syntect-backed syntax highlighting for pretty diffs.

use std::path::Path;
use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static Theme {
    let ts = THEME_SET.get_or_init(ThemeSet::load_defaults);
    ts.themes
        .get("base16-ocean.dark")
        .or_else(|| ts.themes.values().next())
        .expect("syntect default themes must include at least one theme")
}

fn syntax_for_path(path: &str) -> &'static syntect::parsing::SyntaxReference {
    let ss = syntax_set();
    ss.find_syntax_for_file(Path::new(path))
        .ok()
        .flatten()
        .or_else(|| {
            Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .and_then(|ext| ss.find_syntax_by_extension(ext))
        })
        .unwrap_or_else(|| ss.find_syntax_plain_text())
}

/// Per-file highlighter that advances parse state line-by-line.
pub struct FileHighlighter {
    inner: Option<HighlightLines<'static>>,
}

impl FileHighlighter {
    /// Build a highlighter for `path` when `enabled`; otherwise a no-op.
    pub fn new(path: &str, enabled: bool) -> Self {
        if !enabled {
            return Self { inner: None };
        }
        Self {
            inner: Some(HighlightLines::new(syntax_for_path(path), theme())),
        }
    }

    /// Advance parse state without using the styles (e.g. collapsed context).
    pub fn advance(&mut self, line: &str) {
        let _ = self.highlight(line);
    }

    /// Highlight one line; returns `(style, text)` spans covering `line`.
    pub fn highlight<'a>(&mut self, line: &'a str) -> Vec<(Style, &'a str)> {
        let Some(h) = self.inner.as_mut() else {
            return Vec::new();
        };
        match h.highlight_line(line, syntax_set()) {
            Ok(spans) if !spans.is_empty() => spans,
            _ => Vec::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.inner.is_some()
    }
}

/// One-shot highlight (no multi-line state) — used for side-by-side rows.
pub fn highlight_line(path: &str, line: &str) -> Vec<(Style, String)> {
    let mut h = FileHighlighter::new(path, true);
    h.highlight(line)
        .into_iter()
        .map(|(s, t)| (s, t.to_string()))
        .collect()
}

/// Truncate styled spans to `max_width` display columns, appending `…` when needed.
pub fn truncate_spans(spans: &[(Style, &str)], max_width: usize) -> Vec<(Style, String)> {
    let total: usize = spans.iter().map(|(_, t)| t.width()).sum();
    if total <= max_width {
        return spans.iter().map(|(s, t)| (*s, (*t).to_string())).collect();
    }
    if max_width == 0 {
        return Vec::new();
    }
    let budget = max_width.saturating_sub(1);
    let mut out: Vec<(Style, String)> = Vec::new();
    let mut w = 0usize;
    'spans: for (style, text) in spans {
        let mut buf = String::new();
        for ch in text.chars() {
            let cw = ch.width().unwrap_or(0);
            if w + cw > budget {
                if !buf.is_empty() {
                    out.push((*style, buf));
                }
                out.push((*style, "\u{2026}".to_string()));
                break 'spans;
            }
            buf.push(ch);
            w += cw;
        }
        if !buf.is_empty() {
            out.push((*style, buf));
        }
    }
    if out.is_empty() {
        out.push((Style::default(), "\u{2026}".to_string()));
    }
    out
}

/// Emit 24-bit ANSI for spans (no background); ends with reset.
pub fn spans_to_ansi(spans: &[(Style, String)]) -> String {
    let mut out = String::new();
    for (style, text) in spans {
        if text.is_empty() {
            continue;
        }
        let c = style.foreground;
        out.push_str(&format!("\x1b[38;2;{};{};{}m", c.r, c.g, c.b));
        out.push_str(text);
    }
    out.push_str("\x1b[0m");
    out
}
