impl<'a> Checker<'a> {
        /// D-A11YGATE1=B (c134 Phase 6, E2930): flag `ui.node_role(label, w, h, role)`
        /// when `label` is a literal empty string and `role` is a literal interactive
        /// role (`ui.aria_role_button()` / `ui.aria_role_text_input()`). Static and
        /// literal-only by design: a computed label isn't traced, so this never
        /// second-guesses a runtime value — only a call site that's provably wrong.
        pub(crate) fn check_a11y_node_role_label(&mut self, args: &[crate::AST::CallArg], span: Span) {
            if args.len() != 4 {
                return;
            }
            if !is_empty_string_literal(&args[0].expr) {
                return;
            }
            let Some(role) = self.interactive_aria_role_name(&args[3].expr) else {
                return;
            };
            self.diags.push(a11y_unlabeled_control(role, span));
        }
    
        /// D-A11YGATE1=B (E2931): within an inline `[ui.node_role(...), ...]` list
        /// literal passed to `backend.set_focus_group(...)`, flag two interactive
        /// nodes that share the same non-empty literal label. Inline-construction
        /// only (like E2930) — a list of pre-bound variables isn't traced back to
        /// their `node_role` call sites, so this catches the common "copy-pasted a
        /// focus group" mistake, not every possible duplicate.
        pub(crate) fn check_a11y_focus_group_duplicates(
            &mut self,
            args: &[crate::AST::CallArg],
            span: Span,
        ) {
            let Some(list_arg) = args.first() else {
                return;
            };
            let Expr::ListLit(items, _) = &list_arg.expr else {
                return;
            };
            let mut seen: std::collections::HashMap<String, ()> = std::collections::HashMap::new();
            for item in items {
                let Expr::MethodCall {
                    receiver,
                    method,
                    args: call_args,
                    ..
                } = item
                else {
                    continue;
                };
                if method != "node_role" || call_args.len() != 4 {
                    continue;
                }
                let Expr::Ident(alias, _) = &**receiver else {
                    continue;
                };
                if self.core_imports.get(alias).map(|m| m.as_str()) != Some("core.ui") {
                    continue;
                }
                if self
                    .interactive_aria_role_name(&call_args[3].expr)
                    .is_none()
                {
                    continue;
                }
                let Some(label) = literal_string_value(&call_args[0].expr) else {
                    continue;
                };
                if label.is_empty() {
                    continue;
                }
                if seen.insert(label.clone(), ()).is_some() {
                    self.diags.push(a11y_duplicate_label(&label, span));
                    return;
                }
            }
        }
    
        /// D-A11YGATE1=B: is `expr` a literal `ui.aria_role_button()` /
        /// `ui.aria_role_text_input()` call through a `use core.ui as ui` alias?
        /// Returns the display name (`"button"` / `"text input"`) when so.
        fn interactive_aria_role_name(&self, expr: &Expr) -> Option<&'static str> {
            let Expr::MethodCall {
                receiver, method, ..
            } = expr
            else {
                return None;
            };
            let Expr::Ident(alias, _) = &**receiver else {
                return None;
            };
            if self.core_imports.get(alias).map(|m| m.as_str()) != Some("core.ui") {
                return None;
            }
            match method.as_str() {
                "aria_role_button" => Some("button"),
                "aria_role_text_input" => Some("text input"),
                _ => None,
            }
        }
    
}
