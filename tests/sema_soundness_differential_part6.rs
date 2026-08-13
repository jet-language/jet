const SUITE: &str = "sema_soundness_differential_part6";
const DIFFERENTIAL_PARTITION: usize = 5;
const DIFFERENTIAL_PARTITIONS: usize = 8;
mod common;
include!("sema_soundness_parts/support.rs");
include!("sema_soundness_parts/differential.rs");
