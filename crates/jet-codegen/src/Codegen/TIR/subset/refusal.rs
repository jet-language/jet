//! I2 self-report for the TIR coverage gate.
//!
//! `tir_covers*` answers one bool and the emitter has no fallback, so a `false`
//! is an internal compiler error (I2/R7). Reporting only the enclosing function
//! name turned every gate/emitter disagreement into a bisect: the two instances
//! fixed on 2026-08-19 (an unlisted fallible payload, an unadmitted builtin
//! receiver) each cost a search over the whole body to attribute. So the gate
//! now leaves the construct it refused here and the `ice!` sites read it back.
//!
//! This is compiler-internal breadcrumb state. It is never a user diagnostic:
//! nothing renders it except `ice!`, it carries no registered code, and it says
//! "compiler bug" in the same voice as every other I2 abort.

use crate::Codegen::Cx;
use crate::Diagnostics::Span;
use std::cell::Cell;

/// The construct the gate refused: a noun phrase naming what kind of position
/// it is, plus its source span. `Copy`, so recording costs no allocation on the
/// refusal path — which the JIT walks for every function it declines to compile.
#[derive(Clone, Copy)]
struct Refusal {
    what: &'static str,
    span: Span,
}

pub(crate) const EXPR: &str = "expression";
pub(crate) const STMT: &str = "statement";
pub(crate) const PARAM_TY: &str = "parameter type";
pub(crate) const RETURN_TY: &str = "return type";
pub(crate) const SELF_PARAM: &str = "`self` parameter on a plain function";
pub(crate) const EXTRA_SELF_PARAM: &str = "`self` parameter after the first";
pub(crate) const MISSING_SELF_PARAM: &str = "trait method with no `self` receiver";
pub(crate) const UNCOVERED_OWNER: &str = "method whose owner type is not covered";
pub(crate) const TYPE_PARAMS: &str = "method's own type parameters";
pub(crate) const DEFAULTED_TRAIT_PARAM: &str = "defaulted trait-method parameter";

/// How much of the construct's source text an ICE line carries. Enough to name
/// it; not enough to paste a whole body into a panic message.
const MAX_SNIPPET_CHARS: usize = 60;

thread_local! {
    static FIRST: Cell<Option<Refusal>> = const { Cell::new(None) };
}

/// Open a fresh coverage question. Every `tir_covers*` entry point calls this,
/// so a speculative probe — the JIT gates every function before choosing which
/// ones it compiles — cannot leave its breadcrumb behind for the next question.
pub(crate) fn begin() {
    FIRST.with(|slot| slot.set(None));
}

/// Record a refused construct. The first record wins: the routers recurse
/// outside-in, so the innermost node records before the enclosing nodes that
/// only propagate its `false`.
pub(crate) fn note(what: &'static str, span: Span) {
    FIRST.with(|slot| {
        if slot.get().is_none() {
            slot.set(Some(Refusal { what, span }));
        }
    });
}

/// Render the recorded refusal for an `ice!` message: the construct's own
/// source text and `file:line:column`. A gate can still refuse without
/// reaching a recorded position, so the no-record case says so plainly instead
/// of inventing a location.
pub(crate) fn describe(cx: &Cx) -> String {
    let Some(refusal) = FIRST.with(|slot| slot.get()) else {
        return "no construct was recorded".to_string();
    };
    let (line, column) =
        crate::Diagnostics::span_line_col(&cx.src, refusal.span.start.min(cx.src.len()));
    format!(
        "the {} `{}` at {}:{}:{}",
        refusal.what,
        snippet(&cx.src, refusal.span),
        cx.file,
        line,
        column
    )
}

/// One bounded single-line rendering of a span's source text.
fn snippet(src: &str, span: Span) -> String {
    let start = span.start.min(src.len());
    let end = span.end.clamp(start, src.len());
    // A span can land off a character boundary in recovered source; say so
    // rather than panicking inside the panic path.
    let Some(text) = src.get(start..end) else {
        return "<span off a character boundary>".to_string();
    };
    let mut flat = String::with_capacity(text.len().min(MAX_SNIPPET_CHARS + 4));
    for (taken, ch) in text.chars().enumerate() {
        if taken == MAX_SNIPPET_CHARS {
            flat.push('…');
            break;
        }
        flat.push(if ch.is_control() { ' ' } else { ch });
    }
    let trimmed = flat.trim();
    if trimmed.is_empty() {
        "<empty span>".to_string()
    } else {
        trimmed.to_string()
    }
}
