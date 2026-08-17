//! Whole-corpus default-backend (tiered Cranelift) parity battery (#2020).
//!
//! parity: guard tests/dev_default_parity.rs::dev_default_matches_compiled_binary
#![allow(dead_code, unused_imports)]

mod common;
include!("dev_parts/support.rs");
include!("dev_parts/default_parity.rs");
