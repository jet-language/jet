// Re-export the parent `Sema` glob so the split-out submodules
// (`expr`/`fallible`/`calls`/`binary`) reach `Checker`, etc. via `use super::*`.
pub(crate) use super::*;
use crate::Syntax;
use crate::AST::Type;

/// D-ITER1: returns true when `ty` or an immediate inner layer is `Type::Tuple`.
/// Used to decide whether to store `resolved_ret` on a `MethodCall` node so that
/// `Tuples.rs` can collect the JetTup_ shape for `enumerate`/`zip`/`partition`.
pub(crate) fn contains_tuple_type(ty: &Type) -> bool {
    match ty {
        Type::Tuple(_) => true,
        Type::List(inner) => matches!(inner.as_ref(), Type::Tuple(_)),
        _ => false,
    }
}

/// D-REACT1=B: a reactive handle type `Signal<T>`/`Derived<T>` — an Rc-backed shared
/// value whose "copy" shares the same reactive cell (so L0801 doesn't apply).
pub(crate) fn is_reactive_handle_ty(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Apply { name, .. }
            if name == Syntax::TYPE_SIGNAL || name == Syntax::TYPE_DERIVED
    )
}

mod binary;
mod calls;
mod expr;
mod fallible;
