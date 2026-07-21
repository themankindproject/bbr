//! Align deletion/addition blocks for word-diff pairing.
//!
//! Within a contiguous change run (deletions followed by additions):
//! 1. Greedily pair lines whose word-level similarity is above
//!    [`WORD_DIFF_THRESHOLD`] (so reordered edits highlight correctly).
//! 2. Positionally zip any leftovers (stable interleaved display).
//!
//! Intra-line word highlighting is still gated by the same threshold at
//! render time — a positional pair below the threshold is shown as a full
//! line change, not a noisy word-diff.

use super::parser::DiffLine;
use super::word_diff::{similarity, WORD_DIFF_THRESHOLD};

/// One display row after aligning a deletion/addition run.
#[derive(Debug, Clone, Copy)]
pub enum AlignedRow<'a> {
    /// Matched deletion + addition (eligible for word-diff when similar enough).
    Pair(&'a DiffLine, &'a DiffLine),
    /// Unmatched deletion.
    DeleteOnly(&'a DiffLine),
    /// Unmatched addition.
    AddOnly(&'a DiffLine),
}

/// Align a contiguous run of deletions followed by additions.
pub fn align_change_block<'a>(
    deletions: &[&'a DiffLine],
    additions: &[&'a DiffLine],
) -> Vec<AlignedRow<'a>> {
    if deletions.is_empty() {
        return additions.iter().copied().map(AlignedRow::AddOnly).collect();
    }
    if additions.is_empty() {
        return deletions
            .iter()
            .copied()
            .map(AlignedRow::DeleteOnly)
            .collect();
    }
    align_by_similarity_then_position(deletions, additions)
}

fn align_by_similarity_then_position<'a>(
    deletions: &[&'a DiffLine],
    additions: &[&'a DiffLine],
) -> Vec<AlignedRow<'a>> {
    let mut candidates: Vec<(usize, usize, f64)> = Vec::new();
    for (di, d) in deletions.iter().enumerate() {
        for (ai, a) in additions.iter().enumerate() {
            let sim = similarity(&d.content, &a.content);
            if sim >= WORD_DIFF_THRESHOLD {
                candidates.push((di, ai, sim));
            }
        }
    }
    candidates.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.cmp(&b.1))
    });

    let mut del_to_add: Vec<Option<usize>> = vec![None; deletions.len()];
    let mut add_taken = vec![false; additions.len()];
    for (di, ai, _) in candidates {
        if del_to_add[di].is_none() && !add_taken[ai] {
            del_to_add[di] = Some(ai);
            add_taken[ai] = true;
        }
    }

    // Positional zip for leftovers — keeps traditional interleaved order when
    // lines are too different for similarity matching (word-diff still skipped).
    let rem_d: Vec<usize> = del_to_add
        .iter()
        .enumerate()
        .filter_map(|(i, a)| a.is_none().then_some(i))
        .collect();
    let rem_a: Vec<usize> = add_taken
        .iter()
        .enumerate()
        .filter_map(|(i, t)| (!*t).then_some(i))
        .collect();
    for (di, ai) in rem_d.into_iter().zip(rem_a) {
        del_to_add[di] = Some(ai);
        add_taken[ai] = true;
    }

    let mut rows = Vec::with_capacity(deletions.len() + additions.len());
    for (di, d) in deletions.iter().enumerate() {
        if let Some(ai) = del_to_add[di] {
            rows.push(AlignedRow::Pair(d, additions[ai]));
        } else {
            rows.push(AlignedRow::DeleteOnly(d));
        }
    }
    for (ai, a) in additions.iter().enumerate() {
        if !add_taken[ai] {
            rows.push(AlignedRow::AddOnly(a));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::parser::DiffLineKind;

    fn line(kind: DiffLineKind, content: &str) -> DiffLine {
        DiffLine {
            kind,
            old_lineno: None,
            new_lineno: None,
            content: content.to_string(),
        }
    }

    #[test]
    fn identical_lines_pair_as_equal() {
        let d0 = line(DiffLineKind::Deletion, "same");
        let a0 = line(DiffLineKind::Addition, "same");
        let d1 = line(DiffLineKind::Deletion, "other");
        let a1 = line(DiffLineKind::Addition, "other");
        let rows = align_change_block(&[&d0, &d1], &[&a0, &a1]);
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0], AlignedRow::Pair(_, _)));
        assert!(matches!(rows[1], AlignedRow::Pair(_, _)));
    }

    #[test]
    fn reordered_similar_lines_pair_by_similarity() {
        let d0 = line(DiffLineKind::Deletion, "alpha one two three");
        let d1 = line(DiffLineKind::Deletion, "gamma one two three");
        let a0 = line(DiffLineKind::Addition, "gamma one two three!");
        let a1 = line(DiffLineKind::Addition, "alpha one two three!");
        let rows = align_change_block(&[&d0, &d1], &[&a0, &a1]);

        let mut paired_contents = Vec::new();
        for row in &rows {
            if let AlignedRow::Pair(d, a) = row {
                paired_contents.push((d.content.as_str(), a.content.as_str()));
            }
        }
        assert!(
            paired_contents
                .iter()
                .any(|(d, a)| d.starts_with("alpha") && a.starts_with("alpha")),
            "alpha should pair with alpha, got {paired_contents:?}"
        );
        assert!(
            paired_contents
                .iter()
                .any(|(d, a)| d.starts_with("gamma") && a.starts_with("gamma")),
            "gamma should pair with gamma, got {paired_contents:?}"
        );
    }

    #[test]
    fn interleaved_similar_edits_emit_pairs_in_order() {
        let d = [
            line(DiffLineKind::Deletion, "line1_old"),
            line(DiffLineKind::Deletion, "line2_old"),
            line(DiffLineKind::Deletion, "line3_old"),
        ];
        let a = [
            line(DiffLineKind::Addition, "line1_new"),
            line(DiffLineKind::Addition, "line2_new"),
            line(DiffLineKind::Addition, "line3_new"),
        ];
        let dels: Vec<_> = d.iter().collect();
        let adds: Vec<_> = a.iter().collect();
        let rows = align_change_block(&dels, &adds);
        assert_eq!(rows.len(), 3);
        assert!(matches!(
            rows[0],
            AlignedRow::Pair(d, a)
                if d.content.starts_with("line1") && a.content.starts_with("line1")
        ));
        assert!(matches!(
            rows[1],
            AlignedRow::Pair(d, a)
                if d.content.starts_with("line2") && a.content.starts_with("line2")
        ));
        assert!(matches!(
            rows[2],
            AlignedRow::Pair(d, a)
                if d.content.starts_with("line3") && a.content.starts_with("line3")
        ));
    }

    #[test]
    fn pure_inserts_are_add_only() {
        let a0 = line(DiffLineKind::Addition, "new");
        let rows = align_change_block(&[], &[&a0]);
        assert!(matches!(rows.as_slice(), [AlignedRow::AddOnly(_)]));
    }

    #[test]
    fn pure_deletes_are_delete_only() {
        let d0 = line(DiffLineKind::Deletion, "old");
        let rows = align_change_block(&[&d0], &[]);
        assert!(matches!(rows.as_slice(), [AlignedRow::DeleteOnly(_)]));
    }

    #[test]
    fn unrelated_lines_still_pair_positionally_for_display() {
        let d0 = line(DiffLineKind::Deletion, "aaaaaaaaaaaaaaaaaaaa");
        let a0 = line(DiffLineKind::Addition, "bbbbbbbbbbbbbbbbbbbb");
        let rows = align_change_block(&[&d0], &[&a0]);
        // Positional fallback pairs for interleaved display; word-diff remains
        // gated by WORD_DIFF_THRESHOLD in the renderer.
        assert!(matches!(rows.as_slice(), [AlignedRow::Pair(_, _)]));
        assert!(similarity("aaaaaaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbbbbbb") < WORD_DIFF_THRESHOLD);
    }

    #[test]
    fn unequal_counts_leave_extras_unpaired() {
        let d0 = line(DiffLineKind::Deletion, "only_del");
        let d1 = line(DiffLineKind::Deletion, "shared_line_here");
        let a0 = line(DiffLineKind::Addition, "shared_line_here!");
        let rows = align_change_block(&[&d0, &d1], &[&a0]);
        assert!(rows.iter().any(|r| matches!(r, AlignedRow::DeleteOnly(_))));
        assert!(rows.iter().any(|r| matches!(r, AlignedRow::Pair(_, _))));
    }
}
