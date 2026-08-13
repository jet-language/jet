use crate::AST::{Stmt, Type};
use crate::Diagnostics::Diagnostic;
use crate::Sema::Checker;

impl<'a> Checker<'a> {
    pub(crate) fn check_comptime_block(&mut self, stmt: &mut Stmt) {
        let Stmt::ComptimeBlock { body, .. } = stmt else {
            return;
        };
        let globals = self.current_ct_globals();
        match crate::Comptime::run_block_with_imports(
            body,
            self.ct_funcs,
            self.ct_externs,
            self.ct_base_dir,
            &globals,
            self.core_imports,
        ) {
            Ok(scope) => {
                let current = self.ct_scopes.last_mut().unwrap();
                for (name, value) in scope {
                    current.insert(name, value);
                }
            }
            Err(diagnostic) => self.diags.push(diagnostic),
        }
    }

    pub(crate) fn check_comptime_if(&mut self, stmt: &mut Stmt) {
        let Stmt::ComptimeIf {
            cond,
            cond_span,
            then_body,
            else_body,
            selected_then,
            ..
        } = stmt
        else {
            return;
        };
        let globals = self.current_ct_globals();
        let selected = match crate::Comptime::evaluate_owned_with_imports_opts(
            cond,
            self.ct_funcs,
            self.ct_externs,
            self.ct_base_dir,
            &globals,
            self.core_imports,
            self.gates,
            self.ct_impure_depth,
        ) {
            Ok(crate::Comptime::CtValue::Bool(value)) => value,
            Ok(_) => {
                self.diags.push(Diagnostic::error(
                    "E0989",
                    format!(
                        "a `$if` condition must be {}, not another type",
                        Type::Bool.show()
                    ),
                    "the condition selects a branch at compile time — it must be true or false"
                        .to_string(),
                    "write a Bool known-time expression, like `$if flag { … }`"
                        .to_string(),
                    Some(*cond_span),
                ));
                return;
            }
            Err(_) => {
                self.diags.push(Diagnostic::error(
                    "E0989",
                    "this `$if` condition can't be known at compile time".to_string(),
                    "a `$if` condition must be a known-time expression — a `$` binding, a literal, or a pure function call with known arguments (D-WHEN1)".to_string(),
                    "use a `$` binding: `$flag :: …; $if flag { … }`"
                        .to_string(),
                    Some(*cond_span),
                ));
                return;
            }
        };
        *selected_then = Some(selected);

        if selected {
            self.check_block(then_body, true);
        } else if let Some(body) = else_body {
            self.check_block(body, true);
        }

        let dropped_arm = if selected {
            else_body.as_mut()
        } else {
            Some(then_body)
        };
        if let Some(dropped) = dropped_arm {
            let diagnostic_start = self.diags.len();
            let previous = self.in_dropped_comptime_arm;
            // D-FACT-FLOW1: the dropped arm is walked for name resolution only.
            // It is not a path through this code, so nothing it does to the
            // flow facts survives it.
            let facts = self.flow.clone();
            self.in_dropped_comptime_arm = true;
            self.check_block(dropped, true);
            self.in_dropped_comptime_arm = previous;
            self.flow = facts;
            let diagnostics: Vec<Diagnostic> =
                self.diags.drain(diagnostic_start..).collect();
            self.diags.extend(
                diagnostics
                    .into_iter()
                    .filter(|diagnostic| matches!(diagnostic.code.as_str(), "E0107" | "E0102")),
            );
        }
    }
}
