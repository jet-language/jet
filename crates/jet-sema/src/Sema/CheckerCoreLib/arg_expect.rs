impl<'a> Checker<'a> {
        pub(crate) fn expect_core_arg(
            &mut self,
            call_name: &str,
            idx: usize,
            param_ty: &Type,
            arg: &mut crate::AST::CallArg,
        ) {
            self.expect_core_arg_impl(call_name, idx, param_ty, arg, false);
        }
    
        pub(crate) fn expect_url_arg(
            &mut self,
            call_name: &str,
            idx: usize,
            arg: &mut crate::AST::CallArg,
        ) {
            self.borrow_ctx = true;
            let got = self.infer(&mut arg.expr);
            if let Some(got) = got {
                if matches!(got, Type::String) || matches!(got, Type::Named(ref n) if n == "Url") {
                    return;
                }
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "`{}` wants String or Url for argument {}, but this is {}",
                        call_name,
                        idx + 1,
                        got.show()
                    ),
                    "HTTP client calls accept raw strings or typed Url values".to_string(),
                    "pass a String, or build a Url with core.url.parse".to_string(),
                    Some(arg.expr.span()),
                ));
            }
        }
    
        /// Same elaboration as `expect_core_arg`, but for the handful of std
        /// constructors that genuinely store the argument as their own payload
        /// (`Json.Text`/`DbValue.Text` own their `String`, etc. — see
        /// `check_core_json_lit` / `check_core_dbvalue_lit`). Only these call
        /// sites may trip the implicit-clone E0209 below; an ordinary read-only
        /// stdlib call (e.g. `fs.read(path)`) must not, even though its param
        /// type is also String/List/Map (D-MEM1/S2 false-positive fix).
        pub(crate) fn expect_core_arg_consuming(
            &mut self,
            call_name: &str,
            idx: usize,
            param_ty: &Type,
            arg: &mut crate::AST::CallArg,
        ) {
            self.expect_core_arg_impl(call_name, idx, param_ty, arg, true);
        }
    
        fn expect_core_arg_impl(
            &mut self,
            call_name: &str,
            idx: usize,
            param_ty: &Type,
            arg: &mut crate::AST::CallArg,
            consumes: bool,
        ) {
            if matches!(arg.convention, AccessConvention::Move)
                && !matches!(param_ty, Type::Named(n) if n == "Unit")
            {
                self.diags.push(Diagnostic::error(
                    "E0203",
                    format!("`{}` passed to a parameter that does not consume", Syntax::SIGIL_MOVE),
                    "standard library functions in M10 read their ordinary arguments unless documented otherwise"
                        .to_string(),
                    format!("remove `{}` here", Syntax::SIGIL_MOVE),
                    Some(arg.span),
                ));
            }
            if matches!(param_ty, Type::String | Type::List(_) | Type::Map { .. }) {
                self.borrow_ctx = true;
            }
            // D-SG9: expose the parameter's type to `infer` so a fixed-width integer
            // literal argument (`f(5)` where `f` takes a `U8`) adopts that width and
            // is range-checked (E1003) at the literal. Restored after the argument.
            let saved_expected = self.expected_type.clone();
            self.expected_type = Some(param_ty.clone());
            let got = self.infer(&mut arg.expr);
            self.expected_type = saved_expected;
            if let Some(got) = got {
                let reported = self.check_type_assignable(param_ty, &got, arg.expr.span());
                if !reported && got != *param_ty {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` wants {} for argument {}, but this is {}",
                            call_name,
                            param_ty.show(),
                            idx + 1,
                            got.show()
                        ),
                        "every argument must match its parameter's type".to_string(),
                        type_fix_hint(param_ty, &got),
                        Some(arg.expr.span()),
                    ));
                }
            }
            // A std constructor that stores a non-scalar payload (e.g. `JSON.Text`
            // owns its `String`) consumes the argument. When the value is read from
            // a borrowed binding (a `view` parameter), moving it out would not
            // compile — insert a clone, exactly as a consuming `fn` call does (B1).
            if consumes
                && matches!(arg.convention, AccessConvention::Read)
                && matches!(param_ty, Type::String | Type::List(_) | Type::Map { .. })
            {
                if let Expr::Ident(name, ispan) = &arg.expr {
                    let name = name.clone();
                    let ispan = *ispan;
                    if self.is_borrowed_binding(&name) {
                        arg.flags.implicit_clone = true;
                        // D-MEM1/S2 (was D-L0201 lint): a hard error now, regardless
                        // of liveness — no clone is ever silent. Unlike a Move-param
                        // user function, this is a fixed std read-only signature —
                        // `^` is never accepted here (E0203), so `copy name` (D-CAP2,
                        // D-MEM1/S4) is the only fix, not a liveness-dependent
                        // move/reorder menu.
                        self.diags.push(Diagnostic::error(
                            "E0209",
                            format!("implicit clone of `{}`", name),
                            format!("`{}` stores its own copy of this value", call_name),
                            format!("write `{} {}` to copy explicitly", Syntax::KW_COPY, name),
                            Some(ispan),
                        ));
                    }
                }
            }
        }
    
}
