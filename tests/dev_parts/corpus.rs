// The shared corpus observation and everything ratcheted against it (#2020).
//
// `jit_coverage_audit`, `jit_try_compile_manifest_matches` and
// `cranelift_three_way_differential_battery` all read the ONE memoised
// `collect_jit_coverage` walk, so they are deliberately kept in the SAME target:
// splitting them apart would pay for that whole-corpus compile walk twice and
// let two copies of the same answer drift (AGENTS.md I8).

/// The one compile-coverage ratchet (#125 / #778, hole closed by #1663).
///
/// Audits which examples compile through the JIT lowerer against
/// `tests/jit_gaps.txt`, directionally: `compile_covered:` may only grow, `gaps:`
/// and `run_gaps:` may only shrink. Moving the ratchet takes two edits in one
/// diff — the rows in the ledger, and `COMPILE_COVERED_FLOOR` below, which is
/// pinned here precisely because it must live outside the file it guards.
///
/// #1998 closed the hole under all of that: the audit now states its own
/// denominator. `EXAMPLE_CORPUS_FLOOR` pins how many stems it measured and
/// `OUT_OF_UNIVERSE_CEILING` pins how many of them it could not judge, both
/// outside the ledger for the same reason the coverage floor is. Before that,
/// a stem that failed the in-process `Loader`/`Sema` pass joined neither
/// `compile_covered:` nor `gaps:`, so `gaps: 0` meant "no gaps among the stems
/// that happened to load" and never said how many did not.
///
/// This is the only copy of that law. `jit_try_compile_manifest_matches` used to
/// assert the same two sets from a second hand-maintained comparison, which is
/// how the ledger was falsified three times: green one copy, never learn the
/// other exists.
#[test]
fn jit_coverage_audit() {
    with_jit_test_scope(jit_coverage_audit_inner);
}

/// c139 M3+: three-way differential (JIT == interpreter == AOT) on resident-safe examples.
///
/// The universe is stated, not implied: the `resident_safe` bucket of the shared
/// `collect_jit_coverage` observation — the compile-covered stems the resident
/// JIT will also run — restricted to the ones carrying an `.out` golden. Every
/// other stem of the corpus is counted and named on every run, and
/// `EXAMPLE_CORPUS_FLOOR`, `OUT_OF_UNIVERSE_CEILING`, `RESIDENT_SAFE_FLOOR` and
/// `THREE_WAY_RAN_FLOOR` pin all of it, so this battery cannot pass over a
/// universe that quietly shrank (#2012).
#[test]
fn cranelift_three_way_differential_battery() {
    with_jit_test_scope(cranelift_three_way_differential_battery_inner);
}

/// The CI entry point for `jit_coverage_audit` — not a second law.
///
/// `tools/ci/jit-aot-parity.sh` runs this exact name to produce `RATCHET_STATUS`,
/// and `cargo test -- --exact <missing name>` runs zero tests and exits 0, so
/// deleting the name would report the ratchet green forever. It therefore runs
/// `jit_coverage_audit_inner` rather than repeating its assertions. Delete this
/// entry point in the same diff that repoints the CI script at
/// `jit_coverage_audit`.
#[test]
fn jit_try_compile_manifest_matches() {
    with_jit_test_scope(jit_coverage_audit_inner);
}
