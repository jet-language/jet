use crate::AST::{
    AccessConvention, BinOp, EnumLitArg, Expr, IndexKind, OrFallback, StrPart, TryConvert, Type,
    TypedLitBody,
};
use crate::Codegen::Cx;
use crate::Codegen::emit_named_fn_value;
use crate::Codegen::escape_rust_str;
use crate::Codegen::is_db_value_type_name;
use crate::Codegen::is_json_type_name;
use crate::Codegen::mangle;
use crate::Codegen::net_handle_rust_type;
use crate::Codegen::TIR::ast_operand_is_integer;
use crate::Codegen::TIR::{call_return_type, call_return_type_with_args};
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::int_lit_type;
use crate::Codegen::TIR::is_numeric_bounds_const;
use crate::Codegen::TIR::ListSpreadPart;
use crate::Codegen::TIR::lower_enum_arg;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::TIR::TMethodRef;
use crate::Codegen::TIR::lower_extern_call_arg;
use crate::Codegen::TIR::lower::is_binding_free_user_variant_pattern_test;
use crate::Codegen::TIR::lower_lambda;
use crate::Codegen::TIR::lower::lower_binding_free_variant_pattern_test;
use crate::Codegen::TIR::lower::lower_comptime_scalar;
use crate::Codegen::TIR::lower::lower_incdec_place;
use crate::Codegen::TIR::lower_method_call;
use crate::Codegen::TIR::lower_one_call_arg;
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::lower_panic_stop;
use crate::Codegen::TIR::lower_require_eq_stop;
use crate::Codegen::TIR::lower_require_stop;
use crate::Codegen::TIR::TRequireKind;
use crate::Codegen::TIR::struct_field_type;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TBuiltinOp;
use crate::Codegen::TIR::TEnumPayload;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TFnValueKind;
use crate::Codegen::TIR::TModuleCallForm;
use crate::Codegen::TIR::TOrFallback;
use crate::Codegen::TIR::TStrPart;
use crate::Codegen::TIR::TTryConvert;
use crate::Codegen::TIR::unit_type;
use crate::Codegen::tuple_fields_plain;
use crate::Codegen::tuple_struct_name;
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
        match field {
            None => TExpr {
                ty: elem_ty,
                kind: TExprKind::PoolSlot {
                    pool: Box::new(pool_t),
                    id: Box::new(id_t),
                    mutable: true,
                    field: None,
                    line,
                },
            },
            Some(f) => {
                let field_ty = struct_field_type(cx, &elem_ty, f).unwrap_or(Type::Int);
                TExpr {
                    ty: field_ty,
                    kind: TExprKind::PoolSlot {
                        pool: Box::new(pool_t),
                        id: Box::new(id_t),
                        mutable: true,
                        field: Some(f.to_string()),
                        line,
                    },
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

/// Lower a fluent method receiver from the innermost call outward.
///
/// Method-call ASTs nest through `receiver`. Walking that spine iteratively
/// keeps long fluent APIs off the Rust call stack while preserving the normal
/// method dispatcher for every link.
fn lower_method_chain(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let mut calls = Vec::new();
    let mut cursor = e;
    while let Expr::MethodCall { receiver, .. } = cursor {
        calls.push(cursor);
        cursor = receiver;
    }

    let mut lowered_receiver = None;
    while let Some(call) = calls.pop() {
        let Expr::MethodCall {
            receiver,
            method,
            method_span,
            owner_type_args,
            type_args,
            args,
            recv_type,
            resolved_ret,
            checked_widen,
        } = call
        else {
            unreachable!("method chain contains only method calls")
        };
        let lowered = lower_method_call(
            receiver,
            method,
            *method_span,
            owner_type_args,
            type_args,
            args,
            recv_type,
            resolved_ret.as_ref(),
            *checked_widen,
            cx,
            env,
            lowered_receiver,
        );
        // D-APILABEL1=A: a method whose labels reordered its arguments keeps
        // the same source evaluation order as a free call.
        lowered_receiver = Some(match source_arg_order(args) {
            Some(order) => preserve_source_arg_order(lowered, &order, args.len(), method_span.start as u32),
            None => lowered,
        });
    }

    lowered_receiver.expect("method chain is non-empty")
}

pub(crate) fn lower_expr(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let mut e = e;
    while let Expr::Paren(inner, _) = e {
        e = inner;
    }
    thread_local! {
        static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    let too_deep = DEPTH.with(|d| {
        // Keep well under Linux default stack for large lower frames (ws/http).
        if d.get() > 256 {
            true
        } else {
            d.set(d.get() + 1);
            false
        }
    });
    if too_deep {
        return TExpr {
            ty: Type::Int,
            kind: crate::Codegen::TIR::TExprKind::Todo {
                line: 0,
                expected_type: "lower depth".into(),
            },
        };
    }
    let out = lower_expr_inner(e, cx, env);
    DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    out
}

/// D-BOUND-HEAD1=A: comptime can lower a typed head before sema has rewritten
/// it to the ordinary alternating literal/hole call. Keep that early path on
/// the same TIR host node used after sema.
fn lower_boundary_typed_lit(
    type_name: &str,
    body: &TypedLitBody,
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    let TypedLitBody::Value(inner) = body else {
        return None;
    };
    let Expr::Str(parts, _) = inner.as_ref() else {
        return None;
    };
    let mut literals = Vec::new();
    let mut holes = Vec::new();
    for part in parts {
        match part {
            StrPart::Lit(text) => literals.push(text.clone()),
            StrPart::Interp(expr, _) => {
                if literals.len() == holes.len() {
                    literals.push(String::new());
                }
                holes.push(lower_expr(expr, cx, env));
            }
        }
    }
    if literals.len() == holes.len() {
        literals.push(String::new());
    }
    let kind = match type_name {
        Syntax::TYPE_URL => crate::Codegen::TIR::TTypedTextInterpKind::URL,
        Syntax::TYPE_PATH => crate::Codegen::TIR::TTypedTextInterpKind::Path,
        Syntax::TYPE_DATETIME => crate::Codegen::TIR::TTypedTextInterpKind::DateTime,
        _ => return None,
    };
    let ty = if type_name == Syntax::TYPE_URL {
        Type::Named("Url".to_string())
    } else {
        Type::Named(type_name.to_string())
    };
    Some(TExpr {
        ty,
        kind: TExprKind::HostCall(Box::new(
            crate::Codegen::TIR::THostCall::TypedTextInterp {
                kind,
                literals,
                holes,
            },
        )),
    })
}

fn expr_tag(e: &Expr) -> &'static str {
    match e {
        Expr::Str(..) => "Str",
        Expr::StrMatchLit(..) => "StrMatchLit",
        Expr::BinMatchLit(..) => "BinMatchLit",
        Expr::Int(..) => "Int",
        Expr::Float(..) => "Float",
        Expr::Bool(..) => "Bool",
        Expr::Char(..) => "Char",
        Expr::ListLit(..) => "ListLit",
        Expr::MemberSpread { .. } => "MemberSpread",
        Expr::Spread(..) => "Spread",
        Expr::MapLit(..) => "MapLit",
        Expr::Index { .. } => "Index",
        Expr::Slice { .. } => "Slice",
        Expr::Range { .. } => "Range",
        Expr::Ident(..) => "Ident",
        Expr::Call(..) => "Call",
        Expr::Unary(..) => "Unary",
        Expr::Binary(..) => "Binary",
        Expr::CompareChain { .. } => "CompareChain",
        Expr::UnitLit { .. } => "UnitLit",
        Expr::Deref(..) => "Deref",
        Expr::RawOf(..) => "RawOf",
        Expr::Copy(..) => "Copy",
        Expr::Place(..) => "Place",
        Expr::Field(..) => "Field",
        Expr::OptField { .. } => "OptField",
        Expr::MethodCall { .. } => "MethodCall",
        Expr::If { .. } => "If",
        Expr::StructLit { .. } => "StructLit",
        Expr::EnumLit { .. } => "EnumLit",
        Expr::Tainted(..) => "Tainted",
        Expr::Present(..) => "Present",
        Expr::Absent(_) => "Absent",
        Expr::Todo { .. } => "Todo",
        Expr::NoElse(_) => "NoElse",
        Expr::ReduceMarker(..) => "ReduceMarker",
        Expr::Ok(..) => "Ok",
        Expr::Err(..) => "Err",
        Expr::Try(..) => "Try",
        Expr::OrFallback { .. } => "OrFallback",
        Expr::TupleLit(..) => "TupleLit",
        Expr::Lambda(..) => "Lambda",
        Expr::PtrFromAddr { .. } => "PtrFromAddr",
        Expr::TypedLit { .. } => "TypedLit",
        Expr::Paren(..) => "Paren",
        Expr::PatternTest { .. } => "PatternTest",
        Expr::ComptimeName { .. } => "ComptimeName",
        Expr::CallValue { .. } => "CallValue",
        Expr::IncDec { .. } => "IncDec",
    }
}

fn lower_unit_text(
    value: TExpr,
    style: crate::AST::UnitFormat,
    cx: &Cx,
) -> TExpr {
    let original_ty = value.ty.clone();
    let raw = if let Type::Named(name) = &original_ty {
        let base = cx
            .distinct_types
            .get(name)
            .map(|(base, _)| base.clone())
            .unwrap_or(Type::Float);
        TExpr {
            ty: base,
            kind: TExprKind::DistinctRaw(Box::new(value)),
        }
    } else {
        value
    };
    let source_span = crate::Diagnostics::Span::new(0, 0);
    let magnitude = TExpr {
        ty: Type::String,
        kind: TExprKind::CoreCall {
            module: "jet.unit".to_string(),
            method: "magnitude".to_string(),
            args: vec![raw],
            source_span,
            widen_to_vec: vec![false],
        },
    };
    let mut parts = vec![TStrPart::Interp(
        magnitude,
        crate::AST::StrFormat::Display,
    )];
    if style != crate::AST::UnitFormat::Bare {
        let label = cx
            .unit_label(&original_ty)
            .map(|label| match style {
                crate::AST::UnitFormat::Name => label.name.clone(),
                crate::AST::UnitFormat::Symbol | crate::AST::UnitFormat::Bare => {
                    label.symbol.clone()
                }
            })
            .or_else(|| {
                original_ty
                    .quantity_parts()
                    .map(|(_, dimension)| cx.quantity_unit_label(dimension, style))
            })
            .expect("sema accepted unit formatting only for unit values");
        parts.push(TStrPart::Lit(format!(" {label}")));
    }
    TExpr {
        ty: Type::String,
        kind: TExprKind::StrLit(parts),
    }
}

/// D-VERDICT-1321-1: variadic `print`/`io.print`/`io.eprint` — join the
/// arguments with newline separators into one string value, so downstream
/// engines see the ordinary single-value print they already implement.
pub(crate) fn join_print_args(
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    join_print_values(
        args.iter()
            .map(|arg| lower_expr(&arg.expr, cx, env)),
        cx,
    )
}

pub(crate) fn join_print_values(
    values: impl IntoIterator<Item = TExpr>,
    cx: &Cx,
) -> TExpr {
    let values: Vec<TExpr> = values
        .into_iter()
        .map(|value| lower_display_value(value, cx))
        .collect();
    let mut parts = Vec::with_capacity(values.len() * 2);
    for (index, value) in values.into_iter().enumerate() {
        if index > 0 {
            parts.push(TStrPart::Lit("\n".to_string()));
        }
        parts.push(TStrPart::Interp(value, crate::AST::StrFormat::Display));
    }
    TExpr {
        ty: Type::String,
        kind: TExprKind::StrLit(parts),
    }
}

fn lower_display_value(value: TExpr, cx: &Cx) -> TExpr {
    if value.ty.quantity_parts().is_some() {
        return lower_unit_text(value, crate::AST::UnitFormat::Symbol, cx);
    }
    let Type::Named(name) = &value.ty else {
        return value;
    };
    if cx.unit_label(&value.ty).is_none() {
        return value;
    }
    if !cx.has_display_type(name) {
        return lower_unit_text(value, crate::AST::UnitFormat::Symbol, cx);
    }
    TExpr {
        ty: Type::String,
        kind: TExprKind::MethodCall {
            recv: Box::new(value),
            method: TMethodRef::bare("display"),
            type_args: Vec::new(),
            args: Vec::new(),
            source_first_string_literal: None,
            operator_line: None,
        },
    }
}

fn lower_expr_inner(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
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
        Expr::Range {
            start,
            end,
            exclusive,
            ..
        } => TExpr {
            ty: Type::Named(Syntax::TYPE_RANGE.to_string()),
            kind: TExprKind::StructLit {
                fields: vec![
                    ("start".to_string(), lower_expr(start, cx, env), false),
                    ("end".to_string(), lower_expr(end, cx, env), false),
                    (
                        "exclusive".to_string(),
                        TExpr {
                            ty: Type::Bool,
                            kind: TExprKind::BoolLit(*exclusive),
                        },
                        false,
                    ),
                ],
                extra: None,
                as_trait: None,
            },
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
                    StrPart::Interp(e, crate::AST::StrFormat::Fixed(precision)) => {
                        let source_span = e.span();
                        let formatted = TExpr {
                            ty: Type::String,
                            kind: TExprKind::CoreCall {
                                module: "core.fmt".to_string(),
                                method: "decimal".to_string(),
                                args: vec![
                                    lower_expr(e, cx, env),
                                    TExpr {
                                        ty: Type::Int,
                                        kind: TExprKind::IntLit(*precision, None),
                                    },
                                ],
                                source_span,
                                widen_to_vec: vec![false, false],
                            },
                        };
                        TStrPart::Interp(formatted, crate::AST::StrFormat::Display)
                    }
                    StrPart::Interp(e, crate::AST::StrFormat::Unit(style)) => {
                        let value = lower_expr(e, cx, env);
                        TStrPart::Interp(
                            lower_unit_text(value, *style, cx),
                            crate::AST::StrFormat::Display,
                        )
                    }
                    StrPart::Interp(e, crate::AST::StrFormat::Display) => {
                        TStrPart::Interp(
                            lower_display_value(lower_expr(e, cx, env), cx),
                            crate::AST::StrFormat::Display,
                        )
                    }
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
            // byte-for-byte). The evaluated value supplies the total scalar type so an
            // inlined F32 keeps its width in every TIR consumer.
            // parity: guard tests/tir_patterns_and_fields.rs::comptime_local_is_literal_data
            if cx.consts.contains_key(name) {
                let value = cx.const_values.get(name);
                let ty = env
                    .ty_of(name)
                    .or_else(|| value.map(crate::AST::CtValue::jet_type))
                    .unwrap_or(Type::Int);
                return TExpr {
                    kind: lower_comptime_scalar(value, Some(&ty))
                        .unwrap_or_else(|| TExprKind::ConstRef(name.clone())),
                    ty,
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
                                name: Some(name.clone()),
                            },
                        },
                    };
                }
            }
            let ty = env.ty_of(name).unwrap_or(Type::Int);
            if env.is_gc(name) {
                return TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::GcRead {
                        root: env.place_of(name),
                    })),
                };
            }
            TExpr {
                ty,
                kind: TExprKind::Local(env.local_of(name)),
            }
        }
        // Fragment eval must win over the sema value stamp: a baked CtLit is a
        // value, not a place, so a marked receiver could never advance
        // (`$r.read_u8()` folded the same byte forever). Mirror the Ident
        // consts branch: scalars inline, everything else is a ConstRef place
        // that the evaluator reads and writes back through the comptime scope.
        Expr::ComptimeName { name, .. }
            if super::is_eval_fragment() && cx.const_values.contains_key(name) =>
        {
            let value = cx.const_values.get(name);
            let ty = value.map(crate::AST::CtValue::jet_type).unwrap_or(Type::Int);
            TExpr {
                kind: lower_comptime_scalar(value, Some(&ty))
                    .unwrap_or_else(|| TExprKind::ConstRef(name.clone())),
                ty,
            }
        }
        Expr::ComptimeName {
            value: Some(value), ..
        } => TExpr {
            ty: value.jet_type(),
            kind: TExprKind::CtLit(value.clone()),
        },
        Expr::ComptimeName { name, .. } if super::is_eval_fragment() => {
            // `$name` resolves from the comptime scope at eval time (D-META-STAGE1=B, formerly D-CTMARKER1=C).
            if !env.locals.contains_key(name) {
                env.bind(name, TLocal::user(name), None);
            }
            TExpr {
                ty: Type::Int,
                kind: TExprKind::Local(env.local_of(name)),
            }
        }
        Expr::ComptimeName { .. } => TExpr {
            ty: Type::Int,
            kind: TExprKind::DefaultLit,
        },
        // D-LOOPEVAL1: the parser carries a yielding loop through sema as an
        // immediately-called private lambda. Lower it as a block in the current
        // function, not as a closure call: captures, effects, `return`, and cleanup
        // must keep ordinary loop behavior.
        Expr::CallValue { callee, args, .. }
            if args.is_empty()
                && matches!(
                    callee.as_ref(),
                    Expr::Lambda(lam) if lam.meta.collecting_loop || lam.meta.result_loop
                ) =>
        {
            let Expr::Lambda(lam) = callee.as_ref() else {
                unreachable!("collecting loop guard requires a lambda")
            };
            let crate::AST::LambdaBody::Block(body) = &lam.body else {
                unreachable!("collecting loops always carry a block")
            };
            let ty = if lam.meta.collecting_loop {
                Type::List(Box::new(
                    lam.meta.collect_item_type.clone().unwrap_or(Type::Int),
                ))
            } else {
                lam.meta.loop_result_type.clone().unwrap_or(Type::Int)
            };
            let mut block_env = clone_env(env);
            TExpr {
                ty,
                kind: TExprKind::InlineBlock(lower_stmts(body, cx, &mut block_env)),
            }
        }
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
            let lowered = TExpr {
                ty: ret_ty,
                kind: TExprKind::FnValue {
                    kind: TFnValueKind::Call {
                        callee: Box::new(callee_t),
                        args: targs,
                    },
                },
            };
            // D-APILABEL1=A: a function type may declare a call contract, so a
            // call through the value can reorder just like a named one.
            match source_arg_order(args) {
                Some(order) => preserve_source_arg_order(lowered, &order, args.len(), e.span().start as u32),
                None => lowered,
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
            if let Expr::Slice {
                base,
                start,
                end,
                range,
                ..
            } = inner.as_ref()
            {
                let recv = lower_expr(base, cx, env);
                let is_tensor = match &recv.ty {
                    Type::Named(name) | Type::Apply { name, .. } if name == "Tensor" => true,
                    _ => false,
                };
                let elem = if is_tensor {
                    Type::Float
                } else {
                    match &recv.ty {
                        Type::List(elem) | Type::FixedList { elem, .. } => (**elem).clone(),
                        _ => Type::Int,
                    }
                };
                let mutable = *access == crate::AST::PlaceAccess::Write;
                let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
                let args = if let Some(range) = range {
                    vec![lower_expr(range, cx, env)]
                } else {
                    vec![lower_expr(start, cx, env), lower_expr(end, cx, env)]
                };
                TExpr {
                    ty: Type::Apply {
                        name: if mutable { "ViewMut" } else { "View" }.to_string(),
                        args: vec![elem],
                    },
                    kind: TExprKind::BuiltinMethod {
                        recv: Box::new(recv),
                        op: if mutable {
                            if is_tensor {
                                TBuiltinOp::ComputeViewMutNew { line }
                            } else {
                                TBuiltinOp::ViewMutNew { line }
                            }
                        } else {
                            if is_tensor {
                                TBuiltinOp::ComputeViewNew { line }
                            } else {
                                TBuiltinOp::ViewNew { line }
                            }
                        },
                        args,
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
                let left = ldim.unwrap_or_else(crate::AST::Dimension::scalar);
                let right = rdim.unwrap_or_else(crate::AST::Dimension::scalar);
                let dimension = if *op == BinOp::Mul {
                    left.multiply(&right)
                } else {
                    left.divide(&right)
                }
                .expect("sema checked physical dimension exponent bounds");
                let ty = if dimension == crate::AST::Dimension::scalar() {
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
            // Overflow decision for trapping JetArith helpers. Prefer the
            // resolved TIR operand types so call results, fields, and other
            // shapes the AST replay cannot see still trap (I2 / #1484). The
            // AST replay remains for cases where lowering types are not yet
            // integer-shaped but the source operand structurally is.
            // D-NUMOPS1: `+`/`-`/`*`/`/` trap on value overflow; `<<`/`>>`
            // trap on a bit-count out of the type's width (both via the `JetArith`
            // helpers, so no raw Rust overflow panic leaks — I2). A shift's
            // overflow is governed by its LEFT operand's integer-ness (the value),
            // never the count.
            // D-INTDIV1=A: `Int / Int` is widened to Float before lowering, so
            // those operands are not `is_integer()` here and keep bare `/`.
            // Fixed-width `IntN` keeps same-width `/` and must trap via jet_div.
            // D-MODSEM1=A: `%` and `%%` always call their Prelude helper above.
            let tir_integer = lhs.ty.is_integer() || rhs.ty.is_integer();
            let arith_overflow = matches!(
                op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div
            ) && (tir_integer
                || ast_operand_is_integer(l, env) == Some(true)
                || ast_operand_is_integer(r, env) == Some(true));
            let shift_overflow = matches!(op, BinOp::Shl | BinOp::Shr)
                && (lhs.ty.is_integer() || ast_operand_is_integer(l, env) == Some(true));
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
            // An `approx(value)` not consumed immediately by an integer-to-float
            // crossing grants nothing later. Erase its unspellable marker now.
            if call.widen_approx && call.args.len() == 1 {
                return lower_expr(&call.args[0].expr, cx, env);
            }
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
                    kind: TExprKind::Local(env.local_of(&call.name)),
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
                let lowered = TExpr {
                    ty: ret_ty,
                    kind: TExprKind::FnValue {
                        kind: TFnValueKind::Call {
                            callee: Box::new(callee_t),
                            args: targs,
                        },
                    },
                };
                return match source_arg_order(&call.args) {
                    Some(order) => preserve_source_arg_order(
                        lowered, &order, call.args.len(), call.name_span.start as u32,
                    ),
                    None => lowered,
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
            // D-TYPEDTEXT1=D / D-BOUND-HEAD1=A: the synthetic typed-head call sema rewrote a typed
            // text literal into (mirrors D-UNITLIT1's rewrite pattern). Args
            // alternate literal-segment, hole, literal-segment, ..., always closing
            // on a literal (`literals.len() == holes.len() + 1`) — even index is a
            // compile-time-known literal segment, odd index is a hole value. A hole
            // never re-enters the template text: `SQL` keeps it as a separate bound
            // param, `HTML` HTML-escapes it before joining.
            if (call.name == "SQL"
                || call.name == "HTML"
                || call.name == "Sh"
                || call.name == Syntax::TYPE_URL
                || call.name == Syntax::TYPE_PATH
                || call.name == Syntax::TYPE_DATETIME)
                && !cx.sigs.contains_key(&call.name)
            {
                let mut literals: Vec<String> = Vec::new();
                let mut holes: Vec<TExpr> = Vec::new();
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
                        holes.push(lower_expr(&a.expr, cx, env));
                    }
                }
                let kind = match call.name.as_str() {
                    "SQL" => crate::Codegen::TIR::TTypedTextInterpKind::SQL,
                    "Sh" => crate::Codegen::TIR::TTypedTextInterpKind::Sh,
                    "HTML" => crate::Codegen::TIR::TTypedTextInterpKind::HTML,
                    Syntax::TYPE_URL => crate::Codegen::TIR::TTypedTextInterpKind::URL,
                    Syntax::TYPE_PATH => crate::Codegen::TIR::TTypedTextInterpKind::Path,
                    Syntax::TYPE_DATETIME => crate::Codegen::TIR::TTypedTextInterpKind::DateTime,
                    _ => unreachable!("typed-head lowering guard and kind table disagree"),
                };
                let ty = if call.name == Syntax::TYPE_URL {
                    Type::Named("Url".to_string())
                } else {
                    Type::Named(call.name.clone())
                };
                return TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::TypedTextInterp {
                        kind,
                        literals,
                        holes,
                    })),
                };
            }
            // D-REGEX-LIT1=D: sema already validated this complete literal.
            // Keep one Regex value through AOT/JIT instead of a fallible Result.
            if call.name == Syntax::TYPE_REGEX
                && !cx.sigs.contains_key(&call.name)
                && call.args.len() == 1
            {
                return TExpr {
                    ty: Type::Named(Syntax::TYPE_REGEX.to_string()),
                    kind: TExprKind::CoreCall {
                        module: "core.regex".to_string(),
                        method: "literal".to_string(),
                        args: vec![lower_expr(&call.args[0].expr, cx, env)],
                        source_span: call.name_span,
                        widen_to_vec: vec![false],
                    },
                };
            }
            // `print` is ambient only when the user has not defined their own
            // `print` function (matches emit_call; sema enforces the shadowing).
            // D-VERDICT-1321-1: multiple arguments join with newlines into one
            // Print, so every engine keeps its single-value Print semantics.
            if call.name == Syntax::BUILTIN_PRINT && !cx.sigs.contains_key(&call.name) {
                // `join_print_args` also absorbs the sema-rejected zero-arg
                // form (empty line) so lowering never indexes out of bounds.
                let arg = if call.args.len() == 1 {
                    lower_display_value(lower_expr(&call.args[0].expr, cx, env), cx)
                } else {
                    join_print_args(&call.args, cx, env)
                };
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
            // D-TOOL4: `expect(x)` builds a snapshot harness holder. At TirBridge
            // comptime the holder is the value itself — `consume(expect(x))` only
            // needs the binding to exist; `.snapshot()` is a separate HostCall.
            if call.name == Syntax::BUILTIN_EXPECT
                && !cx.sigs.contains_key(&call.name)
                && !env.locals.contains_key(&call.name)
                && call.args.len() == 1
            {
                let arg = lower_expr(&call.args[0].expr, cx, env);
                return TExpr {
                    ty: arg.ty.clone(),
                    kind: TExprKind::Clone(Box::new(arg)),
                };
            }
            // c109 Phase 26: the rich-runtime-report builtins (S36) — render the whole
            // emit string at lowering, byte-for-byte the AST helper. `require`/`panic`
            // are statement-position calls (a `()` result); the string is the `{ … }`
            // block emit emits as an expr-statement. Disjoint from a user fn of the same
            // name (`cx.sigs.contains_key` would be true then).
            if !cx.sigs.contains_key(&call.name) && !env.locals.contains_key(&call.name) {
                if call.name == Syntax::BUILTIN_REQUIRE {
                    let (kind, loc) = lower_require_stop(call, cx, env);
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop {
                            kind,
                            loc,
                            always_stops: false,
                        },
                    };
                }
                if call.name == Syntax::BUILTIN_REQUIRE_EQ {
                    let (kind, loc) = lower_require_eq_stop(call, cx, env);
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop {
                            kind,
                            loc,
                            always_stops: false,
                        },
                    };
                }
                if call.name == Syntax::BUILTIN_PANIC {
                    let (kind, loc) = lower_panic_stop(&call.name_span, &call.args, cx, env);
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop {
                            kind,
                            loc,
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
                    // Return type comes from `cx.fn_types` (including extern rust /
                    // CModule entries registered in Context).
                    let lowered = TExpr {
                        ty: call_return_type(cx, &call.name),
                        kind: TExprKind::ExternCall {
                            wrapper,
                            args: eargs,
                        },
                    };
                    return match source_arg_order(&call.args) {
                        Some(order) => preserve_source_arg_order(
                            lowered,
                            &order,
                            call.args.len(),
                            call.name_span.start as u32,
                        ),
                        None => lowered,
                    };
                }
                // c109 Phase 14: unqualified inline-module import (`emit_call`'s
                // `unqualified_inline` arm) → `{root}__jet_{mangled}(args)`.
                let inline_mangled = cx
                    .inline_unqualified
                    .get(&env.fn_name)
                    .and_then(|scope| scope.get(&call.name))
                    .or_else(|| cx.unqualified_inline.get(&call.name))
                    .cloned();
                if let Some(mangled_key) = inline_mangled
                {
                    let sig = cx.sigs.get(&mangled_key).cloned();
                    let args: Vec<_> = call
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
                        ty: call_return_type_with_args(
                            cx,
                            &mangled_key,
                            &call.type_args,
                            &args,
                        ),
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::InlineMangled {
                                mangled: mangled_key,
                            },
                            type_args: call.type_args.clone(),
                            args,
                        },
                    };
                }
                // c109 Phase 14: unqualified file-module import (`emit_call`'s
                // `unqualified_file` arm) → `{root}{rust_mod}::{mangle(fn)}(args)`. The
                // AST looks up the sig under `(call.name, fn_name)`.
                let inline_file = cx
                    .inline_unqualified_file
                    .get(&env.fn_name)
                    .and_then(|scope| scope.get(&call.name))
                    .or_else(|| cx.unqualified_file.get(&call.name))
                    .cloned();
                if let Some((rust_mod, fn_name)) = inline_file
                {
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
                            type_args: call.type_args.clone(),
                            args,
                        },
                    };
                }
            }
            // D-ZIPPAD1: free zip-family calls carry their complete result type
            // from sema. Lower the same variadic contract as the method form;
            // no ordinary function lookup or codegen-side type inference is
            // involved.
            if matches!(call.name.as_str(), "zip" | "zip_short" | "zip_pad")
                && !cx.sigs.contains_key(&call.name)
                && call.resolved_ret.is_some()
            {
                let is_pad = call.name == "zip_pad";
                let mut inputs = Vec::new();
                let mut fields = Vec::new();
                let mut fills = Vec::new();
                for arg in &call.args {
                    match (is_pad, arg.label.as_ref().map(|(name, _)| name.as_str())) {
                        (true, Some("fill")) | (true, Some("fills")) => {
                            fills.push(lower_expr(&arg.expr, cx, env));
                        }
                        _ => {
                            let index = fields.len();
                            let name = arg.label.as_ref().map(|(name, _)| name.as_str());
                            let field = name.map_or_else(
                                || {
                                    ["a", "b", "c", "d", "e", "f"]
                                        .get(index)
                                        .map_or_else(|| format!("column_{index}"), |name| (*name).to_string())
                                },
                                str::to_string,
                            );
                            fields.push(field);
                            inputs.push(lower_expr(&arg.expr, cx, env));
                        }
                    }
                }
                let ret = call.resolved_ret.as_ref().expect("zip result resolved by sema");
                if inputs.is_empty() {
                    return crate::Codegen::TIR::lower_empty_zip_family(ret, &call.name);
                }
                let mut all = inputs.into_iter();
                let first = all.next().expect("non-empty zip inputs");
                return crate::Codegen::TIR::lower_zip_family(
                    first,
                    all.collect(),
                    fills,
                    fields,
                    &call.name,
                    Some(ret),
                );
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
            let args: Vec<TCallArg> = call
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
            cx.jit_generic_calls
                .borrow_mut()
                .entry(call.name.clone())
                .or_default()
                .push(
                    {
                        let mut shape: Vec<Type> = args
                            .iter()
                            .map(|arg| {
                                if arg.widen_to_vec {
                                    if let Type::FixedList { elem, .. } = &arg.value.ty {
                                        return Type::List(elem.clone());
                                    }
                                }
                                arg.value.ty.clone()
                            })
                            .collect();
                        shape.extend(call.type_args.iter().cloned());
                        shape
                    },
                );
            let ret = call_return_type_with_args(cx, &call.name, &call.type_args, &args);
            let lowered = TExpr {
                ty: ret,
                kind: TExprKind::Call {
                    name: cx.jit_local_call_prefix.as_ref().map_or_else(
                        || call.name.clone(),
                        |prefix| format!("{prefix}{}", mangle(&call.name)),
                    ),
                    type_args: call.type_args.clone(),
                    args,
                },
            };
            match source_arg_order(&call.args) {
                Some(order) => preserve_source_arg_order(lowered, &order, call.args.len(), call.name_span.start as u32),
                None => lowered,
            }
        }
        // c109 Phase 6: a method call. The gate (`method_call_in_subset`) admitted
        // exactly the synthetic `.clone()` or a user instance method on a covered
        // type; lower accordingly. Every dispatch fact is resolved here (totality).
        Expr::MethodCall { .. } => lower_method_chain(e, cx, env),
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
        // generic args), so the Rust head is `__jet_<name>` and field names mangle.
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
                .map(|trait_name| (trait_name.clone(), type_name.clone()));
            // c109 Phase 19: a FOREIGN (imported user) struct literal `alias.Type { … }`
            // (`import_ns`). The AST `emit_struct_lit` `import_ns` branch emits
            // `{root}{import_mods[alias]}::{mangle(Type)}[::<args>]` with MANGLED fields.
            // Resolve the head here (totality); a missing alias falls to `user_unknown`,
            // exactly as the AST path (the gate already required the alias to resolve).
            if let Some(alias) = import_ns {
                if cx
                    .core_import_module_for_function(&env.fn_name, alias)
                    == Some("core.encoding")
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
                            fields: tfields,
                            extra: None,
                            as_trait: None,
                        },
                    };
                }
                if cx
                    .core_import_module_for_function(&env.fn_name, alias)
                    == Some("core.encoding.cbor")
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
                            fields: tfields,
                            extra: None,
                            as_trait: None,
                        },
                    };
                }
                if cx
                    .core_import_module_for_function(&env.fn_name, alias)
                    == Some("core.encoding.xml")
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
                            fields: tfields,
                            extra: None,
                            as_trait: None,
                        },
                    };
                }
                if cx
                    .core_import_module_for_function(&env.fn_name, alias)
                    == Some(crate::Syntax::CORE_EMAIL_MODULE)
                    && matches!(type_name.as_str(), "RecipientReport" | "SendReport" | "Limits" | "DkimConfig" | "SMTPConfig")
                {
                    let tfields = fields
                        .iter()
                        .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                        .collect();
                    return TExpr {
                        ty: if type_args.is_empty() {
                            Type::Named(type_name.clone())
                        } else {
                            Type::Apply {
                                name: type_name.clone(),
                                args: type_args.clone(),
                            }
                        },
                        kind: TExprKind::StructLit {
                            fields: tfields,
                            extra: None,
                            as_trait: None,
                        },
                    };
                }
                let tfields = fields
                    .iter()
                    .map(|(n, _, fe)| (n.clone(), lower_expr(fe, cx, env), false))
                    .collect();
                return TExpr {
                    ty: if type_args.is_empty() {
                        Type::Named(type_name.clone())
                    } else {
                        Type::Apply {
                            name: type_name.clone(),
                            args: type_args.clone(),
                        }
                    },
                    kind: TExprKind::StructLit {
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            // c109 Phase 17: a PRELUDE struct literal (HTTPRequest/HTTPResponse).
            if net_handle_rust_type(type_name).is_some() {
                // A prelude struct has no boxed (recursive) edges.
                let mut tfields: Vec<(String, TExpr, bool)> = fields
                    .iter()
                    .map(|(n, _, fe)| (n.clone(), lower_expr(fe, cx, env), false))
                    .collect();
                let extra = if type_name == "HTTPRequest" {
                    Some(crate::Codegen::TIR::TStructExtra::HTTPRequestParams)
                } else {
                    None
                };
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        fields: tfields.drain(..).collect(),
                        extra,
                        as_trait: None,
                    },
                };
            }
            // D-TEXTWIDTH1=B: `TextWidth.{ ambiguous: .Wide, controls: .Reject }` —
            // a plain dot-ctor core struct, `jet_std::TextWidth` head, no injected
            // extra field (unlike HTTPRequest's `params`).
            if type_name == Syntax::TYPE_ERR {
                let tfields = fields
                    .iter()
                    .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                    .collect();
                return TExpr {
                    ty: Type::Named(Syntax::TYPE_ERR.to_string()),
                    kind: TExprKind::StructLit {
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            if matches!(
                type_name.as_str(),
                "TextWidth" | "TerminalSize" | "TerminalPolicy"
            ) {
                let tfields: Vec<(String, TExpr, bool)> = fields
                    .iter()
                    .map(|(n, _, fe)| (n.clone(), lower_expr(fe, cx, env), false))
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                        kind: TExprKind::StructLit {
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
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            if matches!(type_name.as_str(), "RecipientReport" | "SendReport" | "Limits" | "DkimConfig" | "SMTPConfig") {
                let tfields = fields
                    .iter()
                    .map(|(name, _, value)| (name.clone(), lower_expr(value, cx, env), false))
                    .collect();
                return TExpr {
                    ty: if type_args.is_empty() {
                        Type::Named(type_name.clone())
                    } else {
                        Type::Apply {
                            name: type_name.clone(),
                            args: type_args.clone(),
                        }
                    },
                    kind: TExprKind::StructLit {
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            // c109 Phase 19: a GENERIC struct literal carries `type_args` (`Pair<T> {…}`).
            // The Rust head is the turbofish `__jet_<Name>::<args>` (`user_type_apply_rust`),
            // resolved at lowering; fields mangle. A non-generic literal renders `__jet_<Name>`.
            // c109: an UNqualified FOREIGN struct (`Note { … }`, no `import_ns`) prefixes its
            // module head (`{root}__jet_<mod>::__jet_<Note>`), exactly as `user_type_apply_rust`
            // — or rustc can't find the type (E0422). A local struct keeps the plain head.
            // Struct head spelling comes from `TExpr.ty` at emit (`cx.rust_type`).
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
                        (fname.clone(), te, false)
                    })
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
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
                    let mut value = lower_owned_expr(fe, cx, env);
                    // D-UNIONTYPE1=A: member → union inject at Codable/struct field sites.
                    if let Some(fty) =
                        struct_field_type(cx, &Type::Named(type_name.clone()), n)
                    {
                        value = crate::Codegen::TIR::maybe_widen_expr_to_union(value, &fty);
                    }
                    (n.clone(), value, boxed)
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
            // D-LAYOUT-FACTS1=B: derive bodies bind their type parameter as a
            // comptime `TypeInfo` value, but fragment lowering has no ordinary
            // local type fact for that binding. Keep `$layout` on the field
            // path so `T.$layout` is not mistaken for an enum literal.
            let compiler_fact_receiver = match receiver.as_ref() {
                // A qualified type path lowers as a field chain. The final
                // segment is still the type name (`module.Packet`), while a
                // value path such as `info.layout` remains lowercase.
                Expr::Ident(name, _) | Expr::Field(_, name, _) => {
                    name.chars().next().is_some_and(char::is_uppercase)
                }
                _ => false,
            };
            if Syntax::compiler_fact_member(member).is_some() && compiler_fact_receiver {
                let recv = lower_expr(receiver, cx, env);
                return TExpr {
                    ty: compiler_fact_type(member),
                    kind: TExprKind::Field {
                        recv: Box::new(recv),
                        field: member.clone(),
                        boxed: false,
                    },
                };
            }
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
                            enum_type: enum_name.clone(),
                            variant: member.clone(),
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
                            enum_type: "DataEvent".to_string(),
                            variant: member.clone(),
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
                // D-LISTREMOVE1/F: `RemoveBy.Val` / `RemoveBy.Slot` is a
                // built-in enum with a registered type fact, so it does not
                // pass through the user-enum field path above.
                if enum_name == crate::Syntax::TYPE_REMOVE_BY
                    && matches!(member.as_str(), "Val" | "Slot")
                {
                    return TExpr {
                        ty: Type::Named(crate::Syntax::TYPE_REMOVE_BY.to_string()),
                        kind: TExprKind::EnumLit {
                            enum_type: crate::Syntax::TYPE_REMOVE_BY.to_string(),
                            variant: member.clone(),
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
                            enum_type: enum_name.clone(),
                            variant: member.clone(),
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
                if env.ty_of(enum_name).is_none()
                    && matches!(resolved_enum, "SMTPSecurity" | "RecipientPolicy" | "SMTPAuth" | "TLSTrust")
                    && ((resolved_enum == "SMTPSecurity" && matches!(member.as_str(), "StartTls" | "TLS"))
                        || (resolved_enum == "RecipientPolicy" && matches!(member.as_str(), "RequireAll" | "DeliverAccepted"))
                        || (resolved_enum == "SMTPAuth" && member == "None")
                        || (resolved_enum == "TLSTrust" && member == "System"))
                {
                    return TExpr {
                        ty: Type::Named(resolved_enum.to_string()),
                        kind: TExprKind::EnumLit {
                            enum_type: resolved_enum.to_string(),
                            variant: member.clone(),
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
                // Core net unit enums reach codegen as Field (`NetReadyInterest.Write`).
                if env.ty_of(enum_name).is_none()
                    && ((resolved_enum == "NetReadyInterest"
                        && matches!(member.as_str(), "Read" | "Write" | "ReadWrite"))
                        || (resolved_enum == "NetShutdown"
                            && matches!(member.as_str(), "Read" | "Write" | "Both")))
                {
                    return TExpr {
                        ty: Type::Named(resolved_enum.to_string()),
                        kind: TExprKind::EnumLit {
                            enum_type: resolved_enum.to_string(),
                            variant: member.clone(),
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
                if env.ty_of(enum_name).is_none()
                    && (cx.variant_owner.get(member).map(String::as_str) == Some(enum_name.as_str())
                        // Fragment eval (#722): REPL/comptime often lacks `variant_owner`
                        // on empty_cx; an unbound PascalCase type.Variant is a unit enum lit.
                        // Skip numeric bounds (`F32.MAX`) — those are HostCall::NumericBounds.
                        || (super::is_eval_fragment()
                            && enum_name
                                .chars()
                                .next()
                                .is_some_and(|c| c.is_ascii_uppercase())
                            && crate::AST::numeric_type_from_name(enum_name).is_none()
                            && !is_numeric_bounds_const(member)))
                {
                    // c109 Phase 24: a FOREIGN enum's unit literal (`NoteType.User` in
                    // search.jet) qualifies with the module path, exactly as `emit_expr`'s
                    // `Field` arm (Expression.rs ~L232): `{root}{mod}::user_<Enum>::<V>`.
                    // Keyed on the ENUM-name (`enum_name`, the receiver) in `cx.foreign_types`,
                    // NOT the variant — matching the AST byte-for-byte.
                    return TExpr {
                        ty: Type::Named(enum_name.clone()),
                        kind: TExprKind::EnumLit {
                            enum_type: enum_name.clone(),
                            variant: member.clone(),
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
                        kind: TExprKind::JSONLit {
                            variant: "Null".to_string(),
                            arg: None,
                        },
                    };
                }
                // D-DBDRIVER1: `DBValue.Null` — same no-arg-`Field` shape as `Data.Null`.
                if env.ty_of(enum_name).is_none()
                    && is_db_value_type_name(enum_name)
                    && member == "Null"
                {
                    return TExpr {
                        ty: Type::Named(Syntax::TYPE_DB_VALUE.to_string()),
                        kind: TExprKind::DBValueLit {
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
                                kind: TExprKind::HostCall(Box::new(
                                    crate::Codegen::TIR::THostCall::NumericBounds {
                                        ty: nt.clone(),
                                        member: member.to_string(),
                                    },
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
            if member == "value" {
                if let Type::Apply { name, args } = recv.ty.clone() {
                    if name == Syntax::TYPE_SHARED_GUARD && args.len() == 1 {
                        return TExpr {
                            ty: args[0].clone(),
                            kind: TExprKind::SharedGuardValue {
                                guard: Box::new(recv),
                                editable: false,
                            },
                        };
                    }
                }
                if let Type::Tagged { marker, inner } = recv.ty.clone() {
                    if matches!(
                        marker,
                        crate::AST::TagMarker::Internal(
                            crate::AST::InternalTag::SharedGuardRead
                                | crate::AST::InternalTag::SharedGuardEdit
                        )
                    ) {
                        if let Type::Apply { name, args } = inner.as_ref() {
                            if name == Syntax::TYPE_SHARED_GUARD && args.len() == 1 {
                                return TExpr {
                                    ty: args[0].clone(),
                                    kind: TExprKind::SharedGuardValue {
                                        guard: Box::new(recv),
                                        editable: matches!(
                                            marker,
                                            crate::AST::TagMarker::Internal(crate::AST::InternalTag::SharedGuardEdit)
                                        ),
                                    },
                                };
                            }
                        }
                    }
                }
            }
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
                        kind: TExprKind::MethodCall {
                            recv: Box::new(recv),
                            method: crate::Codegen::TIR::TMethodRef::inherent(member),
                            type_args: Vec::new(),
                            args: vec![],
                            source_first_string_literal: None,
                            operator_line: None,
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
            // A field of a CORE struct (`ProcessResult.code`, `JSONError.message`, …) is
            // emitted by its PLAIN Rust name, never `user_<name>` (the core structs in
            // Source/Prelude/Core.rs declare unprefixed fields — B2). Reproduce
            // `core_struct_field_rust_name` (Expression.rs) from the resolved receiver
            // type so the field read is byte-exact for both core and user structs.
            let field = member.to_string();
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
                    field,
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
                                    "EmailError"
                                        | "SMTPAuth"
                                        | "TLSTrust"
                                        | "AuthError"
                                        | "HTTPRedirectPolicy"
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
                kind: TExprKind::EnumLit {
                    enum_type: resolved_type.to_string(),
                    variant: variant.clone(),
                    payload,
                },
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
        // #779: map literals desugar to empty `MapLit` + `IndexAssign` inserts inside
        // an `IfExpr(true)` block. Engines keep only the empty-map constructor arm.
        Expr::MapLit(entries, _) => {
            let tentries: Vec<(TExpr, TExpr)> = entries
                .iter()
                .map(|(k, v)| (lower_expr(k, cx, env), lower_expr(v, cx, env)))
                .collect();
            let (kt, vt) = tentries
                .first()
                .map(|(k, v)| (k.ty.clone(), v.ty.clone()))
                .unwrap_or((Type::String, Type::Int));
            let map_ty = Type::Map {
                key: Box::new(kt),
                key_span: None,
                value: Box::new(vt),
            };
            if tentries.is_empty() {
                return TExpr {
                    ty: map_ty,
                    kind: TExprKind::MapLit(Vec::new()),
                };
            }
            let map_name = "__jet_m";
            let empty = TExpr {
                ty: map_ty.clone(),
                kind: TExprKind::MapLit(Vec::new()),
            };
            let mut then_body = vec![crate::Codegen::TIR::TStmt::Let {
                name: map_name.to_string(),
                kw: "let mut",
                let_ty: crate::Codegen::TIR::TLetTy::Inferred,
                init: empty,
                gc_promotion: None,
                gc_transferred: false,
            }];
            for (k, v) in tentries {
                then_body.push(crate::Codegen::TIR::TStmt::IndexAssign {
                    uninit: false,
                    base: TExpr {
                        ty: map_ty.clone(),
                        kind: TExprKind::Local(TLocal::user(map_name)),
                    },
                    index: k,
                    is_map: true,
                    value: v,
                });
            }
            let result = TExpr {
                ty: map_ty.clone(),
                kind: TExprKind::Local(TLocal::user(map_name)),
            };
            TExpr {
                ty: map_ty.clone(),
                kind: TExprKind::IfExpr {
                    cond: Box::new(crate::Codegen::TIR::TIfCond::Plain(TExpr {
                        ty: Type::Bool,
                        kind: TExprKind::BoolLit(true),
                    })),
                    then_body,
                    then_value: Box::new(result),
                    else_body: Vec::new(),
                    else_value: Box::new(TExpr {
                        ty: map_ty,
                        kind: TExprKind::MapLit(Vec::new()),
                    }),
                },
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
            // D-LAYOUT-FACTS1=B: the parser stores `[.field]` as an internal
            // selector identifier. Project it through the same `Field` TIR
            // node used by every other comptime struct read; the evaluator
            // resolves the selected `LayoutField` from `LayoutInfo.fields`.
            if let Expr::Ident(name, _) = index.as_ref() {
                if let Some(field_name) = Syntax::layout_selector_name(name) {
                    let base_t = lower_expr(base, cx, env);
                    return TExpr {
                        ty: Type::Named(Syntax::TYPE_LAYOUT_FIELD.to_string()),
                        kind: TExprKind::Field {
                            recv: Box::new(base_t),
                            field: format!(
                                "{}{}",
                                Syntax::LAYOUT_FIELD_PROJECTION_PREFIX,
                                field_name
                            ),
                            boxed: false,
                        },
                    };
                }
            }
            // Sema's IndexKind::Unknown violates the handoff invariant. Catch it
            // in debug builds; release builds retain the list fallback so an
            // interpreter path remains total if an unresolved kind leaks through.
            debug_assert!(
                !matches!(kind, IndexKind::Unknown),
                "sema-to-TIR handoff violated"
            );
            let kind = if matches!(kind, IndexKind::Unknown) {
                &IndexKind::List
            } else {
                kind
            };
            let base_t = lower_expr(base, cx, env);
            let index_t = lower_expr(index, cx, env);
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            if matches!(kind, IndexKind::Range) {
                let zero = || TExpr {
                    ty: Type::Int,
                    kind: TExprKind::IntLit(0, None),
                };
                return TExpr {
                    ty: base_t.ty.clone(),
                    kind: TExprKind::Slice {
                        base: Box::new(base_t),
                        start: Box::new(zero()),
                        end: Box::new(zero()),
                        range: Some(Box::new(index_t)),
                        line,
                    },
                };
            }
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
            // `SQL.raw`/`.context` escapes in `lower_method_call` below.
            if matches!(kind, IndexKind::Pool) {
                let elem_ty = match &base_t.ty {
                    Type::Apply { name, args } if name == "Pool" && !args.is_empty() => {
                        args[0].clone()
                    }
                    _ => Type::Int,
                };
                return TExpr {
                    ty: elem_ty,
                    kind: TExprKind::PoolSlot {
                        pool: Box::new(base_t),
                        id: Box::new(index_t),
                        mutable: false,
                        field: None,
                        line,
                    },
                };
            }
            if matches!(kind, IndexKind::FixedListProof) {
                let elem_ty = match &base_t.ty {
                    Type::FixedList { elem, .. } => (**elem).clone(),
                    _ => Type::Int,
                };
                return TExpr {
                    ty: elem_ty,
                    kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::FixedListIndex {
                        base: Box::new(base_t),
                        index: Box::new(index_t),
                    })),
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
                    uninit_fixed: matches!(
                        base.as_ref(),
                        Expr::Ident(name, _) if env.is_uninit_fixed(name)
                    ),
                    line,
                },
            }
        }
        // Owned slicing lowers here. Place contexts are handled above and use
        // ViewNew/ViewMutNew, preserving the owner's storage.
        Expr::Slice {
            base,
            start,
            end,
            range,
            span,
        } => {
            let base_t = lower_expr(base, cx, env);
            let start_t = lower_expr(start, cx, env);
            let end_t = lower_expr(end, cx, env);
            let range_t = range.as_ref().map(|range| Box::new(lower_expr(range, cx, env)));
            let result_ty = base_t.ty.clone();
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: result_ty,
                kind: TExprKind::Slice {
                    base: Box::new(base_t),
                    start: Box::new(start_t),
                    end: Box::new(end_t),
                    range: range_t,
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
        // Card #1440: the dead end of an else-less exhaustive dispatch. Sema
        // proved coverage (E0307); like Todo, the result `ty` is never
        // load-bearing — the node diverges on every tier.
        Expr::NoElse(span) => {
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: Type::Named("Unit".to_string()),
                kind: TExprKind::Unreachable { line },
            }
        }
        // c109 Phase 8: `Ok(x)` → `Ok(x)`. The result is a `Result` whose ok type is
        // the inner's; the err type is unresolved here (Rust infers it from the
        // function return context, exactly as the AST path's bare `Ok(x)` does).
        Expr::Ok(inner, _) => {
            let mut t = lower_expr(inner, cx, env);
            if let Some(Type::Result { ok, .. }) = &env.ret_ty {
                t = crate::Codegen::TIR::maybe_widen_expr_to_union(t, ok);
            }
            TExpr {
                ty: Type::Result {
                    ok: Box::new(t.ty.clone()),
                    err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
                },
                kind: TExprKind::Ok(Box::new(t)),
            }
        }
        // c109 Phase 8: `Err(e)` → `Err(e)`. The err type is the inner's; the ok type
        // is unresolved here (inferred from the function return context).
        Expr::Err(inner, _) => {
            let mut t = lower_expr(inner, cx, env);
            if let Some(Type::Result { err, .. }) = &env.ret_ty {
                t = crate::Codegen::TIR::maybe_widen_expr_to_union(t, err);
            }
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
                TryConvert::DefaultErr => TTryConvert::DefaultErr,
                TryConvert::Fallible => TTryConvert::Fallible,
                TryConvert::Typed(fn_name) => TTryConvert::Typed(fn_name.clone()),
                TryConvert::WidenUnion { enum_name, tag } => TTryConvert::WidenUnion {
                    enum_name: enum_name.clone(),
                    tag: tag.clone(),
                },
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
        // c109 Phase 8: the `??` fallback operator. D-FAIL-CARRIER1=A: one carrier,
        // so the value type alone gives the payload type. Mirrors `emit_or_fallback`.
        Expr::OrFallback {
            value, fallback, ..
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
                    let (kind, loc) = lower_panic_stop(name_span, args, cx, env);
                    let TRequireKind::Panic { msg } = kind else { unreachable!() };
                    TOrFallback::Panic { msg, loc }
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
                    member: member.to_string(),
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
            let params = tl.param_types.clone();
            let ret = tl.ret.clone().map(Box::new);
            TExpr {
                ty: Type::Fn {
                    params,
                    ret,
                    effect_bound: None,
                    param_contract: None,
                    return_view_provenance: lam.meta.return_view_provenance.clone(),
                },
                kind: TExprKind::Lambda(Box::new(tl)),
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
                    elem: elem.clone(),
                    addr: Box::new(taddr),
                },
            }
        }
        // Comptime/TirBridge can evaluate function bodies before sema elaborates
        // `Type.{ … }` (eval_comptime_items runs early). Mirror elaborate_typed_lit.
        Expr::TypedLit { head, body, span } => {
            let Some(head) = head.clone() else {
                return TExpr {
                    ty: Type::Int,
                    kind: TExprKind::Todo {
                        line: 0,
                        expected_type: "inferred typed literal without head".into(),
                    },
                };
            };
            if let Type::Named(type_name) = &head {
                if let Some(lowered) = lower_boundary_typed_lit(type_name, body, cx, env) {
                    return lowered;
                }
            }
            if head == Type::Named(Syntax::TYPE_REGEX.to_string()) {
                if let TypedLitBody::Value(pattern) = body {
                    return TExpr {
                        ty: head,
                        kind: TExprKind::CoreCall {
                            module: "core.regex".to_string(),
                            method: "literal".to_string(),
                            args: vec![lower_expr(pattern, cx, env)],
                            source_span: *span,
                            widen_to_vec: vec![false],
                        },
                    };
                }
            }
            let rewritten = match (head.clone(), body.clone()) {
                (Type::List(_) | Type::FixedList { .. }, TypedLitBody::Empty) => {
                    Expr::ListLit(Vec::new(), *span)
                }
                (Type::List(_) | Type::FixedList { .. }, TypedLitBody::Elements(elems)) => {
                    Expr::ListLit(elems, *span)
                }
                (Type::List(_) | Type::FixedList { .. }, TypedLitBody::Value(inner)) => {
                    Expr::ListLit(vec![*inner], *span)
                }
                (Type::Map { .. }, TypedLitBody::Empty) => Expr::MapLit(Vec::new(), *span),
                (Type::Map { .. }, TypedLitBody::Entries(entries)) => {
                    Expr::MapLit(entries, *span)
                }
                (Type::Named(name), TypedLitBody::Fields(fields)) => Expr::StructLit {
                    type_name: name,
                    type_args: Vec::new(),
                    import_ns: None,
                    as_trait: None,
                    fields,
                    inferred: false,
                    span: *span,
                },
                (Type::Apply { name, args }, TypedLitBody::Fields(fields)) => Expr::StructLit {
                    type_name: name,
                    type_args: args,
                    import_ns: None,
                    as_trait: None,
                    fields,
                    inferred: false,
                    span: *span,
                },
                (Type::Named(name), TypedLitBody::Empty) => Expr::StructLit {
                    type_name: name,
                    type_args: Vec::new(),
                    import_ns: None,
                    as_trait: None,
                    fields: Vec::new(),
                    inferred: false,
                    span: *span,
                },
                (Type::Apply { name, args }, TypedLitBody::Empty) => Expr::StructLit {
                    type_name: name,
                    type_args: args,
                    import_ns: None,
                    as_trait: None,
                    fields: Vec::new(),
                    inferred: false,
                    span: *span,
                },
                (_, TypedLitBody::Value(inner)) => {
                    // Scalar `U8.{ 13 }` / `F32.{ -0.0 }` — lower the value, then
                    // retag with head width (including nested float operands so
                    // unary/binary keep F32/F64 lanes matched).
                    let mut t = lower_expr(&inner, cx, env);
                    retag_numeric_width(&mut t, &head);
                    return t;
                }
                (_, TypedLitBody::Elements(elems)) if elems.len() == 1 => {
                    let mut t = lower_expr(&elems[0], cx, env);
                    retag_numeric_width(&mut t, &head);
                    return t;
                }
                _ => {
                    return TExpr {
                        ty: Type::Int,
                        kind: TExprKind::Todo {
                            line: 0,
                            expected_type: format!(
                                "typed literal body vs head `{}`",
                                head.name()
                            ),
                        },
                    };
                }
            };
            let mut t = lower_expr(&rewritten, cx, env);
            // Prefer the typed head when the rewritten form under-specifies (empty list/map).
            if matches!(
                head,
                Type::List(_)
                    | Type::FixedList { .. }
                    | Type::Map { .. }
                    | Type::Named(_)
                    | Type::Apply { .. }
                    | Type::Int
                    | Type::IntN { .. }
                    | Type::Float
                    | Type::Float32
            ) {
                t.ty = head.clone();
            }
            // D-SG9: retag list-element IntLits from a `[U8]`/`[I32]`/… head so
            // emit uses the right Rust suffix even if sema left width unset.
            if let Type::List(elem) | Type::FixedList { elem, .. } = &head {
                if let Type::IntN { signed, bits } = elem.as_ref() {
                    if let TExprKind::ListLit(elems) = &mut t.kind {
                        for el in elems.iter_mut() {
                            if let TExprKind::IntLit(_, width) = &mut el.kind {
                                *width = Some((*signed, *bits));
                                el.ty = elem.as_ref().clone();
                            }
                        }
                    }
                }
            }
            t
        },
        Expr::PatternTest {
            subject, pattern, ..
        } if is_binding_free_user_variant_pattern_test(pattern, cx) => {
            lower_binding_free_variant_pattern_test(subject, pattern, cx, env)
        }
        // Subset/gate drift: refuse with Todo so the interpreter returns E0956
        // instead of aborting the process (I2: never panic on user programs).
        other => TExpr {
            ty: Type::Int,
            kind: TExprKind::Todo {
                line: 0,
                expected_type: format!("expression outside TIR subset: {}", expr_tag(other)),
            },
        },
    }
}

/// Retag a lowered scalar typed-literal body with the head's numeric width.
/// Nested float unary/binary operands inherit the same width so TirBridge
/// doesn't mix F32/F64 in `F32.{ -0.0 }` / `F32.{ max + max }`.
fn retag_numeric_width(expr: &mut TExpr, head: &Type) {
    expr.ty = head.clone();
    if !matches!(head, Type::Float | Type::Float32) {
        return;
    }
    match &mut expr.kind {
        TExprKind::Unary { operand, .. } => retag_numeric_width(operand, head),
        TExprKind::Binary { lhs, rhs, .. } => {
            retag_numeric_width(lhs, head);
            retag_numeric_width(rhs, head);
        }
        TExprKind::Clone(inner) | TExprKind::MaterializeView(inner) => {
            retag_numeric_width(inner, head)
        }
        _ => {}
    }
}

/// Lower an expression whose result is stored or returned as an owned value.
/// A `Read`/`Write` non-scalar parameter is represented by a dereferenced Rust
/// borrow; moving that place would leak E0507 from rustc. Jet generic functions
/// record the clone's type so generic emission adds the required bound, then
/// materialize the owned value at this semantic boundary.
pub(crate) fn lower_owned_expr(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    fn reads_borrowed_place(e: &Expr, env: &LowerEnv) -> bool {
        match e {
            Expr::Ident(name, _) => env.is_borrowed(name),
            Expr::Field(base, _, _) | Expr::Index { base, .. } => {
                reads_borrowed_place(base, env)
            }
            Expr::Paren(inner, _) => reads_borrowed_place(inner, env),
            _ => false,
        }
    }

    let lowered = lower_expr(e, cx, env);
    if matches!(e, Expr::Ident(name, _) if env.is_resource(name)) {
        let Expr::Ident(name, _) = e else { unreachable!() };
        TExpr {
            ty: lowered.ty,
            kind: TExprKind::ResourceTake(env.rust_name_of(name)),
        }
    } else if reads_borrowed_place(e, env) && !lowered.ty.is_scalar() {
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

/// D-APILABEL1=A: keep the ratified evaluation order across a label reorder.
///
/// The binder rewrote the argument list into declaration order, so lowering
/// it straight through would run the supplied expressions in declaration
/// order too. `order` lists the argument slots in the order the caller wrote
/// them; each is evaluated into a temporary first, and the call then reads
/// the temporaries in declaration order.
///
/// The result is an ordinary `InlineBlock`, so AOT emit, the interpreter, and
/// the JIT all keep the same meaning without an engine-specific rule.
/// `order` lists the *lowered* argument slots in the order the caller wrote
/// them, taken from each source argument's `flags.source_index`. Slots the
/// caller did not write (a filled default) are absent — a default runs after
/// every supplied argument anyway, in the declaration order the rewritten list
/// already has.
pub(crate) fn source_arg_order(args: &[crate::AST::CallArg]) -> Option<Vec<usize>> {
    if !args.iter().any(|arg| arg.flags.source_index.is_some()) {
        return None;
    }
    let mut slots: Vec<usize> = (0..args.len())
        .filter(|slot| args[*slot].flags.source_index.is_some())
        .collect();
    slots.sort_by_key(|slot| args[*slot].flags.source_index);
    Some(slots)
}

pub(crate) fn preserve_source_arg_order(
    mut call: TExpr,
    order: &[usize],
    ast_arg_count: usize,
    site: u32,
) -> TExpr {
    let mut stmts = match &mut call.kind {
        TExprKind::Call { args, .. }
        | TExprKind::MethodCall { args, .. }
        | TExprKind::FnFieldCall { args, .. }
        | TExprKind::StaticCall { args, .. }
        | TExprKind::ModuleCall { args, .. }
        | TExprKind::FnValue {
            kind: crate::Codegen::TIR::TFnValueKind::Call { args, .. },
        } => bind_arg_temporaries(args, order, ast_arg_count, site),
        TExprKind::CoreCall { args, .. } => {
            bind_arg_temporaries(args, order, ast_arg_count, site)
        }
        TExprKind::ExternCall { args, .. } => {
            bind_arg_temporaries(args, order, ast_arg_count, site)
        }
        _ => return call,
    };
    if stmts.is_empty() {
        return call;
    }
    let ty = call.ty.clone();
    stmts.push(TStmt::ExprStmt(call));
    TExpr {
        ty,
        kind: TExprKind::InlineBlock(stmts),
    }
}

/// D-APILABEL1=A: can evaluating this argument be *observed*? A place read can:
/// an earlier supplied call may mutate the place before this argument reads it.
/// Only values independent of runtime state may stay in declaration order.
///
/// Conservative: anything not recognised here is assumed to have an effect.
fn effect_free(e: &TExpr) -> bool {
    match &e.kind {
        TExprKind::IntLit(..)
        | TExprKind::FloatLit(_)
        | TExprKind::BoolLit(_)
        | TExprKind::CharLit(_)
        | TExprKind::Unit
        | TExprKind::DefaultLit
        | TExprKind::CtLit(_)
        | TExprKind::ConstRef(_) => true,
        TExprKind::StrLit(parts) => parts.iter().all(|part| match part {
            crate::Codegen::TIR::TStrPart::Lit(_) => true,
            crate::Codegen::TIR::TStrPart::Interp(inner, ..) => effect_free(inner),
        }),
        // Arithmetic can panic (division by zero, overflow, or negating the
        // minimum integer). Keep it in written order rather than trying to
        // duplicate sema's operator/type proof here.
        TExprKind::Unary { .. } | TExprKind::Binary { .. } => false,
        _ => false,
    }
}

trait OrderedArg {
    fn value(&self) -> &TExpr;
    /// A borrowed place must remain a place: moving it into a temporary changes
    /// ownership, while sema rejects any neighboring access that could make the
    /// borrow's timing observable. Other arguments can be pinned.
    fn can_bind(&self) -> bool {
        true
    }
    fn take_for_binding(&mut self, replacement: TExpr) -> TExpr;
}

impl OrderedArg for crate::Codegen::TIR::TCallArg {
    fn value(&self) -> &TExpr {
        &self.value
    }

    fn can_bind(&self) -> bool {
        !self.mut_borrow && (!self.borrow || self.clone || self.arc_clone)
    }

    fn take_for_binding(&mut self, replacement: TExpr) -> TExpr {
        let mut value = std::mem::replace(&mut self.value, replacement);
        // Cloning is part of evaluating the supplied expression, so perform it
        // in source order rather than leaving the wrapper on the later call.
        if self.clone || self.arc_clone {
            value = TExpr {
                ty: value.ty.clone(),
                kind: TExprKind::Clone(Box::new(value)),
            };
            self.clone = false;
            self.arc_clone = false;
        }
        value
    }
}

impl OrderedArg for crate::Codegen::TIR::TExternArg {
    fn value(&self) -> &TExpr {
        &self.value
    }

    fn take_for_binding(&mut self, replacement: TExpr) -> TExpr {
        let mut value = std::mem::replace(&mut self.value, replacement);
        if self.clone {
            value = TExpr {
                ty: value.ty.clone(),
                kind: TExprKind::Clone(Box::new(value)),
            };
            self.clone = false;
        }
        value
    }
}

impl OrderedArg for TExpr {
    fn value(&self) -> &TExpr {
        self
    }

    fn can_bind(&self) -> bool {
        // Raw Core args do not carry the signature's Read/Move convention.
        // A scalar place is Copy; a computed owning value is safe to move into
        // the source-order temporary. Keep non-scalar places in the call so a
        // later Core emit borrow cannot turn the temporary into an accidental
        // move. Sema rejects a neighboring mutation that would make that Read
        // place's exact borrow instant observable.
        self.ty.is_scalar()
            || matches!(
                &self.kind,
                TExprKind::Call { .. }
                    | TExprKind::MethodCall { .. }
                    | TExprKind::FnFieldCall { .. }
                    | TExprKind::StaticCall { .. }
                    | TExprKind::ModuleCall { .. }
                    | TExprKind::FnValue { .. }
                    | TExprKind::CoreCall { .. }
                    | TExprKind::ExternCall { .. }
                    | TExprKind::HostCall(_)
                    | TExprKind::InlineBlock(_)
                    | TExprKind::Clone(_)
                    | TExprKind::StrLit(_)
            )
    }

    fn take_for_binding(&mut self, replacement: TExpr) -> TExpr {
        std::mem::replace(self, replacement)
    }
}

/// Replace each listed argument with a read of a fresh temporary, and return
/// the `let` statements that bind them — emitted in `order`, which is source
/// order, not declaration order.
fn bind_arg_temporaries<A: OrderedArg>(
    args: &mut [A],
    order: &[usize],
    ast_arg_count: usize,
    site: u32,
) -> Vec<TStmt> {
    // A `#Root` dot call (D-CALLDUAL1=E) lowers its receiver into slot 0 of the
    // TIR argument list, while `order` was computed over the AST list the
    // receiver was stripped from. Recover the offset from the two lengths.
    let offset = args.len().saturating_sub(ast_arg_count);
    let order: Vec<usize> = order.iter().map(|slot| slot + offset).collect();
    // Only arguments that can actually be observed need pinning down. Count
    // them across the whole list, not just the written ones: a filled default
    // with an effect also has to run after every supplied argument, and it
    // sits in the call rather than in `order`. With fewer than two, nothing can
    // be observed out of order and the call stays a plain call.
    let observable_total = args.iter().filter(|arg| !effect_free(arg.value())).count();
    if observable_total < 2 {
        return Vec::new();
    }
    let observable: Vec<usize> = order
        .iter()
        .copied()
        .filter(|slot| {
            args.get(*slot)
                .is_some_and(|arg| arg.can_bind() && !effect_free(arg.value()))
        })
        .collect();
    if observable.is_empty() {
        return Vec::new();
    }
    let mut stmts: Vec<TStmt> = Vec::with_capacity(observable.len() + 1);
    for (step, slot) in observable.iter().enumerate() {
        let Some(arg) = args.get_mut(*slot) else {
            continue;
        };
        // The name has to be unique across nesting: a nested reordered call is
        // lowered as the initialiser of one of these very temporaries, and the
        // interpreter shares one scope with it. `site` is the call's source
        // offset, so two calls can never collide and the name stays stable
        // across runs.
        let temp = format!("__jet_arg{site}_{step}");
        let ty = arg.value().ty.clone();
        let bound = arg.take_for_binding(TExpr {
            ty: ty.clone(),
            // A `user` slot, not `generated`: `TStmt::Let` spells its name
            // through `mangle`, and only `TLocal::user` reads it back the
            // same way. The name itself is unspellable in Jet source.
            kind: TExprKind::Local(TLocal::user(&temp)),
        });
        stmts.push(TStmt::Let {
            name: temp,
            kw: "let",
            // Keep the raw expression's Rust type here. Call-boundary
            // conversions (Fn boxing, fixed-list widening, union injection,
            // and borrows) still belong to the original argument wrapper,
            // which now reads this temporary.
            let_ty: crate::Codegen::TIR::TLetTy::inferred(),
            init: bound,
            gc_promotion: None,
            gc_transferred: false,
        });
    }
    stmts
}

/// D-LAYOUT-FACTS1=B / D-META-STAGE1=B: the type a compiler fact answers.
///
/// Each fact projects one `TypeInfo` member, so its type is that member's type
/// and the three facts stay one spelling with one table behind them.
fn compiler_fact_type(fact: &str) -> Type {
    match fact {
        f if f == Syntax::COMPILER_FACT_NAME => Type::String,
        f if f == Syntax::COMPILER_FACT_FIELDS => {
            Type::List(Box::new(Type::Named("FieldInfo".to_string())))
        }
        _ => Type::Named(Syntax::TYPE_LAYOUT_INFO.to_string()),
    }
}

#[cfg(test)]
mod source_order_tests {
    use super::*;
    use crate::AST::BinOp;
    use crate::Codegen::TIR::TExternArg;

    fn int(value: i64) -> TExpr {
        TExpr {
            ty: Type::Int,
            kind: TExprKind::IntLit(value, None),
        }
    }

    fn division() -> TExpr {
        TExpr {
            ty: Type::Int,
            kind: TExprKind::Binary {
                op: BinOp::Div,
                overflow: true,
                line: 1,
                lhs: Box::new(int(1)),
                rhs: Box::new(int(0)),
            },
        }
    }

    fn print() -> TExpr {
        TExpr {
            ty: unit_type(),
            kind: TExprKind::Print(Box::new(int(1))),
        }
    }

    fn bump() -> TExpr {
        TExpr {
            ty: Type::Int,
            kind: TExprKind::Call {
                name: "bump".to_string(),
                type_args: Vec::new(),
                args: Vec::new(),
            },
        }
    }

    fn assert_division_then_print(lowered: TExpr) {
        let TExprKind::InlineBlock(stmts) = lowered.kind else {
            panic!("reordered call needs argument temporaries");
        };
        assert!(matches!(
            &stmts[0],
            TStmt::Let { init: TExpr { kind: TExprKind::Binary { op: BinOp::Div, .. }, .. }, .. }
        ));
        assert!(matches!(
            &stmts[1],
            TStmt::Let { init: TExpr { kind: TExprKind::Print(_), .. }, .. }
        ));
    }

    #[test]
    fn core_call_keeps_panicking_arithmetic_in_written_order() {
        let call = TExpr {
            ty: unit_type(),
            kind: TExprKind::CoreCall {
                module: "core.test".to_string(),
                method: "ordered".to_string(),
                args: vec![print(), division()],
                source_span: Span::new(0, 1),
                // The first source expression sits in slot 1 and widens at the
                // call site. Its raw value must still be evaluated first.
                widen_to_vec: vec![false, true],
            },
        };
        assert_division_then_print(preserve_source_arg_order(call, &[1, 0], 2, 7));
    }

    #[test]
    fn extern_call_uses_the_same_written_order_wrapper() {
        let call = TExpr {
            ty: unit_type(),
            kind: TExprKind::ExternCall {
                wrapper: "ordered".to_string(),
                args: vec![
                    TExternArg { value: print(), clone: false },
                    TExternArg { value: division(), clone: false },
                ],
            },
        };
        assert_division_then_print(preserve_source_arg_order(call, &[1, 0], 2, 9));
    }

    #[test]
    fn core_call_pins_a_local_read_after_an_earlier_call() {
        let call = TExpr {
            ty: unit_type(),
            kind: TExprKind::CoreCall {
                module: "core.test".to_string(),
                method: "ordered".to_string(),
                args: vec![
                    TExpr {
                        ty: Type::Int,
                        kind: TExprKind::Local(TLocal::user("x")),
                    },
                    bump(),
                ],
                source_span: Span::new(0, 1),
                widen_to_vec: vec![false, false],
            },
        };
        let lowered = preserve_source_arg_order(call, &[1, 0], 2, 11);
        let TExprKind::InlineBlock(stmts) = lowered.kind else {
            panic!("reordered call needs argument temporaries");
        };
        assert!(matches!(
            &stmts[0],
            TStmt::Let { init: TExpr { kind: TExprKind::Call { name, .. }, .. }, .. }
                if name == "bump"
        ));
        assert!(matches!(
            &stmts[1],
            TStmt::Let { init: TExpr { kind: TExprKind::Local(_), .. }, .. }
        ));
    }

    #[test]
    fn core_call_does_not_move_a_read_borrowed_string_place() {
        let call = TExpr {
            ty: unit_type(),
            kind: TExprKind::CoreCall {
                module: "core.test".to_string(),
                method: "ordered".to_string(),
                args: vec![
                    TExpr {
                        ty: Type::String,
                        kind: TExprKind::Local(TLocal::user("key")),
                    },
                    bump(),
                ],
                source_span: Span::new(0, 1),
                widen_to_vec: vec![false, false],
            },
        };
        let lowered = preserve_source_arg_order(call, &[1, 0], 2, 13);
        let TExprKind::InlineBlock(stmts) = lowered.kind else {
            panic!("earlier call still needs an argument temporary");
        };
        assert_eq!(stmts.len(), 2);
        let TStmt::ExprStmt(TExpr {
            kind: TExprKind::CoreCall { args, .. },
            ..
        }) = &stmts[1]
        else {
            panic!("last statement must remain the Core call");
        };
        assert!(matches!(&args[0].kind, TExprKind::Local(local) if local.name == "key"));
    }
}
