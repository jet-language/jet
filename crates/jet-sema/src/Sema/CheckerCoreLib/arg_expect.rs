use crate::AST::{AccessConvention, Expr, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::{type_fix_hint, typed_text_mismatch};
use crate::Syntax;

/// A plain binding or ownership-preserving place chain that a consuming Core
/// constructor would move.
/// Returns the real root binding, the Jet spelling used in the diagnostic, and
/// the full place span. Calls and method calls are deliberately excluded: their
/// results are temporaries, not places whose ownership follows one local.
fn core_consuming_place(expr: &Expr) -> Option<(String, Option<String>, Span)> {
    fn subscript(expr: &Expr) -> Option<String> {
        fn args(args: &[crate::AST::CallArg]) -> Option<String> {
            args.iter()
                .map(|arg| {
                    let value = subscript(&arg.expr)?;
                    Some(match &arg.label {
                        Some((label, _)) => format!("{}: {}", label, value),
                        None => value,
                    })
                })
                .collect::<Option<Vec<_>>>()
                .map(|parts| parts.join(", "))
        }

        match expr {
            Expr::Ident(name, _) => Some(name.clone()),
            Expr::Int(value, _, _, _) => Some(value.to_string()),
            Expr::Bool(value, _) => Some(value.to_string()),
            Expr::Call(call) => Some(format!("{}({})", call.name, args(&call.args)?)),
            Expr::MethodCall {
                receiver,
                method,
                args: call_args,
                ..
            } => Some(format!(
                "{}.{}({})",
                subscript(receiver)?,
                method,
                args(call_args)?
            )),
            Expr::Unary(op, inner, _) => Some(format!(
                "{}{}",
                match op {
                    crate::AST::UnOp::Neg => "-",
                    crate::AST::UnOp::Not => "!",
                },
                subscript(inner)?
            )),
            Expr::Binary(op, left, right, _) => Some(format!(
                "{} {} {}",
                subscript(left)?,
                op.spell(),
                subscript(right)?
            )),
            Expr::Paren(inner, _) => Some(format!("({})", subscript(inner)?)),
            Expr::Field(base, field, _) => {
                Some(format!("{}.{}", subscript(base)?, field))
            }
            Expr::Index { base, index, .. } => {
                Some(format!("{}[{}]", subscript(base)?, subscript(index)?))
            }
            _ => None,
        }
    }

    // Ownership root discovery never depends on whether the index expression
    // can be rendered for product copy. A computed index still selects a place
    // inside the borrowed base.
    fn ownership_root(expr: &Expr) -> Option<(String, Span)> {
        match expr {
            Expr::Ident(name, span) => Some((name.clone(), *span)),
            Expr::Field(base, _, _) | Expr::Index { base, .. } => ownership_root(base),
            // S40 slices are inclusive copies lowered by TIR to owned values.
            Expr::Slice { .. } => None,
            _ => None,
        }
    }

    fn spelling(expr: &Expr) -> Option<String> {
        match expr {
            Expr::Ident(name, _) => Some(name.clone()),
            Expr::Field(base, field, _) => {
                Some(format!("{}.{}", spelling(base)?, field))
            }
            Expr::Index { base, index, .. } => {
                Some(format!("{}[{}]", spelling(base)?, subscript(index)?))
            }
            _ => None,
        }
    }

    let end = expr.span().end;
    let (root, root_span) = ownership_root(expr)?;
    // Root discovery and product spelling are separate facts: the index
    // expression never controls which binding owns the selected place.
    let place = spelling(expr);
    Some((root, place, Span::new(root_span.start, end)))
}

impl<'a> Checker<'a> {
        pub(crate) fn expect_core_arg(
            &mut self,
            call_name: &str,
            idx: usize,
            param_ty: &Type,
            arg: &mut crate::AST::CallArg,
        ) {
            self.expect_core_arg_impl(call_name, idx, param_ty, arg, false, false);
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
        /// (`JSON.Text`/`DBValue.Text` own their `String`, etc. — see
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
            self.expect_core_arg_impl(call_name, idx, param_ty, arg, true, false);
        }

        pub(crate) fn expect_core_arg_moving(
            &mut self,
            call_name: &str,
            idx: usize,
            param_ty: &Type,
            arg: &mut crate::AST::CallArg,
        ) {
            self.expect_core_arg_impl(call_name, idx, param_ty, arg, true, true);
        }
    
        fn expect_core_arg_impl(
            &mut self,
            call_name: &str,
            idx: usize,
            param_ty: &Type,
            arg: &mut crate::AST::CallArg,
            consumes: bool,
            accepts_move: bool,
        ) {
            if matches!(arg.convention, AccessConvention::Move)
                && !accepts_move
                && !matches!(param_ty, Type::Named(n) if n == "Unit")
            {
                self.diags.push(Diagnostic::error(
                    "E0203",
                    "a value was passed with the move-capability marker `^` to a parameter that does not consume".to_string(),
                    "standard library functions in M10 read their ordinary arguments unless documented otherwise"
                        .to_string(),
                    "remove the move-capability marker `^` here".to_string(),
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
            if let Some(mut got) = got {
                self.check_interrupt_callback_expr(&arg.expr, &got);
                if got != *param_ty && got.numeric_widening_to(param_ty).is_some() {
                    self.widen_numeric_expr(&mut arg.expr, &got, param_ty);
                    got = param_ty.clone();
                }
                let reads_expiring_secret_loan = !consumes
                    && arg.convention == AccessConvention::Read
                    && crate::Sema::Diagnostics::expiring_secret_loan_matches(param_ty, &got);
                let reported = reads_expiring_secret_loan
                    || self.check_type_assignable(param_ty, &got, arg.expr.span());
                let fixed_widens = matches!(
                    (param_ty, &got),
                    (Type::List(want), Type::FixedList { elem: actual, .. }) if want == actual
                );
                let union_widens = matches!(
                    param_ty,
                    Type::Union(members) if members.iter().any(|m| m == &got)
                );
                if !reported
                    && got != *param_ty
                    && !fixed_widens
                    && !union_widens
                    && !reads_expiring_secret_loan
                {
                    if let Some(diag) = typed_text_mismatch(param_ty, &got, arg.expr.span()) {
                        self.diags.push(diag);
                    } else {
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
            }
            // A std constructor that stores a non-scalar payload (e.g. `JSON.Text`
            // owns its `String`) consumes the argument. When the value is read from
            // a borrowed binding (a `view` parameter), moving it out would not
            // compile — insert a clone, exactly as a consuming `fn` call does (B1).
            if consumes
                && matches!(arg.convention, AccessConvention::Read)
                && matches!(param_ty, Type::String | Type::List(_) | Type::Map { .. })
            {
                if let Some((root, place, place_span)) = core_consuming_place(&arg.expr) {
                    if self.is_borrowed_binding(&root) {
                        arg.flags.implicit_clone = true;
                        // D-MEM1/S2 (was D-L0201 lint): a hard error now, regardless
                        // of liveness — no clone is ever silent. Unlike a Move-param
                        // user function, this is a fixed std read-only signature —
                        // `^` is never accepted here (E0203), so `~name`
                        // (D-SHAPE-COPY1, supersedes D-CAP2/S4) is the only fix,
                        // not a liveness-dependent move/reorder menu.
                        let (what, fix) = match place {
                            Some(place) => (
                                format!("implicit clone of `{}`", place),
                                format!(
                                    "write `{}{}` to copy explicitly",
                                    Syntax::SIGIL_COPY,
                                    place
                                ),
                            ),
                            None => (
                                "implicit clone of this consumed expression".to_string(),
                                format!(
                                    "write `{}` before the highlighted expression to copy it explicitly",
                                    Syntax::SIGIL_COPY
                                ),
                            ),
                        };
                        self.diags.push(Diagnostic::error(
                            "E0209",
                            what,
                            format!("`{}` stores its own copy of this value", call_name),
                            fix,
                            Some(place_span),
                        ));
                    }
                }
            }
        }
    
}
