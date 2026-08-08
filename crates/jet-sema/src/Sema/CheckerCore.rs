mod scopes;
mod type_assign;
mod blocks;
mod statements;
mod control_flow;
mod switches;
pub(crate) use switches::{
    atomic_absent_optional_subject, contextual_literal, normalize_contextual_pattern,
    ContextualLiteral,
};
mod types;
mod bindings;
pub(crate) use bindings::*;
mod names_incdec;
mod helpers;
