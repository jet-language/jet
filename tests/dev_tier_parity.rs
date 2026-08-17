//! Curated per-topic interpreter/JIT/AOT tier batteries (#2020).
//!
//! parity: guard tests/dev_tier_parity.rs::io_cli_terminal_and_time_match_interpreter_jit_and_aot
#![allow(dead_code, unused_imports)]

mod common;
include!("dev_parts/support.rs");
include!("dev_parts/tier_parity.rs");
