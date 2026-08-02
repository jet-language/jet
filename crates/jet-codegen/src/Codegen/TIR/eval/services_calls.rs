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
        if method == "runtime" {
            if let Some(result) = crate::Comptime::try_ambient_core_call(
                "core.services",
                method,
                argv.clone(),
                source_span,
            ) {
                return result;
            }
            let store = match argv.first() {
                Some(CtValue::Str(store)) => store.clone(),
                _ => return Err(Diagnostic::error(
                    "E0956",
                    "core.services.runtime expects a store path".to_string(),
                    "pass a text path and a Duration retention window".to_string(),
                    "provide both runtime arguments".to_string(),
                    Some(source_span),
                )),
            };
            let retention_ms = match argv.get(1) {
                Some(CtValue::Struct { type_name, fields }) if type_name == "Duration" => fields
                    .iter()
                    .find_map(|(name, value)| match (name.as_str(), value) {
                        ("ms", CtValue::Int(ms)) => Some(*ms),
                        _ => None,
                    })
                    .ok_or_else(|| Diagnostic::error(
                        "E0956",
                        "core.services.runtime received an invalid Duration".to_string(),
                        "pass a checked Duration value".to_string(),
                        "construct the retention window with core.time".to_string(),
                        Some(source_span),
                    ))?,
                _ => return Err(Diagnostic::error(
                    "E0956",
                    "core.services.runtime expects a Duration".to_string(),
                    "pass a checked Duration retention window".to_string(),
                    "construct the retention window with core.time".to_string(),
                    Some(source_span),
                )),
            };
            return Ok(CtValue::Struct {
                type_name: "ServiceRuntime".to_string(),
                fields: vec![
                    ("store".to_string(), CtValue::Str(store)),
                    ("retention_ms".to_string(), CtValue::Int(retention_ms)),
                ],
            });
        }
        let result = apply_core_call("core.services", method, argv, source_span, self.repl_mode)?;
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
