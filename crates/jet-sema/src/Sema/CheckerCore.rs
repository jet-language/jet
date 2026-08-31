mod scopes;
mod type_assign;
pub(crate) use type_assign::is_core_view_generic;
mod blocks;
mod control_flow;
mod statements;
mod switches;
pub(crate) use switches::{
    atomic_absent_optional_subject, contextual_literal, normalize_contextual_pattern,
    pattern_consumes_result_carrier, ContextualLiteral,
};
mod bindings;
mod types;
pub(crate) use bindings::*;
mod helpers;
mod names_incdec;
