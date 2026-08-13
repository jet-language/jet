const SUITE: &str = "sema_soundness_differential";
const DIFFERENTIAL_PARTITION: usize = 0;
const DIFFERENTIAL_PARTITIONS: usize = 8;
mod common;
include!("sema_soundness_parts/support.rs");
include!("sema_soundness_parts/differential.rs");
