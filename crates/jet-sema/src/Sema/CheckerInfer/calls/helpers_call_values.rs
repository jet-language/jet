use crate::AST::{AccessConvention, Expr, StrPart, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::type_fix_hint;
use crate::Syntax;
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
            let inline_loop = matches!(
                callee.as_ref(),
                Expr::Lambda(lam) if lam.meta.collecting_loop || lam.meta.result_loop
            );
            let mut call_access = self.call_access_frame();
            let Some(callee_ty) = self.with_call_access(&mut call_access, |checker| {
                if !inline_loop {
                    checker.check_call_receiver_evaluation(callee, callee.span());
                }
                let saved_escapes = checker.lambda_escapes;
                if inline_loop {
                    checker.lambda_escapes = false;
                }
                let inferred = checker.infer(callee);
                checker.lambda_escapes = saved_escapes;
                if !inline_loop {
                    checker.check_call_argument_captures(callee);
                }
                inferred
            }) else {
                return None;
            };
            let Type::Fn {
                params,
                ret,
                effect_bound,
                ..
            } = callee_ty.clone()
            else {
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
            match effect_bound {
                // A yielding loop uses a compiler-private immediately evaluated
                // carrier. Its body already records effects in the enclosing
                // context; it is not an unknown function-value call.
                _ if inline_loop => {}
                None
                    if !matches!(
                        callee.as_ref(),
                        Expr::Ident(name, _)
                            if self.lookup(name).is_some_and(|info| info.param_conv.is_some())
                    ) =>
                {
                    self.record_maximal(span);
                }
                None => {}
                Some(row) => {
                    for (effect, _) in row {
                        if crate::Sema::effect_row_var(&effect).is_none()
                            && crate::Sema::parse_effect_name(&effect).is_some()
                        {
                            self.record_effect(&effect, span);
                        }
                    }
                }
            }
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
                    let saved_borrow = self.borrow_ctx;
                    self.expected_type = Some(param_ty.clone());
                    self.borrow_ctx = !param_ty.is_scalar();
                    let got = self.with_call_access(&mut call_access, |checker| {
                        checker.check_call_argument_access(
                            arg,
                            AccessConvention::Read,
                            param_ty,
                            true,
                        );
                        let inferred = checker.infer(&mut arg.expr);
                        checker.check_call_argument_captures(&arg.expr);
                        inferred
                    });
                    self.expected_type = saved;
                    self.borrow_ctx = saved_borrow;
                    if let Some(got) = got {
                        let got = self.widen_numeric_argument(
                            &mut arg.expr,
                            got,
                            param_ty,
                            AccessConvention::Read,
                        );
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
                    if arg.convention == AccessConvention::Move {
                        self.diags.push(Diagnostic::error(
                            "E0203",
                            format!(
                                "`{}` passed to a parameter that does not consume",
                                Syntax::SIGIL_MOVE
                            ),
                            "function-value parameters have plain read access; they do not take ownership"
                                .to_string(),
                            format!("remove `{}`", Syntax::SIGIL_MOVE),
                            Some(arg.span),
                        ));
                    }
                    if arg.convention == AccessConvention::Write
                        && !matches!(arg.expr, Expr::Ident(_, _))
                    {
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            format!("`{}` needs a plain named binding after it", Syntax::SIGIL_WRITE),
                            "write access (`&`) can only be granted to a named binding, not an expression"
                                .to_string(),
                            self.non_name_write_argument_fix(&arg.expr),
                            Some(arg.span),
                        ));
                    }
                    if let Expr::Ident(name, _) = &arg.expr {
                        if arg.convention == AccessConvention::Write {
                            if let Some(info) = self.lookup(name) {
                                if !info.mutable {
                                    self.diags.push(Diagnostic::error(
                                        "E0111",
                                        format!("`{name}` cannot be changed"),
                                        "write access requires a mutable binding".to_string(),
                                        format!("declare `{name}` with `:=`"),
                                        Some(arg.span),
                                    ));
                                }
                            }
                        }
                    }
                } else {
                    self.infer(&mut arg.expr);
                }
            }
            ret.map(|r| *r)
        }
    
}
