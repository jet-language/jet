//! D-DBG3 step 2 (dap-debugger): the rust-line <-> jet-line table for the native
//! backend. Built by scanning generated Rust text for the `// jet:line N` markers
//! `TStmt::LineMarker` emits (only present when the bundle was compiled through
//! `jet::Codegen::emit_bundle_dbg(.., true)` — the native `jet debug` build path).
//!
//! I2: this is the ONLY place the native backend is allowed to reason about
//! generated-Rust line numbers directly; everywhere else (the `(jet)` prompt, the
//! DAP adapter) works in Jet lines, translated through this table.

use std::collections::{BTreeMap, HashMap};

pub(crate) struct LineMap {
    /// Generated-Rust line -> Jet line, for every line a marker introduces.
    rust_to_jet: BTreeMap<usize, usize>,
    /// Jet line -> the FIRST rust line that begins it (used to place a
    /// `breakpoint set -f <file> -l <rust_line>` for a user's `break <jet_line>`).
    jet_to_rust: HashMap<usize, usize>,
}

impl LineMap {
    /// Scan `rust_src` for `// jet:line N` markers (one per lowered `Stmt`,
    /// `crates/jet-codegen/src/Codegen/TIR/emit.rs`'s `TStmt::LineMarker` arm). The
    /// marker sits on its own line immediately before the statement's generated
    /// Rust, so the table maps that FOLLOWING rust line (not the comment's own
    /// line) to the Jet line.
    pub(crate) fn build(rust_src: &str) -> LineMap {
        let mut rust_to_jet = BTreeMap::new();
        let mut jet_to_rust = HashMap::new();
        let mut pending: Option<usize> = None;
        for (i, line) in rust_src.lines().enumerate() {
            let rust_line = i + 1;
            if let Some(n) = pending.take() {
                rust_to_jet.insert(rust_line, n);
                jet_to_rust.entry(n).or_insert(rust_line);
            }
            if let Some(rest) = line.trim_start().strip_prefix("// jet:line ") {
                pending = rest.trim().parse::<usize>().ok();
            }
        }
        LineMap {
            rust_to_jet,
            jet_to_rust,
        }
    }

    /// The Jet line a stopped rust frame belongs to: the marker at or immediately
    /// before `rust_line` (a multi-line Rust statement stops anywhere inside its
    /// span, not just the marker's own next line). `None` means the frame has no
    /// Jet line at all (prelude/generated glue) — the caller steps over it (I2).
    pub(crate) fn jet_line_for(&self, rust_line: usize) -> Option<usize> {
        self.rust_to_jet
            .range(..=rust_line)
            .next_back()
            .map(|(_, v)| *v)
    }

    /// The rust line to set a native breakpoint on for `break <jet_line>`.
    pub(crate) fn rust_line_for(&self, jet_line: usize) -> Option<usize> {
        self.jet_to_rust.get(&jet_line).copied()
    }

    /// The first marked rust line AT OR AFTER `rust_line` — used to place the
    /// initial breakpoint at `fn main`'s first real statement by `-f -l`
    /// (line-based) rather than `-n main` (name-based, which can resolve to
    /// more than one symbol and land with no source line at all).
    pub(crate) fn first_at_or_after(&self, rust_line: usize) -> Option<usize> {
        self.rust_to_jet.range(rust_line..).next().map(|(k, _)| *k)
    }

    /// The rust line to set the INITIAL breakpoint on: `fn main`'s first real
    /// statement. Locates the literal `fn main(` header text (codegen always
    /// emits this — the golden tests assert `contains("fn main()")`), then the
    /// first marked line at or after it. Shared by the terminal session
    /// (`Native.rs`) and the DAP `launch` handler (`Dap.rs`) so both use the
    /// same file:line breakpoint, never the ambiguous `-n main`.
    pub(crate) fn main_entry_line(&self, rust_src: &str) -> Option<usize> {
        let main_header = rust_src
            .lines()
            .position(|l| l.trim_start().starts_with("fn main("))
            .map(|i| i + 1)
            .unwrap_or(1);
        self.first_at_or_after(main_header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_marker_to_the_following_line() {
        let rust = "fn main() {\n    // jet:line 7\n    let x = 1;\n    // jet:line 8\n    let y = 2;\n}\n";
        let map = LineMap::build(rust);
        assert_eq!(map.jet_line_for(3), Some(7));
        assert_eq!(map.jet_line_for(5), Some(8));
        assert_eq!(map.rust_line_for(7), Some(3));
        assert_eq!(map.rust_line_for(8), Some(5));
    }

    #[test]
    fn multi_line_statement_resolves_to_the_marker_before_it() {
        let rust = "    // jet:line 12\n    let x = some_call(\n        1,\n        2,\n    );\n";
        let map = LineMap::build(rust);
        assert_eq!(map.jet_line_for(2), Some(12));
        assert_eq!(map.jet_line_for(4), Some(12));
    }

    #[test]
    fn no_marker_before_a_line_is_none() {
        let rust = "fn main() {\n    let x = 1;\n}\n";
        let map = LineMap::build(rust);
        assert_eq!(map.jet_line_for(2), None);
    }

    #[test]
    fn first_at_or_after_finds_the_next_marked_line() {
        let rust = "fn main() {\n    // jet:line 2\n    let x = 1;\n    // jet:line 3\n    let y = 2;\n}\n";
        let map = LineMap::build(rust);
        // The `fn main() {` line itself (1) has no marker on it — the search
        // should land on the first REAL statement's rust line (3), not 1.
        assert_eq!(map.first_at_or_after(1), Some(3));
        assert_eq!(map.first_at_or_after(4), Some(5));
        assert_eq!(map.first_at_or_after(6), None);
    }
}
