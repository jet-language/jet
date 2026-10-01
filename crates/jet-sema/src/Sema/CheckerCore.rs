mod scopes;
mod type_assign;
pub(crate) use type_assign::{is_core_view_generic, reject_never_value_positions};
mod blocks;
mod control_flow;
mod statements;
mod switches;
pub(crate) use switches::{
    atomic_absent_optional_subject, collect_window_names, condition_window_names,
    contextual_literal, expr_is_absent_none, is_write_window_subject, names_optional_state,
    pattern_consumes_result_carrier, ContextualLiteral,
};
mod bindings;
mod types;
pub(crate) use bindings::*;
mod helpers;
mod names_incdec;
