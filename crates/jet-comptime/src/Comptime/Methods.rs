//! Comptime and REPL call dispatch split by runtime ownership.

#[path = "Methods/core_calls.rs"]
mod core_calls;
#[path = "Methods/dispatch.rs"]
mod dispatch;
#[path = "Methods/pool.rs"]
mod pool;
#[path = "Methods/repl_process.rs"]
mod repl_process;
#[path = "Methods/time_deadline.rs"]
mod time_deadline_kernel;

pub(crate) use core_calls::as_string;
pub(crate) use core_calls::data_plot_rt;
pub use core_calls::{
    apply_core_call, apply_core_call_with_type, apply_core_call_without_ambient,
    apply_core_call_without_ambient_with_type, apply_core_call_without_ambient_with_type_args,
    apply_core_pure_call, apply_core_pure_method,
    apply_core_call_without_ambient_with_type_args_and_history_schema,
    apply_data_line_call, apply_history_rng_method, apply_impure_core_call,
    apply_impure_core_call_with_type, apply_impure_core_call_with_type_args,
    apply_raylib_ambient_core_call, data_status_rows, history_callback_fingerprint,
    history_command_schema_from_mir, set_trace_id, sketch_add, with_world_rng_provider,
    HistoryCommandSchema,
};
pub(crate) use core_calls::eval_data_describe;
pub(super) use core_calls::{apply_regex_method, as_float, solver_require};
// I9: the MIR evaluator calls this fake-data kernel too, so it
// must be reachable from outside this crate. One kernel, every tier.
pub use core_calls::apply_fake_method;
/// Public host entry for the MIR evaluator (#777).
pub use core_calls::{
    display_core_pure_value, eval_regex_replace_all_with,
};
pub(crate) use core_calls::{
    eval_data_pivot_sum, evaluate_typed_datetime_literal, url_parts_to_ct,
    validate_datetime_literal,
};
/// Public for MirBridge `Rng.shuffle(&list)` write-back (#777).
pub use dispatch::apply_seeded_rng_method;
pub use dispatch::apply_seeded_rng_method_with_type;
pub(crate) use dispatch::{arg_string_literal, check_literal_embed_path, embed_path_err};
pub(super) use dispatch::{
    invoke_standalone_closure, invoke_standalone_closure_mut, invoke_standalone_closure_mut_args,
};
pub use dispatch::{
    eval_build_embed, eval_build_time_io, eval_locked_find, eval_net_fetch, is_tier2_core_call,
    project_rejection, vault_comptime_denied,
};
pub use repl_process::{
    apply_repl_authorized_core_call, apply_repl_authorized_core_call_with_type,
};

pub(super) fn apply_pool(
    recv: &crate::AST::CtValue,
    method: &str,
    args: &[crate::AST::CtValue],
    span: crate::Diagnostics::Span,
) -> Option<
    Result<(crate::AST::CtValue, Option<crate::AST::CtValue>), crate::Diagnostics::Diagnostic>,
> {
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
            (
                "eval_method",
                include_str!("Methods/dispatch/eval_method.rs"),
            ),
            ("repl_process", include_str!("Methods/repl_process.rs")),
            ("core_calls", include_str!("Methods/core_calls.rs")),
            (
                "core_calls/regex",
                include_str!("Methods/core_calls/regex.rs"),
            ),
            (
                "core_calls/data",
                include_str!("Methods/core_calls/data.rs"),
            ),
            (
                "core_calls/impure",
                include_str!("Methods/core_calls/impure.rs"),
            ),
            (
                "core_calls/random",
                include_str!("Methods/core_calls/random.rs"),
            ),
            (
                "core_calls/history",
                include_str!("Methods/core_calls/history.rs"),
            ),
            (
                "core_calls/plain_calls",
                include_str!("Methods/core_calls/plain_calls.rs"),
            ),
            ("pool", include_str!("Methods/pool.rs")),
        ] {
            let lines = source.lines().count();
            assert!(
                lines < 2_500,
                "{name}.rs regrew to {lines} lines; split it along semantic ownership \
                 without changing the 2,500-line module cap"
            );
        }
    }
}
