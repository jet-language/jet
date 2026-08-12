use crate::AST::{AccessConvention, Expr, Type};
use crate::Collections;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::{expr_root_ident, type_fix_hint};
use crate::Syntax;
impl<'a> Checker<'a> {
        /// D-DET-CAPAPI: the generic `Rng` draws — `rng.pick(list) -> T?` (uniform
        /// choice; null on empty) and `rng.shuffle(&list)` (in-place Fisher–Yates).
        /// Both advance the stream, so the `rng` receiver must have edit access (`&`);
        /// `shuffle` edits its list in place, so the list arg must be `&` too. Mirrors
        /// the ambient `random.pick`/`random.shuffle` (CheckerCoreLib).
        /// D-HOLE1: `Option.lift2(f, a, b)` — lifts a two-argument function into
        /// `Option`: `f(a, b)` when both are present, `null` if either is absent. `f`'s
        /// expected param types come from `a`/`b`'s payload types, so `a`/`b` are
        /// elaborated FIRST (out of source order — the closure `f` is written first but
        /// typed last), mirroring how `.zip` resolves its pair before a chained
        /// `.map(f)` would type its closure.
        pub(super) fn check_option_lift2(
            &mut self,
            args: &mut Vec<crate::AST::CallArg>,
            span: Span,
            resolved_ret_out: &mut Option<Type>,
        ) -> Option<Type> {
            if args.len() != 3 {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!(
                        "`Option.lift2` takes 3 arguments (f, a, b), got {}",
                        args.len()
                    ),
                    "`Option.lift2(f, a, b)` applies a two-argument function only when both optionals are present"
                        .to_string(),
                    "call `Option.lift2((x, y) => ..., a, b)`".to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
            let a_ty = self.infer(&mut args[1].expr);
            let b_ty = self.infer(&mut args[2].expr);
            let (Some(Type::Option(a_inner)), Some(Type::Option(b_inner))) =
                (a_ty.clone(), b_ty.clone())
            else {
                if !matches!(a_ty, Some(Type::Option(_))) {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "`Option.lift2`'s second argument must be optional, got {}",
                            a_ty.map(|t| t.show())
                                .unwrap_or_else(|| "an unknown type".to_string())
                        ),
                        "`Option.lift2(f, a, b)` needs `a: T?`".to_string(),
                        "pass a `T?` value".to_string(),
                        Some(args[1].expr.span()),
                    ));
                }
                if !matches!(b_ty, Some(Type::Option(_))) {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "`Option.lift2`'s third argument must be optional, got {}",
                            b_ty.map(|t| t.show())
                                .unwrap_or_else(|| "an unknown type".to_string())
                        ),
                        "`Option.lift2(f, a, b)` needs `b: U?`".to_string(),
                        "pass a `T?` value".to_string(),
                        Some(args[2].expr.span()),
                    ));
                }
                self.infer(&mut args[0].expr);
                return None;
            };
            let expected_fn = Type::Fn {
                params: vec![(*a_inner).clone(), (*b_inner).clone()],
                ret: None, // sema refines R from the closure's actual return
                effect_bound: None, return_view_provenance: None,
                param_contract: None,
                call_metadata: None,
            };
            let saved_esc = self.lambda_escapes;
            self.lambda_escapes = false;
            let saved_exp = self.expected_type.clone();
            self.expected_type = Some(expected_fn);
            let f_ty = self.infer(&mut args[0].expr);
            self.expected_type = saved_exp;
            self.lambda_escapes = saved_esc;
            match f_ty {
                Some(Type::Fn { params, ret: Some(r), .. })
                    if params.len() == 2
                        && Type::obligations_satisfy(
                            &Type::Fn {
                                params: vec![(*a_inner).clone(), (*b_inner).clone()],
                                ret: None,
                                effect_bound: None,
                                param_contract: None,
                                call_metadata: None,
                                return_view_provenance: None,
                            },
                            &Type::Fn {
                                params: params.clone(),
                                ret: None,
                                effect_bound: None,
                                param_contract: None,
                                call_metadata: None,
                                return_view_provenance: None,
                            },
                        ) => {
                    let ret = Type::Option(r);
                    *resolved_ret_out = Some(ret.clone());
                    Some(ret)
                }
                Some(other) => {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "`Option.lift2`'s first argument must be a function, got {}",
                            other.show()
                        ),
                        "`Option.lift2(f, a, b)` needs `f: fn(T, U) => R`".to_string(),
                        "pass a two-argument function or lambda".to_string(),
                        Some(args[0].expr.span()),
                    ));
                    None
                }
                None => None,
            }
        }
    
        /// D-HOLE1: `a: T?`.zip(`b: U?`) -> `(a: T, b: U)?` — present only when both
        /// operands are present, `null` if either is. See the call site's comment for why
        /// this bypasses the flat builtin-method-arg table (heterogeneous `U`).
        pub(super) fn finish_option_zip(
            &mut self,
            a_inner: Type,
            args: &mut [crate::AST::CallArg],
            span: Span,
            resolved_ret_out: &mut Option<Type>,
        ) -> Option<Type> {
            if args.len() != 1 {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!("`.zip()` takes one optional argument, got {}", args.len()),
                    "`.zip(other)` pairs this optional with another: present only when both are"
                        .to_string(),
                    "call `a.zip(b)` with exactly one `T?` argument".to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
            let got = self.infer(&mut args[0].expr);
            let Some(Type::Option(b_inner)) = got else {
                let shown = got.map(|t| t.show()).unwrap_or_else(|| "that".to_string());
                self.diags.push(Diagnostic::error(
                    "E0108",
                    format!("`.zip()` needs an optional argument, got {}", shown),
                    "`.zip(other)` combines two optionals into one paired optional".to_string(),
                    "pass a `T?` value".to_string(),
                    Some(args[0].expr.span()),
                ));
                return None;
            };
            let elem_ty = Collections::zip_elem_ty(&a_inner, &b_inner);
            let ret = Type::Option(Box::new(elem_ty));
            *resolved_ret_out = Some(ret.clone());
            Some(ret)
        }
    
        pub(super) fn finish_rng_generic(
            &mut self,
            receiver: &Expr,
            method: &str,
            args: &mut [crate::AST::CallArg],
            span: Span,
        ) -> Option<Type> {
            // The receiver must be a `&Rng` (every draw advances the stream).
            if let Some(root) = expr_root_ident(receiver) {
                let root = root.to_string();
                if let Some(info) = self.lookup(&root) {
                    if !info.mutable {
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            format!(
                                "cannot draw from `{}` — it does not have the write-capability marker `&`; required before calling `.{}()`",
                                root, method
                            ),
                            "every `Rng` draw advances the stream, so the receiver needs the write-capability marker `&`".to_string(),
                            format!("declare `{} {} ...`, or pass the rng with the write-capability marker `&`: `&{}`", root, Syntax::SIGIL_BIND_MUT, root),
                            Some(receiver.span()),
                        ));
                    }
                }
            }
            let expected = if method == "weighted_pick" || method == "sample" {
                2
            } else {
                1
            };
            if args.len() != expected {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!("`.{}()` takes {} argument(s), got {}", method, expected, args.len()),
                    "this `Rng` draw operates on a list and optional draw parameter".to_string(),
                    if method == "weighted_pick" {
                        "call `rng.weighted_pick(items, weights)`".to_string()
                    } else if method == "sample" {
                        "call `rng.sample(items, k)`".to_string()
                    } else {
                        format!("call `rng.{}(items)`", method)
                    },
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return if method == "pick" || method == "weighted_pick" {
                    Some(Type::Option(Box::new(Type::Int)))
                } else if method == "sample" {
                    Some(Type::List(Box::new(Type::Int)))
                } else {
                    None
                };
            }
            // `shuffle` edits the list in place — the list arg needs `&`.
            if method == "shuffle" && args[0].convention != AccessConvention::Write {
                self.diags.push(Diagnostic::error(
                    "E0202",
                    "`shuffle` edits its list in place".to_string(),
                    "the write-capability marker `&` is required; pass the list with that marker".to_string(),
                    "write `rng.shuffle(&items)` with the write-capability marker `&`".to_string(),
                    Some(args[0].span),
                ));
            }
            let ty = self.infer(&mut args[0].expr)?;
            if method == "sample" {
                if let Some(k) = self.infer(&mut args[1].expr) {
                    if k != Type::Int {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`sample` count must be Int, not {}", k.show()),
                            "rng.sample chooses up to k items without replacement".to_string(),
                            "pass an Int count".to_string(),
                            Some(args[1].expr.span()),
                        ));
                    }
                }
            }
            if method == "weighted_pick" {
                if let Some(weights_ty) = self.infer(&mut args[1].expr) {
                    if weights_ty != Type::List(Box::new(Type::Float)) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`weighted_pick` weights must be [Float], not {}", weights_ty.show()),
                            "rng.weighted_pick pairs each item with a non-negative Float weight".to_string(),
                            "pass a `[Float]` weights list".to_string(),
                            Some(args[1].expr.span()),
                        ));
                    }
                }
            }
            let Type::List(inner) = ty else {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`{}` needs a list, not {}", method, ty.show()),
                    format!("rng.{} operates on a `[T]`", method),
                    "pass a `[T]` value".to_string(),
                    Some(args[0].expr.span()),
                ));
                return if method == "pick" || method == "weighted_pick" {
                    Some(Type::Option(Box::new(Type::Int)))
                } else if method == "sample" {
                    Some(Type::List(Box::new(Type::Int)))
                } else {
                    None
                };
            };
            // `pick`/`weighted_pick` return `T?`; `sample` returns `[T]`; `shuffle` returns nothing.
            if method == "pick" || method == "weighted_pick" {
                Some(Type::Option(inner))
            } else if method == "sample" {
                Some(Type::List(inner))
            } else {
                None
            }
        }
    
        /// D-SHIFT1 (c7shift): infer + check one `Reader`/`Cursor` core-method
        /// argument against its fixed parameter type, with the same E0112
        /// fallback the general call path uses — `check_type_assignable` only
        /// reports its special shapes, so a plain mismatch (String where `[U8]`
        /// is wanted) must not pass silently.
        ///
        /// D-BINREAD-LEN1=A: `Reader.take` alone accepts U8/U16/U32 as a
        /// length. These widths always widen to Int; U64 and signed sized
        /// integers keep S42's explicit conversion rule.
        pub(super) fn check_shift_arg(&mut self, label: &str, want: &Type, arg: &mut crate::AST::CallArg) {
            let saved = self.expected_type.replace(want.clone());
            let got = self.infer(&mut arg.expr);
            self.expected_type = saved;
            let Some(got) = got else { return };
            let want = self.resolve_type(want.clone());
            let got = self.resolve_type(got);
            let got = self.widen_numeric_argument(
                &mut arg.expr,
                got,
                &want,
                crate::AST::AccessConvention::Read,
            );
            let reported = self.check_type_assignable(&want, &got, arg.expr.span());
            // D-FIXARR1: [T#N] widens to [T] at a call site.
            let fixed_widens = matches!((&want, &got),
                (Type::List(pe), Type::FixedList { elem: ae, .. }) if pe == ae);
            let union_widens = matches!(
                &want,
                Type::Union(members) if members.iter().any(|m| m == &got)
            );
            if !reported
                && got != want
                && !fixed_widens
                && !union_widens
            {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "`{}` wants {}, but this is {}",
                        label,
                        want.show(),
                        got.show()
                    ),
                    "every argument must match its parameter's type".to_string(),
                    type_fix_hint(&want, &got),
                    Some(arg.expr.span()),
                ));
            }
        }
    
}
