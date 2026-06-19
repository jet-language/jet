//! Comptime diagnostic constructors (E0951 impurity · E0953 panic family ·
//! E0956 unsupported construct). E0952/E2202 fuel diagnostics are inline in
//! `Interp::burn`; E0955 embed-file errors are inline in `eval_embed_file`.

use crate::ast::Expr;
use crate::diag::{Diagnostic, Span};

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
        "compute this at runtime instead; the one exception is `embed_file(\"path\")`".to_string(),
        Some(span),
    )
}
