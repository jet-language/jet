//! Whole-corpus pure-interpreter parity batteries for `jet dev` (#2020).
//!
//! parity: guard tests/dev_interp_parity.rs::interpreter_matches_compiled_binary
#![allow(dead_code, unused_imports)]

mod common;
include!("dev_parts/support.rs");
include!("dev_parts/interp_parity.rs");
