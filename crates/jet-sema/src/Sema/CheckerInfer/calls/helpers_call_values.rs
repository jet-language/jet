use crate::AST::{AccessConvention, Expr, StrPart, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::type_fix_hint;

fn is_inline_compute_transform(checker: &Checker<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::Paren(inner, _) => is_inline_compute_transform(checker, inner),
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } if matches!(method.as_str(), "gradient" | "value_and_gradient" | "vjp" | "jvp")
            && checker
                .core_module_path_from_receiver(receiver)
                .is_some_and(|(module, _)| module == "core.compute")
            && args.iter().skip(1).all(|arg| {
                arg.label
                    .as_ref()
                    .is_some_and(|(label, _)| label == "wrt")
            }) => true,
        Expr::CallValue { callee, .. } => is_inline_compute_transform(checker, callee),
        _ => false,
    }
}

impl<'a> Checker<'a> {
        pub(super) fn synthesized_string_arg(value: String, span: Span) -> crate::AST::CallArg {
            crate::AST::CallArg {
                convention: AccessConvention::Read,
                expr: Expr::Str(vec![StrPart::Lit(value)], span),
                span,
                flags: Default::default(),
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
            args: &mut Vec<crate::AST::CallArg>,
            span: Span,
        ) -> Option<Type> {
            if is_inline_compute_transform(self, callee) {
                self.diags.push(Diagnostic::lint(
                    "L1141",
                    "an autodiff transform result is called inline".to_string(),
                    "the transform arity returns a callable; inline calls make it unclear whether a direct gradient or a derivative function was requested".to_string(),
                    "bind the derivative first, then call the binding: `d_loss :: compute.gradient(loss)`".to_string(),
                    Some(span),
                ));
            }
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
                param_contract,
                call_metadata,
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
            if let Expr::Ident(name, _) = callee.as_ref() {
                if let Some(sig) = self.funcs.get(name).cloned() {
                    self.check_foreign_transaction_call(&sig, name, span);
                }
            }
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
            // D-APILABEL1=A: every function value call goes through the one
            // binder. An absent contract is an unlabelled function type, so
            // the empty contract still rejects a written label as E0764 while
            // leaving bare arguments in their ordinary positional shape.
            let metadata = call_metadata.as_ref();
            let bind: Vec<crate::Sema::CallBinder::BindParam<'_>> = (0..params.len())
                .map(|index| {
                    let (label, zone) = param_contract
                        .as_deref()
                        .and_then(|contract| contract.get(index))
                        .map(|(label, zone)| (label.as_str(), *zone))
                        .unwrap_or(("", crate::AST::ParamZone::Either));
                    crate::Sema::CallBinder::BindParam {
                        label,
                        name: metadata
                            .and_then(|meta| meta.names.get(index))
                            .map(String::as_str)
                            .unwrap_or(label),
                        zone,
                        default: metadata
                            .and_then(|meta| meta.defaults.get(index))
                            .and_then(|default| default.as_ref()),
                        convention: metadata
                            .and_then(|meta| meta.conventions.get(index))
                            .copied()
                            .unwrap_or(AccessConvention::Read),
                        ty: params.get(index),
                        variadic: metadata
                            .and_then(|meta| meta.variadic.get(index))
                            .copied()
                            .unwrap_or(false),
                        core_default: None,
                    }
                })
                .collect();
            let callee_name = match callee.as_ref() {
                Expr::Ident(name, _) => name.clone(),
                _ => "this function value".to_string(),
            };
            let bound = crate::Sema::CallBinder::bind_call_args(
                &callee_name, &bind, args, span, &mut self.diags,
            );
            self.register_binder_refs(args);
            if bound.is_none() {
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return ret.map(|r| *r);
            }
            if metadata.is_some_and(|meta| meta.variadic.last().copied().unwrap_or(false)) {
                let fake_sig = crate::AST::FuncSig {
                    params: params
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(index, ty)| {
                            (
                                metadata
                                    .and_then(|meta| meta.conventions.get(index))
                                    .copied()
                                    .unwrap_or(AccessConvention::Read),
                                ty,
                            )
                        })
                        .collect(),
                    root_param: false,
                    return_type: ret.as_deref().cloned(),
                    return_view_provenance: crate::AST::ViewProvenanceCell::new(),
                    is_extern: false,
                    is_unsafe: false,
                    is_pure: false,
                    is_foreign_thread_safe: false,
                    is_sanitizer: false,
                    is_must_use: false,
                    is_c_abi: false,
                    c_abi_name: None,
                    foreign_effect_root: None,
                    undo: None,
                    param_info: (0..params.len())
                        .map(|index| {
                            (
                                metadata
                                    .and_then(|meta| meta.names.get(index))
                                    .cloned()
                                    .or_else(|| {
                                        param_contract
                                            .as_deref()
                                            .and_then(|contract| contract.get(index))
                                            .map(|(label, _)| label.clone())
                                    })
                                    .unwrap_or_default(),
                                metadata
                                    .and_then(|meta| meta.defaults.get(index))
                                    .is_some_and(|default| default.is_some()),
                            )
                        })
                        .collect(),
                    param_call: param_contract.clone().unwrap_or_default(),
                    defaults: metadata
                        .map(|meta| meta.defaults.clone())
                        .unwrap_or_else(|| vec![None; params.len()]),
                    param_variadic: metadata
                        .map(|meta| meta.variadic.clone())
                        .unwrap_or_else(|| vec![false; params.len()]),
                    variadic_bounds: None,
                    param_view_from_names: Vec::new(),
                };
                let mut packed = crate::AST::Call {
                    name: callee_name.clone(),
                    name_span: span,
                    type_args: Vec::new(),
                    args: std::mem::take(args),
                    resolved_ret: None,
                    range_checked: false,
                    widen_approx: false,
                };
                self.normalize_variadic_call(&mut packed, &fake_sig);
                *args = packed.args;
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
                    let param_convention = metadata
                        .and_then(|meta| meta.conventions.get(i))
                        .copied()
                        .unwrap_or(AccessConvention::Read);
                    let saved = self.expected_type.clone();
                    let saved_borrow = self.borrow_ctx;
                    self.expected_type = Some(param_ty.clone());
                    self.borrow_ctx = param_convention == AccessConvention::Read
                        && !param_ty.is_scalar();
                    let got = self.with_call_access(&mut call_access, |checker| {
                        checker.check_call_argument_access(
                            arg,
                            param_convention,
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
                            param_convention,
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
                    self.check_callable_argument_ownership(
                        &callee_name,
                        i,
                        param_convention,
                        param_ty,
                        arg,
                    );
                } else {
                    self.infer(&mut arg.expr);
                }
            }
            ret.map(|r| *r)
        }
    
}
