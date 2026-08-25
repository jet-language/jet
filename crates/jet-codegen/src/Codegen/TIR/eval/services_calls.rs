//! D-SERVICE1=D / I9: `core.services` on the canonical TIR evaluator.

use std::collections::HashMap;

use crate::Codegen::TIR::TExpr;
use crate::Comptime::apply_core_call;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::CtValue;

use super::EvalCtx;

fn workflow_wait(nanos: i64) -> crate::Comptime::ServicesLite::JetServiceWorkflowWait<()> {
    match crate::scheduler::jet_scheduler_wait_without_unwind(|| {
        crate::scheduler::jet_std_time_sleep_duration_ns(nanos)
    }) {
        crate::scheduler::JetSchedulerWait::Ready(()) => {
            crate::Comptime::ServicesLite::JetServiceWorkflowWait::Ready(())
        }
        crate::scheduler::JetSchedulerWait::Cancelled => {
            crate::Comptime::ServicesLite::JetServiceWorkflowWait::Cancelled
        }
        crate::scheduler::JetSchedulerWait::Deadline(reason) => {
            crate::Comptime::ServicesLite::JetServiceWorkflowWait::Deadline(reason)
        }
        crate::scheduler::JetSchedulerWait::Panicked(reason) => {
            crate::Comptime::ServicesLite::JetServiceWorkflowWait::Panicked(reason)
        }
    }
}

impl<'a, 'debug> EvalCtx<'a, 'debug> {
    pub(super) fn eval_core_services_call(
        &mut self,
        module: &str,
        method: &str,
        args: &'a [TExpr],
        source_span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval_expr(a, scope)?);
        }
        if matches!(module, "core.service" | "core.services") && method == "runtime" {
            if let Some(result) =
                crate::Comptime::try_ambient_core_call(module, method, argv.clone(), source_span)
            {
                return result;
            }
            return crate::Comptime::ServicesLite::with_workflow_wait(workflow_wait, || {
                crate::Comptime::ServicesLite::apply("runtime", &argv, source_span)
            });
        }
        let result = if matches!(module, "core.service" | "core.services") {
            crate::Comptime::ServicesLite::with_workflow_wait(workflow_wait, || {
                apply_core_call(module, method, argv, source_span, self.repl_mode)
            })?
        } else {
            apply_core_call(module, method, argv, source_span, self.repl_mode)?
        };
        // Any `mutate_ok` carrier must write the updated tree back (I9). The
        // early allowlist only covered the #444 tree slice and missed
        // delivery/workflow/handoff mutators (#1148–#1153).
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
    }
}
