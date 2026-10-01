//! Output formatting: pretty tables for humans, stable JSON for agents.

pub mod json;
pub mod table;
pub mod theme;

use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};

use crate::error::Result;

/// A formatter decides how a piece of data hits stdout.
///
/// `Human` formatters take already-rendered strings (tables / blocks);
/// `Json` formatters take any `Serialize` value.
pub enum Formatter {
    Human { no_pager: bool },
    Json,
}

impl Formatter {
    /// Pick a formatter from the `--json` flag.
    pub fn from_json_flag(json: bool) -> Self {
        if json {
            Formatter::Json
        } else {
            Formatter::Human { no_pager: false }
        }
    }

    /// Pick a formatter with pager control.
    pub fn from_args(json: bool, no_pager: bool) -> Self {
        if json {
            Formatter::Json
        } else {
            Formatter::Human { no_pager }
        }
    }

    /// Print a serializable value. For JSON, serialize directly. For human
    /// output, the caller must have already built a string.
    pub fn print<T: serde::Serialize>(&self, value: &T, human: &str) -> Result<()> {
        match self {
            Formatter::Json => json::print_json(value),
            Formatter::Human { .. } => print_block(human),
        }
    }

    /// Print a serializable value with pagination if stdout is a terminal.
    pub fn print_paginated<T: serde::Serialize>(&self, value: &T, human: &str) -> Result<()> {
        match self {
            Formatter::Json => json::print_json(value),
            Formatter::Human { no_pager } => {
                if *no_pager {
                    print_block(human)
                } else {
                    print_paginated(human)
                }
            }
        }
    }

    /// Print diff output with syntax highlighting (bat) and paging.
    pub fn print_diff<T: serde::Serialize>(&self, value: &T, human: &str) -> Result<()> {
        match self {
            Formatter::Json => json::print_json(value),
            Formatter::Human { no_pager } => {
                if *no_pager {
                    print_block(human)
                } else {
                    print_diff(human)
                }
            }
        }
    }
}

/// Write a human-readable block to stdout.
pub fn print_block(s: &str) -> Result<()> {
    let s = sanitize_human_output(s);
    let mut out = io::stdout().lock();
    out.write_all(s.as_bytes())?;
    if !s.ends_with('\n') {
        out.write_all(b"\n")?;
    }
    Ok(())
}

/// Neutralize terminal escape sequences that originate from remote data while
/// preserving the SGR color codes bbr itself emits.
///
/// Remote strings (PR titles, branch and pipeline-step names, commit messages,
/// webhook URLs, API error bodies) are printed inside human output. A crafted
/// title such as `\x1b]52;c;<base64>\x07` can hijack the clipboard (OSC 52) or
/// `\x1b[2J\x1b[H` can clear the screen and forge output.
///
/// The filter is deliberately *allow-list* rather than strip-all: `colored`,
/// `comfy-table`/`crossterm` and `syntect` all emit SGR sequences
/// (`ESC [ <params> m`), so those are kept and every other escape is dropped.
/// Other C0 controls are dropped too, except `\n` and `\t`.
pub fn sanitize_human_output(s: &str) -> std::borrow::Cow<'_, str> {
    // Fast path: nothing to do for the overwhelmingly common case.
    let needs_work = s
        .chars()
        .any(|c| c == '\x1b' || c == '\u{9b}' || (c.is_control() && c != '\n' && c != '\t'));
    if !needs_work {
        return std::borrow::Cow::Borrowed(s);
    }

    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => match chars.peek().copied() {
                // CSI: keep only well-formed SGR (`ESC [ <digits ; :> m`); drop
                // screen/cursor manipulation. Parameter bytes are restricted to
                // ECMA-48's 0x30-0x3F range so a control character or C1
                // introducer can never ride along inside a "kept" sequence.
                Some('[') => {
                    chars.next();
                    let mut params = String::new();
                    let mut sgr_candidate = true;
                    while let Some(&c) = chars.peek() {
                        match c {
                            '\x30'..='\x3f' => params.push(c),
                            // Intermediate bytes: valid CSI, but never SGR.
                            '\x20'..='\x2f' => sgr_candidate = false,
                            '\x40'..='\x7e' => {
                                chars.next();
                                if sgr_candidate && c == 'm' {
                                    out.push_str("\x1b[");
                                    out.push_str(&params);
                                    out.push('m');
                                }
                                break;
                            }
                            // Malformed or truncated: drop what was consumed and
                            // let the main loop handle this character.
                            _ => break,
                        }
                        chars.next();
                    }
                }
                // OSC (e.g. clipboard hijack via OSC 52): consume to BEL or ST.
                // An unterminated OSC ends at the line break, so one hostile
                // title cannot swallow every following line of output.
                Some(']') => {
                    chars.next();
                    while let Some(&c) = chars.peek() {
                        if c == '\n' {
                            break;
                        }
                        chars.next();
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // Any other ESC-introduced sequence: drop ESC + introducer.
                Some(_) => {
                    chars.next();
                }
                None => {}
            },
            // Single-byte C1 CSI introducer: drop it with its parameters.
            '\u{9b}' => {
                while let Some(&c) = chars.peek() {
                    if !('\x20'..='\x3f').contains(&c) {
                        if ('\x40'..='\x7e').contains(&c) {
                            chars.next();
                        }
                        break;
                    }
                    chars.next();
                }
            }
            '\n' | '\t' => out.push(ch),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Sanitize one line of CI log output for display.
///
/// Build tools redraw progress lines with carriage returns; a terminal shows
/// only the text after the last `\r`, so do the same (a trailing CRLF `\r` is
/// ignored) instead of gluing every redraw together. Escape sequences other
/// than SGR colors are then removed as for any other remote text.
pub fn sanitize_log_line(line: &str) -> std::borrow::Cow<'_, str> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let visible = match line.rfind('\r') {
        Some(i) => &line[i + 1..],
        None => line,
    };
    sanitize_human_output(visible)
}

/// A writer that removes non-SGR terminal escapes from everything written
/// through it, one complete line at a time.
///
/// Rendering code streams many small writes (diff rows, comment bodies); the
/// sanitizer needs whole lines so a sequence split across two `write` calls is
/// still recognized. Sanitized lines never end inside an escape sequence, so
/// concatenating them cannot recreate one. Call [`SanitizingWriter::finish`]
/// to emit a trailing partial line.
pub struct SanitizingWriter<W: Write> {
    inner: W,
    pending: Vec<u8>,
}

impl<W: Write> SanitizingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            pending: Vec::new(),
        }
    }

    fn emit(&mut self, bytes: &[u8]) -> io::Result<()> {
        let text = String::from_utf8_lossy(bytes);
        self.inner
            .write_all(sanitize_human_output(&text).as_bytes())
    }

    /// Flush the buffered partial line and the inner writer.
    pub fn finish(mut self) -> io::Result<W> {
        let rest = std::mem::take(&mut self.pending);
        if !rest.is_empty() {
            self.emit(&rest)?;
        }
        self.inner.flush()?;
        Ok(self.inner)
    }
}

impl<W: Write> Write for SanitizingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        if let Some(end) = self.pending.iter().rposition(|&b| b == b'\n') {
            let complete: Vec<u8> = self.pending.drain(..=end).collect();
            self.emit(&complete)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        // Partial lines stay buffered until `finish`, so a sequence split at a
        // flush boundary is still sanitized as a whole.
        self.inner.flush()
    }
}

/// Run `write_fn` against `w` through a [`SanitizingWriter`].
fn write_sanitized<F>(w: &mut dyn Write, write_fn: F) -> Result<()>
where
    F: FnOnce(&mut dyn Write) -> Result<()>,
{
    let mut sanitizing = SanitizingWriter::new(w);
    let result = write_fn(&mut sanitizing);
    let finished = sanitizing.finish();
    result?;
    finished?;
    Ok(())
}

/// Stream human output to stdout, through the pager when appropriate.
///
/// Like [`write_paginated`], all text is sanitized; `no_pager` (or a
/// non-terminal stdout) writes straight to stdout.
pub fn write_human<F>(no_pager: bool, write_fn: F) -> Result<()>
where
    F: FnOnce(&mut dyn Write) -> Result<()>,
{
    if no_pager || !io::stdout().is_terminal() {
        let mut out = io::stdout().lock();
        return write_sanitized(&mut out, write_fn);
    }
    write_paginated(write_fn)
}

/// Print text that is meant to be consumed verbatim (`pr diff --raw`,
/// `src cat`): exact bytes when stdout is redirected, so patches and files
/// round-trip unchanged, but sanitized like other remote text on a terminal.
pub fn print_raw(s: &str) -> Result<()> {
    if io::stdout().is_terminal() {
        return print_block(s);
    }
    let mut out = io::stdout().lock();
    out.write_all(s.as_bytes())?;
    out.flush()?;
    Ok(())
}

/// Print a diff with syntax highlighting (via `bat`) and paging, falling
/// back to `print_paginated` if `bat` is not available.
pub fn print_diff(s: &str) -> Result<()> {
    let s = sanitize_human_output(s);
    let s = s.as_ref();
    if !io::stdout().is_terminal() {
        return print_block(s);
    }

    let pager_env = std::env::var("PAGER").unwrap_or_default();

    // If the user explicitly set PAGER, respect it instead of sniffing for bat.
    // Otherwise, try bat first.
    if pager_env.is_empty() {
        // Small diffs that fit on one screen don't need a pager — bat still
        // provides syntax highlighting with --paging=never.
        let line_count = s.lines().count() + 1;
        let fits = crate::output::theme::terminal_height()
            .map(|h| line_count <= h.saturating_sub(2))
            .unwrap_or(false);
        let paging = if fits {
            "--paging=never"
        } else {
            "--paging=always"
        };
        match Command::new("bat")
            .args(["--language=diff", paging, "--color=always"])
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(s.as_bytes());
                    if !s.ends_with('\n') {
                        let _ = stdin.write_all(b"\n");
                    }
                }
                let _ = child.wait();
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // bat is not installed — tell the user once, then fall through
                // to the plain pager.
                eprintln!(
                    "hint: install `bat` for syntax-highlighted diffs (https://github.com/sharkdp/bat)"
                );
            }
            Err(_) => {
                // Any other spawn error — silently fall through.
            }
        }
    }

    print_paginated(s)
}

/// Write a human-readable block to stdout with optional pagination using less/PAGER.
pub fn print_paginated(s: &str) -> Result<()> {
    let s = sanitize_human_output(s);
    if !io::stdout().is_terminal() {
        return print_block(&s);
    }

    write_paginated(|w| {
        w.write_all(s.as_bytes())?;
        if !s.ends_with('\n') {
            w.write_all(b"\n")?;
        }
        Ok(())
    })
}

/// Stream output through a pager (or stdout when not a TTY), avoiding a full buffer.
///
/// Everything written is sanitized with [`sanitize_human_output`]: this is the
/// streaming path for PR diffs, titles, and comment bodies, all of which can
/// carry attacker-controlled escape sequences.
pub fn write_paginated<F>(write_fn: F) -> Result<()>
where
    F: FnOnce(&mut dyn Write) -> Result<()>,
{
    if !io::stdout().is_terminal() {
        let mut out = io::stdout().lock();
        return write_sanitized(&mut out, write_fn);
    }

    let pager_env = std::env::var("PAGER").unwrap_or_else(|_| "less".to_string());
    let mut cmd = if pager_env == "less" {
        let mut c = Command::new("less");
        c.args(["-F", "-R", "-X"]);
        c
    } else {
        let parts = split_pager_args(&pager_env);
        if let Some((bin, args)) = parts.split_first() {
            let mut c = Command::new(bin);
            c.args(args);
            c
        } else {
            let mut out = io::stdout().lock();
            return write_sanitized(&mut out, write_fn);
        }
    };

    cmd.stdin(Stdio::piped());

    if let Ok(mut child) = cmd.spawn() {
        let write_result = if let Some(mut stdin) = child.stdin.take() {
            write_sanitized(&mut stdin, write_fn)
        } else {
            Ok(())
        };
        let _ = child.wait();
        ignore_broken_pipe(write_result)
    } else {
        let mut out = io::stdout().lock();
        write_sanitized(&mut out, write_fn)
    }
}

/// A pager quitting early (`q` in less) closes its stdin mid-write; the
/// resulting EPIPE is expected user behavior, not an error.
fn ignore_broken_pipe(result: Result<()>) -> Result<()> {
    match result {
        Err(crate::error::BitbucketError::Io(e)) if e.kind() == std::io::ErrorKind::BrokenPipe => {
            Ok(())
        }
        other => other,
    }
}

/// Split a `$PAGER` value into argv tokens, honoring single/double quotes.
///
/// `PAGER="less -R"` must yield `["less", "-R"]`, and
/// `PAGER="'my pager' -X"` must yield `["my pager", "-X"]` — a plain
/// `split_whitespace` would pass the quote characters through literally and
/// break pagers whose path contains spaces or whose flags are quoted.
fn split_pager_args(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;

    for ch in s.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => cur.push(ch),
            None => match ch {
                '\'' | '"' => {
                    quote = Some(ch);
                    in_token = true;
                }
                c if c.is_whitespace() => {
                    if in_token {
                        out.push(std::mem::take(&mut cur));
                        in_token = false;
                    }
                }
                c => {
                    cur.push(c);
                    in_token = true;
                }
            },
        }
    }
    if in_token {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_paginated_falls_back_when_not_terminal() {
        let res = print_paginated("hello test");
        assert!(res.is_ok());
    }

    #[test]
    fn split_pager_args_plain() {
        assert_eq!(split_pager_args("less -R"), vec!["less", "-R"]);
    }

    #[test]
    fn split_pager_args_quoted_path_with_space() {
        assert_eq!(split_pager_args("'my pager' -X"), vec!["my pager", "-X"]);
    }

    #[test]
    fn split_pager_args_double_quoted_flag() {
        assert_eq!(split_pager_args("less \"-R -X\""), vec!["less", "-R -X"]);
    }

    #[test]
    fn split_pager_args_empty() {
        assert!(split_pager_args("").is_empty());
        assert!(split_pager_args("   ").is_empty());
    }

    #[test]
    fn split_pager_args_single_token() {
        assert_eq!(split_pager_args("bat"), vec!["bat"]);
    }

    #[test]
    fn sanitize_preserves_sgr_color_codes() {
        // `colored`/crossterm/syntect all emit SGR; those must survive or all
        // human output loses its color.
        let colored = "\x1b[1;31mFAILED\x1b[0m";
        assert_eq!(sanitize_human_output(colored), colored);
    }

    #[test]
    fn sanitize_keeps_ordinary_text_borrowed() {
        let plain = "PR #12: Add login API\n\tindented";
        assert!(matches!(
            sanitize_human_output(plain),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn sanitize_strips_osc52_clipboard_hijack() {
        let hostile = "title\x1b]52;c;aGFja2Vk\x07rest";
        assert_eq!(sanitize_human_output(hostile), "titlerest");
    }

    #[test]
    fn sanitize_strips_screen_clear_and_cursor_moves() {
        assert_eq!(sanitize_human_output("a\x1b[2J\x1b[Hb"), "ab");
        // OSC terminated by ST (ESC \) rather than BEL.
        assert_eq!(sanitize_human_output("a\x1b]0;evil\x1b\\b"), "ab");
    }

    #[test]
    fn sanitize_strips_bare_control_chars_but_keeps_newline_tab() {
        assert_eq!(sanitize_human_output("a\x07b\x0dc\nd\te"), "abc\nd\te");
        // C1 single-byte CSI introducer.
        assert_eq!(sanitize_human_output("a\u{9b}31mb"), "ab");
    }

    #[test]
    fn sanitize_never_keeps_controls_inside_an_sgr_lookalike() {
        // ESC [ <C1 OSC> 52;c;0 BEL m — previously re-emitted verbatim because
        // the final byte was `m`.
        let hostile = "a\x1b[\u{9d}52;c;0123\x07mb";
        let out = sanitize_human_output(hostile);
        assert!(!out.contains('\u{9d}') && !out.contains('\x07'), "{out:?}");
        assert!(!out.contains('\x1b'), "{out:?}");
        // Intermediate bytes make it a non-SGR control function.
        assert_eq!(sanitize_human_output("a\x1b[1 mb"), "ab");
        // Well-formed SGR with colon sub-parameters survives.
        assert_eq!(sanitize_human_output("\x1b[38:5:196mX"), "\x1b[38:5:196mX");
    }

    #[test]
    fn unterminated_osc_does_not_swallow_following_lines() {
        assert_eq!(
            sanitize_human_output("title \x1b]52;c;aGk=\nnext line\nlast"),
            "title \nnext line\nlast"
        );
    }

    #[test]
    fn sanitizing_is_idempotent() {
        for s in [
            "\x1b[1;31mred\x1b[0m",
            "a\x1b[2Jb\x1b]0;t\x07c",
            "x\u{9b}2Jy\tz\n",
        ] {
            let once = sanitize_human_output(s).into_owned();
            assert_eq!(sanitize_human_output(&once), once);
        }
    }

    #[test]
    fn sanitizing_writer_catches_sequences_split_across_writes() {
        let mut w = SanitizingWriter::new(Vec::new());
        for piece in [
            "safe \x1b",
            "]52;c;aGk",
            "=\x07 text\n\x1b[1",
            "mbold\x1b[0m",
            "\x1b[2",
            "J",
        ] {
            w.write_all(piece.as_bytes()).unwrap();
        }
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        assert_eq!(out, "safe  text\n\x1b[1mbold\x1b[0m");
    }

    #[test]
    fn write_sanitized_reports_writer_errors_and_flushes_partial_line() {
        let mut buf = Vec::new();
        write_sanitized(&mut buf, |w| {
            w.write_all(b"no newline \x1b[2J")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(buf, b"no newline ");
        let err = write_sanitized(&mut Vec::new(), |_| {
            Err(crate::error::BitbucketError::Other("boom".into()))
        })
        .unwrap_err();
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn log_lines_show_the_last_carriage_return_redraw() {
        assert_eq!(
            sanitize_log_line("progress 10%\rprogress 100%"),
            "progress 100%"
        );
        assert_eq!(sanitize_log_line("windows line\r"), "windows line");
        assert_eq!(sanitize_log_line("\x1b[32mok\x1b[0m"), "\x1b[32mok\x1b[0m");
        assert_eq!(sanitize_log_line("evil\x1b]52;c;aGk=\x07!"), "evil!");
    }
}
