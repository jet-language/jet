//! D-SERVICE1=D / I9: `core.services` on the canonical TIR evaluator.

use std::collections::HashMap;

use crate::AST::CtValue;
use crate::Comptime::apply_core_call;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Codegen::TIR::TExpr;

use super::EvalCtx;

impl<'a> EvalCtx<'a> {
    pub(super) fn eval_core_services_call(
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
        let result = apply_core_call("core.services", method, argv, source_span, self.repl_mode)?;
        if matches!(
            method,
            "set_restart"
                | "worker"
                | "group"
                | "start"
                | "stop"
                | "send"
                | "receive"
                | "fail_worker"
        ) {
            match crate::Comptime::ServicesLite::take_mut_ok(result) {
                Ok((tree, value)) => {
                    if let Some(place) = args.first() {
                        if !matches!(tree, CtValue::Unit) {
                            self.write_back_place(place, tree, scope)?;
                        }
                    }
                    Ok(value)
                }
                Err(err) => Ok(err),
            }
        } else {
            Ok(result)
        }
    }
}
