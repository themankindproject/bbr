//! Intra-line word-level diffing.
//!
//! Splits old and new line content into word tokens and computes
//! which words changed using the `similar` crate.

use similar::{ChangeTag, TextDiff};

/// The kind of change for a word segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WordChange {
    Equal,
    Inserted,
    Deleted,
}

/// A segment of text within a line, annotated with its change kind.
#[derive(Debug, Clone)]
pub struct WordSegment {
    pub kind: WordChange,
    pub text: String,
}

/// Compute word-level changes between old (deletion) and new (addition) line content.
///
/// Pass the content of a `-` line as `old` and the content of a `+` line as `new`.
/// Returns a list of segments, each tagged as Equal (present in both), Inserted
/// (only in new), or Deleted (only in old).
pub fn word_changes(old: &str, new: &str) -> Vec<WordSegment> {
    let diff = TextDiff::from_words(old, new);

    let mut segments = Vec::new();

    for change in diff.iter_all_changes() {
        let tag = match change.tag() {
            ChangeTag::Equal => WordChange::Equal,
            ChangeTag::Insert => WordChange::Inserted,
            ChangeTag::Delete => WordChange::Deleted,
        };
        // Skip empty tokens that `similar` sometimes emits
        let value = change.value();
        if value.is_empty() {
            continue;
        }
        segments.push(WordSegment {
            kind: tag,
            text: value.to_string(),
        });
    }

    segments
}

/// Minimum similarity ratio (0.0–1.0) below which word-level highlighting
/// should be skipped in favour of showing the entire line as changed.
/// A value of 0.30 means lines that share fewer than 30% of their words
/// are considered too different for useful intra-line highlighting.
pub const WORD_DIFF_THRESHOLD: f64 = 0.30;

/// Compute the similarity ratio between two strings using word-level diffing.
///
/// Returns a value between 0.0 (completely different) and 1.0 (identical).
/// Uses the `similar` crate's `TextDiff::from_words` ratio method which
/// computes `2 * matching_words / total_words`.
pub fn similarity(old: &str, new: &str) -> f64 {
    let diff = TextDiff::from_words(old, new);
    diff.ratio().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_changes_simple_modification() {
        let old = "println!(\"goodbye\");";
        let new = "println!(\"hello\");";
        let changes = word_changes(old, new);

        let deleted: Vec<_> = changes
            .iter()
            .filter(|s| s.kind == WordChange::Deleted)
            .collect();
        let inserted: Vec<_> = changes
            .iter()
            .filter(|s| s.kind == WordChange::Inserted)
            .collect();

        assert!(!deleted.is_empty(), "should have deleted words");
        assert!(!inserted.is_empty(), "should have inserted words");
        assert!(deleted.iter().any(|s| s.text.contains("goodbye")));
        assert!(inserted.iter().any(|s| s.text.contains("hello")));
    }

    #[test]
    fn test_word_changes_identical() {
        let text = "let x = 42;";
        let changes = word_changes(text, text);

        let equal_count = changes
            .iter()
            .filter(|s| s.kind == WordChange::Equal)
            .count();
        let changed_count = changes
            .iter()
            .filter(|s| s.kind != WordChange::Equal)
            .count();

        assert!(equal_count > 0, "identical text should have equal segments");
        assert_eq!(changed_count, 0, "identical text should have no changes");
    }

    #[test]
    fn test_empty_input() {
        let changes = word_changes("", "");
        assert!(
            changes.is_empty() || changes.iter().all(|s| s.kind == WordChange::Equal),
            "empty inputs should produce empty or all-equal output"
        );
    }

    #[test]
    fn test_word_changes_prefix() {
        // With word-level diffing, "foobar" and "foobaz" are each single tokens
        // Just verify it runs without panicking and returns some changes
        let changes = word_changes("foobar", "foobaz");
        assert!(!changes.is_empty(), "should produce some changes");
        // There should be at least one deletion (foobar) or insertion (foobaz)
        let has_deleted_or_inserted = changes.iter().any(|s| s.kind != WordChange::Equal);
        assert!(has_deleted_or_inserted, "should detect changes");
    }
}
