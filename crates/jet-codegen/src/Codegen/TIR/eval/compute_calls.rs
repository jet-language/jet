//! D-COMPUTE1=D / I9: `core.compute` on the canonical TIR evaluator.
//! Semantics come from `ComputeLite` → shared Prelude `Compute.rs`.

use std::collections::HashMap;

use crate::AST::CtValue;
use crate::Comptime::apply_core_call;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Codegen::TIR::TExpr;

use super::EvalCtx;

impl<'a> EvalCtx<'a> {
    pub(super) fn eval_core_compute_call(
        &mut self,
        method: &str,
        args: &'a [TExpr],
        source_span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval_expr(a, scope)?);
        }
        let result = apply_core_call("core.compute", method, argv, source_span, self.repl_mode)?;
        if method == "set" {
            if let Some((tensor, unit)) = crate::Comptime::ComputeLite::take_set_ok(result) {
                if let Some(place) = args.first() {
                    self.write_back_place(place, tensor, scope)?;
                }
                return Ok(unit);
            }
            // take_set_ok always returns Some for ResOk/ResErr shapes; fall through
            // only if the payload was unexpected.
            return Ok(CtValue::ResErr(Box::new(CtValue::Str(
                "core.compute.set: unexpected ambient payload".to_string(),
            ))));
        }
        Ok(result)
    }
}
