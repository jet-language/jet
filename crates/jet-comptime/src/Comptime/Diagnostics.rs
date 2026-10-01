//! Comptime diagnostic constructors (E3401 impurity — D-META-EFFECT1 c3: the
//! comptime purity gate now shares its diagnostic code with the run-time
//! `-[]>` check, since the two are the same rule at different stages ·
//! E0953 panic family · E0956 unsupported construct). E0952/E2202 fuel
//! diagnostics are inline in `Interp::burn`; E0955 embed-file errors are
//! inline in `eval_embed_file`.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::Expr;

pub fn comptime_panic(msg: &str, span: Span) -> Diagnostic {
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

pub(super) fn unsupported(what: &str, span: Span) -> Diagnostic {
    jet_foundation::Prelude::jet_e0956_unsupported(what, span)
}

pub(super) fn unsupported_expr(e: &Expr) -> Diagnostic {
    unsupported("this expression", e.span())
}

/// D-META-EFFECT1 c3: the comptime purity gate's diagnostic — one call-graph
/// walk, one code (E3401), shared with the run-time `-[]>` check
/// (`jet-sema/Sema/Purity.rs::e3401`). E0951 retired into this code; every
/// place that used to see E0951 now sees E3401 with the same shape of
/// message.
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
        "E3401",
        format!("`{}` is not allowed in comptime code", name),
        why,
        "compute this at runtime instead; the exceptions are `embed_file(\"path\")`, `embed_bytes(\"path\")`, and `find(\"glob\")`".to_string(),
        Some(span),
    )
}
