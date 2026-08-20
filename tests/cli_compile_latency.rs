mod common;
include!("cli_parts/support.rs");
// Own target: the pinned compile-latency policy (one warmup, twenty samples, three
// cache scenarios, bootstrap plus check) is 128 real child compiles, which cannot
// share a 900s budget with the rest of `cli`. Splitting the target is half the
// remedy; the other half is the committed guard row for this target in
// tests/suite_budgets.txt, since 128 child compiles do not fit the default
// either (#677). See cli_parts/compile_latency.rs.
#[path = "cli_parts/compile_latency.rs"]
mod cli_compile_latency;
