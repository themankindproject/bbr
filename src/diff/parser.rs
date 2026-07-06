//! Unified diff parser.
//!
//! Parses raw `git diff` / unified diff output into structured Rust types
//! representing files, hunks, and individual lines.

use serde::Serialize;

/// The kind of a single diff line.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}

/// A single line in a diff hunk.
#[derive(Debug, Clone, Serialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    /// Old line number (None for additions in a new file, or when unknown).
    pub old_lineno: Option<u32>,
    /// New line number (None for deletions that no longer exist).
    pub new_lineno: Option<u32>,
    /// The line content (without leading `+`, `-`, or ` `).
    pub content: String,
}

/// A hunk (chunk) of a diff, starting with `@@ -a,b +c,d @@`.
#[derive(Debug, Clone, Serialize)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// The hunk header text after `@@` (e.g. ` fn foo()`).
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// The status of a file in a diff.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
}

/// A single file in the diff output.
#[derive(Debug, Clone, Serialize)]
pub struct DiffFile {
    pub status: FileStatus,
    pub old_path: String,
    pub new_path: String,
    pub hunks: Vec<DiffHunk>,
    /// Number of added lines across all hunks.
    #[serde(skip_serializing_if = "is_zero")]
    pub additions: u32,
    /// Number of deleted lines across all hunks.
    #[serde(skip_serializing_if = "is_zero")]
    pub deletions: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Parse a raw unified diff string into a list of file diffs.
///
/// Returns an empty vec if the input contains no diff output (e.g. empty diff).
pub fn parse(raw: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut current_file: Option<DiffFileBuilder> = None;
    let mut current_hunk: Option<DiffHunkBuilder> = None;
    let mut seen_header = false;

    for line in raw.lines() {
        if line.starts_with("diff --git ") {
            // Flush current hunk/file before starting a new one
            if let Some(file) = current_file.take() {
                if let Some(hunk) = current_hunk.take() {
                    files.push(file.finish(Some(hunk)));
                } else {
                    files.push(file.finish(None));
                }
            } else {
                current_hunk = None;
            }
            seen_header = true;
            current_file = Some(DiffFileBuilder::new(line));
            continue;
        }

        // Binary files have "Binary files a/... and b/... differ"
        if line.starts_with("Binary files ") && line.ends_with(" differ") {
            if let Some(ref mut file) = current_file {
                file.binary = true;
            }
            continue;
        }

        // File mode / index / new-file / deleted-file / rename-from / rename-to lines
        // We skip these but they indicate file status
        if let Some(ref mut file) = current_file {
            if line.starts_with("+++ ") || line.starts_with("--- ") {
                file.set_paths(line);
                continue;
            }
            if let Some(ctx) = rename_context(line) {
                file.set_rename(ctx);
                continue;
            }
            if line.starts_with("new file mode ") {
                file.status = FileStatus::Added;
                continue;
            }
            if line.starts_with("deleted file mode ") {
                file.status = FileStatus::Deleted;
                continue;
            }
            if line.starts_with("index ")
                || line.starts_with("new mode ")
                || line.starts_with("old mode ")
                || line.starts_with("similarity index ")
            {
                continue;
            }
        } else {
            // If there's no diff --git header yet but we see ---/+++, start a builder
            if !seen_header && (line.starts_with("--- ") || line.starts_with("+++ ")) {
                current_file = Some(DiffFileBuilder::new_from_paths(line));
                continue;
            }
        }

        // Hunk header
        if let Some(header) = parse_hunk_header(line) {
            if let Some(file) = current_file.as_mut() {
                // Flush the previous hunk: add its counts AND push it to the file
                if let Some(prev_hunk) = current_hunk.take() {
                    file.add_hunk_counts(&prev_hunk);
                    file.push_hunk(prev_hunk);
                }
                current_hunk = Some(DiffHunkBuilder::new(header));
            } else {
                // Orphan hunk without a file header - create a minimal file
                current_file = Some(DiffFileBuilder::new_from_hunk());
                current_hunk = Some(DiffHunkBuilder::new(header));
            }
            continue;
        }

        // Content line (starts with ' ', '+', or '-')
        if let Some(ref mut hunk) = current_hunk {
            if line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') {
                hunk.add_line(line);
                continue;
            }
        }

        // Lines that don't match any known pattern (like "\ No newline at end of file")
        // We either ignore them or add as context if we're in a hunk
        if current_hunk.is_some() && (line.is_empty() || line.starts_with('\\')) {
            continue;
        }
    }

    // Flush final hunk/file
    if let Some(hunk) = current_hunk {
        if let Some(file) = current_file.take() {
            files.push(file.finish(Some(hunk)));
        } else {
            let builder = DiffFileBuilder::new("");
            let file = builder.finish(Some(hunk));
            files.push(file);
        }
    } else if let Some(file) = current_file.take() {
        files.push(file.finish(None));
    }

    files
}

/// Parse `@@ -old_start,old_lines +new_start,new_lines @@ header` from a hunk line.
fn parse_hunk_header(line: &str) -> Option<HunkHeader> {
    let line = line.trim();
    if !line.starts_with("@@ ") {
        return None;
    }
    let rest = line.strip_prefix("@@ ")?;

    // Find the second @@
    let end = rest.find(" @@")?;
    let ranges = &rest[..end];
    let header = rest[end + 3..].trim().to_string();

    // Parse ranges: "-old_start,old_lines +new_start,new_lines"
    let parts: Vec<&str> = ranges.split_whitespace().collect();
    if parts.len() != 2 {
        return None;
    }

    let (old_part, new_part) = (parts[0], parts[1]);

    let (old_start, old_lines) = parse_range(old_part, '-')?;
    let (new_start, new_lines) = parse_range(new_part, '+')?;

    Some(HunkHeader {
        old_start,
        old_lines,
        new_start,
        new_lines,
        header,
    })
}

fn parse_range(s: &str, prefix: char) -> Option<(u32, u32)> {
    let s = s.strip_prefix(prefix)?;
    if let Some((start, count)) = s.split_once(',') {
        Some((start.parse().ok()?, count.parse().ok()?))
    } else {
        // If no comma, the count is implicitly 1
        Some((s.parse().ok()?, 1))
    }
}

struct HunkHeader {
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    header: String,
}

struct DiffHunkBuilder {
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    header: String,
    lines: Vec<DiffLine>,
    /// Running line numbers
    old_lineno: u32,
    new_lineno: u32,
}

impl DiffHunkBuilder {
    fn from_header(h: HunkHeader) -> Self {
        DiffHunkBuilder {
            old_start: h.old_start,
            old_lines: h.old_lines,
            new_start: h.new_start,
            new_lines: h.new_lines,
            header: h.header,
            lines: Vec::new(),
            old_lineno: h.old_start,
            new_lineno: h.new_start,
        }
    }

    fn new(h: HunkHeader) -> Self {
        Self::from_header(h)
    }

    fn add_line(&mut self, line: &str) {
        let (kind, content) = match line.chars().next() {
            Some(' ') => (DiffLineKind::Context, &line[1..]),
            Some('+') => (DiffLineKind::Addition, &line[1..]),
            Some('-') => (DiffLineKind::Deletion, &line[1..]),
            _ => return,
        };

        let (old_lineno, new_lineno) = match kind {
            DiffLineKind::Context => {
                let o = self.old_lineno;
                let n = self.new_lineno;
                self.old_lineno += 1;
                self.new_lineno += 1;
                (Some(o), Some(n))
            }
            DiffLineKind::Addition => {
                let n = self.new_lineno;
                self.new_lineno += 1;
                (None, Some(n))
            }
            DiffLineKind::Deletion => {
                let o = self.old_lineno;
                self.old_lineno += 1;
                (Some(o), None)
            }
        };

        self.lines.push(DiffLine {
            kind,
            old_lineno,
            new_lineno,
            content: content.to_string(),
        });
    }
}

/// Check if a line is `rename from` or `rename to`.
fn rename_context(line: &str) -> Option<RenameCtx> {
    line.strip_prefix("rename from ")
        .map(|path| RenameCtx::From(path.trim().to_string()))
        .or_else(|| {
            line.strip_prefix("rename to ")
                .map(|path| RenameCtx::To(path.trim().to_string()))
        })
}

enum RenameCtx {
    From(String),
    To(String),
}

struct DiffFileBuilder {
    status: FileStatus,
    old_path: String,
    new_path: String,
    hunks: Vec<DiffHunk>,
    binary: bool,
    additions: u32,
    deletions: u32,
    rename_from: Option<String>,
    rename_to: Option<String>,
}

impl DiffFileBuilder {
    fn new(diff_line: &str) -> Self {
        // Parse "diff --git a/old_path b/new_path"
        let (old_path, new_path) = if let Some(paths) = diff_line
            .strip_prefix("diff --git ")
            .and_then(|s| s.split_once(' '))
        {
            (
                paths.0.strip_prefix("a/").unwrap_or(paths.0).to_string(),
                paths.1.strip_prefix("b/").unwrap_or(paths.1).to_string(),
            )
        } else {
            ("unknown".to_string(), "unknown".to_string())
        };

        let status = if new_path == "/dev/null" {
            FileStatus::Deleted
        } else {
            FileStatus::Modified
        };

        DiffFileBuilder {
            status,
            old_path,
            new_path,
            hunks: Vec::new(),
            binary: false,
            additions: 0,
            deletions: 0,
            rename_from: None,
            rename_to: None,
        }
    }

    fn new_from_paths(line: &str) -> Self {
        let stripped = line.trim_start_matches('-').trim_start_matches('+').trim();
        let (old, new) = if line.starts_with("--- ") {
            (
                stripped.strip_prefix("a/").unwrap_or(stripped).to_string(),
                String::new(),
            )
        } else {
            (
                String::new(),
                stripped.strip_prefix("b/").unwrap_or(stripped).to_string(),
            )
        };
        DiffFileBuilder {
            status: FileStatus::Modified,
            old_path: old,
            new_path: new,
            hunks: Vec::new(),
            binary: false,
            additions: 0,
            deletions: 0,
            rename_from: None,
            rename_to: None,
        }
    }

    fn new_from_hunk() -> Self {
        DiffFileBuilder {
            status: FileStatus::Modified,
            old_path: String::new(),
            new_path: String::new(),
            hunks: Vec::new(),
            binary: false,
            additions: 0,
            deletions: 0,
            rename_from: None,
            rename_to: None,
        }
    }

    fn set_paths(&mut self, line: &str) {
        let stripped = line[4..].trim();
        if line.starts_with("--- ") {
            if stripped != "/dev/null" {
                self.old_path = stripped.strip_prefix("a/").unwrap_or(stripped).to_string();
            }
        } else if line.starts_with("+++ ") && stripped != "/dev/null" {
            self.new_path = stripped.strip_prefix("b/").unwrap_or(stripped).to_string();
        }
    }

    fn set_rename(&mut self, ctx: RenameCtx) {
        match ctx {
            RenameCtx::From(p) => self.rename_from = Some(p),
            RenameCtx::To(p) => self.rename_to = Some(p),
        }
    }

    fn add_hunk_counts(&mut self, hunk: &DiffHunkBuilder) {
        for line in &hunk.lines {
            match line.kind {
                DiffLineKind::Addition => self.additions += 1,
                DiffLineKind::Deletion => self.deletions += 1,
                DiffLineKind::Context => {}
            }
        }
    }

    fn push_hunk(&mut self, hunk: DiffHunkBuilder) {
        self.hunks.push(DiffHunk {
            old_start: hunk.old_start,
            old_lines: hunk.old_lines,
            new_start: hunk.new_start,
            new_lines: hunk.new_lines,
            header: hunk.header,
            lines: hunk.lines,
        });
    }

    fn finish(mut self, last_hunk: Option<DiffHunkBuilder>) -> DiffFile {
        if let Some(hunk_builder) = last_hunk {
            // Add counts from this hunk
            for line in &hunk_builder.lines {
                match line.kind {
                    DiffLineKind::Addition => self.additions += 1,
                    DiffLineKind::Deletion => self.deletions += 1,
                    DiffLineKind::Context => {}
                }
            }
            self.hunks.push(DiffHunk {
                old_start: hunk_builder.old_start,
                old_lines: hunk_builder.old_lines,
                new_start: hunk_builder.new_start,
                new_lines: hunk_builder.new_lines,
                header: hunk_builder.header,
                lines: hunk_builder.lines,
            });
        }

        // Determine final status
        if self.status == FileStatus::Modified && self.rename_from.is_some() {
            self.status = FileStatus::Renamed;
        }
        if self.new_path == "/dev/null" {
            self.status = FileStatus::Deleted;
        }
        // Handle the case where old_path is /dev/null but there are hunks (new file)
        if self.old_path == "/dev/null" && self.additions > 0 && self.deletions == 0 {
            self.status = FileStatus::Added;
        }

        DiffFile {
            status: self.status,
            old_path: if self.old_path == "/dev/null" {
                String::new()
            } else {
                self.old_path
            },
            new_path: if self.new_path == "/dev/null" {
                String::new()
            } else {
                self.new_path
            },
            hunks: self.hunks,
            additions: self.additions,
            deletions: self.deletions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_DIFF: &str = "\
diff --git a/src/main.rs b/src/main.rs
index abc123..def456 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,5 +1,6 @@
 fn hello() {
-    println!(\"goodbye\");
+    println!(\"hello\");
+    println!(\"world\");
 }

 fn main() {
";

    #[test]
    fn test_parse_simple_diff() {
        let files = parse(SIMPLE_DIFF);
        assert_eq!(files.len(), 1, "should parse one file");

        let file = &files[0];
        assert_eq!(file.status, FileStatus::Modified);
        assert_eq!(file.old_path, "src/main.rs");
        assert_eq!(file.new_path, "src/main.rs");
        assert_eq!(file.additions, 2);
        assert_eq!(file.deletions, 1);
        assert_eq!(file.hunks.len(), 1);

        let hunk = &file.hunks[0];
        assert_eq!(hunk.old_start, 1);
        assert_eq!(hunk.new_start, 1);
        assert_eq!(hunk.lines.len(), 6);

        // Context line 1 — leading space in diff is stripped
        assert_eq!(hunk.lines[0].kind, DiffLineKind::Context);
        assert_eq!(hunk.lines[0].old_lineno, Some(1));
        assert_eq!(hunk.lines[0].new_lineno, Some(1));
        assert_eq!(hunk.lines[0].content, "fn hello() {");

        // Deleted line
        assert_eq!(hunk.lines[1].kind, DiffLineKind::Deletion);
        assert_eq!(hunk.lines[1].old_lineno, Some(2));
        assert_eq!(hunk.lines[1].new_lineno, None);
        assert_eq!(hunk.lines[1].content, "    println!(\"goodbye\");");

        // Added line
        assert_eq!(hunk.lines[2].kind, DiffLineKind::Addition);
        assert_eq!(hunk.lines[2].old_lineno, None);
        assert_eq!(hunk.lines[2].new_lineno, Some(2));
        assert_eq!(hunk.lines[2].content, "    println!(\"hello\");");
    }

    #[test]
    fn test_parse_new_file() {
        let diff = "\
diff --git a/new_file.rs b/new_file.rs
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/new_file.rs
@@ -0,0 +1,3 @@
+fn new_func() {
+    println!(\"new\");
+}
";
        let files = parse(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Added);
        assert_eq!(files[0].new_path, "new_file.rs");
        assert_eq!(files[0].additions, 3);
        assert_eq!(files[0].deletions, 0);
    }

    #[test]
    fn test_parse_deleted_file() {
        let diff = "\
diff --git a/deleted.rs b/deleted.rs
deleted file mode 100644
index abc1234..0000000
--- a/deleted.rs
+++ /dev/null
@@ -1,4 +0,0 @@
-fn old_func() {
-    println!(\"delete me\");
-}
";
        let files = parse(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Deleted);
        assert_eq!(files[0].old_path, "deleted.rs");
        assert_eq!(files[0].additions, 0);
        assert_eq!(files[0].deletions, 3);
    }

    #[test]
    fn test_parse_multiple_files() {
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
        assert_eq!(files[0].new_path, "a.rs");
        assert_eq!(files[1].new_path, "b.rs");
        assert_eq!(files[0].additions, 1);
        assert_eq!(files[1].deletions, 1);
    }

    #[test]
    fn test_parse_empty_diff() {
        let files = parse("");
        assert!(files.is_empty(), "empty string should produce no files");
    }

    #[test]
    fn test_parse_hunk_header() {
        let h = parse_hunk_header("@@ -10,4 +20,5 @@ fn foo()").unwrap();
        assert_eq!(h.old_start, 10);
        assert_eq!(h.old_lines, 4);
        assert_eq!(h.new_start, 20);
        assert_eq!(h.new_lines, 5);
        assert_eq!(h.header, "fn foo()");
    }

    #[test]
    fn test_parse_hunk_header_single_line() {
        let h = parse_hunk_header("@@ -1 +1 @@").unwrap();
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_lines, 1);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_lines, 1);
        assert!(h.header.is_empty());
    }

    #[test]
    fn test_parse_hunk_header_no_match() {
        assert!(parse_hunk_header("normal line").is_none());
        assert!(parse_hunk_header("").is_none());
    }

    #[test]
    fn test_parse_rename() {
        let diff = "\
diff --git a/old.rs b/new.rs
similarity index 100%
rename from old.rs
rename to new.rs
--- a/old.rs
+++ b/new.rs
@@ -1,1 +1,1 @@
-foo
+bar
";
        let files = parse(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Renamed);
        assert_eq!(files[0].old_path, "old.rs");
        assert_eq!(files[0].new_path, "new.rs");
    }

    #[test]
    fn test_rejects_non_diff_input() {
        let diff = "This is just some text\nNot a diff at all\n";
        let files = parse(diff);
        assert!(files.is_empty(), "non-diff text should produce no files");
    }

    #[test]
    fn test_counts_are_correct() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,5 +1,8 @@
 unchanged
+add1
+add2
-del1
+add3
 unchanged
-del2
 unchanged
+add4
";
        let files = parse(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].additions, 4);
        assert_eq!(files[0].deletions, 2);
    }

    #[test]
    fn test_parse_multiple_hunks_preserved() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,4 @@
 fn first() {
-    old1();
+    new1();
 }
@@ -10,4 +10,4 @@
 fn second() {
-    old2();
+    new2();
 }
@@ -20,4 +20,4 @@
 fn third() {
-    old3();
+    new3();
 }
";
        let files = parse(diff);
        assert_eq!(files.len(), 1, "should parse one file");

        let file = &files[0];
        assert_eq!(file.hunks.len(), 3, "all three hunks should be preserved");
        assert_eq!(file.additions, 3);
        assert_eq!(file.deletions, 3);

        // Verify each hunk has the correct start lines
        assert_eq!(file.hunks[0].old_start, 1);
        assert_eq!(file.hunks[1].old_start, 10);
        assert_eq!(file.hunks[2].old_start, 20);

        // Verify each hunk has lines (not empty)
        assert_eq!(file.hunks[0].lines.len(), 4);
        assert_eq!(file.hunks[1].lines.len(), 4);
        assert_eq!(file.hunks[2].lines.len(), 4);
    }
}
