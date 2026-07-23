use crate::AST::{AccessConvention, BinOp, EnumLitArg, Expr, IndexKind, OrFallback, StrPart, TryConvert, Type};
use crate::Codegen::Cx;
use crate::Codegen::emit_named_fn_value;
use crate::Codegen::escape_rust_str;
use crate::Codegen::is_db_value_type_name;
use crate::Codegen::is_json_type_name;
use crate::Codegen::mangle;
use crate::Codegen::net_handle_rust_type;
use crate::Codegen::TIR::ast_operand_is_integer;
use crate::Codegen::TIR::call_return_type;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::core_struct_field_rust_name;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::int_lit_type;
use crate::Codegen::TIR::is_numeric_bounds_const;
use crate::Codegen::TIR::ListSpreadPart;
use crate::Codegen::TIR::lower_enum_arg;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_extern_call_arg;
use crate::Codegen::TIR::lower::is_binding_free_user_variant_pattern_test;
use crate::Codegen::TIR::lower_lambda;
use crate::Codegen::TIR::lower::lower_binding_free_variant_pattern_test;
use crate::Codegen::TIR::lower::lower_incdec_place;
use crate::Codegen::TIR::lower_method_call;
use crate::Codegen::TIR::lower_one_call_arg;
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::render_panic_stop;
use crate::Codegen::TIR::render_require;
use crate::Codegen::TIR::render_require_eq;
use crate::Codegen::TIR::struct_field_type;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TBuiltinOp;
use crate::Codegen::TIR::TEnumPayload;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TFnValueKind;
use crate::Codegen::TIR::tir_enum_lit_prefix;
use crate::Codegen::TIR::TModuleCallForm;
use crate::Codegen::TIR::TOrFallback;
use crate::Codegen::TIR::TStrPart;
use crate::Codegen::TIR::TTryConvert;
use crate::Codegen::TIR::unit_type;
use crate::Codegen::tuple_fields_plain;
use crate::Codegen::tuple_struct_name;
use crate::Codegen::user_type_rust;
use crate::Diagnostics::Span;
use crate::Syntax;

/// D-MEM1 S6: lower `e` for use as a MUTATING method's receiver (`.push()`,
/// `.insert()`, …). Ordinarily identical to `lower_expr`; the one exception is
/// a place rooted in a `Pool` index (`pool[id]`, or `pool[id].field`) — the
/// plain read there is a generation-checked VALUE CLONE (`jet_pool_get`,
/// matching `world[id].attack`'s read semantics), so mutating it in place
/// would silently edit a throwaway copy. Reroute through `jet_pool_get_mut`
/// instead, mirroring the `LValue::Field`/`LValue::Index` place-building this
/// same stage added for `tree[root].children.push(child)` on writes.
pub(crate) fn lower_expr_as_mut_place(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    fn pool_mut_place(
        pool_expr: &Expr,
        id_expr: &Expr,
        idx_span: Span,
        field: Option<&str>,
        cx: &Cx,
        env: &mut LowerEnv,
    ) -> TExpr {
        let line = crate::Diagnostics::span_line_col(&cx.src, idx_span.start).0;
        let pool_t = lower_expr(pool_expr, cx, env);
        let id_t = lower_expr(id_expr, cx, env);
        let elem_ty = match &pool_t.ty {
            Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
            _ => Type::Int,
        };
        let p = emit_tir_expr(&pool_t, cx);
        let i = emit_tir_expr(&id_t, cx);
        let base_place = format!(
            "(*{root}jet_std::jet_pool_get_mut(&mut ({p}), {i}, {file:?}, {line}))",
            root = cx.root_prefix,
            file = cx.file,
        );
        match field {
            None => TExpr {
                ty: elem_ty,
                kind: TExprKind::ConstInline(base_place),
            },
            Some(f) => {
                let field_ty = struct_field_type(cx, &elem_ty, f).unwrap_or(Type::Int);
                TExpr {
                    ty: field_ty,
                    kind: TExprKind::ConstInline(format!("{}.{}", base_place, mangle(f))),
                }
            }
        }
    }
    match e {
        Expr::Index {
            base,
            index,
            span,
            kind: IndexKind::Pool,
        } => pool_mut_place(base, index, *span, None, cx, env),
        Expr::Field(base, field, _) => {
            if let Expr::Index {
                base: pool_expr,
                index: id_expr,
                span: idx_span,
                kind: IndexKind::Pool,
            } = base.as_ref()
            {
                pool_mut_place(pool_expr, id_expr, *idx_span, Some(field), cx, env)
            } else {
                lower_expr(e, cx, env)
            }
        }
        _ => lower_expr(e, cx, env),
    }
}

pub(crate) fn lower_expr(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    match e {
        Expr::Int(n, _, width, _) => TExpr {
            ty: int_lit_type(width),
            kind: TExprKind::IntLit(*n, *width),
        },
        Expr::Float(v, _, is_f32) => TExpr {
            // D-FLOATW1: sema resolves F32 context and writes `is_f32=true` on the
            // node; carry that width through to TIR so emit produces the right suffix.
            ty: if *is_f32 { Type::Float32 } else { Type::Float },
            kind: TExprKind::FloatLit(*v),
        },
        Expr::Bool(b, _) => TExpr {
            ty: Type::Bool,
            kind: TExprKind::BoolLit(*b),
        },
        Expr::Char(c, _) => TExpr {
            ty: Type::Char,
            kind: TExprKind::CharLit(*c),
        },
        Expr::Str(parts, _) => {
            let tparts = parts
                .iter()
                .map(|p| match p {
                    StrPart::Lit(s) => TStrPart::Lit(s.clone()),
                    StrPart::Interp(e, fmt) => TStrPart::Interp(lower_expr(e, cx, env), *fmt),
                })
                .collect();
            TExpr {
                ty: Type::String,
                kind: TExprKind::StrLit(tparts),
            }
        }
        Expr::Ident(name, _) => {
            // c109 Phase 24: a comptime CONST inlines its pre-rendered value FIRST (the
            // AST `emit_expr` Ident arm returns `cx.consts[name]` before any env/fn-value
            // check — so a const takes precedence even over a same-named local, matching
            // byte-for-byte). The `ty` is a placeholder (never read — see `ConstInline`).
            if let Some(val) = cx.consts.get(name) {
                return TExpr {
                    ty: env.ty_of(name).unwrap_or(Type::Int),
                    kind: TExprKind::ConstInline(val.clone()),
                };
            }
            // c109 Phase 13: a bare function name used as a VALUE (not a local, not a
            // const) emits `emit_named_fn_value` — `Box::new(move |…| user_<name>(…))
            // as <fn-type>`. Mirrors `emit_expr`'s `Expr::Ident` arm (Expression.rs).
            if !env.locals.contains_key(name) && !cx.consts.contains_key(name) {
                if let Some(ft @ Type::Fn { .. }) = cx.fn_types.get(name) {
                    return TExpr {
                        ty: ft.clone(),
                        kind: TExprKind::FnValue {
                            kind: TFnValueKind::NamedFn {
                                wrapper: emit_named_fn_value(cx, name, ft),
                            },
                        },
                    };
                }
            }
            let ty = env.ty_of(name).unwrap_or(Type::Int);
            if env.is_gc(name) {
                return TExpr {
                    ty,
                    kind: TExprKind::ConstInline(format!(
                        "jet_gc::runtime_or_exit({}.read(|__jet_value| __jet_value.clone()))",
                        env.place_of(name)
                    )),
                };
            }
            TExpr {
                ty,
                kind: TExprKind::Local(env.place_of(name)),
            }
        }
        Expr::ComptimeSplice {
            value: Some(value), ..
        } => TExpr {
            ty: value.jet_type(),
            kind: TExprKind::ConstInline(value.serialize()),
        },
        Expr::ComptimeSplice { .. } => TExpr {
            ty: Type::Int,
            kind: TExprKind::ConstInline("Default::default()".to_string()),
        },
        // c109 Phase 13: a call THROUGH a fn-value `(f)(args)` (`Expr::CallValue`). The
        // Function-type parameters are unmarked, therefore Read under D-MEM-PARAM1.
        Expr::CallValue { callee, args, .. } => {
            let callee_t = lower_expr(callee, cx, env);
            let ret_ty = match &callee_t.ty {
                Type::Fn { ret: Some(r), .. } => (**r).clone(),
                _ => unit_type(),
            };
            let params = match &callee_t.ty {
                Type::Fn { params, .. } => Some(params.as_slice()),
                _ => None,
            };
            let targs = args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let conv = params
                        .and_then(|ps| ps.get(i))
                        .cloned()
                        .map(|ty| (AccessConvention::Read, ty));
                    lower_one_call_arg(a, conv, env, cx)
                })
                .collect();
            TExpr {
                ty: ret_ty,
                kind: TExprKind::FnValue {
                    kind: TFnValueKind::Call {
                        callee: Box::new(callee_t),
                        args: targs,
                    },
                },
            }
        }
        Expr::Unary(op, inner, _) => {
            let operand = lower_expr(inner, cx, env);
            let ty = operand.ty.clone();
            TExpr {
                ty,
                kind: TExprKind::Unary {
                    op: *op,
                    operand: Box::new(operand),
                },
            }
        }
        Expr::IncDec {
            op,
            operand,
            postfix,
            ..
        } => {
            let read = lower_expr(operand, cx, env);
            let place = lower_incdec_place(operand, cx, env);
            TExpr {
                ty: read.ty.clone(),
                kind: TExprKind::IncDec {
                    op: *op,
                    place,
                    postfix: *postfix,
                    ty: read.ty,
                },
            }
        }
        // D-CAP9: postfix `p.*` deref. Result type is the pointer's element type.
        Expr::Deref(inner, _) => {
            let operand = lower_expr(inner, cx, env);
            let ty = crate::Sema::ptr_elem(&operand.ty).unwrap_or_else(|| operand.ty.clone());
            TExpr {
                ty,
                kind: TExprKind::Deref(Box::new(operand)),
            }
        }
        // D-CAP9: prefix `*x` raw-pointer-of. Result type is `*T` (`Ptr<T>`).
        Expr::RawOf(inner, _) => {
            let operand = lower_expr(inner, cx, env);
            let ty = crate::Sema::ptr_type(operand.ty.clone());
            TExpr {
                ty,
                kind: TExprKind::RawOf(Box::new(operand)),
            }
        }
        // D-CAP2 (D-MEM1/S4): `copy x` — a fresh, independent value. Result
        // type is `x`'s own type (sema already proved it cloneable, E0211).
        // Reuses the existing `TExprKind::Clone` node (c109 Phase 6's
        // sema-inserted-clone lowering target) — one TIR shape whether the
        // compiler inserted the clone or the user wrote `copy` (I8).
        Expr::Copy(inner, _) => {
            let operand = lower_expr(inner, cx, env);
            let ty = operand.ty.clone();
            // D-MEM1 stage S5: `copy d` where `d` is a string-view local is the
            // one legal way to materialize it into an owned `String` that can
            // leave the view's scope. `d`'s Rust place is a bare `&str` — a
            // plain `.clone()` on that would hand back another `&str` (the
            // wrong Rust type for `ty: Type::String`), not an owned `String`;
            // `.to_string()` is the correct materialization here.
            let is_view_copy =
                matches!(&**inner, Expr::Ident(name, _) if env.is_string_view_local(name));
            let kind = if is_view_copy {
                TExprKind::MaterializeView(Box::new(operand))
            } else {
                env.note_clone(&ty);
                TExprKind::Clone(Box::new(operand))
            };
            TExpr { ty, kind }
        }
        Expr::Place(inner, access, span) => {
            if let Expr::Slice { base, start, end, .. } = inner.as_ref() {
                let recv = lower_expr(base, cx, env);
                let elem = match &recv.ty {
                    Type::List(elem) | Type::FixedList { elem, .. } => (**elem).clone(),
                    _ => Type::Int,
                };
                let mutable = *access == crate::AST::PlaceAccess::Write;
                let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
                TExpr {
                    ty: Type::Apply {
                        name: if mutable { "ViewMut" } else { "View" }.to_string(),
                        args: vec![elem],
                    },
                    kind: TExprKind::BuiltinMethod {
                        recv: Box::new(recv),
                        op: if mutable {
                            TBuiltinOp::ViewMutNew { line }
                        } else {
                            TBuiltinOp::ViewNew { line }
                        },
                        args: vec![lower_expr(start, cx, env), lower_expr(end, cx, env)],
                    },
                }
            } else {
                let place = lower_expr(inner, cx, env);
                TExpr {
                    ty: place.ty.clone(),
                    kind: TExprKind::Borrow {
                        place: Box::new(place),
                        mutable: *access == crate::AST::PlaceAccess::Write,
                    },
                }
            }
        }
        Expr::Binary(op, l, r, span) => {
            let lhs = lower_expr(l, cx, env);
            let rhs = lower_expr(r, cx, env);
            // D-SHAPE-QUANTITY1=A: sema has already validated compatibility.
            // Multiplication/division unwrap nominal unit values and emit a
            // plain numeric operation; the normalized result type is retained
            // only as a TIR fact and `rust_type` erases it to the numeric base.
            let ldim = cx.quantity_dimension(&lhs.ty);
            let rdim = cx.quantity_dimension(&rhs.ty);
            if (ldim.is_some() || rdim.is_some()) && matches!(op, BinOp::Mul | BinOp::Div) {
                let raw = |expr: TExpr| {
                    if cx.quantity_dimension(&expr.ty).is_some()
                        && matches!(expr.ty, Type::Named(_))
                    {
                        TExpr {
                            ty: Type::Float,
                            kind: TExprKind::DistinctRaw(Box::new(expr)),
                        }
                    } else {
                        expr
                    }
                };
                let lhs = raw(lhs);
                let rhs = raw(rhs);
                let left = ldim.unwrap_or(crate::AST::Dimension::SCALAR);
                let right = rdim.unwrap_or(crate::AST::Dimension::SCALAR);
                let dimension = if *op == BinOp::Mul {
                    left.multiply(right)
                } else {
                    left.divide(right)
                }
                .expect("sema checked physical dimension exponent bounds");
                let ty = if dimension == crate::AST::Dimension::SCALAR {
                    Type::Float
                } else {
                    Type::quantity(Type::Float, dimension)
                };
                let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32;
                return TExpr {
                    ty,
                    kind: TExprKind::Binary {
                        op: *op,
                        overflow: false,
                        line,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                };
            }
            // D-LAYOUT1 / D-LAYOUT-GATES1: layout-typed `+`/`-`/`>=`/`<=`/`==`.
            // Recompute via the SAME table sema used (mirrors the math/BigInt
            // early-return pattern below) rather than trusting `lhs.ty.clone()`
            // — that default is wrong here: e.g. `16.0 + label.right` has a
            // plain `Float` LEFT operand, so the result axis (`HVar`) comes
            // from the RIGHT side. Comparisons need a DEDICATED node
            // (`LayoutCompare`) since Rust's `>=`/`==` can't return a custom
            // type; `+`/`-` stay plain `Binary` (`jet_layout::LinExpr`
            // implements `std::ops::{Add,Sub}`).
            {
                let l_axis =
                    matches!(&lhs.ty, Type::Named(n) if crate::Sema::is_layout_axis_type(n));
                let r_axis =
                    matches!(&rhs.ty, Type::Named(n) if crate::Sema::is_layout_axis_type(n));
                if (l_axis || r_axis)
                    && matches!(
                        op,
                        BinOp::Ge | BinOp::Le | BinOp::Eq | BinOp::Add | BinOp::Sub
                    )
                {
                    if let Some(Ok(result_ty)) =
                        crate::Sema::layout_binop_result(*op, &lhs.ty, &rhs.ty)
                    {
                        // A bare `Int`/`Float` operand (axis-neutral) isn't a
                        // `jet_layout::LinExpr` at the Rust level yet — wrap it
                        // so `+`/`-`/`ge`/`le`/`eq_` only ever see `LinExpr`.
                        let wrap = |t: TExpr| -> TExpr {
                            if matches!(t.ty, Type::Int | Type::Float) {
                                TExpr {
                                    ty: Type::Named(
                                        crate::Syntax::LAYOUT_LENGTHVAR_TYPE.to_string(),
                                    ),
                                    kind: TExprKind::LayoutLit { inner: Box::new(t) },
                                }
                            } else {
                                t
                            }
                        };
                        let lhs = wrap(lhs);
                        let rhs = wrap(rhs);
                        if matches!(op, BinOp::Add | BinOp::Sub) {
                            let line =
                                crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32;
                            return TExpr {
                                ty: result_ty,
                                kind: TExprKind::Binary {
                                    op: *op,
                                    overflow: false,
                                    line,
                                    lhs: Box::new(lhs),
                                    rhs: Box::new(rhs),
                                },
                            };
                        }
                        return TExpr {
                            ty: result_ty,
                            kind: TExprKind::LayoutCompare {
                                op: *op,
                                lhs: Box::new(lhs),
                                rhs: Box::new(rhs),
                            },
                        };
                    }
                }
            }
            // Overflow decision, computed here once — this is the fact today's
            // `operand_is_integer` re-derives in codegen. It must mirror that
            // function EXACTLY (Codegen/Expression.rs): only a *resolvable*
            // integer operand traps. A struct-field read resolves to `None` in the
            // AST path (`expr_jet_ty` has no `Field` arm), so it does NOT trap —
            // hence we can't just inspect `TExpr.ty`, which is total even for a
            // field. We instead replay `operand_is_integer` on the AST operands.
            // `operand_is_integer` inspects only the LEFT spine of nested
            // arithmetic, so check the left operand first, then the right.
            // D-NUMOPS1: `+`/`-`/`*`/`/` trap on value overflow; `<<`/`>>` trap on a
            // bit-count out of the type's width (both via the `JetArith` helpers, so
            // no raw Rust overflow panic leaks — I2). A shift's overflow is governed
            // by its LEFT operand's integer-ness (the value), never the count.
            let arith_overflow = matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
                && (ast_operand_is_integer(l, env) == Some(true)
                    || ast_operand_is_integer(r, env) == Some(true));
            let shift_overflow = matches!(op, BinOp::Shl | BinOp::Shr)
                && ast_operand_is_integer(l, env) == Some(true);
            let overflow = arith_overflow || shift_overflow;
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32;
            // A comparison/logical op yields Bool; arithmetic keeps the operand type.
            // D-SIMD2 / D-LINALG1: a math-type operator's result follows the closed
            // family's rule (e.g. `Mat3 * Vec3 → Vec3`), not the left operand — read
            // it from the same sema table so the node's `ty` stays honest.
            let ty = if op.is_comparison() || matches!(op, BinOp::And | BinOp::Or) {
                Type::Bool
            } else if let (Type::Named(ln), Type::Named(rn)) = (&lhs.ty, &rhs.ty) {
                let lm = crate::Sema::is_math_type(ln) && !cx.type_names.contains(ln);
                let rm = crate::Sema::is_math_type(rn) && !cx.type_names.contains(rn);
                if lm || rm {
                    crate::Sema::math_binop_result(*op, ln, rn).unwrap_or_else(|| lhs.ty.clone())
                } else if ln == rn
                    && (ln == crate::Syntax::TYPE_BIGINT || ln == crate::Syntax::TYPE_DECIMAL)
                    && crate::Sema::precise_binop_result(*op, ln, rn).is_some()
                {
                    lhs.ty.clone()
                } else {
                    lhs.ty.clone()
                }
            } else {
                lhs.ty.clone()
            };
            // D-BIGINT1 / D-DECIMAL1: `+`/`-`/`*` lower to prelude helpers.
            if let (Type::Named(ln), Type::Named(rn)) = (&lhs.ty, &rhs.ty) {
                if ln == rn
                    && (ln == crate::Syntax::TYPE_BIGINT || ln == crate::Syntax::TYPE_DECIMAL)
                {
                    if let Some(result_ty) = crate::Sema::precise_binop_result(*op, ln, rn) {
                        if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) {
                            let func = match op {
                                BinOp::Add => "add",
                                BinOp::Sub => "sub",
                                BinOp::Mul => "mul",
                                _ => unreachable!(),
                            };
                            return TExpr {
                                ty: result_ty,
                                kind: TExprKind::PreciseBuiltin {
                                    type_name: ln.clone(),
                                    func: func.to_string(),
                                    args: vec![lhs, rhs],
                                },
                            };
                        }
                    }
                }
            }
            TExpr {
                ty,
                kind: TExprKind::Binary {
                    op: *op,
                    overflow,
                    line,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            }
        }
        // D-CHAINCMP1: `0 <= sev < 10` — lower each operand plainly, once each
        // (the shared-middle-operand single-evaluation guarantee is emit's
        // job: it binds each operand to a temp in a Rust block before ANDing
        // the pairwise comparisons). Always `Bool`; relational ops never trap.
        Expr::CompareChain {
            operands,
            ops,
            hooks,
            ..
        } => {
            let toperands: Vec<TExpr> = operands.iter().map(|e| lower_expr(e, cx, env)).collect();
            TExpr {
                ty: Type::Bool,
                kind: TExprKind::CompareChain {
                    operands: toperands,
                    ops: ops.clone(),
                    hooks: hooks.clone(),
                },
            }
        }
        Expr::Call(call) => {
            // c109 Phase 13: `f(args)` where `f` is a LOCAL (a fn-typed binding/param)
            // parses as `Expr::Call`. Function-type params are unmarked Read params.
            if env.locals.contains_key(&call.name) && !cx.consts.contains_key(&call.name) {
                let callee_ty = env.ty_of(&call.name).unwrap_or_else(unit_type);
                let ret_ty = match &callee_ty {
                    Type::Fn { ret: Some(r), .. } => (**r).clone(),
                    _ => unit_type(),
                };
                let callee_t = TExpr {
                    ty: callee_ty,
                    kind: TExprKind::Local(env.place_of(&call.name)),
                };
                let params = match &callee_t.ty {
                    Type::Fn { params, .. } => Some(params.as_slice()),
                    _ => None,
                };
                let targs = call
                    .args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let conv = params
                            .and_then(|ps| ps.get(i))
                            .cloned()
                            .map(|ty| (AccessConvention::Read, ty));
                        lower_one_call_arg(a, conv, env, cx)
                    })
                    .collect();
                return TExpr {
                    ty: ret_ty,
                    kind: TExprKind::FnValue {
                        kind: TFnValueKind::Call {
                            callee: Box::new(callee_t),
                            args: targs,
                        },
                    },
                };
            }
            if call.name == Syntax::RESOURCE_CLOSE
                && call.args.len() == 1
            {
                let resource = match &call.args[0].expr {
                    Expr::Ident(name, _) if env.is_resource(name) => TExpr {
                        ty: env.ty_of(name).unwrap_or_else(unit_type),
                        kind: TExprKind::ResourceTake(env.rust_name_of(name)),
                    },
                    expr => lower_expr(expr, cx, env),
                };
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Close(Box::new(resource)),
                };
            }
            // D-TYPEDTEXT1=D: the synthetic `Sql`/`Html` call sema rewrote a typed
            // text literal into (mirrors D-UNITLIT1's rewrite pattern). Args
            // alternate literal-segment, hole, literal-segment, ..., always closing
            // on a literal (`literals.len() == holes.len() + 1`) — even index is a
            // compile-time-known literal segment, odd index is a hole value. A hole
            // never re-enters the template text: `Sql` keeps it as a separate bound
            // param, `Html` HTML-escapes it before joining.
            if (call.name == "Sql" || call.name == "Html" || call.name == "Sh")
                && !cx.sigs.contains_key(&call.name)
            {
                let is_sql = call.name == "Sql";
                let is_sh = call.name == "Sh";
                let mut literals: Vec<String> = Vec::new();
                let mut holes: Vec<String> = Vec::new();
                for (i, a) in call.args.iter().enumerate() {
                    if i % 2 == 0 {
                        let lit = match &a.expr {
                            Expr::Str(parts, _) => match parts.as_slice() {
                                [crate::AST::StrPart::Lit(s)] => s.clone(),
                                _ => String::new(),
                            },
                            _ => String::new(),
                        };
                        literals.push(lit);
                    } else {
                        let hole = lower_expr(&a.expr, cx, env);
                        holes.push(format!("({}).jet_show()", emit_tir_expr(&hole, cx)));
                    }
                }
                let ty = Type::Named(call.name.clone());
                let code = if is_sql {
                    // `literals` is compile-time known here (codegen-time Rust
                    // `Vec<String>`, not generated code) — the `?`-joined template
                    // is built now, not with a runtime `+`/`format!` in the output.
                    let template = literals.join("?");
                    format!(
                        "({}.to_string(), vec![{}])",
                        escape_rust_str(&template),
                        holes.join(", ")
                    )
                } else if is_sh {
                    let mut argv = Vec::new();
                    for (i, lit) in literals.iter().enumerate() {
                        for word in lit.split_whitespace() {
                            argv.push(format!("{}.to_string()", escape_rust_str(word)));
                        }
                        if let Some(hole) = holes.get(i) {
                            argv.push(hole.clone());
                        }
                    }
                    format!("vec![{}]", argv.join(", "))
                } else {
                    // Holes are runtime values — use `format!` (not `+`) so a
                    // literal segment's `&str` never needs an owned-`String` LHS.
                    let mut fmt_str = String::new();
                    let mut fmt_args = Vec::new();
                    for (i, lit) in literals.iter().enumerate() {
                        fmt_str.push_str(&lit.replace('{', "{{").replace('}', "}}"));
                        if let Some(h) = holes.get(i) {
                            fmt_str.push_str("{}");
                            fmt_args.push(format!("{}jet_html_escape(&({}))", cx.root_prefix, h));
                        }
                    }
                    if fmt_args.is_empty() {
                        format!("{}.to_string()", escape_rust_str(&fmt_str))
                    } else {
                        format!(
                            "format!({}, {})",
                            escape_rust_str(&fmt_str),
                            fmt_args.join(", ")
                        )
                    }
                };
                return TExpr {
                    ty,
                    kind: TExprKind::ConstInline(code),
                };
            }
            // `print` is ambient only when the user has not defined their own
            // `print` function (matches emit_call; sema enforces the shadowing).
            if call.name == Syntax::BUILTIN_PRINT && !cx.sigs.contains_key(&call.name) {
                let arg = lower_expr(&call.args[0].expr, cx, env);
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Print(Box::new(arg)),
                };
            }
            // D-LIN1-DROP: `drop(x)` — discard the value (move-to-nowhere). Sema
            // proved the discard is audited when the value is `#SingleUse`. Lowers
            // to a plain `drop(arg)`; no `unsafe` (I3). Disjoint from a user `drop`
            // fn or local of that name (`cx.sigs`/`env.locals` would be set then).
            if call.name == Syntax::BUILTIN_CONSUME
                && !cx.sigs.contains_key(&call.name)
                && !env.locals.contains_key(&call.name)
            {
                let arg = lower_expr(&call.args[0].expr, cx, env);
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Drop(Box::new(arg)),
                };
            }
            // c109 Phase 26: the rich-runtime-report builtins (S36) — render the whole
            // emit string at lowering, byte-for-byte the AST helper. `require`/`panic`
            // are statement-position calls (a `()` result); the string is the `{ … }`
            // block emit emits as an expr-statement. Disjoint from a user fn of the same
            // name (`cx.sigs.contains_key` would be true then).
            if !cx.sigs.contains_key(&call.name) && !env.locals.contains_key(&call.name) {
                if call.name == Syntax::BUILTIN_REQUIRE {
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop {
                            rendered: render_require(call, cx, env),
                            always_stops: false,
                        },
                    };
                }
                if call.name == Syntax::BUILTIN_REQUIRE_EQ {
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop {
                            rendered: render_require_eq(call, cx, env),
                            always_stops: false,
                        },
                    };
                }
                if call.name == Syntax::BUILTIN_PANIC {
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop {
                            rendered: render_panic_stop(
                                &call.name_span,
                                &call.args,
                                cx,
                                env,
                            ),
                            always_stops: true,
                        },
                    };
                }
            }
            // c109 Phase 25: the ambient prelude `input(...)` (D-PRELUDE1 = B). Same
            // lowering as `io.input(...)` (CoreCall would also work, but the bare-call
            // surface has no module alias, so it is its own node). Resolves to
            // `Result<String, IOError>` (matching sema), so it composes with the
            // Phase-8 `??` fallback. The prompt arg (if any) is lowered in-subset.
            if call.name == Syntax::BUILTIN_INPUT
                && !cx.sigs.contains_key(&call.name)
                && !env.locals.contains_key(&call.name)
            {
                let prompt = call
                    .args
                    .first()
                    .map(|a| Box::new(lower_expr(&a.expr, cx, env)));
                return TExpr {
                    ty: Type::Result {
                        ok: Box::new(Type::String),
                        err: Box::new(Type::Named(Syntax::TYPE_IO_ERROR.to_string())),
                    },
                    kind: TExprKind::AmbientInput { prompt },
                };
            }
            // c109 Phase 28: the overflow opt-out builtins `wrapping(e)`/`saturating(e)`/
            // `checked(e)` (D-NUMOPS1). The gate proved the name is one of the three (not
            // shadowed) and the sole arg is an integer `Expr::Binary`. Reproduce
            // `emit_call`'s arm (Expression.rs ~L1756): `(lhs).{name}_{op}(rhs)` with PLAIN
            // operands (no trap helper). `checked_*` returns `Option<T>`; the others return
            // `T` — set the result type accordingly so a `checked(...) ?? x` composes.
            if matches!(
                call.name.as_str(),
                Syntax::BUILTIN_WRAPPING | Syntax::BUILTIN_SATURATING | Syntax::BUILTIN_CHECKED
            ) && !cx.sigs.contains_key(&call.name)
                && !env.locals.contains_key(&call.name)
            {
                if let Some(Expr::Binary(op, l, r, _)) = call.args.first().map(|a| &a.expr) {
                    let op_suffix = match op {
                        BinOp::Add => "add",
                        BinOp::Sub => "sub",
                        BinOp::Mul => "mul",
                        BinOp::Div => "div",
                        // Sema validated an arithmetic op; mirror the AST default.
                        _ => "add",
                    };
                    let lhs = lower_expr(l, cx, env);
                    let rhs = lower_expr(r, cx, env);
                    let val_ty = lhs.ty.clone();
                    let result_ty = if call.name == Syntax::BUILTIN_CHECKED {
                        Type::Option(Box::new(val_ty))
                    } else {
                        val_ty
                    };
                    return TExpr {
                        ty: result_ty,
                        kind: TExprKind::OverflowOpt {
                            prefix: call.name.clone(),
                            op: op_suffix,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                    };
                }
            }
            // c109 Phase 14: an FFI extern call (`emit_call`'s `extern_funcs` arm).
            // Checked BEFORE the unqualified arms, matching `emit_call`'s order. Args
            // use `emit_extern_call_args` (a non-scalar `Read` is `(…).clone()`).
            if !env.locals.contains_key(&call.name) {
                if let Some(wrapper) = cx.extern_funcs.get(&call.name).cloned() {
                    let sig = cx.sigs.get(&call.name).cloned();
                    let eargs = call
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let conv = sig
                                .as_ref()
                                .and_then(|ps| ps.get(i))
                                .map(|(c, t)| (*c, t.clone()));
                            lower_extern_call_arg(a, conv, env, cx)
                        })
                        .collect();
                    // The extern fn's return type lives in `cx.fn_types` only if the
                    // function is also a normal sig; extern fns are not in `fn_types`,
                    // so fall back to Unit (the binding carries the real type — the call
                    // result type is rarely load-bearing, like every covered call).
                    return TExpr {
                        ty: call_return_type(cx, &call.name),
                        kind: TExprKind::ExternCall {
                            wrapper,
                            args: eargs,
                        },
                    };
                }
                // c109 Phase 14: unqualified inline-module import (`emit_call`'s
                // `unqualified_inline` arm) → `{root}user_{mangled}(args)`.
                if let Some(mangled_key) = cx.unqualified_inline.get(&call.name).cloned() {
                    let sig = cx.sigs.get(&mangled_key).cloned();
                    let args = call
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let conv = sig
                                .as_ref()
                                .and_then(|ps| ps.get(i))
                                .map(|(c, t)| (*c, t.clone()));
                            lower_one_call_arg(a, conv, env, cx)
                        })
                        .collect();
                    return TExpr {
                        ty: call_return_type(cx, &mangled_key),
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::InlineMangled {
                                mangled: mangled_key,
                            },
                            args,
                        },
                    };
                }
                // c109 Phase 14: unqualified file-module import (`emit_call`'s
                // `unqualified_file` arm) → `{root}{rust_mod}::{mangle(fn)}(args)`. The
                // AST looks up the sig under `(call.name, fn_name)`.
                if let Some((rust_mod, fn_name)) = cx.unqualified_file.get(&call.name).cloned() {
                    let sig = cx
                        .import_sigs
                        .get(&(call.name.clone(), fn_name.clone()))
                        .cloned();
                    let args = call
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let conv = sig
                                .as_ref()
                                .and_then(|ps| ps.get(i))
                                .map(|(c, t)| (*c, t.clone()));
                            lower_one_call_arg(a, conv, env, cx)
                        })
                        .collect();
                    let ret = cx
                        .import_rets
                        .get(&(call.name.clone(), fn_name.clone()))
                        .cloned()
                        .flatten()
                        .unwrap_or_else(unit_type);
                    return TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod,
                                rust_fn: mangle(&fn_name).to_string(),
                            },
                            args,
                        },
                    };
                }
            }
            // D-BIGINT1 / D-DECIMAL1: `BigInt(…)` / `Decimal(…)` constructors.
            if !env.locals.contains_key(&call.name)
                && (call.name == crate::Syntax::TYPE_BIGINT
                    || call.name == crate::Syntax::TYPE_DECIMAL)
                && !cx.type_names.contains(&call.name)
            {
                let targs: Vec<TExpr> = call
                    .args
                    .iter()
                    .map(|a| lower_expr(&a.expr, cx, env))
                    .collect();
                let func = if call.name == crate::Syntax::TYPE_BIGINT {
                    if targs.first().map(|a| a.ty == Type::String).unwrap_or(false) {
                        "from_str"
                    } else {
                        "from_int"
                    }
                } else {
                    "from_str"
                };
                return TExpr {
                    ty: Type::Named(call.name.clone()),
                    kind: TExprKind::PreciseBuiltin {
                        type_name: call.name.clone(),
                        func: func.to_string(),
                        args: targs,
                    },
                };
            }
            // D-SIMD2 / D-LINALG1: a built-in math-type constructor. Plainly lower the
            // float components and emit `{root}jet_math_<T>_new(…)`.
            if !env.locals.contains_key(&call.name)
                && crate::Sema::is_math_type(&call.name)
                && !cx.type_names.contains(&call.name)
            {
                let targs: Vec<TExpr> = call
                    .args
                    .iter()
                    .map(|a| lower_expr(&a.expr, cx, env))
                    .collect();
                return TExpr {
                    ty: Type::Named(call.name.clone()),
                    kind: TExprKind::MathBuiltin {
                        type_name: call.name.clone(),
                        func: "new".to_string(),
                        args: targs,
                    },
                };
            }
            if call.range_checked && !env.locals.contains_key(&call.name) {
                if let Some(arg) = call.args.first() {
                    return TExpr {
                        ty: Type::Result {
                            ok: Box::new(Type::Named(call.name.clone())),
                            err: Box::new(Type::String),
                        },
                        kind: TExprKind::RangeCheckedCtor {
                            name: call.name.clone(),
                            arg: Box::new(lower_expr(&arg.expr, cx, env)),
                        },
                    };
                }
            }
            if !env.locals.contains_key(&call.name) {
                if let (Some((base, _)), Some(arg)) =
                    (cx.distinct_types.get(&call.name), call.args.first())
                {
                    return TExpr {
                        ty: Type::Named(call.name.clone()),
                        kind: TExprKind::DistinctCtor {
                            name: call.name.clone(),
                            arg: Box::new(lower_expr(&arg.expr, cx, env)),
                            base: base.clone(),
                        },
                    };
                }
            }
            // D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): a call to a trait-bounded
            // variadic function — sema left the trailing args unpacked
            // (`CheckerInfer/calls.rs::check_variadic_bound_tail`), so the arity
            // is just "how many args past the fixed prefix". Route to the
            // per-arity function `VariadicBound.rs` synthesizes; record the
            // arity so the post-pass in `Codegen/mod.rs` knows to emit it.
            if let Some((fixed, _bounds)) = cx.variadic_bound_fns.get(&call.name).cloned() {
                return crate::Codegen::VariadicBound::lower_variadic_bound_call(
                    call, fixed, cx, env,
                );
            }
            // Resolve the callee's signature so each arg's borrow/clone/fn-coercion is
            // decided here, totally — via the shared `lower_one_call_arg` (the single
            // `emit_call_args` reproduction). c109 Phase 13: a callee with a Fn-typed
            // param (now in subset) routes its arg through the Box-coercion form.
            let sig = cx.sigs.get(&call.name).cloned();
            let args = call
                .args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let conv = sig
                        .as_ref()
                        .and_then(|ps| ps.get(i))
                        .map(|(c, t)| (*c, t.clone()));
                    lower_one_call_arg(a, conv, env, cx)
                })
                .collect();
            let ret = call_return_type(cx, &call.name);
            TExpr {
                ty: ret,
                kind: TExprKind::Call {
                    name: call.name.clone(),
                    args,
                },
            }
        }
        // c109 Phase 6: a method call. The gate (`method_call_in_subset`) admitted
        // exactly the synthetic `.clone()` or a user instance method on a covered
        // type; lower accordingly. Every dispatch fact is resolved here (totality).
        Expr::MethodCall {
            receiver,
            method,
            method_span,
            type_args,
            args,
            recv_type,
            resolved_ret,
        } => {
            // D-SERDE6: codegen reads the decode target `T` from `resolved_ret`
            // (`Result<T,…>`), so the call-site `type_args` need no separate threading.
            lower_method_call(
                receiver,
                method,
                *method_span,
                type_args,
                args,
                recv_type,
                resolved_ret.as_ref(),
                cx,
                env,
            )
        }
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            let (c, bindings, mut then_prefix) = super::control_flow::lower_if_cond(cond, cx, env);
            // Value blocks scope their own bindings (like lambda block bodies).
            let mut then_env = clone_env(env);
            for (name, place, ty) in bindings {
                then_env.bind(&name, place, ty);
            }
            then_prefix.extend(lower_stmts(then_body, cx, &mut then_env));
            let t_val = lower_expr(then_value, cx, &mut then_env);
            let mut else_env = clone_env(env);
            let e_body = lower_stmts(else_body, cx, &mut else_env);
            let e_val = lower_expr(else_value, cx, &mut else_env);
            // Both arms share a type (sema guaranteed it); take the then arm's.
            let ty = t_val.ty.clone();
            TExpr {
                ty,
                kind: TExprKind::IfExpr {
                    cond: Box::new(c),
                    then_body: then_prefix,
                    then_value: Box::new(t_val),
                    else_body: e_body,
                    else_value: Box::new(e_val),
                },
            }
        }
        // c109 Phase 3: a struct literal. The gate already proved the type is a
        // plain covered user struct (no trait coercion, no import namespace, no
        // generic args), so the Rust head is `user_<name>` and field names mangle.
        // Field values are lowered as-is — no clone/coercion at the literal site
        // (mirrors the AST path; a value's own move/clone facts live in itself).
        Expr::StructLit {
            type_name,
            type_args,
            import_ns,
            as_trait,
            fields,
            ..
        } => {
            // c109 Phase 30: a trait-object coercion (`Circle {…}` in a `[Shape]` list). The
            // AST wraps the rendered literal `Box::new(<lit>) as Box<dyn user_<Trait>>`; the
            // value's type is the trait object. Resolved here (totality) — only the plain
            // user-struct branch below carries it (a coerced import_ns/prelude literal is
            // not a construct any covered program produces; the gate keeps those uncoerced).
            let trait_coerce = as_trait
                .as_ref()
                .map(|t| crate::Generics::user_trait_rust(t));
            // c109 Phase 19: a FOREIGN (imported user) struct literal `alias.Type { … }`
            // (`import_ns`). The AST `emit_struct_lit` `import_ns` branch emits
            // `{root}{import_mods[alias]}::{mangle(Type)}[::<args>]` with MANGLED fields.
            // Resolve the head here (totality); a missing alias falls to `user_unknown`,
            // exactly as the AST path (the gate already required the alias to resolve).
            if let Some(alias) = import_ns {
                if cx.core_imports.get(alias).map(String::as_str) == Some("core.encoding")
                    && matches!(
                        type_name.as_str(),
                        "EncodingLimits" | "EncodingCause" | "EncodingError"
                    )
                    && type_args.is_empty()
                {
                    let tfields = fields
                        .iter()
                        .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                        .collect();
                    return TExpr {
                        ty: Type::Named(type_name.clone()),
                        kind: TExprKind::StructLit {
                            rust_type: format!("{}jet_std::{}", cx.root_prefix, type_name),
                            fields: tfields,
                            extra: None,
                            as_trait: None,
                        },
                    };
                }
                if cx.core_imports.get(alias).map(String::as_str) == Some("core.encoding.cbor")
                    && matches!(type_name.as_str(), "CBOROptions" | "CBORError")
                    && type_args.is_empty()
                {
                    let tfields = fields
                        .iter()
                        .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                        .collect();
                    return TExpr {
                        ty: Type::Named(type_name.clone()),
                        kind: TExprKind::StructLit {
                            rust_type: format!("{}jet_std::{}", cx.root_prefix, type_name),
                            fields: tfields,
                            extra: None,
                            as_trait: None,
                        },
                    };
                }
                if cx.core_imports.get(alias).map(String::as_str) == Some("core.encoding.xml")
                    && matches!(type_name.as_str(), "XMLLimits" | "XMLParseOptions" | "XMLRenderOptions" | "XMLCanonical" | "XMLError")
                    && type_args.is_empty()
                {
                    let tfields = fields
                        .iter()
                        .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                        .collect();
                    return TExpr {
                        ty: Type::Named(type_name.clone()),
                        kind: TExprKind::StructLit {
                            rust_type: format!("{}jet_std::{}", cx.root_prefix, type_name),
                            fields: tfields,
                            extra: None,
                            as_trait: None,
                        },
                    };
                }
                if cx.core_imports.get(alias).map(String::as_str) == Some(crate::Syntax::CORE_EMAIL_MODULE)
                    && matches!(type_name.as_str(), "RecipientReport" | "SendReport" | "Limits" | "DkimConfig" | "SmtpConfig")
                {
                    let tfields = fields
                        .iter()
                        .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                        .collect();
                    return TExpr {
                        ty: Type::Named(type_name.clone()),
                        kind: TExprKind::StructLit {
                            rust_type: if matches!(type_name.as_str(), "DkimConfig" | "SmtpConfig") {
                                format!("{}jet_email::{}::<{}::Secret>", cx.root_prefix, type_name,
                                    cx.ffi_crate.as_deref().unwrap_or("jet_ffi"))
                            } else { cx.rust_type(&Type::Named(type_name.clone())) },
                            fields: tfields,
                            extra: None,
                            as_trait: None,
                        },
                    };
                }
                let mod_name = cx
                    .import_mods
                    .get(alias)
                    .map(|s| s.as_str())
                    .unwrap_or("user_unknown");
                let rust_type = if type_args.is_empty() {
                    format!("{}{}::{}", cx.root_prefix, mod_name, mangle(type_name))
                } else {
                    format!(
                        "{}{}::{}::<{}>",
                        cx.root_prefix,
                        mod_name,
                        mangle(type_name),
                        type_args
                            .iter()
                            .map(|a| cx.rust_type(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                // A foreign struct's fields are never local boxed edges (boxed_edges
                // hold this module's recursive structs), so no field is boxed here.
                let tfields = fields
                    .iter()
                    .map(|(n, _, fe)| (mangle(n), lower_expr(fe, cx, env), false))
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type,
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            // c109 Phase 17: a PRELUDE struct literal (HttpRequest/HttpResponse) uses the
            // `is_prelude_struct` branch of `emit_struct_lit`: a `<root>Jet…` Rust head,
            // PLAIN (unmangled) field names, and — for HttpRequest — an injected
            // route metadata fields. Reproduce them byte-for-byte.
            if let Some(rust) = net_handle_rust_type(type_name) {
                // A prelude struct has no boxed (recursive) edges.
                let mut tfields: Vec<(String, TExpr, bool)> = fields
                    .iter()
                    .map(|(n, _, fe)| (n.clone(), lower_expr(fe, cx, env), false))
                    .collect();
                let extra = if type_name == "HttpRequest" {
                    Some("params: std::collections::BTreeMap::new(), route_template: None".to_string())
                } else {
                    None
                };
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type: format!("{}{}", cx.root_prefix, rust),
                        fields: tfields.drain(..).collect(),
                        extra,
                        as_trait: None,
                    },
                };
            }
            // D-TEXTWIDTH1=B: `TextWidth.{ ambiguous: .Wide, controls: .Reject }` —
            // a plain dot-ctor core struct, `jet_std::TextWidth` head, no injected
            // extra field (unlike HttpRequest's `params`).
            if type_name == "TextWidth" {
                let tfields: Vec<(String, TExpr, bool)> = fields
                    .iter()
                    .map(|(n, _, fe)| (n.clone(), lower_expr(fe, cx, env), false))
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type: format!("{}jet_std::TextWidth", cx.root_prefix),
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            if type_name == "AsyncPolicy" {
                let tfields = fields
                    .iter()
                    .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type: format!("{}jet_std::JetAsyncPolicy", cx.root_prefix),
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            // D-SERDE2 / D-SERDE14=A: the public DecodeError dot constructor is a
            // core struct literal with plain fields and a jet_std Rust head.
            if type_name == "DecodeError" {
                let tfields = fields
                    .iter()
                    .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type: format!("{}jet_std::DecodeError", cx.root_prefix),
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            // D-ENCSTREAM-SURFACE1=A: shared encoding value constructors.
            if matches!(
                type_name.as_str(),
                "EncodingLimits" | "EncodingCause" | "EncodingError"
            ) {
                let tfields = fields
                    .iter()
                    .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type: format!("{}jet_std::{type_name}", cx.root_prefix),
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            // D-VALIDATE1: `FieldError.{ path: …, reason: … }` — same shape,
            // separate jet_std Rust head.
            if type_name == "FieldError" {
                let tfields = fields
                    .iter()
                    .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type: format!("{}jet_std::FieldError", cx.root_prefix),
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            if matches!(type_name.as_str(), "RecipientReport" | "SendReport" | "Limits" | "DkimConfig" | "SmtpConfig") {
                let tfields = fields
                    .iter()
                    .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type: if matches!(type_name.as_str(), "DkimConfig" | "SmtpConfig") {
                            format!("{}jet_email::{}::<{}::Secret>", cx.root_prefix, type_name,
                                cx.ffi_crate.as_deref().unwrap_or("jet_ffi"))
                        } else { cx.rust_type(&Type::Named(type_name.clone())) },
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            // c109 Phase 19: a GENERIC struct literal carries `type_args` (`Pair<T> {…}`).
            // The Rust head is the turbofish `user_<Name>::<args>` (`user_type_apply_rust`),
            // resolved at lowering; fields mangle. A non-generic literal renders `user_<Name>`.
            // c109: an UNqualified FOREIGN struct (`Note { … }`, no `import_ns`) prefixes its
            // module head (`{root}user_<mod>::user_<Note>`), exactly as `user_type_apply_rust`
            // — or rustc can't find the type (E0422). A local struct keeps the plain head.
            let head = match cx.foreign_types.get(type_name) {
                Some(rust_mod) => format!("{}{}::user_{}", cx.root_prefix, rust_mod, type_name),
                None => user_type_rust(type_name),
            };
            let rust_type = if type_args.is_empty() {
                head
            } else {
                format!(
                    "{}::<{}>",
                    head,
                    type_args
                        .iter()
                        .map(|a| cx.rust_type(a))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            // D-PATCH1: partial `T.Patch.{ … }` — fill omitted fields with `None`,
            // wrap provided scalars in `Some(…)`.
            if type_name.ends_with(".Patch")
                && cx
                    .patchable
                    .iter()
                    .any(|b| format!("{b}.Patch") == *type_name)
            {
                let provided: std::collections::HashMap<_, _> =
                    fields.iter().map(|(n, _, fe)| (n.as_str(), fe)).collect();
                let all = cx.struct_fields.get(type_name).cloned().unwrap_or_default();
                let tfields = all
                    .iter()
                    .map(|(fname, fty)| {
                        let m = mangle(fname);
                        let te = if let Some(fe) = provided.get(fname.as_str()) {
                            let inner = lower_expr(fe, cx, env);
                            TExpr {
                                ty: fty.clone(),
                                kind: TExprKind::Present(Box::new(inner)),
                            }
                        } else {
                            TExpr {
                                ty: fty.clone(),
                                kind: TExprKind::Absent,
                            }
                        };
                        (m, te, false)
                    })
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type,
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            // c109: a self-referential field (`child: Tree?` on `Tree`) has Rust type
            // `Box<…>` (`cx.boxed_edges`); resolve the `boxed` flag here (a total fact)
            // so emit can wrap the value in `Box::new(…)`, exactly as `emit_struct_lit`.
            let tfields = fields
                .iter()
                .map(|(n, _, fe)| {
                    let boxed = cx.boxed_edges.contains(&(type_name.clone(), n.clone()));
                    let value = lower_owned_expr(fe, cx, env);
                    (mangle(n), value, boxed)
                })
                .collect();
            // c109 Phase 30: a trait-coerced literal's value type is the trait object (so a
            // list of them types `[Shape]`); an uncoerced literal keeps its struct type.
            let ty = match as_trait {
                Some(t) => Type::TraitObject(vec![t.clone()]),
                None if type_args.is_empty() => Type::Named(type_name.clone()),
                None => Type::Apply {
                    name: type_name.clone(),
                    args: type_args.clone(),
                },
            };
            TExpr {
                ty,
                kind: TExprKind::StructLit {
                    rust_type,
                    fields: tfields,
                    extra: None,
                    as_trait: trait_coerce,
                },
            }
        }
        // c109 Phase 3: a struct field read in borrow position. Resolve the field
        // type ONCE here from the receiver's resolved struct type (totality). A
        // covered function never reaches here with a non-struct receiver (sema
        // guarantees field reads target struct values).
        Expr::Field(receiver, member, _) => {
            // c109 Phase 4: a *unit* enum literal (`Light.Yellow`) reaches codegen as
            // a `Field` whose receiver is the enum-name ident (sema re-types but does
            // not rewrite the node). The gate proved this is a covered enum + unit
            // variant; emit `user_<Enum>::user_<variant>` (the AST path's form).
            if let Expr::Ident(enum_name, _) = receiver.as_ref() {
                let resolved_enum = cx
                    .core_qualified_rust_type_name(enum_name)
                    .unwrap_or(enum_name.as_str());
                if env.ty_of(enum_name).is_none()
                    && matches!(enum_name.as_str(), "Overflow" | "FailurePolicy" | "DispatchState" | "HookPolicy" | "HookDecision" | "HookOutcome")
                {
                    return TExpr {
                        ty: Type::Named(enum_name.clone()),
                        kind: TExprKind::EnumLit {
                            prefix: tir_enum_lit_prefix(cx, enum_name, member),
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
                if env.ty_of(enum_name).is_none()
                    && enum_name == "DataEvent"
                    && matches!(member.as_str(), "Null" | "ArrayStart" | "ArrayEnd" | "ObjectStart" | "ObjectEnd")
                {
                    return TExpr {
                        ty: Type::Named("DataEvent".to_string()),
                        kind: TExprKind::EnumLit {
                            prefix: format!("{}jet_std::DataEvent::{}", cx.root_prefix, member),
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
                if env.ty_of(enum_name).is_none()
                    && ((enum_name == "EncodingFormat"
                        && matches!(member.as_str(), "JSON" | "JSONL" | "CSV" | "XML" | "CBOR"))
                        || (enum_name == "EncodingErrorKind"
                            && matches!(
                                member.as_str(),
                                "Syntax" | "Truncated" | "Unsupported" | "Limit" | "IO" | "State"
                            )))
                {
                    return TExpr {
                        ty: Type::Named(enum_name.clone()),
                        kind: TExprKind::EnumLit {
                            prefix: format!("{}jet_std::{}::{}", cx.root_prefix, enum_name, member),
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
                if env.ty_of(enum_name).is_none()
                    && matches!(resolved_enum, "SmtpSecurity" | "RecipientPolicy" | "SmtpAuth" | "TlsTrust")
                    && ((resolved_enum == "SmtpSecurity" && matches!(member.as_str(), "StartTls" | "Tls"))
                        || (resolved_enum == "RecipientPolicy" && matches!(member.as_str(), "RequireAll" | "DeliverAccepted"))
                        || (resolved_enum == "SmtpAuth" && member == "None")
                        || (resolved_enum == "TlsTrust" && member == "System"))
                {
                    return TExpr {
                        ty: Type::Named(resolved_enum.to_string()),
                        kind: TExprKind::EnumLit {
                            prefix: format!("{}jet_email::{}::{}", cx.root_prefix, resolved_enum, member),
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
                if env.ty_of(enum_name).is_none()
                    && cx.variant_owner.get(member).map(String::as_str) == Some(enum_name.as_str())
                {
                    // c109 Phase 24: a FOREIGN enum's unit literal (`NoteType.User` in
                    // search.jet) qualifies with the module path, exactly as `emit_expr`'s
                    // `Field` arm (Expression.rs ~L232): `{root}{mod}::user_<Enum>::<V>`.
                    // Keyed on the ENUM-name (`enum_name`, the receiver) in `cx.foreign_types`,
                    // NOT the variant — matching the AST byte-for-byte.
                    let prefix = match cx.foreign_types.get(enum_name.as_str()) {
                        Some(rust_mod) => format!(
                            "{}{}::user_{}::{}",
                            cx.root_prefix,
                            rust_mod,
                            enum_name,
                            mangle(member)
                        ),
                        None => format!("user_{}::{}", enum_name, mangle(member)),
                    };
                    return TExpr {
                        ty: Type::Named(enum_name.clone()),
                        kind: TExprKind::EnumLit {
                            prefix,
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
                // D-ENC-DYN1=A+: `Data.Null` → `{root}jet_std::DataTree::Null` (a unit
                // construction reaching codegen as a `Field`, the gate proved it).
                if env.ty_of(enum_name).is_none()
                    && is_json_type_name(enum_name)
                    && member == "Null"
                {
                    return TExpr {
                        ty: Type::Named(Syntax::TYPE_DATA.to_string()),
                        kind: TExprKind::JsonLit {
                            variant: "Null".to_string(),
                            arg: None,
                        },
                    };
                }
                // D-DBDRIVER1: `DbValue.Null` — same no-arg-`Field` shape as `Data.Null`.
                if env.ty_of(enum_name).is_none()
                    && is_db_value_type_name(enum_name)
                    && member == "Null"
                {
                    return TExpr {
                        ty: Type::Named(Syntax::TYPE_DB_VALUE.to_string()),
                        kind: TExprKind::DbValueLit {
                            variant: "Null".to_string(),
                            arg: None,
                        },
                    };
                }
                // c109 Phase 28: a numeric BOUNDS constant (`U8.MAX`/`I32.MIN`/
                // `Float.INFINITY`/…). The gate proved the receiver is a numeric type
                // name and `member` a bounds-const name. Reproduce the AST `emit_expr`
                // Field arm (Expression.rs ~L224): `{rust_type(nt)}::{member}`. The
                // rendered Rust string is total here; the result type is the numeric
                // type itself (`U8` for `U8.MAX`, `Float` for `Float.INFINITY`).
                if env.ty_of(enum_name).is_none() {
                    if let Some(nt) = crate::AST::numeric_type_from_name(enum_name) {
                        if is_numeric_bounds_const(member) {
                            return TExpr {
                                ty: nt.clone(),
                                kind: TExprKind::ConstInline(format!(
                                    "{}::{}",
                                    cx.rust_type(&nt),
                                    member
                                )),
                            };
                        }
                    }
                }
            }
            // D-SOA1: a fused `xs[i].field` where `xs` is a columnar list reads the
            // field's column directly (`jet_index_vec(&(base).user_<field>, i, …)`),
            // the cache-friendly path — no whole-`S` gather. The result is the same
            // owned, bounds-checked field value the AoS form would produce.
            if let Expr::Index {
                base,
                index,
                span,
                kind,
            } = receiver.as_ref()
            {
                if matches!(kind, IndexKind::List) {
                    let base_t = lower_expr(base, cx, env);
                    if let Type::List(elem) = &base_t.ty {
                        if cx.columnar_list_type(elem).is_some() {
                            let field_ty = struct_field_type(cx, elem, member).unwrap_or(Type::Int);
                            let index_t = lower_expr(index, cx, env);
                            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
                            return TExpr {
                                ty: field_ty,
                                kind: TExprKind::ColumnarColumnRead {
                                    base: Box::new(base_t),
                                    index: Box::new(index_t),
                                    column_rust: mangle(member).to_string(),
                                    line,
                                },
                            };
                        }
                    }
                }
            }
            let recv = lower_expr(receiver, cx, env);
            // D-FIELDPOL1: a computed field is not a Rust struct member — sema
            // (`CheckerFieldPolicy`) already synthesized it as a getter method
            // on `s.methods`; route the read to a call of that method instead
            // of a member access. `boxed` never applies (a getter call, not a
            // stored recursive edge). `self`'s own env type is deliberately
            // `None` (`recv.ty` falls back to `Type::Int`, see `LowerEnv::bind`),
            // so a bare `self.field` (every computed field's rewritten body
            // reads its siblings this way) resolves the owner via
            // `env.self_owner` instead of `recv.ty`.
            let field_owner: Option<&str> = if matches!(receiver.as_ref(), Expr::Ident(n, _) if n == Syntax::KW_SELF)
            {
                env.self_owner.as_deref()
            } else if let Type::Named(type_name) = &recv.ty {
                Some(type_name.as_str())
            } else {
                None
            };
            if let Some(type_name) = field_owner {
                if cx
                    .computed_fields
                    .get(type_name)
                    .is_some_and(|c| c.contains(member))
                {
                    let field_ty =
                        struct_field_type(cx, &Type::Named(type_name.to_string()), member)
                            .unwrap_or(Type::Int);
                    return TExpr {
                        ty: field_ty,
                        kind: TExprKind::Field {
                            recv: Box::new(recv),
                            field_rust: format!("{}()", mangle(member)),
                            boxed: false,
                        },
                    };
                }
            }
            if let Type::Named(type_name) = &recv.ty {
                if crate::Sema::is_swizzleable_math_type(type_name)
                    && !cx.struct_fields.contains_key(type_name)
                {
                    if let crate::Sema::SwizzleParse::Ok(lanes) =
                        crate::Sema::parse_swizzle_member(member, type_name)
                    {
                        let lanes_u8: Vec<u8> = lanes.iter().map(|&i| i as u8).collect();
                        return TExpr {
                            ty: crate::Sema::swizzle_read_type(type_name, lanes.len()),
                            kind: TExprKind::MathSwizzleRead {
                                type_name: type_name.clone(),
                                recv: Box::new(recv),
                                lanes: lanes_u8,
                            },
                        };
                    }
                }
            }
            let field_ty = struct_field_type(cx, &recv.ty, member).unwrap_or(Type::Int);
            // A field of a CORE struct (`ProcessResult.code`, `JsonError.message`, …) is
            // emitted by its PLAIN Rust name, never `user_<name>` (the core structs in
            // Source/Prelude/Core.rs declare unprefixed fields — B2). Reproduce
            // `core_struct_field_rust_name` (Expression.rs) from the resolved receiver
            // type so the field read is byte-exact for both core and user structs.
            let field_rust =
                core_struct_field_rust_name(cx, &recv.ty, member).unwrap_or_else(|| mangle(member));
            // A self-referential (recursive) edge has Rust type `Box<…>`; the read derefs
            // to the inner type (total fact from `cx.boxed_edges`, keyed on the receiver's
            // resolved struct name — mirrors the AST `boxed_field_read`).
            let boxed = match &recv.ty {
                Type::Named(n) => cx.boxed_edges.contains(&(n.clone(), member.to_string())),
                _ => false,
            };
            TExpr {
                ty: field_ty,
                kind: TExprKind::Field {
                    recv: Box::new(recv),
                    field_rust,
                    boxed,
                },
            }
        }
        // c109 Phase 4/16: an enum literal. Each payload arg carries its resolved
        // `clone`/`boxed` decisions (`emit_boxed_enum_arg`): a non-scalar payload from
        // a borrowed-in-env ident → `(…).clone()`; a recursive boxed edge →
        // `Box::new(…)`. For a scalar payload from a non-borrowed value both are false
        // (the Phase-4 no-op), so emit is byte-identical. Positional edges key on the
        // variant name; named edges on `"Variant.label"` (never a clone — matches AST).
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } => {
            let resolved_type = cx
                .core_qualified_rust_type_name(type_name)
                .unwrap_or(type_name.as_str());
            let prefix = tir_enum_lit_prefix(cx, resolved_type, variant);
            let payload = if args.is_empty() {
                TEnumPayload::Unit
            } else if args.iter().all(|a| matches!(a, EnumLitArg::Positional(_))) {
                let pos = args
                    .iter()
                    .map(|a| match a {
                        EnumLitArg::Positional(e) => {
                            lower_enum_arg(resolved_type, variant, variant, e, cx, env)
                        }
                        _ => unreachable!("all positional in this branch"),
                    })
                    .collect();
                TEnumPayload::Positional(pos)
            } else {
                // Named-payload variant: each field carries its mangled Rust name.
                let named = args
                    .iter()
                    .map(|a| match a {
                        EnumLitArg::Named { label, expr } => {
                            let edge = format!("{}.{}", variant, label);
                            (
                                if matches!(
                                    resolved_type,
                                    "EmailError" | "SmtpAuth" | "TlsTrust" | "AuthError"
                                ) {
                                    label.clone()
                                } else {
                                    mangle(label)
                                },
                                lower_enum_arg(resolved_type, variant, &edge, expr, cx, env),
                            )
                        }
                        // A positional arg mixed with named is a sema error that
                        // never reaches a covered function; default to a field.
                        EnumLitArg::Positional(e) => (
                            String::new(),
                            lower_enum_arg(resolved_type, variant, variant, e, cx, env),
                        ),
                    })
                    .collect();
                TEnumPayload::Named(named)
            };
            TExpr {
                ty: Type::Named(resolved_type.to_string()),
                kind: TExprKind::EnumLit { prefix, payload },
            }
        }
        // c109 Phase 5: a list literal. Lowers each element as-is (mirrors the AST
        // `vec![…]` form — no clone/coercion at the literal site). The result type
        // is `[E]` with `E` taken from the first element; an empty `[]` has no
        // element to read, so its element type is unresolved (`Int` placeholder),
        // but the emitted `vec![]` is type-inferred by Rust from the binding context.
        Expr::ListLit(elems, _span) => {
            let has_spread = elems.iter().any(|e| matches!(e, Expr::Spread(..)));
            if has_spread {
                let mut parts = Vec::new();
                for e in elems {
                    match e {
                        Expr::Spread(inner, _) => {
                            parts.push(ListSpreadPart::Spread(lower_expr(inner, cx, env)));
                        }
                        other => {
                            parts.push(ListSpreadPart::Elem(lower_expr(other, cx, env)));
                        }
                    }
                }
                let elem_ty = parts
                    .iter()
                    .find_map(|p| match p {
                        ListSpreadPart::Elem(t) => Some(t.ty.clone()),
                        ListSpreadPart::Spread(t) => match &t.ty {
                            Type::List(inner) => Some((**inner).clone()),
                            _ => Some(t.ty.clone()),
                        },
                    })
                    .unwrap_or(Type::Int);
                return TExpr {
                    ty: Type::List(Box::new(elem_ty)),
                    kind: TExprKind::ListSpread { parts },
                };
            }
            let telems: Vec<TExpr> = elems.iter().map(|e| lower_expr(e, cx, env)).collect();
            let elem_ty = telems.first().map(|e| e.ty.clone()).unwrap_or(Type::Int);
            // D-SOA1: a list of a columnar struct builds via `from_aos`.
            if let Some(columns_ty) = cx.columnar_list_type(&elem_ty) {
                return TExpr {
                    ty: Type::List(Box::new(elem_ty)),
                    kind: TExprKind::ColumnarListLit {
                        columns_ty,
                        elems: telems,
                    },
                };
            }
            TExpr {
                ty: Type::List(Box::new(elem_ty)),
                kind: TExprKind::ListLit(telems),
            }
        }
        // c109 Phase 23: a named-tuple literal → a generated `JetTup_<hash>` struct
        // literal. The gate guaranteed `ty` is `Some(Type::Tuple)`. Reproduce
        // `emit_expr`'s `TupleLit` arm: the CANONICAL field order + struct name come
        // from the type; each canonical field's value is taken from the literal (by
        // name) and lowered. Fields are emitted as `user_<f>: <v>` in canonical order.
        Expr::TupleLit(lit_fields, _, ty) => {
            let canonical = match ty {
                Some(Type::Tuple(fs)) => tuple_fields_plain(fs),
                _ => Vec::new(),
            };
            let struct_name = tuple_struct_name(&canonical);
            // Map field-name → its literal value expr (the literal may list fields in
            // any order; the type fixes the canonical order — exactly the AST path).
            let mut value_of: std::collections::HashMap<&str, &Expr> =
                std::collections::HashMap::new();
            for (n, e) in lit_fields {
                value_of.insert(n.as_str(), e);
            }
            let fields: Vec<(String, TExpr)> = canonical
                .iter()
                .map(|(n, fty)| {
                    let v = match value_of.get(n.as_str()) {
                        Some(e) => lower_expr(e, cx, env),
                        // A missing field never occurs in a sema-checked tuple literal;
                        // mirror the AST's `0i64` default defensively (an Int literal).
                        None => TExpr {
                            ty: fty.clone(),
                            kind: TExprKind::IntLit(0, None),
                        },
                    };
                    (mangle(n).to_string(), v)
                })
                .collect();
            TExpr {
                ty: ty.clone().unwrap_or_else(|| Type::Tuple(Vec::new())),
                kind: TExprKind::TupleLit {
                    struct_name,
                    fields,
                },
            }
        }
        // c109 Phase 5: a map literal `[k: v, …]` / `[:]`. Keys/values lower as-is;
        // the result type is `[K: V]` from the first entry (empty `[:]` → unresolved
        // placeholder, type-inferred by Rust from context like `vec![]`).
        Expr::MapLit(entries, _) => {
            let tentries: Vec<(TExpr, TExpr)> = entries
                .iter()
                .map(|(k, v)| (lower_expr(k, cx, env), lower_expr(v, cx, env)))
                .collect();
            let (kt, vt) = tentries
                .first()
                .map(|(k, v)| (k.ty.clone(), v.ty.clone()))
                .unwrap_or((Type::String, Type::Int));
            TExpr {
                ty: Type::Map {
                    key: Box::new(kt),
                    key_span: None,
                    value: Box::new(vt),
                },
                kind: TExprKind::MapLit(tentries),
            }
        }
        // c109 Phase 5: indexing `coll[i]`. The `IndexKind` (List/Map) is the total
        // sema fact (`is_map`); the helper line is resolved at lowering. The result
        // type is the list element / map value type, read from the base's resolved
        // type (totality) — never re-inferred in emit.
        Expr::Index {
            base,
            index,
            span,
            kind,
        } => {
            // Sema-to-TIR handoff assert (ice_regressions b5 bug class): the subset
            // gate must have already excluded `IndexKind::Unknown` before routing
            // here — an `Unknown` default reaching lowering means sema left an
            // index kind unresolved and the gate missed it.
            debug_assert!(
                !matches!(kind, IndexKind::Unknown),
                "TIR lowering reached an index read with unresolved IndexKind::Unknown \
                 (sema-to-TIR handoff violated, ice_regressions b5 bug class)"
            );
            let base_t = lower_expr(base, cx, env);
            let index_t = lower_expr(index, cx, env);
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            // D-SIMD2: `v[i]` lane access on a SIMD lane type → a bounds-checked lane
            // read. The result is the lane scalar; sema resolved `IndexKind::Lane`.
            if let IndexKind::Lane(lane_ty) = kind {
                return TExpr {
                    ty: crate::Sema::math_scalar_ty(lane_ty),
                    kind: TExprKind::MathLaneIndex {
                        lane_ty: lane_ty.clone(),
                        base: Box::new(base_t),
                        index: Box::new(index_t),
                        line: line as u32,
                    },
                };
            }
            if let IndexKind::User(type_name) = kind {
                let value_ty = cx
                    .index_hooks
                    .get(type_name)
                    .map(|h| h.value_type.clone())
                    .unwrap_or(Type::Int);
                return TExpr {
                    ty: value_ty,
                    kind: TExprKind::IndexHook {
                        type_name: type_name.clone(),
                        base: Box::new(base_t),
                        index: Box::new(index_t),
                        line,
                    },
                };
            }
            // D-MEM1 S6 (D-POOLID-API1=A): `pool[id]` read — a generation-checked
            // clone of `T` via `jet_pool_get` (panics on a stale `id`, mirroring the
            // array-oob panic precedent). `ConstInline` is the pragmatic vehicle: no
            // new `TExprKind` needed for a single free-function call, same as the
            // `Sql.raw`/`.context` escapes in `lower_method_call` below.
            if matches!(kind, IndexKind::Pool) {
                let elem_ty = match &base_t.ty {
                    Type::Apply { name, args } if name == "Pool" && !args.is_empty() => {
                        args[0].clone()
                    }
                    _ => Type::Int,
                };
                let b = emit_tir_expr(&base_t, cx);
                let i = emit_tir_expr(&index_t, cx);
                return TExpr {
                    ty: elem_ty,
                    kind: TExprKind::ConstInline(format!(
                        "{root}jet_std::jet_pool_get(&({b}), {i}, {file:?}, {line})",
                        root = cx.root_prefix,
                        file = cx.file,
                    )),
                };
            }
            if matches!(kind, IndexKind::FixedListProof) {
                let elem_ty = match &base_t.ty {
                    Type::FixedList { elem, .. } => (**elem).clone(),
                    _ => Type::Int,
                };
                let b = emit_tir_expr(&base_t, cx);
                let i = emit_tir_expr(&index_t, cx);
                return TExpr {
                    ty: elem_ty,
                    kind: TExprKind::ConstInline(format!("(({b})[({i}).0 as usize].clone())")),
                };
            }
            let result_ty = match &base_t.ty {
                Type::List(elem) => (**elem).clone(),
                Type::Map { value, .. } => (**value).clone(),
                Type::FixedList { elem, .. } => (**elem).clone(),
                // D-DYNARRAY1: `window[i]` on a `View<T>`.
                Type::Apply { name, args }
                    if matches!(name.as_str(), "View" | "ViewMut") && args.len() == 1 =>
                {
                    args[0].clone()
                }
                _ => Type::Int,
            };
            // D-SOA1: `xs[i]` on a columnar list gathers the logical `S` from the
            // columns. (A fused `xs[i].field` is handled in the `Field` arm before
            // this point — that path reads a single column directly.)
            if let Type::List(elem) = &base_t.ty {
                if cx.columnar_list_type(elem).is_some() {
                    return TExpr {
                        ty: result_ty,
                        kind: TExprKind::ColumnarGather {
                            base: Box::new(base_t),
                            index: Box::new(index_t),
                            line,
                        },
                    };
                }
            }
            TExpr {
                ty: result_ty,
                kind: TExprKind::Index {
                    base: Box::new(base_t),
                    index: Box::new(index_t),
                    is_map: matches!(kind, IndexKind::Map),
                    line,
                },
            }
        }
        // c109 Phase 5: an inclusive copy slice `coll[a..b]` (lists). Lowers to the
        // `jet_slice_vec` helper; the result is a list of the same element type.
        Expr::Slice {
            base,
            start,
            end,
            span,
        } => {
            let base_t = lower_expr(base, cx, env);
            let start_t = lower_expr(start, cx, env);
            let end_t = lower_expr(end, cx, env);
            let result_ty = base_t.ty.clone();
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: result_ty,
                kind: TExprKind::Slice {
                    base: Box::new(base_t),
                    start: Box::new(start_t),
                    end: Box::new(end_t),
                    line,
                },
            }
        }
        // D-TAINT1: `#Tainted expr` — the value-fact tag is **erased in codegen**
        // (I3). Lower the inner expression unchanged; taint exists only as a
        // compile-time sema proof, never a runtime value.
        Expr::Tainted(inner, _, _) => lower_expr(inner, cx, env),
        // c109 Phase 8: `value(x)` → `Some(x)`. The result type is `T?` where `T` is
        // the inner's resolved type (totality). Mirrors `Expr::Present`.
        Expr::Present(inner, _) => {
            let t = lower_expr(inner, cx, env);
            TExpr {
                ty: Type::Option(Box::new(t.ty.clone())),
                kind: TExprKind::Present(Box::new(t)),
            }
        }
        // c109 Phase 8: bare `null` → `None`. The element type is unresolved here
        // (`Int` placeholder) — like an empty `vec![]`, Rust infers it from the
        // binding/return context. Mirrors `Expr::Absent`.
        Expr::Absent(_) => TExpr {
            ty: Type::Option(Box::new(Type::Int)),
            kind: TExprKind::Absent,
        },
        // c109 Phase 23: a `#Todo` typed hole → diverging `todo!(…)`. The expected-type
        // STRING is the total sema fact (gate guarantees `Some`); the source line is
        // resolved here. The result `ty` is never load-bearing (a `todo!()` diverges and
        // is never an arithmetic operand), so a placeholder suffices — the emitted Rust
        // reads only `expected_type`/`line`/`cx.file`, byte-for-byte Expression.rs.
        Expr::Todo {
            span,
            expected_type,
        } => {
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: Type::Named("Unit".to_string()),
                kind: TExprKind::Todo {
                    line,
                    expected_type: expected_type
                        .clone()
                        .unwrap_or_else(|| "(unknown)".to_string()),
                },
            }
        }
        // c109 Phase 8: `Ok(x)` → `Ok(x)`. The result is a `Result` whose ok type is
        // the inner's; the err type is unresolved here (Rust infers it from the
        // function return context, exactly as the AST path's bare `Ok(x)` does).
        Expr::Ok(inner, _) => {
            let t = lower_expr(inner, cx, env);
            TExpr {
                ty: Type::Result {
                    ok: Box::new(t.ty.clone()),
                    err: Box::new(Type::Named("Error".to_string())),
                },
                kind: TExprKind::Ok(Box::new(t)),
            }
        }
        // c109 Phase 8: `Err(e)` → `Err(e)`. The err type is the inner's; the ok type
        // is unresolved here (inferred from the function return context).
        Expr::Err(inner, _) => {
            let t = lower_expr(inner, cx, env);
            TExpr {
                ty: Type::Result {
                    ok: Box::new(Type::Int),
                    err: Box::new(t.ty.clone()),
                },
                kind: TExprKind::Err(Box::new(t)),
            }
        }
        // c109 Phase 8: the `?` propagation operator. The `TryConvert` decision is the
        // total sema fact — reproduce it exactly (none/Fallible/Typed). The result
        // type is the inner `Result`'s ok type (the `?` unwraps it). The trace-frame
        // location is resolved here so emit never reads `cx.current_fn`/`cx.src`.
        Expr::Try(inner, span, convert) => {
            let inner_t = lower_expr(inner, cx, env);
            // `?` unwraps a `Result<T, E>` to `T` (the value type). If the inner type
            // resolved to a Result, take its ok type; else fall back to the inner type
            // (never load-bearing in the covered subset — a `?` result feeds a binding
            // carrying sema's `b.ty`, or an `Ok(...)` wrap whose own type is total).
            let result_ty = match &inner_t.ty {
                Type::Result { ok, .. } => (**ok).clone(),
                other => other.clone(),
            };
            let tconvert = match convert {
                TryConvert::None => TTryConvert::None,
                TryConvert::Fallible => TTryConvert::Fallible,
                TryConvert::Typed(fn_name) => TTryConvert::Typed(fn_name.clone()),
            };
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: result_ty,
                kind: TExprKind::Try {
                    inner: Box::new(inner_t),
                    convert: tconvert,
                    file: escape_rust_str(&cx.file),
                    line,
                    fn_name: escape_rust_str(&env.fn_name),
                },
            }
        }
        // c109 Phase 8: the `??` fallback operator. `is_option` is the total sema fact
        // (Result vs Option). The value + fallback are lowered; the result type is the
        // unwrapped value type (Some/Ok payload). Mirrors `emit_or_fallback`.
        Expr::OrFallback {
            value,
            fallback,
            is_option,
            ..
        } => {
            let value_t = lower_expr(value, cx, env);
            let result_ty = match &value_t.ty {
                Type::Option(inner) => (**inner).clone(),
                Type::Result { ok, .. } => (**ok).clone(),
                other => other.clone(),
            };
            let tfallback = match fallback {
                OrFallback::Value(e) => TOrFallback::Value(Box::new(lower_expr(e, cx, env))),
                OrFallback::Return(None, _) => TOrFallback::Return(None),
                OrFallback::Return(Some(e), _) => {
                    TOrFallback::Return(Some(Box::new(lower_expr(e, cx, env))))
                }
                // c109 Phase 15: the `panic(…)` form — render the whole
                // `{ jet_panic_rich(…); }` statement string at lowering, byte-for-byte
                // `emit_panic_stop`/`safe_locals_expr`, so emit reads nothing from
                // `cx.src`/`cx.current_fn`.
                OrFallback::Panic { name_span, args } => {
                    TOrFallback::Panic(render_panic_stop(name_span, args, cx, env))
                }
                OrFallback::Break(_) => TOrFallback::Break,
                OrFallback::Continue(_) => TOrFallback::Continue,
                OrFallback::BreakLabel(name, _) => TOrFallback::BreakLabel(name.clone()),
                OrFallback::ContinueLabel(name, _) => TOrFallback::ContinueLabel(name.clone()),
            };
            TExpr {
                ty: result_ty,
                kind: TExprKind::OrFallback {
                    value: Box::new(value_t),
                    fallback: tfallback,
                    is_option: *is_option,
                },
            }
        }
        // c109 Phase 8: optional chaining `base?.member`. The `flatten` fact is total
        // (from sema): true → `.and_then`, false → `.map`. The result type is `T?`;
        // resolving the inner field type here is not load-bearing (emit only formats
        // the combinator + member access), so carry the base's optional type.
        Expr::OptField {
            base,
            member,
            flatten,
            ..
        } => {
            let base_t = lower_expr(base, cx, env);
            TExpr {
                ty: base_t.ty.clone(),
                kind: TExprKind::OptField {
                    base: Box::new(base_t),
                    member_rust: mangle(member),
                    flatten: *flatten,
                },
            }
        }
        // c109 Phase 11: a lambda/closure literal. The gate proved the body is
        // in-subset; lower it via `lower_lambda` (capture/escape facts total from
        // `Lambda.meta`). The result type is the closure's fn type — rarely
        // load-bearing in emit (a closure is consumed in arg position), so carry a
        // placeholder `Fn` type; the binding/arg context supplies the real Rust type.
        Expr::Lambda(lam) => {
            let tl = lower_lambda(lam, cx, env);
            TExpr {
                ty: Type::Fn {
                    params: Vec::new(),
                    ret: None,
                    effect_bound: None,
                },
                kind: TExprKind::Lambda(Box::new(tl)),
            }
        }
        // c109 Phase 11: fan-out `f.[a, b, c]` (S75/S76). The gate proved the callee
        // is a plain top-level fn ident and every item is in-subset. The AST path
        // routes the Ident callee through `emit_call` with a SYNTHETIC single-arg
        // `Call` (`convention: Read`, default flags) per item; reproduce that exactly
        // as a `TExprKind::Call` per item, then `vec![…]`. The result type is `[T#N]`
        // (S76), erased to a list of the callee's return type.
        Expr::FanOut { callee, items, .. } => {
            let Expr::Ident(name, _) = callee.as_ref() else {
                unreachable!("gate proved fan-out callee is a plain fn ident");
            };
            // The callee's signature drives each synthetic arg's borrow wrapper,
            // exactly as `emit_call_args` does for the synthetic `Read` arg (whose
            // `implicit_clone` is false — the synthetic CallArg carries default flags).
            let sig = cx.sigs.get(name);
            let borrow = matches!(
                sig.and_then(|ps| ps.first()),
                Some((AccessConvention::Read, t)) if !t.is_scalar()
            );
            let calls: Vec<TExpr> = items
                .iter()
                .map(|item| {
                    let value = lower_expr(item, cx, env);
                    TExpr {
                        ty: call_return_type(cx, name),
                        kind: TExprKind::Call {
                            name: name.clone(),
                            args: vec![TCallArg {
                                value,
                                borrow,
                                mut_borrow: false,
                                clone: false,
                                arc_clone: false,
                                fn_coerce: None,
                                widen_to_vec: false,
                            }],
                        },
                    }
                })
                .collect();
            // D-FIXARR1: fan-out result is `[T#N]` — a real Rust stack array.
            let elem_ty = call_return_type(cx, name);
            let len = items.len() as u64;
            TExpr {
                ty: Type::FixedList {
                    elem: Box::new(elem_ty),
                    len,
                    len_symbol: None,
                },
                kind: TExprKind::FanOut { calls },
            }
        }
        // c109 Phase 18: `mem.Ptr<T>.from_addr(addr)` (S58). The result type is
        // `Ptr<elem>` (`ptr_type`), total from the node's `elem`. The element's Rust type
        // is resolved here (`cx.rust_type`) so emit makes no decision (I3). The cast is
        // safe Rust (no `unsafe`).
        Expr::PtrFromAddr { elem, addr, .. } => {
            let taddr = lower_expr(addr, cx, env);
            TExpr {
                ty: crate::Sema::ptr_type(elem.clone()),
                kind: TExprKind::PtrFromAddr {
                    elem_rust: cx.rust_type(elem),
                    addr: Box::new(taddr),
                },
            }
        }
        Expr::Paren(inner, _) => lower_expr(inner, cx, env),
        Expr::PatternTest {
            subject, pattern, ..
        } if is_binding_free_user_variant_pattern_test(pattern, cx) => {
            lower_binding_free_variant_pattern_test(subject, pattern, cx, env)
        }
        _ => unreachable!("expression not in TIR subset"),
    }
}

/// Lower an expression whose result is stored or returned as an owned value.
/// A `Read`/`Write` non-scalar parameter is represented by a dereferenced Rust
/// borrow; moving that place would leak E0507 from rustc. Jet generic functions
/// record the clone's type so generic emission adds the required bound, then
/// materialize the owned value at this semantic boundary.
pub(crate) fn lower_owned_expr(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let lowered = lower_expr(e, cx, env);
    if matches!(e, Expr::Ident(name, _) if env.is_resource(name)) {
        let Expr::Ident(name, _) = e else { unreachable!() };
        TExpr {
            ty: lowered.ty,
            kind: TExprKind::ResourceTake(env.rust_name_of(name)),
        }
    } else if matches!(e, Expr::Ident(name, _) if env.is_borrowed(name)) && !lowered.ty.is_scalar() {
        let ty = lowered.ty.clone();
        env.note_clone(&ty);
        TExpr {
            ty,
            kind: TExprKind::Clone(Box::new(lowered)),
        }
    } else {
        lowered
    }
}
