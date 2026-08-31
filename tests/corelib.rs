#![allow(dead_code, unused_imports)]
mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;
include!("corelib_parts/support.rs");
include!("corelib_parts/derives.rs");
include!("corelib_parts/email.rs");
