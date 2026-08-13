//! #353: deterministic accepts-invalid and miscompile adversary corpus.

const SUITE: &str = "sema_soundness";
mod common;
include!("sema_soundness_parts/support.rs");
include!("sema_soundness_parts/metadata.rs");
