//! Regex callbacks for the shared TIR evaluator (deopt / interpret).
use crate::Comptime::CtValue;
use crate::Diagnostics::Diagnostic;

use super::{unsupported, EvalCtx};

impl<'a> EvalCtx<'a> {
    pub(super) fn eval_regex_replace_all_with(
        &mut self,
        recv: &CtValue,
        args: &[CtValue],
    ) -> Result<CtValue, Diagnostic> {
        let span = self.span();
        let mut invoke = |callback: CtValue, argv: Vec<CtValue>| {
            self.call_callable(&callback, argv)
        };
        crate::Comptime::eval_regex_replace_all_with(recv, args, span, &mut invoke)
            .unwrap_or_else(|| Err(unsupported("regex.replace_all_with receiver", span)))
    }
}
