//! Comptime and REPL call dispatch split by runtime ownership.

#[path = "Methods/dispatch.rs"]
mod dispatch;
#[path = "Methods/repl_process.rs"]
mod repl_process;
#[path = "Methods/core_calls.rs"]
mod core_calls;
#[path = "Methods/pool.rs"]
mod pool;

pub(super) use core_calls::{apply_core_pure_method, as_float, as_string};

#[cfg(test)]
mod structure_tests {
    #[test]
    fn method_modules_stay_below_the_split_threshold() {
        for (name, source) in [
            ("dispatch", include_str!("Methods/dispatch.rs")),
            ("eval_method", include_str!("Methods/dispatch/eval_method.rs")),
            ("repl_process", include_str!("Methods/repl_process.rs")),
            ("core_calls", include_str!("Methods/core_calls.rs")),
            ("pool", include_str!("Methods/pool.rs")),
        ] {
            let lines = source.lines().count();
            assert!(
                lines < 3_300,
                "{name}.rs regrew to {lines} lines; split it along semantic ownership \
                 (cap raised from 2_500 when dispatch absorbed empty-schema/REPL \
                 binding-type plumbing for core.data; still under one module)"
            );
        }
    }
}
