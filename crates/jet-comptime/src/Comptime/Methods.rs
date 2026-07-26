//! Comptime and REPL call dispatch split by runtime ownership.

#[path = "Methods/dispatch.rs"]
mod dispatch;
#[path = "Methods/repl_process.rs"]
mod repl_process;
#[path = "Methods/core_calls.rs"]
mod core_calls;
#[path = "Methods/pool.rs"]
mod pool;

pub(super) use core_calls::{apply_core_pure_method, as_float, as_string, solver_require};
/// Public host entry for the TIR evaluator (#777).
pub use core_calls::{apply_core_call, apply_impure_core_call, display_core_pure_value};
pub use dispatch::apply_dollar_splices;
/// Public for TirBridge `Rng.shuffle(&list)` write-back (#777).
pub use dispatch::apply_seeded_rng_method;
pub(crate) use dispatch::{check_literal_embed_path, embed_path_err, find_glob};

pub(super) fn apply_pool(
    recv: &crate::AST::CtValue,
    method: &str,
    args: &[crate::AST::CtValue],
    span: crate::Diagnostics::Span,
) -> Option<Result<(crate::AST::CtValue, Option<crate::AST::CtValue>), crate::Diagnostics::Diagnostic>>
{
    if !pool::is_method(recv, method) {
        return None;
    }
    Some(pool::apply(recv, method, args, None, span).map(|o| (o.value, o.updated)))
}

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
