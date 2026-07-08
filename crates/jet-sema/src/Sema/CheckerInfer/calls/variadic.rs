impl<'a> Checker<'a> {
        /// D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): check each trait-bounded variadic
        /// call-site argument against `bounds` (E1313) and infer its type. `tail`
        /// is the already-split-off trailing slice of `call.args`; the caller
        /// re-attaches it once the rest of `check_call`'s per-index checking (which
        /// only understands one shared element type) has run past it.
        fn check_variadic_bound_tail(
            &mut self,
            call: &Call,
            sig: &crate::AST::FuncSig,
            tail: &mut [crate::AST::CallArg],
            bounds: &[String],
        ) {
            let param_name = sig
                .param_info
                .last()
                .map(|(n, _)| n.clone())
                .unwrap_or_default();
            for arg in tail.iter_mut() {
                if arg.spread {
                    self.diags.push(Diagnostic::error(
                        "E1312",
                        format!("`{}` doesn't accept a spread argument", call.name),
                        "a trait-bounded variadic (`...Trait` / `...[A, B]`) takes each argument as \
                         its own type — spread would need one shared list type"
                            .to_string(),
                        "pass arguments individually".to_string(),
                        Some(call.name_span),
                    ));
                    self.infer(&mut arg.expr);
                    continue;
                }
                let Some(ty) = self.infer(&mut arg.expr) else {
                    continue;
                };
                // D-LIN1: a rest parameter collects by value (S61 defaults/D-VARIADIC1
                // give it no other convention today), matching the ordinary
                // homogeneous variadic's move semantics. Mark moved *after*
                // inferring (inference itself checks "already moved") — same order
                // the ordinary per-index arg loop below uses.
                if !ty.is_scalar() {
                    if let Expr::Ident(name, span) = &arg.expr {
                        self.mark_moved(name.clone(), *span);
                    }
                }
                let arg_name = ty.name();
                for b in bounds {
                    if !self.trait_reg.implements_trait(&arg_name, b) {
                        self.diags.push(e1313(
                            &arg_name,
                            b,
                            &param_name,
                            &call.name,
                            arg.expr.span(),
                        ));
                    }
                }
            }
        }
    
        /// D-VARIADIC1: pack trailing call arguments (and spreads) into the final list
        /// parameter so codegen sees a normal fixed-arity call.
        fn normalize_variadic_call(&mut self, call: &mut Call, sig: &crate::AST::FuncSig) {
            let fixed = sig.params.len().saturating_sub(1);
            let Some((variadic_conv, variadic_ty)) = sig.params.last().cloned() else {
                return;
            };
            let elem_ty = match &variadic_ty {
                Type::List(inner) => (**inner).clone(),
                _ => return,
            };
    
            if call.args.len() < fixed {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!(
                        "`{}` expects at least {} argument{}, got {}",
                        call.name,
                        fixed,
                        if fixed == 1 { "" } else { "s" },
                        call.args.len()
                    ),
                    "every fixed parameter must receive a value before the variadic tail".to_string(),
                    format!("check the definition of `{}`", call.name),
                    Some(call.name_span),
                ));
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return;
            }
    
            let tail = call.args.split_off(fixed);
            let mut packed_elems: Vec<Expr> = Vec::new();
            let mut pack_span = call.name_span;
            for mut arg in tail {
                if arg.spread {
                    let spread_span = arg.expr.span();
                    let got = self.infer(&mut arg.expr);
                    pack_span = Span::new(pack_span.start, spread_span.end);
                    match got {
                        Some(Type::List(inner)) => {
                            if *inner != elem_ty {
                                self.diags.push(Diagnostic::error(
                                    "E1311",
                                    format!(
                                        "spread list is `[{}]`, but `{}` expects `[{}]`",
                                        inner.name(),
                                        call.name,
                                        elem_ty.name()
                                    ),
                                    "call spread expands a list into the callee's variadic tail"
                                        .to_string(),
                                    format!(
                                        "pass a `[{}]` list, or change the element types",
                                        elem_ty.name()
                                    ),
                                    Some(arg.expr.span()),
                                ));
                            }
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E1311",
                                format!("spread needs a list, not `{}`", other.name()),
                                "call spread `f(...xs)` expands a list into the remaining parameter slots".to_string(),
                                format!("pass a `[{}]` value here", elem_ty.name()),
                                Some(arg.expr.span()),
                            ));
                        }
                        None => {}
                    }
                    packed_elems.push(Expr::Spread(Box::new(arg.expr), spread_span));
                } else {
                    let saved = self.expected_type.replace(elem_ty.clone());
                    let got = self.infer(&mut arg.expr);
                    self.expected_type = saved;
                    if let Some(got) = got {
                        if got != elem_ty {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "variadic argument should be {}, not {}",
                                    elem_ty.show(),
                                    got.show()
                                ),
                                "each trailing argument is collected into the rest parameter's list"
                                    .to_string(),
                                type_fix_hint(&elem_ty, &got),
                                Some(arg.expr.span()),
                            ));
                        }
                    }
                    pack_span = Span::new(pack_span.start, arg.expr.span().end);
                    packed_elems.push(arg.expr);
                }
            }
    
            let packed_expr = if packed_elems.len() == 1 {
                match packed_elems.pop().unwrap() {
                    Expr::Spread(inner, _span) => *inner,
                    other => other,
                }
            } else if packed_elems.is_empty() {
                Expr::ListLit(Vec::new(), pack_span)
            } else {
                Expr::ListLit(packed_elems, pack_span)
            };
    
            call.args.push(crate::AST::CallArg {
                convention: variadic_conv,
                expr: packed_expr,
                span: pack_span,
                flags: Default::default(),
                label: None,
                spread: false,
            });
        }
}
