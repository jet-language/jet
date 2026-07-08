//! Comptime diagnostic constructors (E0951 impurity · E0953 panic family ·
//! E0956 unsupported construct). E0952/E2202 fuel diagnostics are inline in
//! `Interp::burn`; E0955 embed-file errors are inline in `eval_embed_file`.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::Expr;

pub(super) fn comptime_panic(msg: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0953",
        "your comptime code stopped the build".to_string(),
        format!(
            "while computing this value at compile time, the program panicked: {}",
            msg
        ),
        "this is the sanctioned way to validate at compile time — fix the input the check rejects"
            .to_string(),
        Some(span),
    )
}

pub(super) fn overflow(verb: &str, span: Span) -> Diagnostic {
    comptime_panic(
        &format!(
            "tried to {} two numbers and the result was too big for an Int",
            verb
        ),
        span,
    )
}

pub(super) fn divide_by_zero(span: Span) -> Diagnostic {
    comptime_panic("divided by zero", span)
}

pub(super) fn index_oob(len: usize, i: i64, span: Span) -> Diagnostic {
    comptime_panic(
        &format!(
            "the list has {} items, so position {} doesn't exist",
            len, i
        ),
        span,
    )
}

pub(super) fn slice_oob(len: i64, a: i64, b: i64, span: Span) -> Diagnostic {
    comptime_panic(
        &format!("can't slice {} items from {} to {} (inclusive)", len, a, b),
        span,
    )
}

pub(super) fn map_missing(span: Span) -> Diagnostic {
    comptime_panic("the map has no entry for this key", span)
}

/// c97/D-STRPARSE1: internal sentinel used by `Expr::Try` to propagate an
/// error value through the interpreter call stack. The error value is encoded
/// in `what`; `eval_call` intercepts this code and converts it to a
/// `CtValue::ResErr(...)` return instead of surfacing it as a user diagnostic.
pub(super) const ERR_PROPAGATE_CODE: &str = "__CT_ERR_PROPAGATE__";

/// c97/D-STRPARSE1: internal sentinel for `?? return expr` — signals an early
/// return (not an error) from the current comptime function. The stringified
/// return value is in `what`. `eval_call` intercepts this and returns the
/// value wrapped in a `Flow::Return`-equivalent `CtValue`.
pub(super) const EARLY_RETURN_CODE: &str = "__CT_EARLY_RETURN__";

pub(super) fn err_propagate_sentinel(encoded_err: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        ERR_PROPAGATE_CODE,
        encoded_err.to_string(),
        String::new(),
        String::new(),
        Some(span),
    )
}

pub(super) fn early_return_sentinel(encoded_val: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        EARLY_RETURN_CODE,
        encoded_val.to_string(),
        String::new(),
        String::new(),
        Some(span),
    )
}

pub(super) fn unsupported(what: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        format!("{} can't run at compile time yet", what),
        "comptime evaluates a pure subset of Jet; this construct isn't supported there yet"
            .to_string(),
        "compute this value at runtime, or use a simpler comptime expression".to_string(),
        Some(span),
    )
}

pub(super) fn unsupported_expr(e: &Expr) -> Diagnostic {
    unsupported("this expression", e.span())
}

pub(super) fn impurity_diag(name: &str, path: &[String], span: Span) -> Diagnostic {
    let why = if path.is_empty() {
        format!(
            "`{}` touches the outside world, so it can't run while compiling",
            name
        )
    } else {
        format!(
            "{} calls `{}`, which touches the outside world — comptime must give the same answer on every machine",
            path.join(" calls "),
            name
        )
    };
    Diagnostic::error(
        "E0951",
        format!("`{}` is not allowed in comptime code", name),
        why,
        "compute this at runtime instead; the exceptions are `embed_file(\"path\")`, `embed_bytes(\"path\")`, and `find(\"glob\")`".to_string(),
        Some(span),
    )
}
