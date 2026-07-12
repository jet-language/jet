use crate::AST::{AccessConvention, Expr, StrPart, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::type_fix_hint;
impl<'a> Checker<'a> {
        pub(super) fn synthesized_string_arg(value: String, span: Span) -> crate::AST::CallArg {
            crate::AST::CallArg {
                convention: AccessConvention::Read,
                expr: Expr::Str(vec![StrPart::Lit(value)], span),
                span,
                flags: crate::AST::CallArgFlags {
                    implicit_clone: false,
                    shared_auto_clone: false,
                    is_trailing_block: false,
                    c_callback_symbol: false,
                },
                label: None,
                spread: false,
            }
        }
    
        pub(super) fn type_arg_name(t: &Type) -> String {
            match t {
                Type::Named(n) => n.clone(),
                Type::Apply { name, args } if args.is_empty() => name.clone(),
                _ => t.show(),
            }
        }
    
        pub(super) fn core_module_path_from_receiver(&self, receiver: &Expr) -> Option<(String, Span)> {
            match receiver {
                Expr::Ident(alias, span) => self.core_imports.get(alias).cloned().map(|m| (m, *span)),
                Expr::Field(base, leaf, _) => {
                    let (module, span) = self.core_module_path_from_receiver(base)?;
                    let submodule = format!("{module}.{leaf}");
                    crate::Syntax::is_known_core_module(&submodule).then_some((submodule, span))
                }
                _ => None,
            }
        }
    
        pub(crate) fn infer_call_value(
            &mut self,
            callee: &mut Box<Expr>,
            args: &mut [crate::AST::CallArg],
            span: Span,
        ) -> Option<Type> {
            let callee_ty = self.infer(callee)?;
            let Type::Fn { params, ret, .. } = callee_ty.clone() else {
                self.diags.push(Diagnostic::error(
                    "E0803",
                    format!("this is {}, not a function", callee_ty.show()),
                    "only a function value can be called with `(…)`".to_string(),
                    "call a defined `fn` by name, or store a lambda in a binding first".to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            };
            if args.len() != params.len() {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!(
                        "this function wants {} argument{}, got {}",
                        params.len(),
                        if params.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                    "every argument must match a parameter".to_string(),
                    "check how many values this function expects".to_string(),
                    Some(span),
                ));
            }
            for (i, arg) in args.iter_mut().enumerate() {
                if let Some(param_ty) = params.get(i) {
                    let saved = self.expected_type.clone();
                    self.expected_type = Some(param_ty.clone());
                    let got = self.infer(&mut arg.expr);
                    self.expected_type = saved;
                    if let Some(got) = got {
                        if got != *param_ty {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "argument {} should be {}, not {}",
                                    i + 1,
                                    param_ty.show(),
                                    got.show()
                                ),
                                "every argument must match its parameter's type".to_string(),
                                type_fix_hint(param_ty, &got),
                                Some(arg.expr.span()),
                            ));
                        }
                    }
                } else {
                    self.infer(&mut arg.expr);
                }
            }
            ret.map(|r| *r)
        }
    
}
