//! E2-M4 — `jet dev` interpreter tests: the routine slice.
//!
//! The crux is the **differential battery** (D-DEV, I2): for each supported
//! program, the interpreter's stdout/stderr/exit code MUST be byte-for-byte
//! identical to the compiled native binary. Any divergence is a P0
//! miscompile-class bug — the interpreter is a dev convenience that must never
//! lie about what the real build does. This mirrors `tests/comptime_diff.rs`.
//!
//! Also tested here:
//!   - the E2201 honest-boundary note (FFI/`#Unsafe`/native std),
//!   - the per-iteration `dev_iteration` function the watch loop is built on.
//!
//! The D-DEV3 save-to-diagnostic latency budget is NOT here: a wall-clock
//! verdict cannot be taken inside a parallel suite, so #2005 moved it to
//! `tests/dev_latency.rs`.
//!
//! #2020: the whole-corpus batteries are NOT here. One binary could not run its
//! own 165 declared tests inside the 900s suite guard, so the corpus batteries
//! moved to sibling targets — `dev_interp_parity`, `dev_default_parity`,
//! `dev_tier_parity`, `dev_corpus`, `dev_corpus_gate` — each with its own budget.
//! Every one of them is a `tests/*.rs` file, so they are all routine by
//! construction: `scripts/agent/time-suites.sh` times each root test binary and
//! `tools/ci/test-shards.sh` shards the whole inventory (D-CI1=A).
//! `every_dev_slice_is_wired_into_a_test_target` pins that nothing was dropped.
#![allow(dead_code, unused_imports)]

mod common;
include!("dev_parts/support.rs");
include!("dev_parts/routine.rs");
