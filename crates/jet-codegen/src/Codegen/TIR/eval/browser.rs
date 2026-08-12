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
            | "BrowserFrame"
            | "BrowserLocator"
            | "BrowserIntercept"
            | "BrowserEvent"
            | "BrowserTrace"
            | "BrowserReceipt"
            | "BrowserPrivacy"
            | "BrowserCapabilities"
            | "BrowserProtocol"
            | "BrowserLocked"
    ) {
        return None;
    }
    Some(if matches!(
        kind.as_str(),
        "BrowserEvent"
            | "BrowserTrace"
            | "BrowserReceipt"
            | "BrowserPrivacy"
            | "BrowserCapabilities"
            | "BrowserLocked"
    ) {
        crate::BrowserHost::eval_value_method(kind, method, recv, span).and_then(|value| {
            value.ok_or_else(|| {
                crate::Sema::Diagnostics::browser_tier0_unsupported(
                    &format!("method `{method}`"),
                    span,
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
