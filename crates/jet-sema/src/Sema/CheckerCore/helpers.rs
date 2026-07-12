use crate::AST::{Expr, Type};
use crate::Diagnostics::{Diagnostic, Span};

/// D-UNINIT1 engine (reused by D-UNINIT-SENTINEL1): a `:= uninit` binding is
/// restricted to plain-data ("POD") types — no heap ownership, no Drop glue —
/// so an uninitialized value can never expose freed/owned state. v1 allows
/// scalars, `Char`, `U8`, and fixed arrays of those.
pub(crate) fn is_pod_uninit_type(ty: &Type) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::FixedList { elem, .. } => is_pod_uninit_type(elem),
        _ => false,
    }
}

/// D-LAYOUT1 (E2934): a structural fingerprint of a (parser-desugared)
/// layout constraint expression, used only to catch EXACT duplicate lines
/// within one `layout { … }` block. Not a general expression hasher — it
/// only needs to be stable and injective enough for the small shapes a
/// constraint line can take (`Binary`/`MethodCall`/`Ident`/literals).
pub(super) fn layout_constraint_fingerprint(e: &Expr) -> String {
    match e {
        Expr::Binary(op, l, r, _) => format!(
            "({} {:?} {})",
            layout_constraint_fingerprint(l),
            op,
            layout_constraint_fingerprint(r)
        ),
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            let recv = layout_constraint_fingerprint(receiver);
            let a: Vec<String> = args
                .iter()
                .map(|a| layout_constraint_fingerprint(&a.expr))
                .collect();
            format!("{}.{}({})", recv, method, a.join(","))
        }
        Expr::Ident(n, _) => n.clone(),
        Expr::Str(parts, _) => match parts.as_slice() {
            [crate::AST::StrPart::Lit(s)] => format!("{:?}", s),
            _ => "<str>".to_string(),
        },
        Expr::Float(f, _, _) => f.to_string(),
        Expr::Int(i, _, _) => i.to_string(),
        _ => "<?>".to_string(),
    }
}

pub(super) fn no_any_type(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0350",
        "Jet does not have an `Any` type".to_string(),
        "a value should keep a precise shape: use an enum for known variants, generics or traits for abstraction, `T?` for absence, and `DataTree` for parsed dynamic data".to_string(),
        "replace `Any` with the specific mechanism for this value".to_string(),
        Some(span),
    )
}
