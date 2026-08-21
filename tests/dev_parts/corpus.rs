// The shared corpus observation and everything ratcheted against it (#2020).
//
// `jit_coverage_audit` and `cranelift_three_way_differential_battery` both read the ONE memoised
// `collect_jit_coverage` walk, so they are deliberately kept in the SAME target:
// splitting them apart would pay for that whole-corpus compile walk twice and
// let two copies of the same answer drift (AGENTS.md I8).

/// The one compile-coverage ratchet (#125 / #778, hole closed by #1663).
///
/// Audits every example through the JIT lowerer and the observed run-tier gate.
/// The old hand-maintained `jit_gaps.txt` baseline is retired. The audit now
/// states its own denominator: `EXAMPLE_CORPUS_FLOOR` pins how many stems it
/// measures and `OUT_OF_UNIVERSE_CEILING` pins how many it cannot judge.
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
