//! The strict JIT<->AOT differential example-corpus gate (#2020).
//!
//! parity: guard tests/dev_corpus_gate.rs::example_corpus_strict_jit_aot_differential_gate
#![allow(dead_code, unused_imports)]

mod common;
include!("dev_parts/support.rs");
include!("dev_parts/corpus_gate.rs");
