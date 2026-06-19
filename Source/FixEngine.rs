//! The unified fix engine (E2-M3, D-REL5): one applier shared by the CLI
//! `jet fix` and the LSP code-action path.
//!
//! Every machine-applicable suggestion in Jet is a `TextEdit` (diag.rs): a byte
//! span `[start, end)` and the `new_text` that replaces it. Both front ends
//! consume that same representation; this module owns the only correct way to
//! turn a set of those edits into new file text, so the offset math lives in
//! exactly one place (I3-flavoured: no duplicate, drift-prone copies).
//!
//! Correctness rules, all enforced here:
//!   * Edits are applied **right-to-left** (highest start offset first) so an
//!     earlier edit never shifts the byte offsets a later edit still needs.
//!   * **Overlapping** edits are rejected, not silently mis-applied: two fixes
//!     that touch the same bytes can't both be right, so we refuse the batch and
//!     let the caller decide (re-run after the first round, or report).
//!   * Spans are clamped to the source length defensively; a stale span can
//!     never panic the compiler (I2: rustc/ICE banners are for *our* bugs, but a
//!     user file plus stale edit should still degrade gracefully).
//!
//! std-only (I6): a `Vec`, a sort, and string slicing — nothing else.

use crate::Diagnostics::TextEdit;

/// Why a batch of edits could not be applied as-is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// Two edits cover overlapping byte ranges; applying both is ambiguous.
    /// Carries the two offending spans (as `[start, end)` pairs) for reporting.
    Overlap {
        first: (usize, usize),
        second: (usize, usize),
    },
}

/// Apply a set of `TextEdit`s to `src`, returning the rewritten text.
///
/// The single source of truth for edit application. Edits may arrive in any
/// order; we sort by start offset, reject overlaps, then splice from the end of
/// the file toward the start so each splice leaves the offsets of the remaining
/// (earlier) edits valid.
///
/// A zero-width edit (`start == end`) is an insertion and never overlaps an
/// adjacent edit that merely abuts it.
pub fn apply_edits(src: &str, edits: &[TextEdit]) -> Result<String, ApplyError> {
    if edits.is_empty() {
        return Ok(src.to_string());
    }

    // Sort a copy by start offset (then end), ascending, so overlap detection is
    // a single linear scan and right-to-left application is just a reverse walk.
    let mut ordered: Vec<&TextEdit> = edits.iter().collect();
    ordered.sort_by_key(|e| (e.span.start, e.span.end));

    // Reject overlaps. An edit overlaps its predecessor when it starts strictly
    // before the predecessor ends. Equal endpoints (abutting / shared insertion
    // point) are allowed.
    for win in ordered.windows(2) {
        let (a, b) = (win[0], win[1]);
        if b.span.start < a.span.end {
            return Err(ApplyError::Overlap {
                first: (a.span.start, a.span.end),
                second: (b.span.start, b.span.end),
            });
        }
    }

    // Apply right-to-left so earlier splices don't invalidate later offsets.
    let len = src.len();
    let mut out = src.to_string();
    for e in ordered.iter().rev() {
        let start = e.span.start.min(len);
        let end = e.span.end.min(len).max(start);
        out.replace_range(start..end, &e.new_text);
    }
    Ok(out)
}

/// True if any edit in the batch would actually change `src` (a no-op batch —
/// e.g. an edit whose `new_text` already equals the spanned bytes — is treated
/// as "no change" by callers so `jet fix` can report honestly).
pub fn changes_text(src: &str, edits: &[TextEdit]) -> bool {
    match apply_edits(src, edits) {
        Ok(new) => new != src,
        Err(_) => true, // an overlap is a meaningful event, not a silent no-op
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Diagnostics::Span;

    fn ed(start: usize, end: usize, text: &str) -> TextEdit {
        TextEdit {
            span: Span::new(start, end),
            new_text: text.to_string(),
        }
    }

    #[test]
    fn empty_is_identity() {
        assert_eq!(apply_edits("hello", &[]).unwrap(), "hello");
    }

    #[test]
    fn single_replacement() {
        // replace "let" (0..3) with "val"
        let src = "let x = 1;";
        let out = apply_edits(src, &[ed(0, 3, "val")]).unwrap();
        assert_eq!(out, "val x = 1;");
    }

    #[test]
    fn right_to_left_keeps_offsets_valid() {
        // Two edits on the same line. If applied left-to-right naively, the
        // first replacement (different length) would shift the second span.
        // "let a = 1; let b = 2;"
        //  0..3 -> val             14..17 -> val
        let src = "let a = 1; let b = 2;";
        let edits = vec![ed(0, 3, "val"), ed(11, 14, "val")];
        let out = apply_edits(src, &edits).unwrap();
        assert_eq!(out, "val a = 1; val b = 2;");
    }

    #[test]
    fn unordered_input_is_sorted() {
        // Same as above but the edits arrive highest-first; result must match.
        let src = "let a = 1; let b = 2;";
        let edits = vec![ed(11, 14, "val"), ed(0, 3, "val")];
        let out = apply_edits(src, &edits).unwrap();
        assert_eq!(out, "val a = 1; val b = 2;");
    }

    #[test]
    fn length_changing_edits_compose() {
        // First edit grows, second edit shrinks; right-to-left keeps both right.
        let src = "ab cd";
        // "ab" (0..2) -> "XXXX"; "cd" (3..5) -> "Y"
        let out = apply_edits(src, &[ed(0, 2, "XXXX"), ed(3, 5, "Y")]).unwrap();
        assert_eq!(out, "XXXX Y");
    }

    #[test]
    fn abutting_edits_are_allowed() {
        // [0,2) and [2,4): share an endpoint but do not overlap.
        let src = "abcd";
        let out = apply_edits(src, &[ed(0, 2, "X"), ed(2, 4, "Y")]).unwrap();
        assert_eq!(out, "XY");
    }

    #[test]
    fn insertion_at_same_point_does_not_overlap() {
        // Two zero-width insertions at offset 2 abut; allowed.
        let src = "abcd";
        let out = apply_edits(src, &[ed(2, 2, "X"), ed(2, 2, "Y")]).unwrap();
        // Both insert at 2; right-to-left applies the later-sorted one first.
        assert!(out.starts_with("ab") && out.ends_with("cd"));
        assert_eq!(out.len(), 6);
    }

    #[test]
    fn overlapping_edits_are_rejected() {
        // [0,4) and [2,6) overlap on bytes 2..4.
        let src = "abcdef";
        let err = apply_edits(src, &[ed(0, 4, "X"), ed(2, 6, "Y")]).unwrap_err();
        assert_eq!(
            err,
            ApplyError::Overlap {
                first: (0, 4),
                second: (2, 6)
            }
        );
    }

    #[test]
    fn stale_span_clamps_without_panic() {
        // A span past the end of src must clamp, never panic.
        let src = "ab";
        let out = apply_edits(src, &[ed(5, 9, "Z")]).unwrap();
        assert_eq!(out, "abZ");
    }

    #[test]
    fn changes_text_detects_noop() {
        let src = "val x = 1;";
        // replacing "val" with "val" is a no-op
        assert!(!changes_text(src, &[ed(0, 3, "val")]));
        assert!(changes_text(src, &[ed(0, 3, "let")]));
    }
}
