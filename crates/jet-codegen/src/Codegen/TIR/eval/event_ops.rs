//! `core.event` handle methods for the shared TIR evaluator (deopt / interpret).
use crate::Comptime::CtValue;
use crate::Diagnostics::Diagnostic;

use super::{unsupported, EvalCtx};

impl<'a> EvalCtx<'a> {
    pub(super) fn eval_event_method(
        &mut self,
        method: &str,
        recv: &mut CtValue,
        args: &[CtValue],
    ) -> Result<CtValue, Diagnostic> {
        let span = self.span();
        // Split the &mut self borrow across EventLite + call_callable: EventLite
        // only calls `invoke` while not holding EvalCtx state, and we do not
        // touch `self` except through that callback.
        let this = self as *mut EvalCtx<'a>;
        let mut invoke = |handler: CtValue, argv: Vec<CtValue>| -> Result<CtValue, Diagnostic> {
            // SAFETY: uniquely borrowed `self` for the EventLite call; invoke is
            // the only re-entry into EvalCtx, and EventLite holds no &self.
            unsafe { &mut *this }.call_callable(&handler, argv)
        };
        match crate::Comptime::eval_event_method(method, recv, args, span, &mut invoke) {
            Some(result) => result,
            None => Err(unsupported(&format!("event method `{method}`"), span)),
        }
    }
}
