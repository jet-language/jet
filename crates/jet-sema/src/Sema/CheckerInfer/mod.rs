// Re-export the parent `Sema` glob so the split-out submodules
// (`expr`/`fallible`/`calls`/`binary`) reach `Checker`, etc. via `use super::*`.
pub(crate) use super::*;
use crate::Syntax;
use crate::AST::Type;

/// D-ITER1: returns true when `ty` or an immediate inner layer is `Type::Tuple`.
/// Used to decide whether to store `resolved_ret` on a `MethodCall` node so that
/// `Tuples.rs` can collect the JetTup_ shape for `indexed`/`zip`/`partition`.
pub(crate) fn contains_tuple_type(ty: &Type) -> bool {
    match ty {
        Type::Tuple(_) => true,
        Type::List(inner) => matches!(inner.as_ref(), Type::Tuple(_)),
        // D-ITERTOOLS1=A / D-RANGE-EXCL1=C: adapters return `Iter<(…)>`.
        Type::Apply { name, args }
            if name == Syntax::TYPE_ITER && args.len() == 1 =>
        {
            matches!(&args[0], Type::Tuple(_))
        }
        Type::Option(inner) => matches!(inner.as_ref(), Type::Tuple(_)),
        _ => false,
    }
}

/// D-REACT1=B / D-DATARACE1=C: a reactive handle type `Signal<T>`/`Derived<T>`/
/// `Computed<T>` — an Arc-backed shared value whose "copy" shares the same
/// reactive cell (so L0801 doesn't apply). Lock-ordered so task/channel
/// crossings do not lean on rustc `Send`.
pub(crate) fn is_reactive_handle_ty(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Apply { name, .. }
            if name == Syntax::TYPE_SIGNAL
                || name == Syntax::TYPE_DERIVED
                || name == Syntax::TYPE_COMPUTED
    )
}

mod binary;
mod calls;
mod expr;
mod fallible;
