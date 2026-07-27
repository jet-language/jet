//! Browser host dispatch for the canonical tier-0 evaluator (#772).

use crate::Codegen::TIR::THandleOp;
use crate::Comptime::CtValue;
use crate::Diagnostics::{Diagnostic, Span};

pub(super) fn core_call(
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    crate::BrowserHost::eval_core_call(method, args, span)
}

pub(super) fn handle(
    op: &THandleOp,
    recv: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let THandleOp::HTTPServerMethod { kind, method } = op else {
        return None;
    };
    if !matches!(
        kind.as_str(),
        "Browser"
            | "BrowserContext"
            | "BrowserPage"
            | "BrowserLocator"
            | "BrowserEvent"
            | "BrowserTrace"
            | "BrowserCapabilities"
            | "BrowserProtocol"
            | "BrowserLocked"
    ) {
        return None;
    }
    Some(if matches!(
        kind.as_str(),
        "BrowserEvent" | "BrowserTrace" | "BrowserCapabilities" | "BrowserLocked"
    ) {
        crate::BrowserHost::eval_value_method(kind, method, recv, span).and_then(|value| {
            value.ok_or_else(|| {
                Diagnostic::error(
                    "E0956",
                    format!("Browser method `{method}` can't run in tier 0"),
                    "the Browser value does not support this method".to_string(),
                    "use a method defined for this Browser value".to_string(),
                    Some(span),
                )
            })
        })
    } else {
        crate::BrowserHost::eval_method(kind, method, recv, args, span)
    })
}

pub(super) struct SessionGuard;

impl SessionGuard {
    pub(super) fn new() -> Self {
        crate::BrowserHost::clear();
        Self
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        crate::BrowserHost::clear();
    }
}
