// This target include!s the shared perf-trace support module, which three
// targets share and each uses a different subset of.
#![allow(dead_code)]

mod common;
include!("jet_perf_trace_parts/support.rs");
include!("jet_perf_trace_parts/capture_attach.rs");
