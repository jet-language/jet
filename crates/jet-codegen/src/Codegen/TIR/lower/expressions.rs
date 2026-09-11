use crate::jet_generated_format as jet_format;
use crate::Codegen::escape_rust_str;
use crate::Codegen::is_db_value_type_name;
use crate::Codegen::is_json_type_name;
use crate::Codegen::mangle;
use crate::Codegen::mangle_generated;
use crate::Codegen::net_handle_rust_type;
use crate::Codegen::tuple_fields_plain;
use crate::Codegen::tuple_struct_name;
use crate::Codegen::Cx;
use crate::Codegen::TIR::ambient_err_local;
use crate::Codegen::TIR::ast_operand_is_integer;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::data_plan_for_core_call;
use crate::Codegen::TIR::extern_call_return_type;
use crate::Codegen::TIR::imported_module_call_target_return;
use crate::Codegen::TIR::int_lit_type;
use crate::Codegen::TIR::is_numeric_bounds_const;
use crate::Codegen::TIR::lower::contract_expr_proven;
use crate::Codegen::TIR::lower::core_module_path_from_receiver;
use crate::Codegen::TIR::lower::is_binding_free_user_variant_pattern_test;
use crate::Codegen::TIR::lower::lower_binding_free_variant_pattern_test;
use crate::Codegen::TIR::lower::lower_comptime_scalar;
use crate::Codegen::TIR::lower::lower_incdec_place;
use crate::Codegen::TIR::lower_enum_arg;
use crate::Codegen::TIR::lower_extern_call_arg;
use crate::Codegen::TIR::lower_lambda;
use crate::Codegen::TIR::lower_method_call_with_sig;
use crate::Codegen::TIR::lower_one_call_arg;
use crate::Codegen::TIR::lower_panic_stop;
use crate::Codegen::TIR::lower_require_eq_stop;
use crate::Codegen::TIR::lower_require_stop;
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::module_call_source_return_type_with_args;
use crate::Codegen::TIR::preserve_typed_list_shape;
use crate::Codegen::TIR::struct_field_type;
use crate::Codegen::TIR::tir_address_lifetime;
use crate::Codegen::TIR::unit_type;
use crate::Codegen::TIR::ListSpreadPart;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::TAddressLifetime;
use crate::Codegen::TIR::TBuiltinOp;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TCoreClosureKind;
use crate::Codegen::TIR::TEnumPayload;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TExternArg;
use crate::Codegen::TIR::TFailureCarrier;
use crate::Codegen::TIR::TFnValueKind;
use crate::Codegen::TIR::THostCall;
use crate::Codegen::TIR::TIfCond;
use crate::Codegen::TIR::TLambda;
use crate::Codegen::TIR::TLambdaBody;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TMethodRef;
use crate::Codegen::TIR::TModuleCallForm;
use crate::Codegen::TIR::TNumericOp;
use crate::Codegen::TIR::TOptionProbe;
use crate::Codegen::TIR::TOrFallback;
use crate::Codegen::TIR::TPreludeArg;
use crate::Codegen::TIR::TRequireKind;
use crate::Codegen::TIR::TStaticOwner;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::TIR::TStrPart;
use crate::Codegen::TIR::TTryConvert;
use crate::Codegen::TIR::TirWorklist;
use crate::Codegen::TIR::{
    call_return_type, call_return_type_with_args, demand_generic_free_function, THandleOp,
};
use crate::Codegen::TIR::{TContract, TContractDisposition, TContractKind};
use crate::Diagnostics::Span;
use crate::Syntax;
use crate::AST::{
    AccessConvention, BinOp, Call, CallArg, ContractClause, CtValue, EnumLitArg, Expr, IndexKind,
    OrFallback, Pattern, Stmt, StrPart, TryConvert, Type, TypedLitBody, UnOp,
};
use jet_foundation::CanonicalPass;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
/// Run one branch body in its own stack frame.
///
/// At `opt-level=0` rustc emits no `llvm.lifetime` markers, so LLVM's
/// StackColoring pass never runs and one giant `match`/gate chain reserves the
/// SUM of every branch's locals in a single frame instead of the maximum.
/// `lower_expr` -> `lower_expr_inner` -> `lower_method_call_impl` ->
/// `lower_expr` is a recursive cycle, so that sum is multiplied by source
/// nesting depth and a debug-built test binary overflowed libtest's 2 MiB
/// worker stack after two levels. Wrapping a branch body here keeps its locals
/// in the closure's own frame, so one nesting level costs the ONE branch it
/// takes rather than all of them.
///
/// `#[inline(never)]` keeps the call boundary at every optimization level; at
/// `opt-level=0`, where the overflow lives, the closure is a plain call.
#[inline(never)]
pub(crate) fn in_own_frame<R>(body: impl FnOnce() -> R) -> R {
    body()
}

/// D-PLACE1: sema checks an `Atomic<T>` field initializer as `T`; construct
/// the private carrier only after that checked scalar reaches TIR.
fn lower_atomic_initializer(value: TExpr, field_ty: &Type) -> TExpr {
    let Type::Apply { name, args } = field_ty else {
        return value;
    };
    let [inner] = args.as_slice() else {
        return value;
    };
    if name != "Atomic" || !jet_foundation::Layout::atomic_scalar_type(inner) {
        return value;
    }
    TExpr {
        ty: field_ty.clone(),
        kind: TExprKind::StaticCall {
            owner: TStaticOwner::Prelude {
                rooted: true,
                path: "JetAtomic".to_string(),
                generics: vec![TPreludeArg::Jet(inner.clone())],
            },
            owner_type: None,
            method: TMethodRef::bare("new"),
            type_args: Vec::new(),
            args: vec![TCallArg {
                value,
                template_items: None,
                borrow: false,
                mut_borrow: false,
                clone: false,
                arc_clone: false,
                fn_coerce: None,
                widen_to_vec: false,
                widen_to_union: None,
                box_as_trait: None,
            }],
        },
    }
}
/// A propagated call owns the only ABI-carrier projection. Argument-order
/// preservation and foreign undo registration may put the call in a terminal
/// `InlineBlock`, but the surrounding `Try` still performs the projection.
/// Clear the module-call adapter in that shape so it cannot emit a second `?`.
pub(super) fn suppress_module_call_target_return(expr: &mut TExpr) {
    let next_ty = match &mut expr.kind {
        TExprKind::ModuleCall { target_return, .. } => target_return.take(),
        TExprKind::InlineBlock(stmts) => {
            if let Some(TStmt::ExprStmt(tail)) = stmts.last_mut() {
                suppress_module_call_target_return(tail);
                Some(tail.ty.clone())
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(ty) = next_ty {
        expr.ty = ty;
    }
}

fn thread_callback_ident(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(name, _) => Some(name),
        Expr::Paren(inner, _) => thread_callback_ident(inner),
        _ => None,
    }
}

/// Lower one function-value invocation. Both the explicit AST node and sema's
/// builtin `.call(...)` method marker use this path, so argument conventions,
/// callback representation, and source-order preservation stay one mechanism.
pub(crate) fn lower_fn_value_call(
    callee_expr: Option<&Expr>,
    mut callee_t: TExpr,
    args: &[CallArg],
    site: u32,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    // Sema exposes the executable function-value signature at this boundary,
    // even when the local binding keeps the source-facing return spelling.
    // Keep both views: the raw view drives the actual call, while the
    // effective view supplies the canonical carrier at this boundary.
    let effective_callee_ty = match &callee_t.ty {
        Type::Fn { .. } => callee_t.ty.with_effective_fn_returns(),
        _ => callee_t.ty.clone(),
    };
    let (source_ret_ty, effective_ret_ty, needs_carrier) =
        match (&callee_t.ty, &effective_callee_ty) {
            (
                Type::Fn {
                    ret: Some(source_ret),
                    ..
                },
                Type::Fn {
                    ret: Some(effective_ret),
                    ..
                },
            ) => {
                let source_ret_ty = (**source_ret).clone();
                let effective_ret_ty = (**effective_ret).clone();
                let source_is_carrier =
                    matches!(&source_ret_ty, Type::Result { .. } | Type::Option(_))
                        || matches!(
                            &source_ret_ty,
                            Type::Named(name) if name == Syntax::TYPE_NEVER
                        );
                let needs_carrier = !source_is_carrier && source_ret_ty != effective_ret_ty;
                (source_ret_ty, effective_ret_ty, needs_carrier)
            }
            _ => (unit_type(), unit_type(), false),
        };
    let effective_params = match &effective_callee_ty {
        Type::Fn { params, .. } => Some(params.as_slice()),
        _ => None,
    };
    let conventions = match &effective_callee_ty {
        Type::Fn {
            call_metadata: Some(metadata),
            ..
        } => Some(metadata.conventions.as_slice()),
        _ => None,
    };
    if callee_expr.and_then(thread_callback_ident).is_some_and(|name| env.is_send_fn(name))
        && matches!(&callee_t.ty, Type::Fn { .. })
    {
        // A callback-safe local is stored in the canonical callback
        // representation. Mark call-through uses too, so the JIT invokes its
        // `(function, environment)` record instead of treating the record
        // handle as a raw function address.
        let ty = callee_t.ty.clone();
        callee_t = TExpr {
            ty,
            kind: TExprKind::FnValue {
                kind: TFnValueKind::Send {
                    value: Box::new(callee_t),
                },
            },
        };
    }
    let targs = args
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            let conv = effective_params
                .and_then(|params| params.get(index))
                .cloned()
                .map(|ty| {
                    (
                        conventions
                            .and_then(|row| row.get(index))
                            .copied()
                            .unwrap_or(AccessConvention::Read),
                        ty,
                    )
                });
            lower_one_call_arg(arg, conv, env, cx)
        })
        .collect();
    let call = TExpr {
        ty: source_ret_ty,
        kind: TExprKind::FnValue {
            kind: TFnValueKind::Call {
                callee: Box::new(callee_t),
                args: targs,
            },
        },
    };
    // D-APILABEL1=A: a function type may declare a call contract, so a call
    // through the value can reorder just like a named one. Apply the ordering
    // to the raw call before adding the carrier adapter.
    let call = match source_arg_order(args) {
        Some(order) => preserve_source_arg_order(call, &order, args.len(), site),
        None => call,
    };
    if needs_carrier {
        // A raw local/transformed callable returns its source value. Marshal it
        // into the one effective Result carrier; an enclosing Try/?? consumes
        // this adapter exactly once.
        TExpr {
            ty: effective_ret_ty,
            kind: TExprKind::Ok(Box::new(call)),
        }
    } else {
        TExpr {
            ty: effective_ret_ty,
            kind: call.kind,
        }
    }
}

/// D-MEM1 S6: lower `e` for use as a MUTATING method's receiver (`.push()`,
/// `.insert()`, …). Ordinarily identical to `lower_expr`; indexed collections
/// and their fields must retain a recursive place shape, while a Pool index
/// uses its generation-checked mutable accessor instead of the read clone.
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
        let src_line = cx
            .src
            .lines()
            .nth(line.saturating_sub(1))
            .unwrap_or_default()
            .to_string();
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
                    src_line: src_line.clone(),
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
                        src_line,
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
        Expr::Index {
            base,
            index,
            span,
            kind,
        } => {
            let base_t = lower_expr_as_mut_place(base, cx, env);
            let index_t = lower_expr(index, cx, env);
            let base_ty = base_t.ty.without_user_tags();
            let is_map = match kind {
                IndexKind::Map => true,
                IndexKind::List | IndexKind::FixedListProof => false,
                IndexKind::Unknown => matches!(&base_ty, Type::Map { .. }),
                _ => return lower_expr(e, cx, env),
            };
            let ty = match base_ty {
                Type::List(elem) | Type::FixedList { elem, .. } => (**elem).clone(),
                Type::Map { value, .. } => (**value).clone(),
                _ => return lower_expr(e, cx, env),
            };
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty,
                kind: TExprKind::Index {
                    base: Box::new(base_t),
                    index: Box::new(index_t),
                    is_map,
                    uninit_fixed: matches!(
                        base.as_ref(),
                        Expr::Ident(name, _) if env.is_uninit_fixed(name)
                    ),
                    line,
                },
            }
        }
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
                let recv = lower_expr_as_mut_place(base, cx, env);
                let field_ty = struct_field_type(cx, &recv.ty, field).unwrap_or(Type::Int);
                let boxed = match &recv.ty {
                    Type::Named(n) => cx.boxed_edges.contains(&(n.clone(), field.to_string())),
                    _ => false,
                };
                TExpr {
                    ty: field_ty,
                    kind: TExprKind::Field {
                        recv: Box::new(recv),
                        field: field.to_string(),
                        boxed,
                    },
                }
            }
        }
        Expr::Paren(inner, _) => lower_expr_as_mut_place(inner, cx, env),
        _ => lower_expr(e, cx, env),
    }
}

/// Lower a fluent method receiver from the innermost call outward.
///
/// Method-call ASTs nest through `receiver`. Walking that spine iteratively
/// keeps long fluent APIs off the Rust call stack while preserving the normal
/// method dispatcher for every link.
fn lower_method_chain(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let mut calls = TirWorklist::new();
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
            operator_rhs,
            resolved_ret,
            checked_widen,
        } = call
        else {
            unreachable!("method chain contains only method calls")
        };
        // A field receiver does not persist `recv_type` on the AST method
        // node. Pre-lower precise/string conversion receivers so the
        // receiver's checked TIR type can still select the canonical builtin
        // operation instead of the user-method fallback.
        let mut call_receiver = lowered_receiver;
        if call_receiver.is_none() && matches!(method.as_str(), "to_string" | "to_float" | "to_int")
        {
            call_receiver = Some(lower_expr(receiver, cx, env));
        }
        let receiver_type = call_receiver.as_ref().and_then(|recv| match &recv.ty {
            Type::String => Some("String".to_string()),
            Type::Named(name) if matches!(name.as_str(), "Decimal" | "Fraction" | "String") => {
                Some(name.clone())
            }
            _ => None,
        });
        let dispatch_recv_type = receiver_type.or_else(|| recv_type.clone());
        let method_sig = expr_cache_take_method_sig(call, cx);
        let precise_method = matches!(
            (dispatch_recv_type.as_deref(), method.as_str(), args.len()),
            (Some("Decimal"), "add" | "sub" | "mul" | "div" | "equal", 1)
                | (
                    Some("Decimal"),
                    "round" | "floor" | "ceil" | "to_string" | "to_float",
                    0
                )
                | (Some("Fraction"), "add" | "sub" | "mul" | "div" | "equal", 1)
                | (
                    Some("Fraction"),
                    "numerator" | "denominator" | "to_string" | "to_float" | "is_zero",
                    0
                )
        );
        let mut lowered = if precise_method {
            let recv = call_receiver
                .take()
                .unwrap_or_else(|| lower_expr(receiver, cx, env));
            let mut value_args = vec![recv];
            value_args.extend(args.iter().map(|arg| lower_expr(&arg.expr, cx, env)));
            let ty = match method.as_str() {
                "to_string" => Type::String,
                "div" => Type::Named("Fraction".to_string()),
                "numerator" | "denominator" => Type::Int,
                "to_float" => Type::Float,
                "is_zero" | "equal" => Type::Bool,
                _ => Type::Named(
                    dispatch_recv_type
                        .as_deref()
                        .unwrap_or("Decimal")
                        .to_string(),
                ),
            };
            TExpr {
                ty: resolved_ret.clone().unwrap_or(ty),
                kind: TExprKind::PreciseBuiltin {
                    type_name: dispatch_recv_type
                        .clone()
                        .unwrap_or_else(|| "Decimal".to_string()),
                    func: method.to_string(),
                    args: value_args,
                },
            }
        } else if method == "to_float"
            && args.is_empty()
            && dispatch_recv_type.as_deref() == Some("String")
        {
            let recv = call_receiver
                .take()
                .unwrap_or_else(|| lower_expr(receiver, cx, env));
            TExpr {
                ty: resolved_ret.clone().unwrap_or_else(|| Type::Result {
                    ok: Box::new(Type::Float),
                    err: Box::new(Type::Named("ParseError".to_string())),
                }),
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(recv),
                    op: TBuiltinOp::ParseFloat,
                    args: Vec::new(),
                },
            }
        } else {
            lower_method_call_with_sig(
                receiver,
                method,
                *method_span,
                owner_type_args,
                type_args,
                args,
                &dispatch_recv_type,
                operator_rhs.as_ref(),
                resolved_ret.as_ref(),
                *checked_widen,
                cx,
                env,
                call_receiver,
                method_sig.as_deref(),
            )
        };
        // D-MAPTYPE1: `shared [K:V]{}` is elaborated to an untyped empty
        // `MapLit` before TIR lowering. Its `Shared<T>` return still carries
        // the exact payload, so restore that context before Rust infers `V`.
        let shared_payload = resolved_ret.as_ref().and_then(|ty| match ty {
            Type::Shared(inner) => Some((**inner).clone()),
            _ => None,
        });
        if let Some(shared_payload) = shared_payload {
            let retagged = match &mut lowered.kind {
                TExprKind::StaticCall { owner, args, .. } => {
                    if let TStaticOwner::Prelude { path, generics, .. } = owner {
                        if path == "jet_std::JetShared" {
                            if let Some(TPreludeArg::Jet(ty)) = generics.first_mut() {
                                *ty = shared_payload.clone();
                            }
                        }
                    }
                    args.first_mut()
                        .map(|arg| {
                            let empty_collection = matches!(
                                &arg.value.kind,
                                TExprKind::MapLit(entries) if entries.is_empty()
                            ) || matches!(
                                &arg.value.kind,
                                TExprKind::ListLit(items) if items.is_empty()
                            );
                            if empty_collection {
                                arg.value.ty = shared_payload.clone();
                            }
                            empty_collection
                        })
                        .unwrap_or(false)
                }
                _ => false,
            };
            if retagged {
                lowered.ty = resolved_ret
                    .clone()
                    .unwrap_or_else(|| Type::Shared(Box::new(shared_payload)));
            }
        }
        // A cross-module call's executable carrier is separate from the
        // source-visible success type. Inline code modules register their
        // callable return in `fn_types` as the effective `Result`/`Option`
        // ABI, while sema records the source type on `resolved_ret`. Keep
        // that source type on the TIR node so field/pattern/print lowering
        // consumes the payload; `ModuleCall.target_return` remains the ABI
        // carrier used by the MIR adapter.
        if let Some(resolved_ret) = resolved_ret.as_ref() {
            if matches!(&lowered.kind, TExprKind::ModuleCall { .. }) {
                lowered.ty = resolved_ret.clone();
            }
        }
        let lowered = lower_method_pre_contracts(call, lowered, cx, env);
        // D-APILABEL1=A: a method whose labels reordered its arguments keeps
        // the same source evaluation order as a free call.
        lowered_receiver = Some(match source_arg_order(args) {
            Some(order) => {
                preserve_source_arg_order(lowered, &order, args.len(), method_span.start as u32)
            }
            None => lowered,
        });
    }

    lowered_receiver.expect("method chain is non-empty")
}

struct ExprWorklistCache {
    active: bool,
    values: HashMap<(usize, usize), VecDeque<TExpr>>,
    types: HashMap<(usize, usize), Type>,
    method_sigs: HashMap<(usize, usize), VecDeque<Vec<(AccessConvention, Type)>>>,
}

impl Default for ExprWorklistCache {
    fn default() -> Self {
        Self {
            active: false,
            values: HashMap::new(),
            types: HashMap::new(),
            method_sigs: HashMap::new(),
        }
    }
}

thread_local! {
    static EXPR_WORKLIST_CACHE: RefCell<ExprWorklistCache> =
        RefCell::new(ExprWorklistCache::default());
    static EXPR_COMPTIME_CACHE_DEPTH: Cell<usize> = const { Cell::new(0) };
}

fn expr_key(expr: &Expr, cx: &Cx) -> (usize, usize) {
    (expr as *const Expr as usize, cx as *const Cx as usize)
}

fn strip_expr_parens(mut expr: &Expr) -> &Expr {
    while let Expr::Paren(inner, _) = expr {
        expr = inner;
    }
    expr
}

fn lower_list_lit(elems: &[Expr], cx: &Cx, env: &mut LowerEnv) -> TExpr {
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
    // A bare `[]` has no element to read a type from; the binding or argument
    // position restores its sema-expected type (`preserve_typed_list_shape`).
    // A typed head (`[U8]{}`) never reaches here: sema keeps it on
    // `Expr::TypedLit`, whose arm below carries the head as the list type.
    let elem_ty = telems.first().map(|e| e.ty.clone()).unwrap_or(Type::Int);
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

pub(super) fn lower_or_fallback(
    value: &Expr,
    fallback: &OrFallback,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    fn return_value(value: TExpr, env: &LowerEnv, cx: &Cx) -> TExpr {
        let Some(return_ty) = env.ret_ty.clone() else {
            return value;
        };
        // A typed empty list is rewritten by sema to `ListLit([])`, so its
        // element type is absent from the AST by the time TIR lowers it.
        // Restore the enclosing return payload before constructing the carrier.
        let payload_ty = match &return_ty {
            Type::Result { ok, .. } | Type::Option(ok) => ok.as_ref(),
            other => other,
        };
        let value = if matches!(&value.ty, Type::Result { .. } | Type::Option(_)) {
            value
        } else {
            preserve_typed_list_shape(value, payload_ty, cx)
        };
        // An explicit `Err(...)`/`Ok(...)` (or any carrier-typed expression)
        // already IS the failure carrier; wrapping it again emits the invalid
        // double carrier `Ok(Err(...))` (leaked rustc E0308 on every
        // `?? return Err(...)` inside a fallible callable).
        let kind = match &return_ty {
            Type::Result { .. } if !matches!(&value.ty, Type::Result { .. }) => {
                TExprKind::Ok(Box::new(value))
            }
            Type::Option(_) if !matches!(&value.ty, Type::Option(_)) => {
                TExprKind::Present(Box::new(value))
            }
            _ => return value,
        };
        TExpr {
            ty: return_ty,
            kind,
        }
    }
    fn fit_fallback_value(value: TExpr, result_ty: &Type, cx: &Cx) -> TExpr {
        let value = preserve_typed_list_shape(value, result_ty, cx);
        match result_ty {
            // A nested fallible/optional value is the successful payload of
            // the outer `??`. Lift a bare fallback once so both match arms
            // produce the same carrier.
            Type::Result { .. } if !matches!(&value.ty, Type::Result { .. }) => TExpr {
                ty: result_ty.clone(),
                kind: TExprKind::Ok(Box::new(value)),
            },
            Type::Option(_) if !matches!(&value.ty, Type::Option(_)) => TExpr {
                ty: result_ty.clone(),
                kind: TExprKind::Present(Box::new(value)),
            },
            _ => value,
        }
    }

    fn lower_fallback(
        fallback: &OrFallback,
        result_ty: &Type,
        cx: &Cx,
        env: &LowerEnv,
        fallback_env: &mut LowerEnv,
    ) -> TOrFallback {
        // Fallback identifiers are resolved against the carrier-specific
        // environment below. Do not replay an expression lowered under the
        // enclosing environment, where the ambient `err` slot is absent.
        let _fallback_cache_scope = ExprCacheScope::enter();
        match fallback {
            OrFallback::Value(e) => {
                let value = lower_expr(e, cx, fallback_env);
                let value = fit_fallback_value(value, result_ty, cx);
                TOrFallback::Value(Box::new(value))
            }
            OrFallback::Block { body, value, .. } => {
                let mut stmts = lower_stmts(body, cx, fallback_env);
                if let Some(value) = value {
                    let value = lower_expr(value, cx, fallback_env);
                    let value = fit_fallback_value(value, result_ty, cx);
                    stmts.push(TStmt::ExprStmt(value));
                }
                TOrFallback::Value(Box::new(TExpr {
                    ty: result_ty.clone(),
                    kind: TExprKind::InlineBlock(stmts),
                }))
            }
            OrFallback::Return(None, _) => {
                if matches!(
                    &env.ret_ty,
                    Some(Type::Result { ok, .. })
                        if matches!(ok.as_ref(), Type::Named(n) if n == crate::Syntax::INTERNAL_UNIT_TYPE)
                ) {
                    let ret_ty = env.ret_ty.clone().expect("fallible void return");
                    let unit = TExpr {
                        ty: Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string()),
                        kind: TExprKind::Unit,
                    };
                    TOrFallback::Return(Some(Box::new(TExpr {
                        ty: ret_ty,
                        kind: TExprKind::Ok(Box::new(unit)),
                    })))
                } else {
                    TOrFallback::Return(None)
                }
            }
            OrFallback::Return(Some(e), _) => {
                // A return fallback is checked against the enclosing carrier, so
                // sema intentionally leaves a direct Jet call unwrapped. Lower
                // that call through the same traced Try path as ordinary value
                // propagation before return_value restores the outer Ok.
                let value = if matches!(e.without_parens(), Expr::Call(..)) {
                    let wrapped = Expr::Try(e.clone(), e.span(), TryConvert::None, None);
                    lower_expr(&wrapped, cx, fallback_env)
                } else {
                    lower_expr(e, cx, fallback_env)
                };
                TOrFallback::Return(Some(Box::new(return_value(value, env, cx))))
            }
            OrFallback::Panic { name_span, args } => {
                let (kind, loc) = lower_panic_stop(name_span, args, cx, fallback_env);
                let TRequireKind::Panic { msg } = kind else {
                    unreachable!()
                };
                TOrFallback::Panic { msg, loc }
            }
            OrFallback::Break(_) => TOrFallback::Break,
            OrFallback::Continue(_) => TOrFallback::Continue,
            OrFallback::BreakLabel(name, _) => TOrFallback::BreakLabel(name.clone()),
            OrFallback::ContinueLabel(name, _) => TOrFallback::ContinueLabel(name.clone()),
        }
    }

    fn lift_value_fallback_to_option(fallback: TOrFallback, payload: &Type) -> TOrFallback {
        match fallback {
            TOrFallback::Value(value) => TOrFallback::Value(Box::new(TExpr {
                ty: Type::Option(Box::new(payload.clone())),
                kind: TExprKind::Present(value),
            })),
            other => other,
        }
    }

    // The subject may already have a cache entry from a type probe under a
    // different carrier context. Re-lower it in a private memo.
    let mut value_t = {
        let _fallback_subject_cache_scope = ExprCacheScope::enter();
        let fallback_subject = env.fallback_subject;
        env.fallback_subject = true;
        let value_t = lower_owned_expr(value, cx, env);
        env.fallback_subject = fallback_subject;
        value_t
    };
    suppress_module_call_target_return(&mut value_t);
    // D-NEVER2=B: a `Result<Never, E>` has no success branch. Lower a value
    // fallback as the expression's real type instead of manufacturing a
    // `Never` merge that would reject `f() ?? x` in Rust.
    if matches!(&value_t.ty, Type::Result { ok, .. } if ok.is_never()) {
        let mut fallback_env = clone_env(env);
        if let Type::Result { err, .. } = &value_t.ty {
            fallback_env.bind(
                Syntax::AMBIENT_ERR,
                ambient_err_local(),
                Some((**err).clone()),
            );
        }
        match fallback {
            OrFallback::Value(e) => {
                let fallback_t = lower_expr(e, cx, &mut fallback_env);
                let result_ty = fallback_t.ty.clone();
                return TExpr {
                    ty: result_ty,
                    kind: TExprKind::OrFallback {
                        value: Box::new(value_t),
                        fallback: TOrFallback::Value(Box::new(fallback_t)),
                    },
                };
            }
            OrFallback::Block { body, value, .. } => {
                let mut stmts = lower_stmts(body, cx, &mut fallback_env);
                let Some(value) = value else {
                    return TExpr {
                        ty: Type::Named(Syntax::TYPE_NEVER.to_string()),
                        kind: TExprKind::OrFallback {
                            value: Box::new(value_t),
                            fallback: TOrFallback::Value(Box::new(TExpr {
                                ty: Type::Named(Syntax::TYPE_NEVER.to_string()),
                                kind: TExprKind::InlineBlock(stmts),
                            })),
                        },
                    };
                };
                let fallback_t = lower_expr(value, cx, &mut fallback_env);
                let result_ty = fallback_t.ty.clone();
                stmts.push(TStmt::ExprStmt(fallback_t));
                return TExpr {
                    ty: result_ty.clone(),
                    kind: TExprKind::OrFallback {
                        value: Box::new(value_t),
                        fallback: TOrFallback::Value(Box::new(TExpr {
                            ty: result_ty,
                            kind: TExprKind::InlineBlock(stmts),
                        })),
                    },
                };
            }
            _ => {}
        }
    }
    // D-FAILURE-FOUNDATION1=A / D-FAIL-BIND1=A: a mixed `?T !E` carrier has
    // three routes. First consume its Result route, making an `Err(e)` fallback
    // carry `Present(fallback)`; then consume the remaining Option route. The
    // fallback is lowered in both branch environments but only one branch runs,
    // so `err` is visible on the failure route without evaluating the fallback
    // twice.
    let mixed_payload = match &value_t.ty {
        Type::Result { ok, .. } => match ok.as_ref() {
            Type::Option(inner) => Some((**inner).clone()),
            _ => None,
        },
        _ => None,
    };
    if let Some(payload) = mixed_payload {
        let option_ty = Type::Option(Box::new(payload.clone()));
        let mut failure_env = clone_env(env);
        if let Type::Result { err, .. } = &value_t.ty {
            failure_env.bind(
                Syntax::AMBIENT_ERR,
                ambient_err_local(),
                Some((**err).clone()),
            );
        }
        let failure_fallback = lower_fallback(fallback, &payload, cx, env, &mut failure_env);
        let failure_fallback = lift_value_fallback_to_option(failure_fallback, &payload);
        let after_result = TExpr {
            ty: option_ty,
            kind: TExprKind::OrFallback {
                value: Box::new(value_t),
                fallback: failure_fallback,
            },
        };
        let mut absence_env = clone_env(env);
        let absence_fallback = lower_fallback(fallback, &payload, cx, env, &mut absence_env);
        return TExpr {
            ty: payload,
            kind: TExprKind::OrFallback {
                value: Box::new(after_result),
                fallback: absence_fallback,
            },
        };
    }

    // `??` removes exactly one carrier. A plain Result or Option reaches this
    // path; the mixed Result<Option<T>, E> shape was normalized above.
    let result_ty = match &value_t.ty {
        Type::Option(inner) => (**inner).clone(),
        Type::Result { ok, .. } => ok.as_ref().clone(),
        other => other.clone(),
    };
    let optional_success = matches!(&value_t.ty, Type::Option(_));
    let mut fallback_env = clone_env(env);
    if let Type::Result { err, .. } = &value_t.ty {
        // Only a direct Option has no error carrier. A Result<Option<T>, E>
        // mixed carrier is handled above, and its failure branch binds E.
        if !optional_success {
            fallback_env.bind(
                Syntax::AMBIENT_ERR,
                ambient_err_local(),
                Some((**err).clone()),
            );
        }
    }
    let tfallback = lower_fallback(fallback, &result_ty, cx, env, &mut fallback_env);
    TExpr {
        ty: result_ty,
        kind: TExprKind::OrFallback {
            value: Box::new(value_t),
            fallback: tfallback,
        },
    }
}

fn lower_expr_node(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    canonicalize_pre_tier_expr(match e {
        Expr::MethodCall { .. } => lower_method_chain(e, cx, env),
        Expr::ListLit(elems, _) => lower_list_lit(elems, cx, env),
        Expr::OrFallback {
            value, fallback, ..
        } => lower_or_fallback(value, fallback, cx, env),
        _ => lower_expr_inner(e, cx, env),
    })
}

/// Lower callee preconditions at the call site.  The argument values are
/// pinned once into compiler-owned locals, then both the condition and the
/// eventual call read those locals.  Borrowed arguments stay borrowed through
/// the temp, so this does not turn a read/write convention into an ownership
/// move.
fn lower_pre_contracts_for_args(
    call_span: Span,
    args: &mut [TCallArg],
    param_names: &[String],
    sig: Option<&[(AccessConvention, Type)]>,
    clauses: &[ContractClause],
    cx: &Cx,
    caller_env: &LowerEnv,
) -> (Vec<TStmt>, Vec<TContract>) {
    if clauses.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut contract_env = LowerEnv::new(caller_env.fn_name.clone());
    contract_env.sentries_fenced = caller_env.sentries_fenced;
    // Preconditions execute at this call site. Share the enclosing lowering
    // fact so an address minted while evaluating the condition receives the
    // same current-frame token as the call body.
    contract_env.stack_sentry_needed = caller_env.stack_sentry_needed.clone();
    let mut proof_bindings = HashMap::new();
    let mut bindings = Vec::new();
    for (index, arg) in args.iter_mut().enumerate() {
        let Some(param_name) = param_names.get(index) else {
            break;
        };
        let ty = sig
            .and_then(|params| params.get(index))
            .map(|(_, ty)| ty.clone())
            .unwrap_or_else(|| arg.value.ty.clone());
        let temp = format!(
            "{}contract_arg_{}_{}",
            crate::Syntax::GENERATED_NAME_PREFIX,
            call_span.start,
            index
        );
        let original = std::mem::replace(
            &mut arg.value,
            TExpr {
                ty: ty.clone(),
                kind: TExprKind::Unit,
            },
        );
        let mut init = original;
        let actual_ty = init.ty.clone();
        if arg.clone || arg.arc_clone {
            init = TExpr {
                ty: init.ty.clone(),
                kind: TExprKind::Clone(Box::new(init)),
            };
            arg.clone = false;
            arg.arc_clone = false;
        }
        let local = if arg.borrow || arg.mut_borrow {
            let mutable = arg.mut_borrow;
            init = TExpr {
                ty: init.ty.clone(),
                kind: TExprKind::Borrow {
                    place: Box::new(init),
                    mutable,
                },
            };
            TLocal::user(&temp).through_ref()
        } else {
            TLocal::user(&temp)
        };
        arg.value = TExpr {
            ty: ty.clone(),
            kind: TExprKind::Local(local.clone()),
        };
        contract_env.bind(param_name, local, Some(ty.clone()));
        proof_bindings.insert(param_name.clone(), actual_ty);
        bindings.push(TStmt::Let {
            name: temp,
            kw: "let",
            let_ty: crate::Codegen::TIR::TLetTy::inferred(),
            init,
            gc_promotion: None,
            gc_transferred: false,
        });
    }
    let (_, line, _) = crate::Codegen::TIR::tir_src_line_at(&cx.src, call_span.start);
    let mut lowered = Vec::with_capacity(clauses.len());
    for clause in clauses {
        let condition = lower_expr(&clause.cond, cx, &mut contract_env);
        let message = lower_expr(&clause.message_expr, cx, &mut contract_env);
        lowered.push(TContract {
            kind: TContractKind::Pre,
            condition,
            message,
            file: cx.file.clone(),
            line,
            span: call_span,
            disposition: if contract_expr_proven(&clause.cond, &proof_bindings, cx) {
                TContractDisposition::Proven
            } else {
                TContractDisposition::Check
            },
        });
    }
    (bindings, lowered)
}

fn lower_call_pre_contracts(
    call: &Call,
    args: &mut [TCallArg],
    clauses: &[ContractClause],
    cx: &Cx,
    caller_env: &LowerEnv,
) -> (Vec<TStmt>, Vec<TContract>) {
    let Some(param_names) = cx.fn_param_names.get(&call.name) else {
        return (Vec::new(), Vec::new());
    };
    lower_pre_contracts_for_args(
        call.name_span,
        args,
        param_names,
        cx.sigs.get(&call.name).map(Vec::as_slice),
        clauses,
        cx,
        caller_env,
    )
}

/// Pin a user method's receiver and arguments before checking its preconditions.
/// The method lowering already resolved dispatch and argument ownership; this
/// pass only replaces each evaluated value with a local so the contract and the
/// eventual call observe one value on every tier.
fn lower_method_pre_contracts(
    call: &Expr,
    mut lowered: TExpr,
    cx: &Cx,
    caller_env: &LowerEnv,
) -> TExpr {
    let Expr::MethodCall {
        method,
        method_span,
        recv_type,
        ..
    } = call
    else {
        return lowered;
    };
    let owner = recv_type
        .as_deref()
        .map(|name| name.rsplit_once('.').map_or(name, |(_, leaf)| leaf));
    let (owner, clauses, param_names) = match &lowered.kind {
        TExprKind::MethodCall { .. } => {
            let Some(owner) = owner else {
                return lowered;
            };
            let key = format!("{owner}::{method}");
            let Some((pre, _)) = cx.contract_sigs.get(&key) else {
                return lowered;
            };
            let Some(param_names) = cx.fn_param_names.get(&key) else {
                return lowered;
            };
            (key, pre.clone(), param_names.clone())
        }
        TExprKind::StaticCall {
            owner: TStaticOwner::User(owner),
            ..
        } => {
            let key = format!("{owner}::{method}");
            let Some((pre, _)) = cx.contract_sigs.get(&key) else {
                return lowered;
            };
            let Some(param_names) = cx.fn_param_names.get(&key) else {
                return lowered;
            };
            (key, pre.clone(), param_names.clone())
        }
        _ => return lowered,
    };
    if clauses.is_empty() {
        return lowered;
    }

    let (bindings, contracts) = match &mut lowered.kind {
        TExprKind::MethodCall { recv, args, .. } => {
            let recv_value = std::mem::replace(
                recv,
                Box::new(TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Unit,
                }),
            );
            let type_name = owner
                .split_once("::")
                .map_or(owner.as_str(), |(type_name, _)| type_name);
            let self_conv = cx
                .method_self_convs
                .get(&(type_name.to_string(), method.to_string()))
                .copied()
                .unwrap_or(AccessConvention::Read);
            let self_ty = recv_value.ty.clone();
            let receiver = TCallArg {
                borrow: self_conv == AccessConvention::Read && !self_ty.is_scalar(),
                mut_borrow: self_conv == AccessConvention::Write,
                value: *recv_value,
                template_items: None,
                clone: false,
                arc_clone: false,
                fn_coerce: None,
                widen_to_vec: false,
                widen_to_union: None,
                box_as_trait: None,
            };
            let method_sig = cx
                .method_sigs
                .get(&(type_name.to_string(), method.to_string()));
            let mut sig = Vec::with_capacity(method_sig.map_or(0, |sig| sig.len()) + 1);
            sig.push((self_conv, self_ty));
            if let Some(method_sig) = method_sig {
                sig.extend(method_sig.iter().cloned());
            }
            let mut all_args = Vec::with_capacity(args.len() + 1);
            all_args.push(receiver);
            all_args.append(args);
            let result = lower_pre_contracts_for_args(
                *method_span,
                &mut all_args,
                &param_names,
                Some(&sig),
                &clauses,
                cx,
                caller_env,
            );
            let receiver = all_args.remove(0);
            *recv = Box::new(receiver.value);
            *args = all_args;
            result
        }
        TExprKind::StaticCall { args, .. } => {
            let type_name = owner
                .split_once("::")
                .map_or(owner.as_str(), |(type_name, _)| type_name);
            let method_sig = cx
                .method_sigs
                .get(&(type_name.to_string(), method.to_string()));
            lower_pre_contracts_for_args(
                *method_span,
                args,
                &param_names,
                method_sig.map(Vec::as_slice),
                &clauses,
                cx,
                caller_env,
            )
        }
        _ => unreachable!("method contract dispatch changed during lowering"),
    };
    if contracts.is_empty() {
        return lowered;
    }
    let ty = lowered.ty.clone();
    let mut stmts = bindings;
    stmts.extend(
        contracts
            .into_iter()
            .map(|contract| TStmt::Contract { contract }),
    );
    stmts.push(TStmt::ExprStmt(lowered));
    TExpr {
        ty,
        kind: TExprKind::InlineBlock(stmts),
    }
}

/// D-QUAL4/I9: user tags are compile-time facts. Remove them at the shared TIR
/// boundary, but retain compiler-owned tags except for `Range`, whose adapters
/// all require the exact nominal carrier.
fn canonicalize_pre_tier_expr(mut expr: TExpr) -> TExpr {
    let ty = expr.ty.without_user_tags().erased_inline_ranges();
    expr.ty = if matches!(ty.erased_carrier(), Type::Named(name) if name == Syntax::TYPE_RANGE) {
        Type::Named(Syntax::TYPE_RANGE.to_string())
    } else {
        ty
    };
    expr
}

fn expr_cache_begin() -> bool {
    EXPR_WORKLIST_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.active {
            false
        } else {
            cache.active = true;
            cache.values.clear();
            cache.types.clear();
            cache.method_sigs.clear();
            true
        }
    })
}

fn expr_cache_end() {
    EXPR_WORKLIST_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.active = false;
        cache.values.clear();
        cache.types.clear();
        cache.method_sigs.clear();
    });
}

struct ExprCacheOwner {
    owns_cache: bool,
}

/// Run `body` on its own AST-pointer expression memo.
///
/// The memo below is keyed by node identity alone, so it is only sound while
/// one `LowerEnv` is in force. Two lowerings cross that line:
///
///   * comptime re-entry, which lowers under a different `Cx` (D-CTCACHE1); and
///   * a lambda body, which is lowered once per pass — the AOT closure text,
///     the `executable` TIR, and the JIT spawn body — under a DIFFERENT env
///     each time. The clone pack rebinds a capture to `__jet___cap_<n>` while
///     the spawn pack keeps the source slot (`lower/lambdas.rs`), so a value
///     memoized under one pass replays a `TLocal` the other pass never binds.
///
/// One mechanism serves both: take the memo, restore it on the way out.
pub(crate) struct ExprCacheScope {
    saved: ExprWorklistCache,
}

impl ExprCacheScope {
    pub(crate) fn enter() -> Self {
        Self {
            saved: EXPR_WORKLIST_CACHE.with(|cache| std::mem::take(&mut *cache.borrow_mut())),
        }
    }
}

impl Drop for ExprCacheScope {
    fn drop(&mut self) {
        let saved = std::mem::take(&mut self.saved);
        EXPR_WORKLIST_CACHE.with(|cache| *cache.borrow_mut() = saved);
    }
}

/// Lower one lambda body on its own memo (see [`ExprCacheScope`]). Every pass
/// that re-lowers the same body under its own env goes through here.
pub(crate) fn with_lambda_body_expr_cache<R>(body: impl FnOnce() -> R) -> R {
    let _scope = ExprCacheScope::enter();
    body()
}

// D-CTCACHE1: comptime lowering can recursively invoke TIR while an outer
// executable lowering worklist is active. Keep the two AST-pointer caches
// separate; otherwise a comptime-shaped expression can be consumed later by
// runtime lowering under a different Cx and environment.
struct ExprComptimeCacheGuard {
    scope: Option<ExprCacheScope>,
}

impl ExprComptimeCacheGuard {
    fn enter(env: &LowerEnv) -> Self {
        if env.fn_name != "__ct" {
            return Self { scope: None };
        }
        let nested = EXPR_COMPTIME_CACHE_DEPTH.with(|depth| {
            let nested = depth.get() > 0;
            depth.set(depth.get() + 1);
            nested
        });
        if nested {
            return Self { scope: None };
        }
        Self {
            scope: Some(ExprCacheScope::enter()),
        }
    }
}

impl Drop for ExprComptimeCacheGuard {
    fn drop(&mut self) {
        // The scope restores the outer memo as it drops.
        self.scope.take();
        EXPR_COMPTIME_CACHE_DEPTH.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

impl Drop for ExprCacheOwner {
    fn drop(&mut self) {
        if self.owns_cache {
            expr_cache_end();
        }
    }
}

fn expr_cache_take(expr: &Expr, cx: &Cx) -> Option<TExpr> {
    EXPR_WORKLIST_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.active {
            return None;
        }
        let key = expr_key(expr, cx);
        let value = cache
            .values
            .get_mut(&key)
            .and_then(|values| values.pop_front());
        if cache
            .values
            .get(&key)
            .is_some_and(|values| values.is_empty())
        {
            cache.values.remove(&key);
        }
        value
    })
}

fn expr_cache_put(expr: &Expr, value: TExpr, cx: &Cx) {
    EXPR_WORKLIST_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.types.insert(expr_key(expr, cx), value.ty.clone());
        cache
            .values
            .entry(expr_key(expr, cx))
            .or_default()
            .push_back(value);
    });
}

fn expr_cache_type(expr: &Expr, cx: &Cx) -> Option<Type> {
    let expr = strip_expr_parens(expr);
    EXPR_WORKLIST_CACHE.with(|cache| {
        let cache = cache.borrow();
        cache
            .active
            .then(|| cache.types.get(&expr_key(expr, cx)).cloned())
            .flatten()
    })
}

fn expr_cache_put_method_sig(expr: &Expr, sig: Vec<(AccessConvention, Type)>, cx: &Cx) {
    EXPR_WORKLIST_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .method_sigs
            .entry(expr_key(expr, cx))
            .or_default()
            .push_back(sig);
    });
}

fn expr_cache_take_method_sig(expr: &Expr, cx: &Cx) -> Option<Vec<(AccessConvention, Type)>> {
    EXPR_WORKLIST_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.active {
            return None;
        }
        let key = expr_key(expr, cx);
        let sig = cache
            .method_sigs
            .get_mut(&key)
            .and_then(|sigs| sigs.pop_front());
        if cache
            .method_sigs
            .get(&key)
            .is_some_and(|sigs| sigs.is_empty())
        {
            cache.method_sigs.remove(&key);
        }
        sig
    })
}

pub(crate) fn take_scheduled_expr(expr: &Expr, cx: &Cx) -> Option<TExpr> {
    expr_cache_take(expr, cx).or_else(|| {
        let stripped = strip_expr_parens(expr);
        (!std::ptr::eq(expr, stripped))
            .then(|| expr_cache_take(stripped, cx))
            .flatten()
    })
}

/// Lower one expression after its work-item children are ready. A condition
/// builder uses this instead of reopening a nested expression segment, so a
/// value is consumed from the cache exactly once.
pub(crate) fn lower_cached_expr(expr: &Expr, cx: &Cx, _env: &mut LowerEnv) -> TExpr {
    let expr = strip_expr_parens(expr);
    take_scheduled_expr(expr, cx).expect("condition child missing from expression worklist")
}

/// Return the non-call children of an expression. Calls use the unified work
/// item path below so argument context and source order stay explicit.
fn plain_expr_children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Str(parts, _) => parts
            .iter()
            .filter_map(|part| match part {
                StrPart::Interp(expr, _) => Some(expr.as_ref()),
                StrPart::Lit(_) => None,
            })
            .collect(),
        Expr::ListLit(items, _) => {
            let mut children = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Expr::Spread(inner, _) => children.push(inner.as_ref()),
                    item => children.push(item),
                }
            }
            children
        }
        Expr::MemberSpread { base, .. } => vec![base.as_ref()],
        Expr::Spread(inner, _)
        | Expr::Unary(_, inner, _)
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _) => vec![inner.as_ref()],
        Expr::Try(inner, _, _, note) => std::iter::once(inner.as_ref())
            .chain(note.as_deref())
            .collect(),
        Expr::MapLit(entries, _) => entries
            .iter()
            .flat_map(|(key, value)| [key, value])
            .collect(),
        Expr::Index { base, index, .. } => vec![base.as_ref(), index.as_ref()],
        Expr::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            if let Some(range) = range {
                vec![base.as_ref(), range.as_ref()]
            } else {
                vec![base.as_ref(), start.as_ref(), end.as_ref()]
            }
        }
        Expr::Range { start, end, .. } => vec![start.as_ref(), end.as_ref()],
        Expr::Binary(_, left, right, _) => vec![left.as_ref(), right.as_ref()],
        Expr::CompareChain { operands, .. } => operands.iter().collect(),
        Expr::Place(inner, _, _) => {
            if let Expr::Slice {
                base,
                start,
                end,
                range,
                ..
            } = inner.as_ref()
            {
                let mut children = vec![base.as_ref()];
                if let Some(range) = range {
                    children.push(range.as_ref());
                } else {
                    children.push(start.as_ref());
                    children.push(end.as_ref());
                }
                children
            } else {
                vec![inner.as_ref()]
            }
        }
        Expr::Field(receiver, _, _) => {
            if let Expr::Index { base, index, .. } = receiver.as_ref() {
                vec![base.as_ref(), index.as_ref()]
            } else {
                vec![receiver.as_ref()]
            }
        }
        Expr::OptField { base, .. } => vec![base.as_ref()],
        Expr::StructLit { fields, .. } => fields.iter().map(|(_, _, value)| value).collect(),
        Expr::EnumLit { args, .. } => args
            .iter()
            .map(|arg| match arg {
                EnumLitArg::Positional(expr) => expr,
                EnumLitArg::Named { expr, .. } => expr,
            })
            .collect(),
        Expr::PatternTest { subject, .. } => vec![subject.as_ref()],
        Expr::Call(_) | Expr::CallValue { .. } | Expr::MethodCall { .. } => Vec::new(),
        Expr::TypedLit { body, .. } => {
            let mut children = Vec::new();
            match body {
                TypedLitBody::Fields(fields) => {
                    for (_, _, value) in fields.iter() {
                        children.push(value);
                    }
                }
                TypedLitBody::Elements(elements) => children.extend(elements.iter()),
                TypedLitBody::Entries(entries) => {
                    for (key, value) in entries.iter() {
                        children.push(key);
                        children.push(value);
                    }
                }
                TypedLitBody::Value(value) => children.push(value),
                TypedLitBody::ByteText(_) => {}
                TypedLitBody::Empty => {}
            }
            children
        }
        Expr::OrFallback { value, .. } => {
            // D-FAIL-BIND1=A: lower the fallback only from `lower_or_fallback`,
            // after that function installs the contextual `err` slot. If the
            // fallback is pre-lowered here, an `err` identifier becomes an
            // ordinary user local before the slot exists and the interpreter
            // reports it as unbound.
            vec![value.as_ref()]
        }
        Expr::If { cond, .. } => vec![cond.as_ref()],
        Expr::Lambda(_) => Vec::new(),
        Expr::TupleLit(fields, _, _) => fields.iter().map(|(_, value)| value).collect(),
        Expr::PtrFromAddr { addr, .. } => vec![addr.as_ref()],
        Expr::Paren(inner, _) => vec![inner.as_ref()],
        Expr::IncDec { operand, .. } => vec![operand.as_ref()],
        Expr::StrMatchLit(..)
        | Expr::BinMatchLit(..)
        | Expr::Int(..)
        | Expr::Float(..)
        | Expr::Bool(..)
        | Expr::Unit(..)
        | Expr::Char(..)
        | Expr::Ident(..)
        | Expr::UnitLit { .. }
        | Expr::ComptimeName { .. }
        | Expr::Absent(_)
        | Expr::Todo { .. }
        | Expr::NoElse(_)
        | Expr::ReduceMarker(..) => Vec::new(),
    }
}

enum ExprArgMode<'a> {
    Plain,
    Convention(Option<(AccessConvention, Type)>),
    CallValue { callee: &'a Expr, index: usize },
}

struct ExprWorkArg<'a> {
    arg: &'a CallArg,
    mode: ExprArgMode<'a>,
}

enum ExprWorkChild<'a> {
    Expr(&'a Expr),
    Arg(ExprWorkArg<'a>),
}

fn source_arg_indices(args: &[CallArg]) -> Vec<usize> {
    let Some(order) = source_arg_order(args) else {
        return (0..args.len()).collect();
    };
    let mut seen = vec![false; args.len()];
    let mut indices = Vec::with_capacity(args.len());
    for index in order {
        seen[index] = true;
        indices.push(index);
    }
    indices.extend(
        seen.into_iter()
            .enumerate()
            .filter_map(|(index, seen)| (!seen).then_some(index)),
    );
    indices
}

fn direct_call_conventions(
    call: &crate::AST::Call,
    cx: &Cx,
    env: &LowerEnv,
) -> Option<Vec<(AccessConvention, Type)>> {
    if env.locals.contains_key(&call.name) && !cx.consts.contains_key(&call.name) {
        return match env.ty_of(&call.name) {
            Some(Type::Fn { params, .. }) => Some(
                params
                    .iter()
                    .cloned()
                    .map(|ty| (AccessConvention::Read, ty))
                    .collect(),
            ),
            _ => Some(Vec::new()),
        };
    }
    if let Some(sig) = cx.sigs.get(&call.name) {
        return Some(sig.clone());
    }
    let inline_mangled = cx
        .inline_unqualified
        .get(&env.fn_name)
        .and_then(|scope| scope.get(&call.name))
        .or_else(|| cx.unqualified_inline.get(&call.name));
    if let Some(mangled) = inline_mangled {
        return cx.sigs.get(mangled).cloned();
    }
    let inline_file = cx
        .inline_unqualified_file
        .get(&env.fn_name)
        .and_then(|scope| scope.get(&call.name))
        .or_else(|| cx.unqualified_file.get(&call.name));
    inline_file.and_then(|(_, function)| {
        cx.import_sigs
            .get(&(call.name.clone(), function.clone()))
            .cloned()
    })
}

fn method_arg_mode(sig: Option<&[(AccessConvention, Type)]>, index: usize) -> ExprArgMode<'static> {
    sig.map(|sig| ExprArgMode::Convention(sig.get(index).cloned()))
        .unwrap_or(ExprArgMode::Plain)
}

fn method_arg_contract(
    method: &str,
    recv_type: &Option<String>,
    owner_type_args: &[Type],
    type_args: &[Type],
    cx: &Cx,
) -> Option<Vec<(AccessConvention, Type)>> {
    let ty = recv_type.as_ref()?;
    let sig = cx.method_sigs.get(&(ty.clone(), method.to_string()))?;
    Some(crate::Codegen::TIR::instantiate_method_sig(
        cx,
        ty,
        method,
        sig,
        owner_type_args,
        type_args,
    ))
}

fn inline_loop_body(expr: &Expr) -> Option<&[Stmt]> {
    let Expr::CallValue { callee, args, .. } = expr else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    let Expr::Lambda(lam) = callee.as_ref() else {
        return None;
    };
    if !(lam.meta.collecting_loop || lam.meta.result_loop) {
        return None;
    }
    let crate::AST::LambdaBody::Block(body) = &lam.body else {
        return None;
    };
    Some(body)
}

fn expr_children<'a>(expr: &'a Expr, cx: &Cx, env: &LowerEnv) -> Vec<ExprWorkChild<'a>> {
    match expr {
        Expr::Call(call) => {
            // D-CALLPOLICY1=E: policy arguments are typed compile-time values.
            // The checked final callable is the only runtime child; lowering
            // policy calls here would invent a second engine policy path.
            if call.name == "apply"
                && !cx.sigs.contains_key(&call.name)
                && !env.locals.contains_key(&call.name)
                && call
                    .args
                    .last()
                    .is_some_and(|arg| arg.flags.callable_policy.is_some())
            {
                return call
                    .args
                    .last()
                    .map(|arg| {
                        vec![ExprWorkChild::Arg(ExprWorkArg {
                            arg,
                            mode: ExprArgMode::Plain,
                        })]
                    })
                    .unwrap_or_default();
            }
            let conventions = direct_call_conventions(call, cx, env);
            source_arg_indices(&call.args)
                .into_iter()
                .map(|index| {
                    ExprWorkChild::Arg(ExprWorkArg {
                        arg: &call.args[index],
                        mode: conventions
                            .as_ref()
                            .map(|sig| ExprArgMode::Convention(sig.get(index).cloned()))
                            .unwrap_or(ExprArgMode::Plain),
                    })
                })
                .collect()
        }
        Expr::CallValue { callee, args, .. } => {
            let mut children = Vec::with_capacity(args.len() + 1);
            if inline_loop_body(expr).is_none() {
                children.push(ExprWorkChild::Expr(callee.as_ref()));
                children.extend(source_arg_indices(args).into_iter().map(|index| {
                    ExprWorkChild::Arg(ExprWorkArg {
                        arg: &args[index],
                        mode: ExprArgMode::CallValue {
                            callee: callee.as_ref(),
                            index,
                        },
                    })
                }));
            }
            children
        }
        Expr::MethodCall { .. } => {
            let mut calls = Vec::new();
            let mut cursor = expr;
            while let Expr::MethodCall { receiver, .. } = cursor {
                calls.push(cursor);
                cursor = receiver;
            }
            let mut children = vec![ExprWorkChild::Expr(cursor)];
            for call in calls.into_iter().rev() {
                let Expr::MethodCall {
                    method,
                    owner_type_args,
                    type_args,
                    args,
                    recv_type,
                    ..
                } = call
                else {
                    unreachable!("method chain contains only method calls")
                };
                let contract =
                    method_arg_contract(method, recv_type, owner_type_args, type_args, cx);
                if let Some(contract) = contract.as_ref() {
                    expr_cache_put_method_sig(call, contract.clone(), cx);
                }
                children.extend(source_arg_indices(args).into_iter().map(|index| {
                    ExprWorkChild::Arg(ExprWorkArg {
                        arg: &args[index],
                        mode: method_arg_mode(contract.as_deref(), index),
                    })
                }));
            }
            children
        }
        _ => plain_expr_children(expr)
            .into_iter()
            .map(ExprWorkChild::Expr)
            .collect(),
    }
}

enum ExprWork<'a> {
    Enter(&'a Expr),
    EnterArg(ExprWorkArg<'a>),
    Build(&'a Expr),
    BuildArg(ExprWorkArg<'a>),
    LowerInlineLoop { expr: &'a Expr, body: &'a [Stmt] },
    BuildIf(&'a Expr),
    LowerIfCondition(Box<ExprIfConditionWork<'a>>),
    LowerIfThenBody(Box<ExprIfWork<'a>>),
    LowerIfThenValue(Box<ExprIfWork<'a>>),
    LowerIfElseBody(Box<ExprIfWork<'a>>),
    LowerIfElseValue(Box<ExprIfWork<'a>>),
}

struct ExprIfConditionWork<'a> {
    expr: &'a Expr,
    then_body: &'a [Stmt],
    then_value: &'a Expr,
    else_body: &'a [Stmt],
    else_value: &'a Expr,
    terms: Vec<&'a Expr>,
    next: usize,
    lowered: Vec<TIfCond>,
    bindings: Vec<(String, TLocal, Option<Type>)>,
    prefixes: Vec<TStmt>,
    base_env: LowerEnv,
}

struct ExprIfWork<'a> {
    expr: &'a Expr,
    condition: TIfCond,
    then_prefix: Vec<TStmt>,
    then_body: &'a [Stmt],
    then_value: &'a Expr,
    else_body: &'a [Stmt],
    else_value: &'a Expr,
    base_env: LowerEnv,
    then_env: Option<LowerEnv>,
    then_lowered: Vec<TStmt>,
    then_value_lowered: Option<TExpr>,
    else_lowered: Vec<TStmt>,
}

struct ResultHandlerAst<'a> {
    subject: &'a Expr,
    ok_pattern: &'a Pattern,
    ok_body: &'a [Stmt],
    ok_value: &'a Expr,
    err_pattern: &'a Pattern,
    err_body: &'a [Stmt],
    err_value: &'a Expr,
    terminal: &'a Expr,
    ok_cond_span: Span,
    err_cond_span: Span,
}

/// Recognize the parser's fixed two-pattern Result handler shape. The receiver
/// is evaluated once into an ordinary local before the existing `.Ok`/`.Err`
/// conditions are lowered, so every backend observes one source evaluation.
fn result_handler_ast(expr: &Expr) -> Option<ResultHandlerAst<'_>> {
    let Expr::If {
        cond: ok_cond,
        then_body: ok_body,
        then_value: ok_value,
        else_body: outer_else_body,
        else_value: outer_else_value,
        span: _,
    } = expr
    else {
        return None;
    };
    if !outer_else_body.is_empty() {
        return None;
    }
    let Expr::PatternTest {
        subject: ok_subject,
        pattern: ok_pattern @ Pattern::Ok { .. },
        span: ok_cond_span,
    } = ok_cond.as_ref()
    else {
        return None;
    };
    let Expr::If {
        cond: err_cond,
        then_body: err_body,
        then_value: err_value,
        else_body: err_else_body,
        else_value: terminal,
        span: _,
    } = outer_else_value.as_ref()
    else {
        return None;
    };
    if !err_else_body.is_empty() || !matches!(terminal.as_ref(), Expr::NoElse(_)) {
        return None;
    }
    let Expr::PatternTest {
        subject: err_subject,
        pattern: err_pattern @ Pattern::Err { .. },
        span: err_cond_span,
    } = err_cond.as_ref()
    else {
        return None;
    };
    // The parser clones the exact receiver into both pattern tests. A source
    // span is sufficient here: two distinct source expressions cannot occupy
    // the same span, while the synthetic local used below deliberately gets a
    // different span for the second read so this rewrite does not recurse.
    if ok_subject.span() != err_subject.span() {
        return None;
    }
    Some(ResultHandlerAst {
        subject: ok_subject,
        ok_pattern,
        ok_body,
        ok_value,
        err_pattern,
        err_body,
        err_value,
        terminal,
        ok_cond_span: *ok_cond_span,
        err_cond_span: *err_cond_span,
    })
}

fn lower_result_handler_expr(expr: &Expr, cx: &Cx, env: &mut LowerEnv) -> Option<TExpr> {
    let shape = result_handler_ast(expr)?;
    let base_env = clone_env(env);
    let fallback_subject = env.fallback_subject;
    env.fallback_subject = true;
    let subject = super::control_flow::lower_if_let_subject(shape.subject, cx, env, false);
    env.fallback_subject = fallback_subject;
    let temp = jet_format!("{jet_prefix}result_handler_{}", shape.subject.span().start);
    let temp_local = TLocal::generated(&temp);

    // Lower the two branches directly. Rebuilding a synthetic `Expr::If` here
    // loses the branch environment at the value tail: the worklist sees the
    // cloned pattern-test nodes, but the payload binding belongs to the
    // original branch. That made a valid payload tail lower as `Unit` while the
    // other branch retained its Result carrier. The statement-position path
    // below already uses this explicit condition/body/value sequence; keep the
    // value form on the same one-mechanism path.
    let (lowered, lowered_ty) = {
        let mut handler_env = clone_env(env);
        handler_env.bind(&temp, temp_local, Some(subject.ty.clone()));

        let ok_cond_expr = Expr::PatternTest {
            subject: Box::new(Expr::Ident(temp.clone(), shape.subject.span())),
            pattern: shape.ok_pattern.clone(),
            span: shape.ok_cond_span,
        };
        let mut ok_env = clone_env(&handler_env);
        let (ok_cond, ok_bindings, mut ok_body) =
            super::control_flow::lower_if_cond(&ok_cond_expr, cx, &mut ok_env);
        for (name, place, ty) in ok_bindings {
            ok_env.bind(&name, place, ty);
        }
        ok_body.extend(lower_stmts(shape.ok_body, cx, &mut ok_env));
        let ok_value = lower_expr(shape.ok_value, cx, &mut ok_env);

        let err_cond_expr = Expr::PatternTest {
            subject: Box::new(Expr::Ident(temp.clone(), shape.err_cond_span)),
            pattern: shape.err_pattern.clone(),
            span: shape.err_cond_span,
        };
        let mut err_env = clone_env(&handler_env);
        let (err_cond, err_bindings, mut err_body) =
            super::control_flow::lower_if_cond(&err_cond_expr, cx, &mut err_env);
        for (name, place, ty) in err_bindings {
            err_env.bind(&name, place, ty);
        }
        err_body.extend(lower_stmts(shape.err_body, cx, &mut err_env));
        let err_value = lower_expr(shape.err_value, cx, &mut err_env);
        let terminal = lower_expr(shape.terminal, cx, &mut handler_env);

        let inner_ty = tir_if_join_type(
            tir_if_branch_reaches_merge(&err_body, &err_value),
            &err_value.ty,
            tir_expr_reaches_merge(&terminal),
            &terminal.ty,
        );
        let inner = TExpr {
            ty: inner_ty,
            kind: TExprKind::IfExpr {
                cond: Box::new(err_cond),
                then_body: err_body,
                then_value: Box::new(err_value),
                else_body: Vec::new(),
                else_value: Box::new(terminal),
            },
        };
        let outer_ty = tir_if_join_type(
            tir_if_branch_reaches_merge(&ok_body, &ok_value),
            &ok_value.ty,
            tir_expr_reaches_merge(&inner),
            &inner.ty,
        );
        (
            TExpr {
                ty: outer_ty.clone(),
                kind: TExprKind::IfExpr {
                    cond: Box::new(ok_cond),
                    then_body: ok_body,
                    then_value: Box::new(ok_value),
                    else_body: Vec::new(),
                    else_value: Box::new(inner),
                },
            },
            outer_ty,
        )
    };
    *env = base_env;
    Some(canonicalize_pre_tier_expr(TExpr {
        ty: lowered_ty,
        kind: TExprKind::InlineBlock(vec![
            TStmt::Let {
                name: temp,
                kw: "let",
                let_ty: crate::Codegen::TIR::TLetTy::inferred(),
                init: subject,
                gc_promotion: None,
                gc_transferred: false,
            },
            TStmt::ExprStmt(lowered),
        ]),
    }))
}

fn dispatch_condition_subject(expr: &Expr) -> Option<&Expr> {
    match expr.without_parens() {
        Expr::PatternTest { subject, .. } => Some(subject),
        Expr::Binary(op, left, _, _) if op.is_comparison() => Some(left),
        Expr::Binary(BinOp::And | BinOp::Or, left, right, _) => {
            dispatch_condition_subject(left).or_else(|| dispatch_condition_subject(right))
        }
        _ => None,
    }
}

fn replace_dispatch_subject(expr: &Expr, subject_span: Span, replacement: &Expr) -> Expr {
    match expr {
        Expr::PatternTest {
            subject,
            pattern,
            span,
        } if subject.span() == subject_span => Expr::PatternTest {
            subject: Box::new(replacement.clone()),
            pattern: pattern.clone(),
            span: *span,
        },
        Expr::Binary(op, left, right, span)
            if op.is_comparison() && left.span() == subject_span =>
        {
            Expr::Binary(*op, Box::new(replacement.clone()), right.clone(), *span)
        }
        Expr::Binary(op @ (BinOp::And | BinOp::Or), left, right, span) => Expr::Binary(
            *op,
            Box::new(replace_dispatch_subject(left, subject_span, replacement)),
            Box::new(replace_dispatch_subject(right, subject_span, replacement)),
            *span,
        ),
        _ => expr.clone(),
    }
}

/// Value-form dispatch is parsed as an `Expr::If` chain. When two or more
/// arms test the same subject, retain that subject once for the whole chain;
/// lowering each range arm independently would repeat calls and side effects.
fn discarded_dispatch_subject(expr: &Expr) -> Option<&Expr> {
    if result_handler_ast(expr).is_some() {
        return None;
    }
    let Expr::If {
        cond, else_value, ..
    } = expr.without_parens()
    else {
        return None;
    };
    let subject = dispatch_condition_subject(cond)?;
    let subject_span = subject.span();
    let mut matched = 1usize;
    let mut tail = else_value.without_parens();
    while let Expr::If {
        cond, else_value, ..
    } = tail
    {
        let Some(next_subject) = dispatch_condition_subject(cond) else {
            break;
        };
        if next_subject.span() != subject_span {
            break;
        }
        matched += 1;
        tail = else_value.without_parens();
    }
    (matched >= 2).then_some(subject)
}

enum DiscardedExprWork<'a> {
    Enter {
        expr: &'a Expr,
        env: LowerEnv,
    },
    FinishIf {
        cond: TIfCond,
        then_body: Vec<TStmt>,
        else_body: Option<Vec<TStmt>>,
    },
    FinishResultHandler {
        temp: String,
        subject: TExpr,
        ok_cond: TIfCond,
        ok_body: Vec<TStmt>,
        err_cond: TIfCond,
        err_body: Vec<TStmt>,
    },
}

fn is_single_if_body(body: &[TStmt]) -> bool {
    body.len() == 1 && matches!(body.first(), Some(TStmt::If { .. }))
}

/// Lower an `if` expression whose enclosing source position discards its value.
/// Keep branch tails as expression statements, including compact Result handlers,
/// so discarded values never enter value-form branch unification. The worklist
/// keeps a deep else-if chain off the native stack.
pub(crate) fn lower_discarded_expr(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    let root = e.without_parens();
    let mut root_env = clone_env(env);
    let dispatch = discarded_dispatch_subject(root).map(|subject| {
        let name = jet_format!("{jet_prefix}switch_subject");
        let init = lower_expr(subject, cx, &mut root_env);
        let ty = init.ty.clone();
        root_env.bind(&name, TLocal::generated(&name), Some(ty.clone()));
        (name, subject.span(), ty, init)
    });
    // The replacement conditions below are short-lived AST nodes. Keep their
    // pointer-keyed expression-cache entries inside this lowering call so a
    // later web/native pass cannot observe an address-reused entry.
    let _dispatch_cache_scope = dispatch.as_ref().map(|_| ExprCacheScope::enter());
    let mut work = TirWorklist::new();
    let mut lowered = Vec::new();
    work.push(DiscardedExprWork::Enter {
        expr: root,
        env: root_env,
    });

    while let Some(task) = work.pop() {
        match task {
            DiscardedExprWork::Enter { expr, mut env } => {
                let expr = expr.without_parens();
                if let Some(shape) = result_handler_ast(expr) {
                    // These condition nodes are compiler-private, just like
                    // the value-form rewrite below. Keep their cache entries
                    // inside this task so no synthetic node address survives
                    // the discarded lowering.
                    let _cache_scope = ExprCacheScope::enter();
                    let fallback_subject = env.fallback_subject;
                    env.fallback_subject = true;
                    let subject = super::control_flow::lower_if_let_subject(
                        shape.subject,
                        cx,
                        &mut env,
                        false,
                    );
                    env.fallback_subject = fallback_subject;
                    let temp =
                        jet_format!("{jet_prefix}result_handler_{}", shape.subject.span().start);
                    let temp_local = TLocal::generated(&temp);
                    let mut handler_env = clone_env(&env);
                    handler_env.bind(&temp, temp_local, Some(subject.ty.clone()));

                    let ok_cond_expr = Expr::PatternTest {
                        subject: Box::new(Expr::Ident(temp.clone(), shape.subject.span())),
                        pattern: shape.ok_pattern.clone(),
                        span: shape.ok_cond_span,
                    };
                    let mut ok_env = clone_env(&handler_env);
                    let (ok_cond, ok_bindings, mut ok_body) =
                        super::control_flow::lower_if_cond(&ok_cond_expr, cx, &mut ok_env);
                    for (name, place, ty) in ok_bindings {
                        ok_env.bind(&name, place, ty);
                    }
                    ok_body.extend(lower_stmts(shape.ok_body, cx, &mut ok_env));

                    let err_cond_expr = Expr::PatternTest {
                        subject: Box::new(Expr::Ident(temp.clone(), shape.err_cond_span)),
                        pattern: shape.err_pattern.clone(),
                        span: shape.err_cond_span,
                    };
                    let mut err_env = clone_env(&handler_env);
                    let (err_cond, err_bindings, mut err_body) =
                        super::control_flow::lower_if_cond(&err_cond_expr, cx, &mut err_env);
                    for (name, place, ty) in err_bindings {
                        err_env.bind(&name, place, ty);
                    }
                    err_body.extend(lower_stmts(shape.err_body, cx, &mut err_env));

                    work.push(DiscardedExprWork::FinishResultHandler {
                        temp,
                        subject,
                        ok_cond,
                        ok_body,
                        err_cond,
                        err_body,
                    });
                    work.push(DiscardedExprWork::Enter {
                        expr: shape.terminal,
                        env: clone_env(&handler_env),
                    });
                    work.push(DiscardedExprWork::Enter {
                        expr: shape.err_value,
                        env: err_env,
                    });
                    work.push(DiscardedExprWork::Enter {
                        expr: shape.ok_value,
                        env: ok_env,
                    });
                    continue;
                }

                let Expr::If {
                    cond,
                    then_body,
                    then_value,
                    else_body,
                    else_value,
                    ..
                } = expr
                else {
                    lowered.push(TStmt::ExprStmt(lower_expr(expr, cx, &mut env)));
                    continue;
                };

                let mut then_env = clone_env(&env);
                let replaced_cond = dispatch.as_ref().and_then(|(name, subject_span, _, _)| {
                    dispatch_condition_subject(cond)
                        .filter(|subject| subject.span() == *subject_span)
                        .map(|_| {
                            replace_dispatch_subject(
                                cond,
                                *subject_span,
                                &Expr::Ident(name.clone(), *subject_span),
                            )
                        })
                });
                let cond = replaced_cond.as_ref().unwrap_or(cond);
                let (tir_cond, bindings, mut then_lowered) =
                    super::control_flow::lower_if_cond(cond, cx, &mut then_env);
                for (name, place, ty) in bindings {
                    then_env.bind(&name, place, ty);
                }
                then_lowered.extend(lower_stmts(then_body, cx, &mut then_env));

                let (else_lowered, else_env) = if else_body.is_empty()
                    && matches!(else_value.without_parens(), Expr::NoElse(_))
                {
                    (None, None)
                } else {
                    let mut else_env = clone_env(&env);
                    let else_lowered = lower_stmts(else_body, cx, &mut else_env);
                    (Some(else_lowered), Some(else_env))
                };
                work.push(DiscardedExprWork::FinishIf {
                    cond: tir_cond,
                    then_body: then_lowered,
                    else_body: else_lowered,
                });
                if let Some(else_env) = else_env {
                    work.push(DiscardedExprWork::Enter {
                        expr: else_value,
                        env: else_env,
                    });
                }
                work.push(DiscardedExprWork::Enter {
                    expr: then_value,
                    env: then_env,
                });
            }
            DiscardedExprWork::FinishIf {
                cond,
                mut then_body,
                else_body,
            } => {
                let (else_body, else_is_elseif) = match else_body {
                    Some(mut else_body) => {
                        let else_value = lowered.pop().expect("discarded if else tail was lowered");
                        let then_value = lowered.pop().expect("discarded if then tail was lowered");
                        then_body.push(then_value);
                        else_body.push(else_value);
                        let else_is_elseif = is_single_if_body(&else_body);
                        (Some(else_body), else_is_elseif)
                    }
                    None => {
                        let then_value = lowered.pop().expect("discarded if then tail was lowered");
                        then_body.push(then_value);
                        (None, false)
                    }
                };
                lowered.push(TStmt::If {
                    cond,
                    then_body,
                    else_body,
                    else_is_elseif,
                });
            }
            DiscardedExprWork::FinishResultHandler {
                temp,
                subject,
                ok_cond,
                mut ok_body,
                err_cond,
                mut err_body,
            } => {
                let terminal = lowered
                    .pop()
                    .expect("discarded Result handler terminal was lowered");
                let err_value = lowered
                    .pop()
                    .expect("discarded Result handler failure tail was lowered");
                let ok_value = lowered
                    .pop()
                    .expect("discarded Result handler success tail was lowered");
                ok_body.push(ok_value);
                err_body.push(err_value);
                let TStmt::ExprStmt(terminal) = terminal else {
                    unreachable!("discarded Result handler terminal must lower to an expression");
                };
                // Keep the existing Result-handler expression shape so the
                // emitter's one-match path consumes the carrier once. The
                // source tails are statements before a Unit tail, which
                // discards their values without asking Rust to unify them.
                let unit_tail = || TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Unit,
                };
                let inner = TExpr {
                    ty: unit_type(),
                    kind: TExprKind::IfExpr {
                        cond: Box::new(err_cond),
                        then_body: err_body,
                        then_value: Box::new(unit_tail()),
                        else_body: Vec::new(),
                        else_value: Box::new(terminal),
                    },
                };
                let outer = TExpr {
                    ty: unit_type(),
                    kind: TExprKind::IfExpr {
                        cond: Box::new(ok_cond),
                        then_body: ok_body,
                        then_value: Box::new(unit_tail()),
                        else_body: Vec::new(),
                        else_value: Box::new(inner),
                    },
                };
                lowered.push(TStmt::ExprStmt(canonicalize_pre_tier_expr(TExpr {
                    ty: unit_type(),
                    kind: TExprKind::InlineBlock(vec![
                        TStmt::Let {
                            name: temp,
                            kw: "let",
                            let_ty: crate::Codegen::TIR::TLetTy::inferred(),
                            init: subject,
                            gc_promotion: None,
                            gc_transferred: false,
                        },
                        TStmt::ExprStmt(outer),
                    ]),
                })));
            }
        }
    }
    let root = lowered
        .pop()
        .expect("discarded expression worklist lost its root");
    let Some((name, _, ty, init)) = dispatch else {
        return root;
    };
    let TStmt::If {
        cond,
        then_body,
        else_body,
        else_is_elseif,
    } = root
    else {
        unreachable!("discarded dispatch root must lower to an if statement");
    };
    TStmt::If {
        cond: TIfCond::WithPrelude {
            prelude: vec![TStmt::Let {
                name,
                kw: "let",
                let_ty: crate::Codegen::TIR::TLetTy::plain(ty),
                init,
                gc_promotion: None,
                gc_transferred: false,
            }],
            cond: Box::new(cond),
        },
        then_body,
        else_body,
        else_is_elseif,
    }
}

fn condition_terms<'a>(cond: &'a Expr) -> Vec<&'a Expr> {
    let mut work = TirWorklist::new();
    work.push(cond);
    let mut terms = Vec::new();
    while let Some(term) = work.pop() {
        if let Expr::Binary(BinOp::And, left, right, _) = term {
            work.push(right);
            work.push(left);
        } else {
            terms.push(term);
        }
    }
    terms
}

fn push_expr_children<'a>(work: &mut Vec<ExprWork<'a>>, children: Vec<ExprWorkChild<'a>>) {
    for child in children.into_iter().rev() {
        match child {
            ExprWorkChild::Expr(child) => {
                let child = strip_expr_parens(child);
                work.push(ExprWork::Enter(child));
            }
            ExprWorkChild::Arg(arg) => work.push(ExprWork::EnterArg(arg)),
        }
    }
}

fn push_expr_condition_atom<'a>(
    work: &mut Vec<ExprWork<'a>>,
    expr: &'a Expr,
    cx: &Cx,
    env: &LowerEnv,
) {
    if matches!(expr, Expr::PatternTest { .. }) {
        push_expr_children(work, expr_children(expr, cx, env));
    } else {
        push_expr_work(work, expr, cx, env);
    }
}

fn push_expr_work<'a>(work: &mut Vec<ExprWork<'a>>, expr: &'a Expr, cx: &Cx, env: &LowerEnv) {
    let expr = strip_expr_parens(expr);
    if let Some(body) = inline_loop_body(expr) {
        work.push(ExprWork::LowerInlineLoop { expr, body });
    } else if matches!(expr, Expr::If { .. }) {
        work.push(ExprWork::BuildIf(expr));
    } else {
        work.push(ExprWork::Build(expr));
        push_expr_children(work, expr_children(expr, cx, env));
    }
}

fn lower_expr_segment<'a>(root: &'a Expr, cx: &'a Cx, env: &mut LowerEnv) -> TExpr {
    let root = strip_expr_parens(root);
    let mut work = vec![ExprWork::Enter(root)];
    while let Some(task) = work.pop() {
        match task {
            ExprWork::Enter(expr) => {
                let expr = strip_expr_parens(expr);
                push_expr_work(&mut work, expr, cx, env);
            }
            ExprWork::Build(expr) => {
                expr_cache_put(expr, lower_expr_node(expr, cx, env), cx);
            }
            ExprWork::EnterArg(arg) => {
                let expr = strip_expr_parens(&arg.arg.expr);
                let has_binder_refs = !arg.arg.flags.binder_refs.is_empty();
                work.push(ExprWork::BuildArg(arg));
                // Default expressions carry declaration-slot references. Lower
                // them as one argument under that slot mapping; pre-lowering
                // their children would resolve the private names as ordinary
                // locals before `lower_call_arg_value` installs the mapping.
                // A Try/?? subject has the same cache-boundary rule: its
                // argument must be lowered once in normal value context below.
                if has_binder_refs || env.fallback_subject {
                    continue;
                }
                if let Some(body) = inline_loop_body(expr) {
                    work.push(ExprWork::LowerInlineLoop { expr, body });
                } else if matches!(expr, Expr::If { .. }) {
                    work.push(ExprWork::BuildIf(expr));
                } else {
                    push_expr_children(&mut work, expr_children(expr, cx, env));
                }
            }
            ExprWork::BuildArg(arg) => {
                // A lambda is a lazy argument, not a strict child value. Keep its
                // lowering in the call site so special callbacks can apply their
                // host-borrow/value contract exactly once; caching a context-free
                // lambda here would force that call site to lower it a second time.
                if matches!(strip_expr_parens(&arg.arg.expr), Expr::Lambda(_)) {
                    continue;
                }
                let lower_value = |env: &mut LowerEnv| match &arg.mode {
                    ExprArgMode::Plain => lower_expr(&arg.arg.expr, cx, env),
                    ExprArgMode::Convention(conv) => {
                        crate::Codegen::TIR::lower_call_arg_value(arg.arg, conv.clone(), env, cx)
                    }
                    ExprArgMode::CallValue { callee, index } => {
                        let callee_ty = expr_cache_type(callee, cx)
                            .or_else(|| Some(lower_expr(callee, cx, env).ty));
                        let conv = callee_ty.and_then(|ty| match ty.with_effective_fn_returns() {
                            Type::Fn { params, .. } => params
                                .get(*index)
                                .cloned()
                                .map(|ty| (AccessConvention::Read, ty)),
                            _ => None,
                        });
                        crate::Codegen::TIR::lower_call_arg_value(arg.arg, conv, env, cx)
                    }
                };
                let value = if env.fallback_subject {
                    let _arg_cache_scope = ExprCacheScope::enter();
                    let fallback_subject = env.fallback_subject;
                    env.fallback_subject = false;
                    let value = lower_value(env);
                    env.fallback_subject = fallback_subject;
                    value
                } else {
                    lower_value(env)
                };
                expr_cache_put(
                    strip_expr_parens(&arg.arg.expr),
                    canonicalize_pre_tier_expr(value),
                    cx,
                );
            }
            ExprWork::LowerInlineLoop { expr, body } => {
                let mut block_env = clone_env(env);
                let lowered = lower_stmts(body, cx, &mut block_env);
                let Expr::CallValue { callee, .. } = expr else {
                    unreachable!("inline loop work item requires a call value")
                };
                let Expr::Lambda(lam) = callee.as_ref() else {
                    unreachable!("inline loop work item requires a lambda")
                };
                let ty = if lam.meta.collecting_loop {
                    Type::List(Box::new(
                        lam.meta.collect_item_type.clone().unwrap_or(Type::Int),
                    ))
                } else {
                    lam.meta.loop_result_type.clone().unwrap_or(Type::Int)
                };
                expr_cache_put(
                    expr,
                    canonicalize_pre_tier_expr(TExpr {
                        ty,
                        kind: TExprKind::InlineBlock(lowered),
                    }),
                    cx,
                );
            }
            ExprWork::BuildIf(expr) => {
                if let Some(handler) = lower_result_handler_expr(expr, cx, env) {
                    expr_cache_put(expr, handler, cx);
                    continue;
                }
                let Expr::If {
                    cond,
                    then_body,
                    then_value,
                    else_body,
                    else_value,
                    ..
                } = expr
                else {
                    unreachable!("if work item requires an if expression")
                };
                let terms = condition_terms(cond);
                let state = ExprIfConditionWork {
                    expr,
                    then_body,
                    then_value,
                    else_body,
                    else_value,
                    terms,
                    next: 0,
                    lowered: Vec::new(),
                    bindings: Vec::new(),
                    prefixes: Vec::new(),
                    base_env: clone_env(env),
                };
                let first = state
                    .terms
                    .first()
                    .copied()
                    .expect("if condition has one or more terms");
                work.push(ExprWork::LowerIfCondition(Box::new(state)));
                push_expr_condition_atom(&mut work, first, cx, env);
            }
            ExprWork::LowerIfCondition(mut state) => {
                let term = state.terms[state.next];
                let (lowered, binding, prefix) =
                    super::control_flow::lower_if_cond_atom_cached(term, cx, env);
                for (name, place, ty) in binding {
                    env.bind(&name, place.clone(), ty.clone());
                    state.bindings.push((name, place, ty));
                }
                state.lowered.push(lowered);
                state.prefixes.extend(prefix);
                state.next += 1;
                if let Some(next) = state.terms.get(state.next).copied() {
                    work.push(ExprWork::LowerIfCondition(state));
                    push_expr_condition_atom(&mut work, next, cx, env);
                    continue;
                }

                let ExprIfConditionWork {
                    expr,
                    then_body,
                    then_value,
                    else_body,
                    else_value,
                    lowered: lowered_terms,
                    bindings,
                    prefixes,
                    base_env,
                    ..
                } = *state;
                let mut lowered = lowered_terms.into_iter().rev();
                let mut condition = lowered
                    .next()
                    .expect("if condition has one or more lowered terms");
                for left in lowered {
                    condition = TIfCond::And {
                        left: Box::new(left),
                        right: Box::new(condition),
                    };
                }
                let mut then_env = clone_env(&base_env);
                for (name, place, ty) in &bindings {
                    then_env.bind(name, place.clone(), ty.clone());
                }
                let branch_env = clone_env(&then_env);
                let if_state = ExprIfWork {
                    expr,
                    condition,
                    then_prefix: prefixes,
                    then_body,
                    then_value,
                    else_body,
                    else_value,
                    base_env,
                    then_env: Some(then_env),
                    then_lowered: Vec::new(),
                    then_value_lowered: None,
                    else_lowered: Vec::new(),
                };
                *env = branch_env;
                work.push(ExprWork::LowerIfThenBody(Box::new(if_state)));
            }
            ExprWork::LowerIfThenBody(mut state) => {
                let mut branch_env = state
                    .then_env
                    .take()
                    .expect("if then environment is consumed once");
                state.then_lowered = lower_stmts(state.then_body, cx, &mut branch_env);
                *env = branch_env;
                let then_value = state.then_value;
                work.push(ExprWork::LowerIfThenValue(state));
                work.push(ExprWork::Enter(then_value));
            }
            ExprWork::LowerIfThenValue(mut state) => {
                state.then_value_lowered = Some(
                    expr_cache_take(state.then_value, cx)
                        .expect("if then value was lowered exactly once"),
                );
                state.then_env = Some(clone_env(env));
                *env = clone_env(&state.base_env);
                work.push(ExprWork::LowerIfElseBody(state));
            }
            ExprWork::LowerIfElseBody(mut state) => {
                state.else_lowered = lower_stmts(state.else_body, cx, env);
                let else_value = state.else_value;
                work.push(ExprWork::LowerIfElseValue(state));
                work.push(ExprWork::Enter(else_value));
            }
            ExprWork::LowerIfElseValue(state) => {
                let else_value = expr_cache_take(state.else_value, cx)
                    .expect("if else value was lowered exactly once");
                let then_value = state
                    .then_value_lowered
                    .expect("if then value is consumed once");
                let mut then_body = state.then_prefix;
                then_body.extend(state.then_lowered);
                let then_reaches_merge = tir_if_branch_reaches_merge(&then_body, &then_value);
                let else_reaches_merge =
                    tir_if_branch_reaches_merge(&state.else_lowered, &else_value);
                let ty = tir_if_join_type(
                    then_reaches_merge,
                    &then_value.ty,
                    else_reaches_merge,
                    &else_value.ty,
                );
                let value = canonicalize_pre_tier_expr(TExpr {
                    ty,
                    kind: TExprKind::IfExpr {
                        cond: Box::new(state.condition),
                        then_body,
                        then_value: Box::new(then_value),
                        else_body: state.else_lowered,
                        else_value: Box::new(else_value),
                    },
                });
                *env = state.base_env;
                expr_cache_put(state.expr, value, cx);
            }
        }
    }
    expr_cache_take(root, cx).expect("expression worklist lost its root")
}

/// Select the value type at a lowered `if` merge using the same reachability
/// policy as semantic inference.
///
/// Sema reports an incompatible live pair as E0124 and poisons the later tail
/// with a diverging recovery node, but codegen still walks that tree to collect
/// diagnostics. Keep the first live branch as the recovery type instead of
/// asserting on a malformed pair and turning a user error into an ICE.
fn tir_if_join_type(
    then_reaches_merge: bool,
    then_ty: &Type,
    else_reaches_merge: bool,
    else_ty: &Type,
) -> Type {
    match (then_reaches_merge, else_reaches_merge) {
        (false, true) => else_ty.clone(),
        (true, false) => then_ty.clone(),
        (true, true) => then_ty.clone(),
        (false, false) => then_ty.clone(),
    }
}

fn tir_if_branch_reaches_merge(body: &[TStmt], value: &TExpr) -> bool {
    tir_stmt_sequence_reaches_merge(body) && tir_expr_reaches_merge(value)
}

fn tir_stmt_sequence_reaches_merge(stmts: &[TStmt]) -> bool {
    // A lowered sequence can contain an explicit exit before its syntactic
    // tail. Check every statement so an unreachable tail cannot make a
    // divergent branch look live at the value merge.
    stmts.iter().all(tir_stmt_reaches_merge)
}

fn tir_stmt_reaches_merge(stmt: &TStmt) -> bool {
    match stmt {
        TStmt::Return(_) | TStmt::Break(_) | TStmt::BreakValue { .. } | TStmt::Continue(_) => false,
        TStmt::ExprStmt(expr) => tir_expr_reaches_merge(expr),
        TStmt::If {
            then_body,
            else_body,
            ..
        } => {
            tir_stmt_sequence_reaches_merge(then_body)
                || else_body
                    .as_deref()
                    .is_none_or(tir_stmt_sequence_reaches_merge)
        }
        TStmt::ContractScope { body, .. }
        | TStmt::TaskGroup { body, .. }
        | TStmt::Inline(body)
        | TStmt::Unsafe { body, .. }
        | TStmt::SentryPolicy { body, .. }
        | TStmt::Impure(body)
        | TStmt::Region(body)
        | TStmt::Layout { body, .. }
        | TStmt::ContextBlock { body, .. }
        | TStmt::Live { body }
        | TStmt::Shield { body }
        | TStmt::ScopeMember { body, .. }
        | TStmt::Transact { body, .. } => tir_stmt_sequence_reaches_merge(body),
        TStmt::GcEdit { stmt, .. } => tir_stmt_reaches_merge(stmt),
        TStmt::EnumMatch {
            arms,
            else_body,
            fallthrough,
            ..
        } => {
            arms.iter()
                .any(|arm| tir_stmt_sequence_reaches_merge(&arm.body))
                || else_body
                    .as_deref()
                    .map(tir_stmt_sequence_reaches_merge)
                    .unwrap_or(!*fallthrough)
        }
        TStmt::RangeSwitch {
            arms, else_body, ..
        } => {
            arms.iter()
                .any(|(_, _, body)| tir_stmt_sequence_reaches_merge(body))
                || tir_stmt_sequence_reaches_merge(else_body)
        }
        TStmt::MixedSwitch {
            arms, else_body, ..
        } => {
            arms.iter()
                .any(|(_, body)| tir_stmt_sequence_reaches_merge(body))
                || else_body
                    .as_deref()
                    .is_none_or(tir_stmt_sequence_reaches_merge)
        }
        _ => true,
    }
}

fn tir_expr_reaches_merge(expr: &TExpr) -> bool {
    if matches!(&expr.ty, Type::Named(name) if name == Syntax::TYPE_NEVER) {
        return false;
    }
    match &expr.kind {
        TExprKind::Unreachable { .. } | TExprKind::Todo { .. } => false,
        TExprKind::RequireStop { always_stops, .. } => !always_stops,
        TExprKind::IfExpr {
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            tir_if_branch_reaches_merge(then_body, then_value)
                || tir_if_branch_reaches_merge(else_body, else_value)
        }
        TExprKind::InlineBlock(stmts) => tir_stmt_sequence_reaches_merge(stmts),
        TExprKind::Try { inner, .. } => tir_expr_reaches_merge(inner),
        TExprKind::OrFallback { value, fallback } => {
            tir_expr_reaches_merge(value)
                || match fallback {
                    TOrFallback::Value(fallback) => tir_expr_reaches_merge(fallback),
                    TOrFallback::Return(_)
                    | TOrFallback::Panic { .. }
                    | TOrFallback::Break
                    | TOrFallback::Continue
                    | TOrFallback::BreakLabel(_)
                    | TOrFallback::ContinueLabel(_) => false,
                }
        }
        _ => true,
    }
}

/// D-TAG1: recognize a leaf unit variant reached through a grouped value path
/// such as `Damage.Fire.Burn`. The AST shape is a field chain, but the Rust
/// value is one flat enum variant (`__jet_Damage::__jet_Fire__Burn`).
pub(crate) fn grouped_enum_unit_variant(
    cx: &Cx,
    receiver: &Expr,
    member: &str,
    is_bound: impl Fn(&str) -> bool,
) -> Option<(String, String)> {
    fn collect_path(expr: &Expr, path: &mut Vec<String>) -> bool {
        match expr {
            Expr::Ident(name, _) => {
                path.push(name.clone());
                true
            }
            Expr::Field(base, member, _) => {
                if !collect_path(base, path) {
                    return false;
                }
                path.push(member.clone());
                true
            }
            Expr::Paren(inner, _) => collect_path(inner, path),
            _ => false,
        }
    }

    let mut path = Vec::new();
    if !collect_path(receiver, &mut path) {
        return None;
    }
    path.push(member.to_string());
    let enum_name = path.first()?.clone();
    if path.len() < 3 || is_bound(&enum_name) {
        return None;
    }
    let variant = path[1..].join(".");
    if cx.variant_owner.get(&variant).map(String::as_str) != Some(enum_name.as_str()) {
        return None;
    }
    let is_unit = cx.enum_variants.get(&enum_name).is_some_and(|variants| {
        variants.iter().any(|(name, payload)| {
            name == &variant && matches!(payload, crate::AST::VariantPayload::Unit)
        })
    });
    is_unit.then_some((enum_name, variant))
}
#[inline(never)]
pub(crate) fn lower_expr(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let before_payload = CanonicalPass::enabled()
        .then(|| CanonicalPass::debug_payload("ast", "tir.lower-expression", e));
    let before_identity = CanonicalPass::enabled()
        .then(|| CanonicalPass::debug_identity("ast", "tir.lower-expression", e));
    let lowered = lower_expr_impl(e, cx, env);
    if let (Some(before_payload), Some(before_identity)) = (before_payload, before_identity) {
        CanonicalPass::record(
            "lowering",
            "tir.lower-expression",
            "crates/jet-codegen/src/Codegen/TIR/lower/expressions.rs",
            "ast",
            before_payload,
            before_identity,
            "tir",
            crate::Codegen::TIR::canonical_expression_payload(&lowered),
            crate::Codegen::TIR::canonical_expression_identity(&lowered),
            "preserve",
        );
    }
    lowered
}

#[inline(never)]
fn lower_expr_impl(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let _comptime_cache_guard = ExprComptimeCacheGuard::enter(env);
    // Keep expression descent off the native stack. Value-if and inline-loop nodes
    // use the same continuation worklist as every other expression.
    let e = strip_expr_parens(e);
    // A cached worklist value may have been produced before a default argument's
    // binder mapping was installed. Resolve the private name first so cache
    // reuse cannot bypass the declaration-slot substitution.
    if let Expr::Ident(name, _) = e {
        if let Some((temp, ty)) = env.binder_ref(name).cloned() {
            return TExpr {
                ty,
                kind: TExprKind::Local(TLocal::user(temp)),
            };
        }
    }
    if let Some(value) = expr_cache_take(e, cx) {
        // A nested lowering context can rebind the same source identifier,
        // notably a lambda capture, after the outer worklist cached its local
        // read. Refresh the structured local and type from the active
        // environment before using that cached value.
        if let Expr::Ident(name, _) = e {
            if matches!(&value.kind, TExprKind::Local(_)) {
                if let Some(ty) = env.ty_of(name) {
                    return TExpr {
                        ty,
                        kind: TExprKind::Local(env.local_of(name)),
                    };
                }
            }
        }
        return canonicalize_pre_tier_expr(value);
    }
    let cache_owner = ExprCacheOwner {
        owns_cache: expr_cache_begin(),
    };
    let value = lower_expr_segment(e, cx, env);
    let value = canonicalize_pre_tier_expr(value);
    drop(cache_owner);
    value
}

/// D-BOUND-HEAD1=A: comptime can lower a typed head before sema has rewritten
/// it to the ordinary alternating literal/hole call. Keep that early path on
/// the same TIR host node used after sema. Evaluation callers validate the
/// typed boundary before entering this lowering seam.
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
    let kind = Syntax::typed_head_kind(type_name).filter(|kind| kind.is_boundary())?;
    let ty = Type::Named(kind.internal_type_name().to_string());
    Some(TExpr {
        ty,
        kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::TypedTextInterp {
            kind,
            literals,
            holes,
        })),
    })
}

/// D-BYTELIT1=B: mirror sema's byte-text rewrite for raw comptime fragments
/// that reach TIR before ordinary typed-literal elaboration.
fn byte_text_exprs(parts: &[crate::AST::ByteTextPart], span: Span) -> Option<Vec<Expr>> {
    let bytes = TypedLitBody::byte_text_bytes(parts)?;
    Some(
        bytes
            .into_iter()
            .map(|byte| Expr::Int(byte as i64, span, None, Some(byte.to_string())))
            .collect(),
    )
}

fn lower_unit_text(value: TExpr, style: crate::AST::UnitFormat, cx: &Cx) -> TExpr {
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
    } else if let Some((base, _)) = original_ty.quantity_parts() {
        TExpr {
            ty: base.clone(),
            ..value
        }
    } else {
        value
    };
    let mut parts = vec![TStrPart::Interp(raw, crate::AST::StrFormat::Display)];
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
            .or_else(|| {
                cx.quantity_dimension(&original_ty)
                    .map(|dimension| cx.quantity_unit_label(dimension, style))
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
pub(crate) fn join_print_args(args: &[crate::AST::CallArg], cx: &Cx, env: &mut LowerEnv) -> TExpr {
    join_print_values(args.iter().map(|arg| lower_expr(&arg.expr, cx, env)), cx)
}

pub(crate) fn join_print_values(values: impl IntoIterator<Item = TExpr>, cx: &Cx) -> TExpr {
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
    // D-TYPE2-IMAG1=A: the precise `Complex` carrier has no scalar print ABI on
    // any tier — it is a two-`f64` value AOT renders through `JetShow for
    // JetComplex` and the resident/web engines render through a host handle. Name
    // that render once here, as the shared Prelude `Complex.to_string` call every
    // tier already lowers (`jet_complex_to_string`, the same `to_string_rep` the
    // AOT trait impl and the comptime evaluator use), so no engine grows a
    // private display rule for it. Without this, `print(z)` on a `Complex` left
    // TIR with a `Print` whose payload type has no print dispatch, which the
    // resident JIT can only report as a compile gap and deopt on.
    if matches!(&value.ty, Type::Named(name) if name == Syntax::TYPE_COMPLEX)
        && !cx.type_names.contains(Syntax::TYPE_COMPLEX)
    {
        return TExpr {
            ty: Type::String,
            kind: TExprKind::PreciseBuiltin {
                type_name: Syntax::TYPE_COMPLEX.to_string(),
                func: "to_string".to_string(),
                args: vec![value],
            },
        };
    }
    // D-TYPE2-NUM1=A: exact `Int / Int` produces a `Fraction` carrier. Keep
    // display on the shared precise Prelude path so interpolation does not ask
    // the resident engine to treat the opaque rational handle as a user record.
    if matches!(&value.ty, Type::Named(name) if name == Syntax::TYPE_FRACTION)
        && !cx.type_names.contains(Syntax::TYPE_FRACTION)
    {
        return TExpr {
            ty: Type::String,
            kind: TExprKind::PreciseBuiltin {
                type_name: Syntax::TYPE_FRACTION.to_string(),
                func: "to_string".to_string(),
                args: vec![value],
            },
        };
    }
    let Type::Named(name) = &value.ty else {
        return value;
    };
    if cx.has_display_type(name) {
        return TExpr {
            ty: Type::String,
            kind: TExprKind::MethodCall {
                recv: Box::new(value),
                method: TMethodRef::trait_method("Display", "display"),
                type_args: Vec::new(),
                args: Vec::new(),
                source_first_string_literal: None,
                operator_line: None,
            },
        };
    }
    if cx.unit_label(&value.ty).is_none() {
        return value;
    }
    lower_unit_text(value, crate::AST::UnitFormat::Symbol, cx)
}

/// Preserve the checked Printable value for the shared Print operation.
///
/// Only an explicit Display capability is converted to text here.  Printable
/// aggregates and scalar values remain typed so each backend can invoke the
/// canonical JetShow implementation instead of silently selecting Display.
fn lower_print_value(value: TExpr, cx: &Cx) -> TExpr {
    let explicit_display = matches!(&value.ty, Type::Named(name) if cx.has_display_type(name));
    if explicit_display {
        lower_display_value(value, cx)
    } else {
        value
    }
}

/// Lower a variadic print as ordered one-value Print operations.
///
/// Sema checks every argument independently and specifies one output line per
/// argument.  Keeping one Print node per argument preserves both contracts
/// without forcing a Printable-only value through Display.
pub(crate) fn print_values(values: impl IntoIterator<Item = TExpr>, cx: &Cx, site: usize) -> TExpr {
    let values: Vec<TExpr> = values
        .into_iter()
        .map(|value| lower_print_value(value, cx))
        .collect();
    let mut stmts = Vec::with_capacity(values.len() * 2);
    let mut locals = Vec::with_capacity(values.len());
    for (index, value) in values.into_iter().enumerate() {
        let temp = jet_format!("{jet_prefix}print_arg_{site}_{index}");
        let local = TLocal::generated(&temp);
        locals.push((local.clone(), value.ty.clone()));
        stmts.push(TStmt::Let {
            name: temp,
            kw: "let",
            let_ty: crate::Codegen::TIR::TLetTy::inferred(),
            init: value,
            gc_promotion: None,
            gc_transferred: false,
        });
    }
    for (local, ty) in locals {
        let value = TExpr {
            ty,
            kind: TExprKind::Local(local),
        };
        stmts.push(TStmt::ExprStmt(TExpr {
            ty: unit_type(),
            kind: TExprKind::Print(Box::new(value)),
        }));
    }
    TExpr {
        ty: unit_type(),
        kind: TExprKind::InlineBlock(stmts),
    }
}

pub(crate) fn print_args(
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
    site: usize,
) -> TExpr {
    print_values(
        args.iter().map(|arg| lower_expr(&arg.expr, cx, env)),
        cx,
        site,
    )
}

/// D-FMT-PRETTY1=A: convert a Debug-capable value to canonical Debug text
/// before the shared Prelude formatter sees it. This keeps the formatter's
/// only input a String while preserving one value-to-text path across tiers.
pub(crate) fn lower_debug_text(value: TExpr) -> TExpr {
    TExpr {
        ty: Type::String,
        kind: TExprKind::StrLit(vec![TStrPart::Interp(value, crate::AST::StrFormat::Debug)]),
    }
}

fn lower_fmt_call(method: &str, args: Vec<TExpr>, source_span: crate::Diagnostics::Span) -> TExpr {
    let widen_to_vec = vec![false; args.len()];
    core_call_expr(
        Type::String,
        "core.text.fmt",
        method,
        args,
        source_span,
        widen_to_vec,
    )
}

fn lower_fmt_int(value: i64) -> TExpr {
    TExpr {
        ty: Type::Int,
        kind: TExprKind::IntLit(value, None),
    }
}

fn lower_fmt_string(value: String) -> TExpr {
    TExpr {
        ty: Type::String,
        kind: TExprKind::StrLit(vec![TStrPart::Lit(value)]),
    }
}

/// Default `Err` fields that were omitted as `None` still have a checked type
/// (`?String` / `?Err`). Bare `Expr::Absent` otherwise lowers as `?Int`, and
/// AOT then annotates the temp as `JetOutcome<i64, JetAbsent>` — which cannot
/// fill `jet_err`.
fn lower_default_err_field(name: &str, value: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    match (name, strip_expr_parens(value)) {
        ("code", Expr::Absent(_)) => TExpr {
            ty: Type::Option(Box::new(Type::String)),
            kind: TExprKind::Absent,
        },
        ("cause", Expr::Absent(_)) => TExpr {
            ty: Type::Option(Box::new(Type::Named(Syntax::TYPE_ERR.to_string()))),
            kind: TExprKind::Absent,
        },
        _ => lower_expr(value, cx, env),
    }
}

/// D-FAIL-ERROR1=A: top-level comptime values are lowered before sema has
/// rewritten raw `Err(...)` calls to `Expr::Err` plus the default `Err` struct.
/// Normalize that one early shape here so the comptime evaluator consumes the
/// same TIR carrier as the post-sema AOT/JIT paths.
fn lower_raw_err_value(call: &Call, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let message = call
        .args
        .first()
        .filter(|arg| arg.label.is_none())
        .map(|arg| lower_expr(&arg.expr, cx, env))
        .unwrap_or_else(|| TExpr {
            ty: Type::String,
            kind: TExprKind::StrLit(vec![TStrPart::Lit(String::new())]),
        });
    let optional = |value: Option<TExpr>, fallback: Type| match value {
        Some(value) => TExpr {
            ty: Type::Option(Box::new(value.ty.clone())),
            kind: TExprKind::Present(Box::new(value)),
        },
        None => TExpr {
            ty: Type::Option(Box::new(fallback)),
            kind: TExprKind::Absent,
        },
    };
    let code = call
        .args
        .iter()
        .find(|arg| arg.label.as_ref().is_some_and(|(name, _)| name == "code"))
        .map(|arg| lower_expr(&arg.expr, cx, env));
    let cause = call
        .args
        .iter()
        .find(|arg| arg.label.as_ref().is_some_and(|(name, _)| name == "cause"))
        .map(|arg| match strip_expr_parens(&arg.expr) {
            Expr::Call(nested) if nested.name == Syntax::LIT_ERR => {
                lower_raw_err_value(nested, cx, env)
            }
            _ => lower_expr(&arg.expr, cx, env),
        });
    TExpr {
        ty: Type::Named(Syntax::TYPE_ERR.to_string()),
        kind: TExprKind::StructLit {
            fields: vec![
                ("message".to_string(), message, false),
                ("code".to_string(), optional(code, Type::String), false),
                (
                    "cause".to_string(),
                    optional(cause, Type::Named(Syntax::TYPE_ERR.to_string())),
                    false,
                ),
            ],
            extra: None,
            as_trait: None,
        },
    }
}
/// D-FAIL-OK1=A: top-level comptime function bodies can reach TIR before sema
/// rewrites raw `Ok(...)` calls to `Expr::Ok`. Keep that early shape on the
/// same Result carrier path as the post-sema constructor node.
fn lower_raw_ok_call(call: &Call, cx: &Cx, env: &mut LowerEnv) -> Option<TExpr> {
    if call.name != Syntax::LIT_OK
        || cx.sigs.contains_key(&call.name)
        || env.locals.contains_key(&call.name)
    {
        return None;
    }
    let mut payload = call
        .args
        .first()
        .map(|arg| lower_owned_expr(&arg.expr, cx, env))
        .unwrap_or_else(|| TExpr {
            ty: unit_type(),
            kind: TExprKind::Unit,
        });
    let (ok_ty, err_ty) = match env.ret_ty.as_ref() {
        Some(Type::Result { ok, err }) => {
            payload = preserve_typed_list_shape(payload, ok, cx);
            payload = crate::Codegen::TIR::maybe_widen_expr_to_union(payload, ok);
            ((**ok).clone(), (**err).clone())
        }
        _ => (
            payload.ty.clone(),
            Type::Named(Syntax::TYPE_ERR.to_string()),
        ),
    };
    Some(TExpr {
        ty: Type::Result {
            ok: Box::new(ok_ty),
            err: Box::new(err_ty),
        },
        kind: TExprKind::Ok(Box::new(payload)),
    })
}
/// D-FAIL-ENUM1=A: a leading-dot `.Ok(...)`/`.Err(...)` can reach a comptime
/// fragment before sema rewrites the contextual enum literal to `Expr::Ok` or
/// `Expr::Err`. Reuse the raw-call carrier lowering instead of routing Result
/// through user-enum payload layout.
fn lower_raw_contextual_result_variant(
    variant: &str,
    args: &[EnumLitArg],
    span: Span,
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    if !matches!(env.ret_ty, Some(Type::Result { .. }))
        || (variant != Syntax::LIT_OK && variant != Syntax::LIT_ERR)
    {
        return None;
    }
    let call = Call {
        name: variant.to_string(),
        name_span: span,
        type_args: Vec::new(),
        args: args
            .iter()
            .map(|arg| match arg {
                EnumLitArg::Positional(expr) => CallArg {
                    convention: AccessConvention::Move,
                    expr: expr.clone(),
                    span: expr.span(),
                    flags: Default::default(),
                    label: None,
                    spread: false,
                },
                EnumLitArg::Named { label, expr } => CallArg {
                    convention: AccessConvention::Move,
                    expr: expr.clone(),
                    span: expr.span(),
                    flags: Default::default(),
                    label: Some((label.clone(), expr.span())),
                    spread: false,
                },
            })
            .collect(),
        resolved_ret: None,
        range_checked: false,
        widen_approx: false,
    };
    if variant == Syntax::LIT_OK {
        lower_raw_ok_call(&call, cx, env)
    } else {
        lower_raw_err_call(&call, cx, env)
    }
}


fn lower_raw_err_call(call: &Call, cx: &Cx, env: &mut LowerEnv) -> Option<TExpr> {
    if call.name != Syntax::LIT_ERR
        || cx.sigs.contains_key(&call.name)
        || env.locals.contains_key(&call.name)
    {
        return None;
    }
    let target_error = match env.ret_ty.as_ref() {
        Some(Type::Result { err, .. }) => Some((**err).clone()),
        _ => None,
    };
    let default_error = target_error
        .as_ref()
        .is_none_or(|error| matches!(error, Type::Named(name) if name == Syntax::TYPE_ERR));
    if default_error {
        let value = lower_raw_err_value(call, cx, env);
        return Some(match target_error {
            Some(_) => TExpr {
                ty: Type::Result {
                    ok: Box::new(match env.ret_ty.as_ref() {
                        Some(Type::Result { ok, .. }) => (**ok).clone(),
                        _ => Type::Int,
                    }),
                    err: Box::new(value.ty.clone()),
                },
                kind: TExprKind::Err(Box::new(value)),
            },
            None => value,
        });
    }
    let payload = call
        .args
        .first()
        .map(|arg| lower_expr(&arg.expr, cx, env))
        .unwrap_or_else(|| TExpr {
            ty: Type::Named(Syntax::TYPE_ERR.to_string()),
            kind: TExprKind::StructLit {
                fields: Vec::new(),
                extra: None,
                as_trait: None,
            },
        });
    let payload = match target_error.as_ref() {
        Some(error) => crate::Codegen::TIR::maybe_widen_expr_to_union(payload, error),
        None => payload,
    };
    Some(TExpr {
        ty: Type::Result {
            ok: Box::new(match env.ret_ty.as_ref() {
                Some(Type::Result { ok, .. }) => (**ok).clone(),
                _ => Type::Int,
            }),
            err: Box::new(payload.ty.clone()),
        },
        kind: TExprKind::Err(Box::new(payload)),
    })
}

fn lower_positive_integer_literal(expr: &Expr, cx: &Cx, env: &mut LowerEnv) -> Option<TExpr> {
    match expr {
        Expr::Paren(inner, _) => lower_positive_integer_literal(inner, cx, env),
        Expr::Int(..) => Some(lower_expr(expr, cx, env)),
        _ => None,
    }
}

fn integer_literal_is_zero(expr: &Expr) -> bool {
    match expr {
        Expr::Paren(inner, _) => integer_literal_is_zero(inner),
        Expr::Int(value, _, _, raw) => {
            let exact = raw
                .as_deref()
                .and_then(|raw| jet_foundation::Numeric::CtBigInt::from_literal(raw).ok())
                .unwrap_or_else(|| jet_foundation::Numeric::CtBigInt::from_int(*value));
            exact.is_zero()
        }
        _ => false,
    }
}

fn lower_negative_power_exponent(expr: &Expr, cx: &Cx, env: &mut LowerEnv) -> Option<TExpr> {
    match expr {
        Expr::Paren(inner, _) => lower_negative_power_exponent(inner, cx, env),
        Expr::Unary(crate::AST::UnOp::Neg, inner, _) => {
            if integer_literal_is_zero(inner) {
                return None;
            }
            lower_positive_integer_literal(inner, cx, env)
        }
        Expr::Int(value, _, _, raw) => {
            let exact = raw
                .as_deref()
                .and_then(|raw| jet_foundation::Numeric::CtBigInt::from_literal(raw).ok())
                .unwrap_or_else(|| jet_foundation::Numeric::CtBigInt::from_int(*value));
            exact.negative.then(|| TExpr {
                ty: Type::Int,
                kind: TExprKind::CtLit(CtValue::BigInt(exact.abs())),
            })
        }
        _ => None,
    }
}

fn invariant_violation_expr(span: Span, construct: impl Into<String>) -> TExpr {
    TExpr {
        ty: Type::Named(Syntax::TYPE_NEVER.to_string()),
        kind: TExprKind::InvariantViolation {
            construct: construct.into(),
            span,
        },
    }
}

/// A compile-time name is an ordinary identifier whose mark is part of the
/// spelling. When sema has not yet baked `value`, look the binding up the same
/// way `Ident` does: a fragment local first, then the evaluator's const table.
fn lower_named_comptime_binding(name: &str, cx: &Cx, env: &LowerEnv) -> Option<TExpr> {
    if env.locals.contains_key(name) {
        return Some(TExpr {
            ty: env.ty_of(name).unwrap_or(Type::Int),
            kind: TExprKind::Local(env.local_of(name)),
        });
    }
    if !cx.consts.contains_key(name) && !cx.const_values.contains_key(name) {
        return None;
    }
    Some(in_own_frame(|| {
        let value = cx.const_values.get(name);
        let ty = env
            .ty_of(name)
            .or_else(|| value.map(crate::AST::CtValue::jet_type))
            .unwrap_or(Type::Int);
        TExpr {
            kind: lower_comptime_scalar(value, Some(&ty))
                .unwrap_or_else(|| TExprKind::ConstRef(name.to_string())),
            ty,
        }
    }))
}
pub(crate) fn core_call_expr(
    ty: Type,
    module: &str,
    member: &str,
    args: Vec<TExpr>,
    source_span: Span,
    widen_to_vec: Vec<bool>,
) -> TExpr {
    let Some(record) = Syntax::core_call(module, member) else {
        return invariant_violation_expr(source_span, format!("CoreCall `{module}.{member}`"));
    };
    let fallibility = TFailureCarrier::from_checked_type(&ty);
    let data_plan = match data_plan_for_core_call(record, &args, &ty, source_span) {
        Ok(plan) => plan,
        Err(error) => return invariant_violation_expr(source_span, error),
    };
    TExpr {
        ty,
        kind: TExprKind::CoreCall {
            record,
            args,
            source_span,
            type_args: Vec::new(),
            widen_to_vec,
            data_plan,
            fallibility,
        },
    }
}

#[inline(never)]
fn lower_expr_inner(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    if let Expr::PatternTest {
        subject, pattern, ..
    } = e
    {
        if let Some(value) = crate::Codegen::TIR::fold_typed_fact_enum_pattern(subject, pattern) {
            return TExpr {
                ty: Type::Bool,
                kind: TExprKind::BoolLit(value),
            };
        }
    }
    match e {
        Expr::Int(n, span, width, raw) => {
            in_own_frame(|| {
                // D-INTBIG1 / D-FIXED-CONSTRUCT1: the lexer keeps the raw
                // spelling when its i64 fast path overflows. A fixed-width
                // literal must cross the same exact-Int conversion seam as a
                // runtime `U64.from_int(...)` call; emitting `n` here would
                // turn the token's fallback zero into the program's value.
                if let Some(width) = width {
                    if let Some(raw) = raw.as_deref() {
                        let raw = raw.replace('_', "");
                        if let Ok(value) = jet_foundation::Numeric::CtBigInt::from_literal(&raw) {
                            if value.try_i64().is_none() {
                                let target = int_lit_type(&Some(*width));
                                let conversion =
                                    crate::Codegen::TIR::resolve_numeric_conversion_op(
                                        &target.name(),
                                        "Int",
                                    )
                                    .expect("fixed-width Int literal has a numeric conversion");
                                let TNumericOp::TryFrom {
                                    host_kind,
                                    dst_rust,
                                    dst_spelling,
                                } = conversion
                                else {
                                    unreachable!(
                                        "fixed-width Int literal conversion must be checked"
                                    );
                                };
                                return TExpr {
                                    ty: target,
                                    kind: TExprKind::NumericMethod {
                                        recv: Box::new(TExpr {
                                            ty: Type::Int,
                                            kind: TExprKind::CtLit(CtValue::BigInt(value)),
                                        }),
                                        op: TNumericOp::CheckedIntToFixed {
                                            host_kind,
                                            dst_rust,
                                            dst_spelling,
                                            line: crate::Diagnostics::span_line_col(
                                                &cx.src, span.start,
                                            )
                                            .0
                                                as u32,
                                        },
                                    },
                                };
                            }
                        }
                    }
                }
                // D-INTBIG1: the lexer preserves a decimal literal that does not
                // fit its token fast path. Keep it as a normal `Int` literal and
                // let the packed Prelude constructor own the spill.
                if width.is_none() {
                    if let Some(raw) = raw.as_deref() {
                        let raw = raw.replace('_', "");
                        if raw.parse::<i64>().is_err() {
                            if let Ok(value) = jet_foundation::Numeric::CtBigInt::from_literal(&raw)
                            {
                                return TExpr {
                                    ty: Type::Int,
                                    kind: TExprKind::CtLit(CtValue::BigInt(value)),
                                };
                            }
                        }
                    }
                }
                TExpr {
                    ty: int_lit_type(width),
                    kind: TExprKind::IntLit(*n, *width),
                }
            })
        }
        Expr::Float(v, _, is_f32, _) => TExpr {
            // D-FLOATW1: sema resolves F32 context and writes `is_f32=true` on the
            // node; carry that width through to TIR so emit produces the right suffix.
            ty: if *is_f32 { Type::Float32 } else { Type::Float },
            kind: TExprKind::FloatLit(*v),
        },
        Expr::Bool(b, _) => TExpr {
            ty: Type::Bool,
            kind: TExprKind::BoolLit(*b),
        },
        // D-VOID1=A: lower the public unit literal to the existing TIR Unit
        // value and internal Unit type.
        Expr::Unit(_) => TExpr {
            ty: unit_type(),
            kind: TExprKind::Unit,
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
        Expr::Str(parts, _) => in_own_frame(|| {
            let tparts = parts
                .iter()
                .map(|p| match p {
                    StrPart::Lit(s) => TStrPart::Lit(s.clone()),
                    StrPart::Interp(e, crate::AST::StrFormat::Fixed(precision)) => {
                        let formatted = lower_fmt_call(
                            "decimal",
                            vec![lower_expr(e, cx, env), lower_fmt_int(*precision)],
                            e.span(),
                        );
                        TStrPart::Interp(formatted, crate::AST::StrFormat::Display)
                    }
                    StrPart::Interp(e, crate::AST::StrFormat::Grouped(precision)) => {
                        let formatted = lower_fmt_call(
                            "grouped",
                            vec![lower_expr(e, cx, env), lower_fmt_int(*precision)],
                            e.span(),
                        );
                        TStrPart::Interp(formatted, crate::AST::StrFormat::Display)
                    }
                    StrPart::Interp(e, crate::AST::StrFormat::Hex(width)) => {
                        let formatted = lower_fmt_call(
                            "hex",
                            vec![lower_expr(e, cx, env), lower_fmt_int(*width)],
                            e.span(),
                        );
                        TStrPart::Interp(formatted, crate::AST::StrFormat::Display)
                    }
                    StrPart::Interp(e, crate::AST::StrFormat::Pad { width, fill }) => {
                        TStrPart::Interp(
                            lower_fmt_call(
                                "pad",
                                vec![
                                    lower_expr(e, cx, env),
                                    lower_fmt_int(*width),
                                    lower_fmt_string(fill.clone()),
                                ],
                                e.span(),
                            ),
                            crate::AST::StrFormat::Display,
                        )
                    }
                    StrPart::Interp(e, crate::AST::StrFormat::PadLeft { width, fill }) => {
                        TStrPart::Interp(
                            lower_fmt_call(
                                "pad_left",
                                vec![
                                    lower_expr(e, cx, env),
                                    lower_fmt_int(*width),
                                    lower_fmt_string(fill.clone()),
                                ],
                                e.span(),
                            ),
                            crate::AST::StrFormat::Display,
                        )
                    }
                    StrPart::Interp(e, crate::AST::StrFormat::Sci(precision)) => TStrPart::Interp(
                        lower_fmt_call(
                            "sci",
                            vec![lower_expr(e, cx, env), lower_fmt_int(*precision)],
                            e.span(),
                        ),
                        crate::AST::StrFormat::Display,
                    ),
                    StrPart::Interp(e, crate::AST::StrFormat::Percent(precision)) => {
                        TStrPart::Interp(
                            lower_fmt_call(
                                "percent",
                                vec![lower_expr(e, cx, env), lower_fmt_int(*precision)],
                                e.span(),
                            ),
                            crate::AST::StrFormat::Display,
                        )
                    }
                    StrPart::Interp(e, crate::AST::StrFormat::Bin) => TStrPart::Interp(
                        lower_fmt_call("bin", vec![lower_expr(e, cx, env)], e.span()),
                        crate::AST::StrFormat::Display,
                    ),
                    StrPart::Interp(e, crate::AST::StrFormat::Oct) => TStrPart::Interp(
                        lower_fmt_call("oct", vec![lower_expr(e, cx, env)], e.span()),
                        crate::AST::StrFormat::Display,
                    ),
                    StrPart::Interp(e, crate::AST::StrFormat::Unit(style)) => {
                        let value = lower_expr(e, cx, env);
                        TStrPart::Interp(
                            lower_unit_text(value, *style, cx),
                            crate::AST::StrFormat::Display,
                        )
                    }
                    StrPart::Interp(e, crate::AST::StrFormat::Pretty) => {
                        let value = lower_debug_text(lower_expr(e, cx, env));
                        TStrPart::Interp(
                            core_call_expr(
                                Type::String,
                                "core.text.fmt",
                                "pretty",
                                vec![value],
                                e.span(),
                                vec![false],
                            ),
                            crate::AST::StrFormat::Display,
                        )
                    }
                    StrPart::Interp(e, crate::AST::StrFormat::Display) => TStrPart::Interp(
                        lower_display_value(lower_expr(e, cx, env), cx),
                        crate::AST::StrFormat::Display,
                    ),
                    StrPart::Interp(e, fmt) => {
                        TStrPart::Interp(lower_expr(e, cx, env), fmt.clone())
                    }
                })
                .collect();
            TExpr {
                ty: Type::String,
                kind: TExprKind::StrLit(tparts),
            }
        }),
        Expr::Ident(name, _) => {
            in_own_frame(|| {
                if let Some((temp, ty)) = env.binder_ref(name).cloned() {
                    return TExpr {
                        ty,
                        kind: TExprKind::Local(TLocal::user(&temp)),
                    };
                }
                if let Some(ty) = cx.persist_types.get(name) {
                    return TExpr {
                        ty: ty.clone(),
                        kind: TExprKind::Local(
                            cx.persistent_local(name)
                                .expect("persist type and slot must agree"),
                        ),
                    };
                }
                if env.locals.contains_key(name) {
                    return TExpr {
                        ty: env.ty_of(name).unwrap_or(Type::Int),
                        kind: TExprKind::Local(env.local_of(name)),
                    };
                }
                // A compile-time constant is considered only after lexical
                // locals. Parameters and local bindings shadow a same-named
                // package constant just as every other lexical binding does.
                // parity: guard tests/tir_patterns_and_fields.rs::lexical_parameter_shadows_same_named_comptime_constant_on_all_tiers
                if cx.consts.contains_key(name) {
                    return in_own_frame(|| {
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
                    });
                }
                // c109 Phase 13: a bare function name used as a VALUE (not a local, not a
                // const) emits `emit_named_fn_value` — `Box::new(move |…| __jet_<name>(…))
                // as <fn-type>`. Mirrors `emit_expr`'s `Expr::Ident` arm (Expression.rs).
                if !env.locals.contains_key(name) && !cx.consts.contains_key(name) {
                    let fn_value_ty = cx
                        .fn_types
                        .get(name)
                        .or_else(|| cx.fn_source_types.get(name));
                    if let Some(ft @ Type::Fn { .. }) = fn_value_ty {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: ft.clone(),
                                kind: TExprKind::FnValue {
                                    kind: TFnValueKind::NamedFn {
                                        name: Some(name.clone()),
                                        lambda: None,
                                    },
                                },
                            };
                        });
                    }
                }
                let ty = env.ty_of(name).unwrap_or(Type::Int);
                if env.is_gc(name) {
                    return in_own_frame(|| {
                        return TExpr {
                            ty,
                            kind: TExprKind::HostCall(Box::new(
                                crate::Codegen::TIR::THostCall::GcRead {
                                    root: env.local_of(name),
                                },
                            )),
                        };
                    });
                }
                TExpr {
                    ty,
                    kind: TExprKind::Local(env.local_of(name)),
                }
            })
        }
        // Derive-template unit enums retain their comptime-substitution node so
        // canonical compiler facts can fold before dispatch. Non-fact enums
        // resume the same ordinary typed enum path as an author-written literal.
        Expr::ComptimeName {
            value:
                Some(crate::AST::CtValue::Enum {
                    type_name,
                    variant,
                    args,
                }),
            ..
        } if args.is_empty() => in_own_frame(|| {
            let resolved_type = cx
                .core_qualified_rust_type_name(type_name)
                .unwrap_or(type_name.as_str());
            TExpr {
                ty: Type::Named(resolved_type.to_string()),
                kind: TExprKind::EnumLit {
                    enum_type: resolved_type.to_string(),
                    variant: variant.clone(),
                    payload: TEnumPayload::Unit,
                },
            }
        }),
        Expr::ComptimeName {
            value: Some(value), ..
        } => {
            let ty = value.jet_type();
            let kind = lower_comptime_scalar(Some(value), Some(&ty))
                .unwrap_or_else(|| TExprKind::CtLit(value.clone()));
            TExpr { ty, kind }
        }
        Expr::ComptimeName { name, span: _, .. } => lower_named_comptime_binding(name, cx, env)
            .unwrap_or_else(|| TExpr {
                // Fragment lowering also lowers every user function as extra
                // context. A later function may mention a const still being
                // evaluated; keep a named reference instead of poisoning the
                // fragment with an invariant violation.
                ty: env.ty_of(name).unwrap_or(Type::Int),
                kind: TExprKind::ConstRef(name.clone()),
            }),
        // c109 Phase 13: a direct call THROUGH a fn-value (`Expr::CallValue`).
        // Sema's `.call(args)` projection joins this helper below. Function-type
        // parameters are unmarked, therefore Read under D-MEM-PARAM1.
        Expr::CallValue { callee, args, span } => lower_fn_value_call(
            Some(callee),
            lower_expr(callee, cx, env),
            args,
            span.start as u32,
            cx,
            env,
        ),
        Expr::Unary(op, inner, _) => in_own_frame(|| {
            let operand = lower_expr(inner, cx, env);
            let ty = operand.ty.clone();
            TExpr {
                ty,
                kind: TExprKind::Unary {
                    op: *op,
                    operand: Box::new(operand),
                },
            }
        }),
        Expr::IncDec {
            op,
            operand,
            postfix,
            ..
        } => in_own_frame(|| {
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
        }),
        // D-CAP9: postfix `p.*` deref. Result type is the pointer's element type.
        Expr::Deref(inner, _) => in_own_frame(|| {
            let operand = lower_expr(inner, cx, env);
            let ty = crate::Sema::ptr_elem(&operand.ty).unwrap_or_else(|| operand.ty.clone());
            TExpr {
                ty,
                kind: TExprKind::Deref(Box::new(operand)),
            }
        }),
        // D-CAP9: prefix `*x` raw-pointer-of. Result type is `*T` (`Ptr<T>`).
        Expr::RawOf(inner, _) => in_own_frame(|| {
            let operand = lower_expr(inner, cx, env);
            if matches!(tir_address_lifetime(&operand), TAddressLifetime::Stack) {
                env.note_stack_address();
            }
            let ty = crate::Sema::ptr_type(operand.ty.clone());
            TExpr {
                ty,
                kind: TExprKind::RawOf(Box::new(operand)),
            }
        }),
        // D-CAP2 (D-MEM1/S4): `~x` — a fresh, independent value. Keep this
        // signal distinct from compiler-inserted Clone nodes: Tensor's explicit
        // copy is a Prelude storage operation, while ordinary Clone shares its
        // Arc-backed storage.
        Expr::Copy(inner, copy_span) => {
            in_own_frame(|| {
                let operand = lower_expr(inner, cx, env);
                let view_owned_ty = match &operand.ty {
                    Type::Apply { name, args } if name == "View" && args.len() == 1 => Some(
                        if matches!(&args[0], Type::Named(element) if element == "str") {
                            Type::String
                        } else {
                            Type::List(Box::new(args[0].clone()))
                        },
                    ),
                    _ => None,
                };
                let is_view_type = view_owned_ty.is_some();
                let ty = view_owned_ty.unwrap_or_else(|| operand.ty.clone());
                // D-MEM-COPYSEM1: both written `~` and sema-inserted copies of
                // read-only views use the shared materialization path. A string
                // view's Rust place is a bare `&str`, so cloning it would return
                // another `&str`; the Prelude must produce the owned `String`.
                let is_view_copy = is_view_type
                    || matches!(&**inner, Expr::Ident(name, _) if env.is_string_view_local(name))
                    || matches!(
                        &**inner,
                        Expr::Ident(name, _)
                            if matches!(
                                env.split_view_handle(name),
                                Some(Type::Apply { name, .. }) if name == "View"
                            )
                    );
                // Parser-created `~` spans begin at the sigil. Sema-created
                // ownership clones reuse the operand span. Preserve that
                // existing provenance fact as a TIR distinction; do not inspect
                // source text or make a backend guess.
                let explicit = *copy_span != inner.span();
                let kind = if is_view_copy {
                    TExprKind::MaterializeView(Box::new(operand))
                } else if explicit {
                    env.note_clone(&ty);
                    TExprKind::ExplicitCopy(Box::new(operand))
                } else {
                    env.note_clone(&ty);
                    TExprKind::Clone(Box::new(operand))
                };
                TExpr { ty, kind }
            })
        }
        Expr::Place(inner, access, span) => {
            in_own_frame(|| {
                if let Expr::Slice {
                    base,
                    start,
                    end,
                    range,
                    ..
                } = inner.as_ref()
                {
                    let recv = if *access == crate::AST::PlaceAccess::Write {
                        lower_expr_as_mut_place(base, cx, env)
                    } else {
                        lower_expr(base, cx, env)
                    };
                    // Tensor, Vec<N>, and Matrix<M, N> all use the ranked compute
                    // Prelude. The foundation predicate is exact, so ordinary
                    // generics cannot accidentally enter the tensor-view path.
                    let is_tensor = recv.ty.is_compute_tensor_family();
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
                            name: if mutable && is_tensor {
                                "ComputeViewMut"
                            } else if mutable {
                                "ViewMut"
                            } else {
                                "View"
                            }
                            .to_string(),
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
                    let place = if *access == crate::AST::PlaceAccess::Write {
                        lower_expr_as_mut_place(inner, cx, env)
                    } else {
                        lower_expr(inner, cx, env)
                    };
                    TExpr {
                        ty: place.ty.clone(),
                        kind: TExprKind::Borrow {
                            place: Box::new(place),
                            mutable: *access == crate::AST::PlaceAccess::Write,
                        },
                    }
                }
            })
        }
        Expr::Binary(op, l, r, span) => {
            in_own_frame(|| {
                // D-FACT-ENUM-TIR: derive expansion has already substituted the
                // fact read with a typed comptime enum value. Fold it here at the
                // shared typed boundary; compiler-only fact enums never become a
                // runtime TExprKind::EnumLit.
                if let Some(value) = crate::Codegen::TIR::fold_typed_fact_enum_equality(*op, l, r) {
                    return TExpr {
                        ty: Type::Bool,
                        kind: TExprKind::BoolLit(value),
                    };
                }
                let lhs = lower_expr(l, cx, env);
                let mut rhs = lower_expr(r, cx, env);
                // Imported method signatures retain their declaration-module
                // nominal. Apply that canonical owner to a contextually typed enum
                // literal; its source node carries only the visible leaf.
                let expected_enum = match &lhs.ty {
                    Type::Named(name) | Type::Apply { name, .. } if name.contains("::") => {
                        Some(name.clone())
                    }
                    _ => None,
                };
                if let (
                    Some(expected_enum),
                    TExprKind::EnumLit {
                        enum_type, variant, ..
                    },
                ) = (expected_enum, &mut rhs.kind)
                {
                    let visible_owner_has_variant = cx
                        .enum_variants
                        .get(enum_type)
                        .is_some_and(|variants| variants.iter().any(|(name, _)| name == variant));
                    if crate::Codegen::nominal_leaf(enum_type)
                        == crate::Codegen::nominal_leaf(&expected_enum)
                        && visible_owner_has_variant
                    {
                        *enum_type = expected_enum;
                        rhs.ty = lhs.ty.clone();
                    }
                }
                // D-TYPE2-MEASURE1=A: Matrix multiplication carries the composed
                // outer measures in TIR while the shared compute Prelude owns the
                // one fallible runtime operation.
                if *op == BinOp::Mul {
                    if let (
                        Type::Apply { name: left, .. },
                        Type::Apply { name: right, .. },
                        Some([rows, inner]),
                        Some([right_inner, cols]),
                    ) = (
                        &lhs.ty,
                        &rhs.ty,
                        lhs.ty
                            .compute_shape_dimensions()
                            .and_then(|shape| <[u64; 2]>::try_from(shape).ok()),
                        rhs.ty
                            .compute_shape_dimensions()
                            .and_then(|shape| <[u64; 2]>::try_from(shape).ok()),
                    ) {
                        if left == "Matrix" && right == "Matrix" && inner == right_inner {
                            return in_own_frame(|| {
                                return core_call_expr(
                                    Type::Result {
                                        ok: Box::new(Type::compute_shape_type(
                                            "Matrix",
                                            &[rows, cols],
                                        )),
                                        err: Box::new(Type::Named("ComputeError".to_string())),
                                    },
                                    "core.compute",
                                    "matmul",
                                    vec![lhs, rhs],
                                    *span,
                                    vec![false, false],
                                );
                            });
                        }
                    }
                }
                // D-TYPE2-UNCERT1=A: sema has made exact operands explicit
                // zero-uncertainty measurements. Reuse the resident measurement
                // handle op so AOT, JIT, interpreter, comptime, REPL and web call
                // the same Prelude kernel.
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
                    && matches!((&lhs.ty, &rhs.ty), (
                        Type::Apply { name: left, .. },
                        Type::Apply { name: right, .. }
                    ) if left == Syntax::TYPE_MEASUREMENT && right == Syntax::TYPE_MEASUREMENT)
                {
                    return in_own_frame(|| {
                        return TExpr {
                            ty: lhs.ty.clone(),
                            kind: TExprKind::HandleMethod {
                                recv: Box::new(lhs),
                                op: THandleOp::MeasurementMethod {
                                    method: match op {
                                        BinOp::Add => "add",
                                        BinOp::Sub => "sub",
                                        BinOp::Mul => "mul",
                                        BinOp::Div => "div",
                                        _ => unreachable!(),
                                    }
                                    .to_string(),
                                },
                                args: vec![rhs],
                            },
                        };
                    });
                }
                // D-SHAPE-QUANTITY1=A: sema has already validated compatibility.
                // Multiplication/division unwrap nominal unit values and emit a
                // plain numeric operation; the normalized result type is retained
                // only as a TIR fact and `rust_type` erases it to the numeric base.
                let ldim = cx.quantity_dimension(&lhs.ty);
                let rdim = cx.quantity_dimension(&rhs.ty);
                if (ldim.is_some() || rdim.is_some()) && matches!(op, BinOp::Mul | BinOp::Div) {
                    return in_own_frame(|| {
                        let raw = |expr: TExpr| {
                            if cx.quantity_dimension(&expr.ty).is_some()
                                && matches!(expr.ty, Type::Named(_))
                            {
                                if matches!(&expr.ty, Type::Named(name) if name == crate::Syntax::DURATION_TYPE)
                                {
                                    return TExpr {
                                        ty: Type::Float,
                                        kind: TExprKind::HandleMethod {
                                            recv: Box::new(expr),
                                            op: THandleOp::DurationSecondsValue,
                                            args: Vec::new(),
                                        },
                                    };
                                }
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
                    });
                }
                // D-LAYOUT1 / D-LAYOUT-GATES1: layout-typed `+`/`-`/`>=`/`<=`/`==`.
                // Recompute via the SAME table sema used (mirrors the math/Int
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
                            return in_own_frame(|| {
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
                                    return in_own_frame(|| {
                                        let line = crate::Diagnostics::span_line_col(
                                            &cx.src, span.start,
                                        )
                                        .0
                                            as u32;
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
                                    });
                                }
                                return TExpr {
                                    ty: result_ty,
                                    kind: TExprKind::LayoutCompare {
                                        op: *op,
                                        lhs: Box::new(lhs),
                                        rhs: Box::new(rhs),
                                    },
                                };
                            });
                        }
                    }
                }
                // D-TYPE2-IMAG1=A: mirror `Checker::complexize_operand`. Sema
                // promotes the scalar operand of `3 + 4i` through the explicit
                // `Complex` constructor before the precise rule below fires, but a
                // comptime item or MirBridge fragment lowers the raw AST first
                // (`eval_comptime_items` runs early), so the mix still arrives here.
                let (mut lhs, mut rhs) = complexize_operands(*op, lhs, rhs, cx);
                // D-TYPE2-DEFAULT1 / D-NUMTYPE1: sema permits a Fraction to
                // absorb a bare exact integer literal. Materialize that literal
                // through the same Fraction Prelude constructor before the
                // shared precise-builtin path runs.
                let fraction_ty = Type::Named(Syntax::TYPE_FRACTION.to_string());
                let fraction_from_int = |value: TExpr| TExpr {
                    ty: fraction_ty.clone(),
                    kind: TExprKind::PreciseBuiltin {
                        type_name: Syntax::TYPE_FRACTION.to_string(),
                        func: "from_parts".to_string(),
                        args: vec![
                            value,
                            TExpr {
                                ty: Type::Int,
                                kind: TExprKind::IntLit(1, None),
                            },
                        ],
                    },
                };
                // D-EXPNEG1: a written negative exponent on the default
                // exact Int lowers to `1 / (base ^ |exponent|)` in the
                // Fraction Prelude carrier. Dynamic exponents stay on the
                // trapping whole-number power path.
                if *op == BinOp::Pow && lhs.ty == Type::Int && rhs.ty == Type::Int {
                    if let Some(exponent) = lower_negative_power_exponent(r, cx, env) {
                        let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32;
                        let powered = TExpr {
                            ty: Type::Int,
                            kind: TExprKind::Binary {
                                op: BinOp::Pow,
                                overflow: true,
                                line,
                                lhs: Box::new(lhs),
                                rhs: Box::new(exponent),
                            },
                        };
                        return TExpr {
                            ty: fraction_ty.clone(),
                            kind: TExprKind::PreciseBuiltin {
                                type_name: Syntax::TYPE_FRACTION.to_string(),
                                func: "from_parts".to_string(),
                                args: vec![
                                    TExpr {
                                        ty: Type::Int,
                                        kind: TExprKind::IntLit(1, None),
                                    },
                                    powered,
                                ],
                            },
                        };
                    }
                }
                if matches!(
                    *op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Eq | BinOp::Ne
                ) {
                    let lhs_fraction = lhs.ty == fraction_ty;
                    let rhs_fraction = rhs.ty == fraction_ty;
                    if lhs_fraction && rhs.ty == Type::Int {
                        rhs = fraction_from_int(rhs);
                    } else if rhs_fraction && lhs.ty == Type::Int {
                        lhs = fraction_from_int(lhs);
                    }
                }
                // D-TIMERES1=A: sema has already fixed these three forms to
                // Duration. Keep the nanosecond carrier intact and dispatch
                // through the handle seam so every engine calls one kernel.
                let left_duration = matches!(
                    &lhs.ty,
                    Type::Named(name) if name == crate::Syntax::DURATION_TYPE
                );
                let right_duration = matches!(
                    &rhs.ty,
                    Type::Named(name) if name == crate::Syntax::DURATION_TYPE
                );
                if matches!(*op, BinOp::Mul | BinOp::Div)
                    && ((left_duration && rhs.ty == Type::Int)
                        || (*op == BinOp::Mul && lhs.ty == Type::Int && right_duration))
                {
                    let (duration, factor, duration_op) = if left_duration {
                        (
                            lhs,
                            rhs,
                            if *op == BinOp::Mul {
                                THandleOp::DurationScale
                            } else {
                                THandleOp::DurationDivide
                            },
                        )
                    } else {
                        (rhs, lhs, THandleOp::DurationScale)
                    };
                    return TExpr {
                        ty: Type::Named(crate::Syntax::DURATION_TYPE.to_string()),
                        kind: TExprKind::HandleMethod {
                            recv: Box::new(duration),
                            op: duration_op,
                            args: vec![factor],
                        },
                    };
                }
                // D-TYPE2-DEFAULT1 amends D-INTDIV1: exact whole-number
                // division constructs a rational before any machine arithmetic
                // path can see the operands.
                if *op == BinOp::Div && lhs.ty == Type::Int && rhs.ty == Type::Int {
                    return TExpr {
                        ty: fraction_ty,
                        kind: TExprKind::PreciseBuiltin {
                            type_name: Syntax::TYPE_FRACTION.to_string(),
                            func: "from_parts".to_string(),
                            args: vec![lhs, rhs],
                        },
                    };
                }
                // D-SPACE-GEOMETRY1=A: sema owns the coordinate operation
                // table, including whether a generic carrier returns a checked
                // Result.  Lower only the two arithmetic operations for which
                // that table has a geometry builtin; every other binary shape
                // remains on the generic path below.
                let geometry_operand = |ty: &Type| {
                    crate::Sema::geometry_is_point(ty) || crate::Sema::geometry_is_delta(ty)
                };
                if matches!(*op, BinOp::Add | BinOp::Sub)
                    && geometry_operand(&lhs.ty)
                    && geometry_operand(&rhs.ty)
                {
                    if let Some(Ok(result_ty)) =
                        crate::Sema::geometry_binop_result(*op, &lhs.ty, &rhs.ty)
                    {
                        let type_name = match (&lhs.ty, &rhs.ty) {
                            (Type::Apply { name, .. }, _)
                                if matches!(name.as_str(), "Point2" | "Delta2") =>
                            {
                                name.clone()
                            }
                            (_, Type::Apply { name, .. })
                                if matches!(name.as_str(), "Point2" | "Delta2") =>
                            {
                                name.clone()
                            }
                            (Type::Named(name), _) => name.clone(),
                            _ => unreachable!("geometry operands have a nominal carrier"),
                        };
                        return TExpr {
                            ty: result_ty,
                            kind: TExprKind::MathBuiltin {
                                type_name,
                                func: if *op == BinOp::Add {
                                    "add".to_string()
                                } else {
                                    "sub".to_string()
                                },
                                args: vec![lhs, rhs],
                            },
                        };
                    }
                }
                // Overflow decision for trapping JetArith helpers. Prefer the
                // resolved TIR operand types so call results, fields, and other
                // shapes the AST replay cannot see still trap (I2 / #1484). The
                // AST replay remains for cases where lowering types are not yet
                // integer-shaped but the source operand structurally is.
                // D-INTBIG1/D-NUMOPS1: fixed-width `+`/`-`/`*`/`/` trap on value
                // overflow; exact default `Int` uses packed Prelude helpers.
                // Fixed-width `<<`/`>>` trap on a bit-count out of the type's width
                // (both via the `JetArith` helpers, so no raw Rust overflow panic
                // leaks — I2). A shift's
                // overflow is governed by its LEFT operand's integer-ness (the value),
                // never the count.
                // Exact `Int / Int` returned above as a Fraction. Fixed-width
                // integer division remains on the ordinary trapping path.
                // Fixed-width `IntN` keeps same-width `/` and must trap via jet_div.
                // D-MODSEM1=A: `%` and `%%` always call their Prelude helper above.
                let tir_integer = lhs.ty.is_integer() || rhs.ty.is_integer();
                let arith_overflow =
                    matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
                        && (tir_integer
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
                let canonical_time_ty = in_own_frame(|| match (&lhs.ty, &rhs.ty) {
                    (Type::Named(left), Type::Named(right))
                        if matches!(left.as_str(), "Duration" | "Instant")
                            && matches!(right.as_str(), "Duration" | "Instant") =>
                    {
                        let left_kind = cx.unit_facts.get(left).map(|fact| fact.kind);
                        let right_kind = cx.unit_facts.get(right).map(|fact| fact.kind);
                        match (*op, left_kind, right_kind) {
                            (
                                BinOp::Add | BinOp::Sub,
                                Some(crate::AST::QuantityKind::Delta),
                                Some(crate::AST::QuantityKind::Delta),
                            )
                            | (
                                BinOp::Sub,
                                Some(crate::AST::QuantityKind::Point),
                                Some(crate::AST::QuantityKind::Point),
                            ) => Some(Type::Named(crate::Syntax::DURATION_TYPE.to_string())),
                            (
                                BinOp::Add,
                                Some(crate::AST::QuantityKind::Point),
                                Some(crate::AST::QuantityKind::Delta),
                            )
                            | (
                                BinOp::Sub,
                                Some(crate::AST::QuantityKind::Point),
                                Some(crate::AST::QuantityKind::Delta),
                            )
                            | (
                                BinOp::Add,
                                Some(crate::AST::QuantityKind::Delta),
                                Some(crate::AST::QuantityKind::Point),
                            ) => Some(Type::Named(crate::Syntax::TYPE_INSTANT.to_string())),
                            _ => None,
                        }
                    }
                    _ => None,
                });
                let ty = in_own_frame(|| {
                    if *op == BinOp::Compare {
                        Type::Named(crate::Syntax::TYPE_ORDERING.to_string())
                    } else if op.is_comparison() || matches!(op, BinOp::And | BinOp::Or) {
                        Type::Bool
                    } else if let Some(ty) = canonical_time_ty {
                        ty
                    } else if let (Type::Named(ln), Type::Named(rn)) = (&lhs.ty, &rhs.ty) {
                        let lm = crate::Sema::is_math_type(ln) && !cx.type_names.contains(ln);
                        let rm = crate::Sema::is_math_type(rn) && !cx.type_names.contains(rn);
                        if lm || rm {
                            crate::Sema::math_binop_result(*op, ln, rn)
                                .unwrap_or_else(|| lhs.ty.clone())
                        } else if ln == rn
                            && crate::Sema::precise_binop_result(*op, ln, rn).is_some()
                        {
                            lhs.ty.clone()
                        } else {
                            lhs.ty.clone()
                        }
                    } else {
                        lhs.ty.clone()
                    }
                });
                // D-DECIMAL1 / D-NUMTYPE1 / D-TYPE2-IMAG1=A: precise arithmetic lowers through
                // the shared typed-value Prelude helpers.
                if let (Type::Named(ln), Type::Named(rn)) = (&lhs.ty, &rhs.ty) {
                    if ln == rn {
                        if let Some(result_ty) = crate::Sema::precise_binop_result(*op, ln, rn) {
                            if matches!(
                                *op,
                                BinOp::Add
                                    | BinOp::Sub
                                    | BinOp::Mul
                                    | BinOp::Div
                                    | BinOp::Eq
                                    | BinOp::Ne
                            ) {
                                let func = match op {
                                    BinOp::Add => "add",
                                    BinOp::Sub => "sub",
                                    BinOp::Mul => "mul",
                                    BinOp::Div => "div",
                                    BinOp::Eq | BinOp::Ne => "equal",
                                    _ => unreachable!(),
                                };
                                let precise = TExpr {
                                    ty: result_ty,
                                    kind: TExprKind::PreciseBuiltin {
                                        type_name: ln.clone(),
                                        func: func.to_string(),
                                        args: vec![lhs, rhs],
                                    },
                                };
                                if *op == BinOp::Ne {
                                    return TExpr {
                                        ty: Type::Bool,
                                        kind: TExprKind::Unary {
                                            op: crate::AST::UnOp::Not,
                                            operand: Box::new(precise),
                                        },
                                    };
                                }
                                return precise;
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
            })
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
        } => in_own_frame(|| {
            let toperands: Vec<TExpr> = operands
                .iter()
                .map(|expression| {
                    let lowered = lower_expr(expression, cx, env);
                    let Type::Result { ok, .. } = &lowered.ty else {
                        return lowered;
                    };
                    let line =
                        crate::Diagnostics::span_line_col(&cx.src, expression.span().start).0;
                    TExpr {
                        ty: (**ok).clone(),
                        kind: TExprKind::Try {
                            inner: Box::new(lowered),
                            note: None,
                            convert: TTryConvert::None,
                            file: escape_rust_str(&cx.file),
                            line,
                            fn_name: escape_rust_str(&env.fn_name),
                        },
                    }
                })
                .collect();
            TExpr {
                ty: Type::Bool,
                kind: TExprKind::CompareChain {
                    operands: toperands,
                    ops: ops.clone(),
                    hooks: hooks.clone(),
                },
            }
        }),
        Expr::Call(call) => {
            in_own_frame(|| {
                if let Some(lowered) = lower_raw_ok_call(call, cx, env) {
                    return lowered;
                }
                if let Some(lowered) = lower_raw_err_call(call, cx, env) {
                    return lowered;
                }
                // D-CONC-CHAN1=A: `channel<T>()` is a readable builtin. Keep
                // the source surface direct, then normalize to the one existing
                // Core/Prelude channel route consumed by AOT, JIT, and TIR.
                if call.name == Syntax::BUILTIN_CHANNEL
                    && !cx.sigs.contains_key(&call.name)
                    && !env.locals.contains_key(&call.name)
                    && call.args.len() <= 1
                {
                    let Some(ty) = call.resolved_ret.clone() else {
                        return invariant_violation_expr(
                            call.name_span,
                            "channel constructor without a resolved return type",
                        );
                    };
                    let args: Vec<TExpr> = call
                        .args
                        .iter()
                        .map(|arg| lower_expr(&arg.expr, cx, env))
                        .collect();
                    let widen_to_vec = vec![false; args.len()];
                    return core_call_expr(
                        ty,
                        "core.tasks",
                        if call.args.is_empty() {
                            Syntax::INTERNAL_CHANNEL_NEW_METHOD
                        } else {
                            Syntax::INTERNAL_CHANNEL_BOUNDED_METHOD
                        },
                        args,
                        call.name_span,
                        widen_to_vec,
                    );
                }
                // D-CONC-FREEZE1=A: sema has proved the source is an owned,
                // deeply snapshot-able value. Reuse the existing structural
                // clone/materialization nodes so every execution tier consumes
                // one already-typed TIR representation. A nested freeze is the
                // identity by law and therefore does not clone twice.
                if call.name == Syntax::KW_FREEZE
                    && !cx.sigs.contains_key(&call.name)
                    && !env.locals.contains_key(&call.name)
                    && call.args.len() == 1
                {
                    return in_own_frame(|| {
                        let source = &call.args[0].expr;
                        if let Expr::Call(inner) = source {
                            if inner.name == Syntax::KW_FREEZE && inner.args.len() == 1 {
                                return lower_expr(source, cx, env);
                            }
                        }
                        let operand = lower_expr(source, cx, env);
                        let view_owned_ty = match &operand.ty {
                            Type::Apply { name, args } if name == "View" && args.len() == 1 => {
                                Some(if matches!(&args[0], Type::Named(element) if element == "str") {
                                    Type::String
                                } else {
                                    Type::List(Box::new(args[0].clone()))
                                })
                            }
                            _ => None,
                        }
                        .or_else(|| match source {
                            Expr::Ident(name, _)
                                if matches!(
                                    env.split_view_handle(name),
                                    Some(Type::Apply { name, .. }) if name == "View"
                                ) => env.split_view_handle(name).and_then(|ty| match ty {
                                    Type::Apply { args, .. } if args.len() == 1 => Some(
                                        if matches!(&args[0], Type::Named(element) if element == "str") {
                                            Type::String
                                        } else {
                                            Type::List(Box::new(args[0].clone()))
                                        },
                                    ),
                                    _ => None,
                                }),
                            _ => None,
                        });
                        let string_view = matches!(source, Expr::Ident(name, _) if env.is_string_view_local(name));
                        let is_view = view_owned_ty.is_some();
                        let ty = view_owned_ty.unwrap_or_else(|| operand.ty.clone());
                        let kind = if is_view || string_view {
                            TExprKind::MaterializeView(Box::new(operand))
                        } else if operand.ty.is_compute_tensor_family() {
                            env.note_clone(&ty);
                            TExprKind::ExplicitCopy(Box::new(operand))
                        } else {
                            env.note_clone(&ty);
                            TExprKind::Clone(Box::new(operand))
                        };
                        return TExpr { ty, kind };
                    });
                }
                // D-CALLPOLICY1=E: `apply` is a typed value transformation. Sema
                // records the replacement on its final callable argument; lowering
                // forwards that value through the one shared function-value seam.
                if call.name == "apply"
                    && !cx.sigs.contains_key(&call.name)
                    && !env.locals.contains_key(&call.name)
                    && call
                        .args
                        .last()
                        .is_some_and(|arg| arg.flags.callable_policy.is_some())
                {
                    let target_name = match &call.args.last().expect("checked above").expr {
                        Expr::Ident(name, _) => Some(name.as_str()),
                        Expr::Paren(inner, _) => match inner.as_ref() {
                            Expr::Ident(name, _) => Some(name.as_str()),
                            _ => None,
                        },
                        _ => None,
                    };
                    let user_policy = call.args[..call.args.len().saturating_sub(1)]
                        .iter()
                        .find_map(|argument| {
                            let Expr::Call(policy) = &argument.expr else {
                                return None;
                            };
                            if crate::AST::CallablePolicyChain::is_builtin(&policy.name) {
                                return None;
                            }
                            let target_name = target_name?;
                            let wrapper = crate::AST::CallablePolicyChain::user_wrapper_name(
                                &policy.name,
                                target_name,
                            );
                            cx.sigs.contains_key(&wrapper).then_some((wrapper, policy))
                        });
                    if let Some((wrapper, policy)) = user_policy {
                        let callee =
                            lower_expr(&call.args.last().expect("checked above").expr, cx, env);
                        let policy_sig = cx
                            .sigs
                            .get(&wrapper)
                            .expect("sema materialized the user policy wrapper");
                        let policy_args = policy
                            .args
                            .iter()
                            .enumerate()
                            .map(|(index, argument)| {
                                lower_one_call_arg(
                                    argument,
                                    policy_sig.get(index).cloned(),
                                    env,
                                    cx,
                                )
                            })
                            .collect();
                        let policy_conventions = policy
                            .args
                            .iter()
                            .enumerate()
                            .map(|(index, _)| {
                                policy_sig
                                    .get(index)
                                    .map(|(convention, _)| *convention)
                                    .unwrap_or(AccessConvention::Read)
                            })
                            .collect();
                        let fn_type = callee.ty.clone();
                        return TExpr {
                            ty: fn_type.clone(),
                            kind: TExprKind::FnValue {
                                kind: TFnValueKind::Policy {
                                    fn_type,
                                    policy_args,
                                    policy_conventions,
                                    callee: Box::new(callee),
                                },
                            },
                        };
                    }
                    return lower_expr(&call.args.last().expect("checked above").expr, cx, env);
                }
                // Early comptime registration runs before sema annotates calls with
                // `widen_approx`. Recognize the unshadowed builtin here too; sema
                // still owns validation and gate recording, while TIR owns the one
                // canonical Int-to-Float conversion used by every evaluator tier.
                let builtin_approx = call.name == Syntax::BUILTIN_APPROX
                    && !cx.sigs.contains_key(&call.name)
                    && !env.locals.contains_key(&call.name);
                if (call.widen_approx || builtin_approx) && call.args.len() == 1 {
                    return in_own_frame(|| {
                        let source = lower_expr(&call.args[0].expr, cx, env);
                        let op = crate::Codegen::TIR::resolve_numeric_conversion_op("Float", "Int")
                            .expect("Float.from_int is a registered numeric conversion");
                        return TExpr {
                            ty: Type::Float,
                            kind: TExprKind::NumericMethod {
                                recv: Box::new(source),
                                op,
                            },
                        };
                    });
                }
                // D-TYPE2-UNCERT1=A: the canonical source constructor lowers to
                // the same Prelude symbol every engine already uses. `core.units`
                // is an internal route, not a second user-visible spelling.
                if call.name == "measurement"
                    && !cx.sigs.contains_key(&call.name)
                    && !env.locals.contains_key(&call.name)
                    && call.args.len() == 2
                {
                    return in_own_frame(|| {
                        let args = call
                            .args
                            .iter()
                            .map(|argument| lower_expr(&argument.expr, cx, env))
                            .collect::<Vec<_>>();
                        let widen_to_vec = vec![false; args.len()];
                        return core_call_expr(
                            Type::Apply {
                                name: Syntax::TYPE_MEASUREMENT.to_string(),
                                args: vec![Type::Float],
                            },
                            "core.units",
                            "from",
                            args,
                            call.name_span,
                            widen_to_vec,
                        );
                    });
                }
                // c109 Phase 13: `f(args)` where `f` is a LOCAL (a fn-typed binding/param)
                // parses as `Expr::Call`. Function-type params are unmarked Read params.
                if env.locals.contains_key(&call.name) && !cx.consts.contains_key(&call.name) {
                    return in_own_frame(|| {
                        let callee_ty = env.ty_of(&call.name).unwrap_or_else(unit_type);
                        let callee_t = TExpr {
                            ty: callee_ty,
                            kind: TExprKind::Local(env.local_of(&call.name)),
                        };
                        let callee_expr = Expr::Ident(call.name.clone(), call.name_span);
                        lower_fn_value_call(
                            Some(&callee_expr),
                            callee_t,
                            &call.args,
                            call.name_span.start as u32,
                            cx,
                            env,
                        )
                    });
                }
                if call.name == Syntax::RESOURCE_CLOSE && call.args.len() == 1 {
                    return in_own_frame(|| {
                        let resource = match &call.args[0].expr {
                            Expr::Ident(name, _) if env.is_resource(name) => TExpr {
                                ty: env.ty_of(name).unwrap_or_else(unit_type),
                                kind: TExprKind::ResourceTake(env.resource_take_place(name)),
                            },
                            expr => lower_expr(expr, cx, env),
                        };
                        return TExpr {
                            ty: unit_type(),
                            kind: TExprKind::Close(Box::new(resource)),
                        };
                    });
                }
                // D-TYPEDTEXT1=D / D-BOUND-HEAD1=A: the synthetic typed-head call sema rewrote a typed
                // text literal into (mirrors D-UNITLIT1's rewrite pattern). Args
                // alternate literal-segment, hole, literal-segment, ..., always closing
                // on a literal (`literals.len() == holes.len() + 1`) — even index is a
                // compile-time-known literal segment, odd index is a hole value. SQL
                // keeps holes as bound parameters. HTML escapes ordinary holes and
                // directly composes holes already proven to be HTML.
                if let Some(kind) = Syntax::typed_head_kind(&call.name)
                    .filter(|kind| kind.is_interpolated_template())
                    .filter(|_| !cx.sigs.contains_key(&call.name))
                {
                    return in_own_frame(|| {
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
                                let mut hole = lower_expr(&a.expr, cx, env);
                                if a.flags.trusted_html {
                                    debug_assert_eq!(kind, Syntax::TypedHeadKind::HTML);
                                    hole.ty = Type::Named(Syntax::TYPE_HTML.to_string());
                                }
                                holes.push(hole);
                            }
                        }
                        let ty = Type::Named(kind.internal_type_name().to_string());
                        if kind == Syntax::TypedHeadKind::HTML && holes.is_empty() {
                            return TExpr {
                                ty,
                                kind: TExprKind::StrLit(vec![TStrPart::Lit(
                                    literals.into_iter().next().unwrap_or_default(),
                                )]),
                            };
                        }
                        return TExpr {
                            ty,
                            kind: TExprKind::HostCall(Box::new(
                                crate::Codegen::TIR::THostCall::TypedTextInterp {
                                    kind,
                                    literals,
                                    holes,
                                },
                            )),
                        };
                    });
                }
                // D-REGEX-LIT1=D: sema already validated this complete literal.
                // Keep one Regex value through AOT/JIT instead of a fallible Result.
                if call.name == Syntax::TYPE_REGEX
                    && !cx.sigs.contains_key(&call.name)
                    && call.args.len() == 1
                {
                    return in_own_frame(|| {
                        return core_call_expr(
                            Type::Named(Syntax::TYPE_REGEX.to_string()),
                            "core.regex",
                            "literal",
                            vec![lower_expr(&call.args[0].expr, cx, env)],
                            call.name_span,
                            vec![false],
                        );
                    });
                }
                // `print` is ambient only when the user has not defined their own
                // `print` function (matches emit_call; sema enforces the shadowing).
                // Each checked argument keeps its own Printable/Display contract
                // and emits one output line in source evaluation order.
                if call.name == Syntax::BUILTIN_PRINT && !cx.sigs.contains_key(&call.name) {
                    return in_own_frame(|| print_args(&call.args, cx, env, call.name_span.start));
                }
                // D-LIN1-DROP: `drop(x)` — discard the value (move-to-nowhere). Sema
                // proved the discard is audited when the value is `#SingleUse`. Lowers
                // to a plain `drop(arg)`; no `unsafe` (I3). Disjoint from a user `drop`
                // fn or local of that name (`cx.sigs`/`env.locals` would be set then).
                if call.name == Syntax::BUILTIN_CONSUME
                    && !cx.sigs.contains_key(&call.name)
                    && !env.locals.contains_key(&call.name)
                {
                    return in_own_frame(|| {
                        let arg = lower_expr(&call.args[0].expr, cx, env);
                        return TExpr {
                            ty: unit_type(),
                            kind: TExprKind::Drop(Box::new(arg)),
                        };
                    });
                }
                // D-TOOL4: `expect(x)` builds a snapshot harness holder. At MirBridge
                // comptime the holder is the value itself — `consume(expect(x))` only
                // needs the binding to exist; `.snapshot()` is a separate HostCall.
                if call.name == Syntax::BUILTIN_EXPECT
                    && !cx.sigs.contains_key(&call.name)
                    && !env.locals.contains_key(&call.name)
                    && call.args.len() == 1
                {
                    return in_own_frame(|| {
                        let arg = lower_expr(&call.args[0].expr, cx, env);
                        return TExpr {
                            ty: arg.ty.clone(),
                            kind: TExprKind::Clone(Box::new(arg)),
                        };
                    });
                }
                // c109 Phase 26: the rich-runtime-report builtins (S36) — render the whole
                // emit string at lowering, byte-for-byte the AST helper. `assert`/
                // `assert_eq`/`panic`
                // are statement-position calls (a `()` result); the string is the `{ … }`
                // block emit emits as an expr-statement. Disjoint from a user fn of the same
                // name (`cx.sigs.contains_key` would be true then).
                if !cx.sigs.contains_key(&call.name) && !env.locals.contains_key(&call.name) {
                    if call.name == Syntax::BUILTIN_ASSERT {
                        return in_own_frame(|| {
                            let (kind, loc) = lower_require_stop(call, cx, env);
                            return TExpr {
                                ty: unit_type(),
                                kind: TExprKind::RequireStop {
                                    kind,
                                    loc,
                                    always_stops: false,
                                },
                            };
                        });
                    }
                    if call.name == Syntax::BUILTIN_ASSERT_EQ {
                        return in_own_frame(|| {
                            let (kind, loc) = lower_require_eq_stop(call, cx, env);
                            return TExpr {
                                ty: unit_type(),
                                kind: TExprKind::RequireStop {
                                    kind,
                                    loc,
                                    always_stops: false,
                                },
                            };
                        });
                    }
                    if call.name == Syntax::BUILTIN_PANIC {
                        return in_own_frame(|| {
                            let (kind, loc) =
                                lower_panic_stop(&call.name_span, &call.args, cx, env);
                            return TExpr {
                                ty: unit_type(),
                                kind: TExprKind::RequireStop {
                                    kind,
                                    loc,
                                    always_stops: true,
                                },
                            };
                        });
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
                    return in_own_frame(|| {
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
                    });
                }
                // c109 Phase 28: the overflow opt-out builtins `wrapping(e)`/`saturating(e)`/
                // `checked(e)` (D-NUMOPS1), plus the sema-only checked wrapper used by
                // D-WRAP-SCOPE1=A. The gate proved the name is one of the three (not
                // shadowed) and the sole arg is an integer `Expr::Binary`. Reproduce
                // `emit_call`'s arm (Expression.rs ~L1756): `(lhs).{name}_{op}(rhs)` with PLAIN
                // operands (no trap helper). `checked_*` returns `Option<T>`; the others return
                // `T` — set the result type accordingly so a `checked(...) ?? x` composes.
                if matches!(
                    call.name.as_str(),
                    Syntax::BUILTIN_WRAPPING
                        | Syntax::BUILTIN_SATURATING
                        | Syntax::BUILTIN_CHECKED
                        | Syntax::INTERNAL_ARITHMETIC_CHECKED
                ) && !cx.sigs.contains_key(&call.name)
                    && !env.locals.contains_key(&call.name)
                {
                    if let Some(Expr::Binary(op, l, r, _)) = call.args.first().map(|a| &a.expr) {
                        return in_own_frame(|| {
                            let op_suffix = match op {
                                BinOp::Add => "add",
                                BinOp::Sub => "sub",
                                BinOp::Mul => "mul",
                                BinOp::Div => "div",
                                BinOp::Pow => "pow",
                                // Sema validated an arithmetic op; mirror the AST default.
                                _ => "add",
                            };
                            let lhs = lower_expr(l, cx, env);
                            let rhs = lower_expr(r, cx, env);
                            let line =
                                crate::Diagnostics::span_line_col(&cx.src, call.name_span.start).0
                                    as u32;
                            let val_ty = lhs.ty.clone();
                            let result_ty = if call.name == Syntax::BUILTIN_CHECKED {
                                Type::Option(Box::new(val_ty))
                            } else {
                                val_ty
                            };
                            let policy = call
                                .args
                                .first()
                                .and_then(|arg| arg.flags.arithmetic_policy);
                            let prefix = if call.name == Syntax::INTERNAL_ARITHMETIC_CHECKED {
                                "checked_policy".to_string()
                            } else {
                                call.name.clone()
                            };
                            return TExpr {
                                ty: result_ty,
                                kind: TExprKind::OverflowOpt {
                                    prefix,
                                    op: op_suffix,
                                    line,
                                    policy,
                                    lhs: Box::new(lhs),
                                    rhs: Box::new(rhs),
                                },
                            };
                        });
                    }
                }
                // c109 Phase 14: an FFI extern call (`emit_call`'s `extern_funcs` arm).
                // Checked BEFORE the unqualified arms, matching `emit_call`'s order. Args
                // use `emit_extern_call_args` (a non-scalar `Read` is `(…).clone()`).
                if !env.locals.contains_key(&call.name) {
                    if let Some(extern_fn) = cx.extern_funcs.get(&call.name).cloned() {
                        let wrapper = if extern_fn.c_abi
                            && extern_fn.component.is_none()
                            && !call.name.contains("::")
                        {
                            crate::Sema::guest_import_wrapper_name(&cx.module_alias, &call.name)
                        } else {
                            extern_fn.wrapper
                        };
                        let c_abi = extern_fn.c_abi;
                        return in_own_frame(|| {
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
                            // `Context` records foreign declarations with their
                            // raw bridge ABI; ordinary `#Import(c)` functions
                            // retain their effective carrier in `fn_types`.
                            let return_type = extern_call_return_type(cx, &call.name);
                            let lowered = TExpr {
                                ty: return_type,
                                kind: TExprKind::ExternCall {
                                    symbol: wrapper,
                                    c_abi,
                                    args: eargs,
                                },
                            };
                            let lowered = match source_arg_order(&call.args) {
                                Some(order) => preserve_source_arg_order(
                                    lowered,
                                    &order,
                                    call.args.len(),
                                    call.name_span.start as u32,
                                ),
                                None => lowered,
                            };
                            return wrap_foreign_undo(
                                lowered,
                                cx.foreign_undos.get(&call.name).map(String::as_str),
                                call.name_span.start as u32,
                                cx,
                                env,
                            );
                        });
                    }
                    // c109 Phase 14: unqualified inline-module import (`emit_call`'s
                    // `unqualified_inline` arm) → `{root}__jet_{mangled}(args)`.
                    let inline_mangled = cx
                        .inline_unqualified
                        .get(&env.fn_name)
                        .and_then(|scope| scope.get(&call.name))
                        .or_else(|| cx.unqualified_inline.get(&call.name))
                        .cloned();
                    if let Some(mangled_key) = inline_mangled {
                        return in_own_frame(|| {
                            let undo = cx.foreign_undos.get(&mangled_key).map(String::as_str);
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
                            let target_return = call_return_type_with_args(
                                cx,
                                &mangled_key,
                                &call.type_args,
                                &args,
                            );
                            let ret = module_call_source_return_type_with_args(
                                cx,
                                &mangled_key,
                                &call.type_args,
                                &args,
                            );
                            let lowered = TExpr {
                                ty: ret,
                                kind: TExprKind::ModuleCall {
                                    form: TModuleCallForm::InlineMangled {
                                        mangled: demand_generic_free_function(
                                            cx,
                                            &mangled_key,
                                            &args,
                                            &call.type_args,
                                        )
                                        .unwrap_or(mangled_key),
                                    },
                                    target_return: Some(target_return),
                                    type_args: call.type_args.clone(),
                                    args,
                                },
                            };
                            let lowered = match source_arg_order(&call.args) {
                                Some(order) => preserve_source_arg_order(
                                    lowered,
                                    &order,
                                    call.args.len(),
                                    call.name_span.start as u32,
                                ),
                                None => lowered,
                            };
                            return wrap_foreign_undo(
                                lowered,
                                undo,
                                call.name_span.start as u32,
                                cx,
                                env,
                            );
                        });
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
                    if let Some((rust_mod, fn_name)) = inline_file {
                        return in_own_frame(|| {
                            let undo = cx
                                .foreign_undos
                                .get(&format!("{rust_mod}::{fn_name}"))
                                .map(String::as_str);
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
                            let target_return =
                                imported_module_call_target_return(cx, &call.name, &fn_name);
                            let ret = cx
                                .import_rets
                                .get(&(call.name.clone(), fn_name.clone()))
                                .cloned()
                                .flatten()
                                .unwrap_or_else(unit_type);
                            let lowered = TExpr {
                                ty: ret,
                                kind: TExprKind::ModuleCall {
                                    form: TModuleCallForm::Qualified {
                                        rust_mod,
                                        rust_fn: mangle(&fn_name).to_string(),
                                    },
                                    target_return,
                                    type_args: call.type_args.clone(),
                                    args,
                                },
                            };
                            let lowered = match source_arg_order(&call.args) {
                                Some(order) => preserve_source_arg_order(
                                    lowered,
                                    &order,
                                    call.args.len(),
                                    call.name_span.start as u32,
                                ),
                                None => lowered,
                            };
                            return wrap_foreign_undo(
                                lowered,
                                undo,
                                call.name_span.start as u32,
                                cx,
                                env,
                            );
                        });
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
                    return in_own_frame(|| {
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
                                            ["a", "b", "c", "d", "e", "f"].get(index).map_or_else(
                                                || format!("column_{index}"),
                                                |name| (*name).to_string(),
                                            )
                                        },
                                        str::to_string,
                                    );
                                    fields.push(field);
                                    inputs.push(lower_expr(&arg.expr, cx, env));
                                }
                            }
                        }
                        let ret = call
                            .resolved_ret
                            .as_ref()
                            .expect("zip result resolved by sema");
                        if inputs.is_empty() {
                            return crate::Codegen::TIR::lower_empty_zip_family(
                                ret,
                                &call.name,
                                call.name_span,
                            );
                        }
                        let mut all = inputs.into_iter();
                        let first = all.next().expect("non-empty zip inputs");
                        return crate::Codegen::TIR::lower_zip_family(
                            first,
                            all.collect(),
                            fills,
                            fields,
                            &call.name,
                            call.name_span,
                            Some(ret),
                        );
                    });
                }
                // D-DECIMAL1: `Decimal(…)` constructor.
                if !env.locals.contains_key(&call.name)
                    && call.name == crate::Syntax::TYPE_DECIMAL
                    && !cx.type_names.contains(&call.name)
                {
                    return in_own_frame(|| {
                        let targs: Vec<TExpr> = call
                            .args
                            .iter()
                            .map(|a| lower_expr(&a.expr, cx, env))
                            .collect();
                        return TExpr {
                            ty: Type::Named(call.name.clone()),
                            kind: TExprKind::PreciseBuiltin {
                                type_name: call.name.clone(),
                                func: "from_str".to_string(),
                                args: targs,
                            },
                        };
                    });
                }
                // D-TYPE2-IMAG1=A: unit-literal elaboration and explicit
                // construction share the precise builtin constructor seam.
                if !env.locals.contains_key(&call.name)
                    && call.name == crate::Syntax::TYPE_COMPLEX
                    && !cx.type_names.contains(&call.name)
                {
                    return in_own_frame(|| {
                        let targs: Vec<TExpr> = call
                            .args
                            .iter()
                            .map(|a| lower_expr(&a.expr, cx, env))
                            .collect();
                        return TExpr {
                            ty: Type::Named(call.name.clone()),
                            kind: TExprKind::PreciseBuiltin {
                                type_name: call.name.clone(),
                                func: "from_parts".to_string(),
                                args: targs,
                            },
                        };
                    });
                }
                // D-SIMD2 / D-LINALG1: a built-in math-type constructor. Plainly lower the
                // float components and emit `{root}jet_math_<T>_new(…)`.
                if !env.locals.contains_key(&call.name)
                    && crate::Sema::is_math_type(&call.name)
                    && !cx.type_names.contains(&call.name)
                {
                    return in_own_frame(|| {
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
                    });
                }
                if call.range_checked && !env.locals.contains_key(&call.name) {
                    if let Some(arg) = call.args.first() {
                        return in_own_frame(|| {
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
                        });
                    }
                }
                if !env.locals.contains_key(&call.name) {
                    if let (Some((base, _)), Some(arg)) =
                        (cx.distinct_types.get(&call.name), call.args.first())
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named(call.name.clone()),
                                kind: TExprKind::DistinctCtor {
                                    name: call.name.clone(),
                                    arg: Box::new(lower_expr(&arg.expr, cx, env)),
                                    base: base.clone(),
                                },
                            };
                        });
                    }
                }
                // D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): a call to a trait-bounded
                // variadic function — sema left the trailing args unpacked
                // (`CheckerInfer/calls.rs::check_variadic_bound_tail`), so the arity
                // is just "how many args past the fixed prefix". Route to the
                // per-arity function `VariadicBound.rs` synthesizes; record the
                // arity so the post-pass in `Codegen/mod.rs` knows to emit it.
                if let Some((fixed, _bounds)) = cx.variadic_bound_fns.get(&call.name).cloned() {
                    return in_own_frame(|| {
                        let lowered = crate::Codegen::VariadicBound::lower_variadic_bound_call(
                            call, fixed, cx, env,
                        );
                        return match source_arg_order(&call.args) {
                            Some(order) => preserve_source_arg_order(
                                lowered,
                                &order,
                                call.args.len(),
                                call.name_span.start as u32,
                            ),
                            None => lowered,
                        };
                    });
                }
                // Resolve the callee's signature so each arg's borrow/clone/fn-coercion is
                // decided here, totally — via the shared `lower_one_call_arg` (the single
                // `emit_call_args` reproduction). c109 Phase 13: a callee with a Fn-typed
                // param (now in subset) routes its arg through the Box-coercion form.
                let sig = cx.sigs.get(&call.name).map(|sig| {
                    let Some(order) = cx.fn_type_param_order.get(&call.name) else {
                        return sig.clone();
                    };
                    if order.len() != call.type_args.len() {
                        return sig.clone();
                    }
                    let subst = order
                        .iter()
                        .zip(&call.type_args)
                        .map(|(param, actual)| (param.clone(), actual.clone()))
                        .collect();
                    sig.iter()
                        .map(|(convention, ty)| {
                            (*convention, crate::Generics::substitute_type(ty, &subst))
                        })
                        .collect()
                });
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
                let target_name =
                    demand_generic_free_function(cx, &call.name, &args, &call.type_args)
                        .unwrap_or_else(|| call.name.clone());
                in_own_frame(|| {
                    let ret = call_return_type_with_args(cx, &call.name, &call.type_args, &args);
                    let mut lowered = TExpr {
                        ty: ret,
                        kind: TExprKind::Call {
                            name: cx.jit_local_call_prefix.as_ref().map_or_else(
                                || target_name.clone(),
                                |prefix| format!("{prefix}{}", mangle(&target_name)),
                            ),
                            type_args: call.type_args.clone(),
                            args,
                        },
                    };
                    if let Some((pre, _)) = cx.contract_sigs.get(&call.name) {
                        let (bindings, contracts) = lower_call_pre_contracts(
                            call,
                            match &mut lowered.kind {
                                TExprKind::Call { args, .. } => args,
                                _ => unreachable!("plain call lowering produces a Call node"),
                            },
                            pre,
                            cx,
                            env,
                        );
                        if !contracts.is_empty() {
                            let mut stmts = bindings;
                            stmts.extend(
                                contracts
                                    .into_iter()
                                    .map(|contract| TStmt::Contract { contract }),
                            );
                            let call_ty = lowered.ty.clone();
                            stmts.push(TStmt::ExprStmt(lowered));
                            lowered = TExpr {
                                ty: call_ty,
                                kind: TExprKind::InlineBlock(stmts),
                            };
                        }
                    }
                    let lowered = match source_arg_order(&call.args) {
                        Some(order) => preserve_source_arg_order(
                            lowered,
                            &order,
                            call.args.len(),
                            call.name_span.start as u32,
                        ),
                        None => lowered,
                    };
                    // A body may be flow-divergent even when its callable contract
                    // promises a value (for example, a panic followed by an
                    // unreachable return). Keep that declared value type at call
                    // sites; only a source-declared `Never` is itself a stopping
                    // expression. This matters for task payloads, whose `Task<T>`
                    // element type is the callable's declared `T`, not its body
                    // reachability fact.
                    let declared_never = cx
                        .fn_source_types
                        .get(&call.name)
                        .and_then(|ty| match ty {
                            Type::Fn { ret: Some(ret), .. } => Some(cx.expand_type_aliases(ret)),
                            _ => None,
                        })
                        .is_some_and(
                            |ty| matches!(ty, Type::Named(name) if name == Syntax::TYPE_NEVER),
                        );
                    if cx.diverging_functions.contains(&call.name) && declared_never {
                        let line =
                            crate::Diagnostics::span_line_col(&cx.src, call.name_span.start).0;
                        let never = Type::Named(Syntax::TYPE_NEVER.to_string());
                        TExpr {
                            ty: never.clone(),
                            kind: TExprKind::InlineBlock(vec![
                                TStmt::ExprStmt(lowered),
                                TStmt::ExprStmt(TExpr {
                                    ty: never,
                                    kind: TExprKind::Unreachable { line },
                                }),
                            ]),
                        }
                    } else {
                        consume_plain_helper_route(call, lowered, cx, env)
                    }
                })
            })
        }
        // c109 Phase 6: a method call. The gate (`method_call_in_subset`) admitted
        // exactly the synthetic `.clone()` or a user instance method on a covered
        // type; lower accordingly. Every dispatch fact is resolved here (totality).
        Expr::MethodCall { .. } => lower_method_chain(e, cx, env),
        // `lower_expr_segment` removes value-level `if` nodes from its build queue;
        // delegate here as well so this dispatch remains total when called directly.
        Expr::If { .. } => lower_expr_segment(e, cx, env),

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
            span,
            ..
        } => {
            in_own_frame(|| {
                // c109 Phase 30: a trait-object coercion (`Circle {…}` in a `[Shape]` list). The
                // AST wraps the rendered literal `Box::new(<lit>) as Box<dyn __jet_<Trait>>`; the
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
                    return in_own_frame(|| {
                        if cx.core_import_module_for_function(&env.fn_name, alias)
                            == Some("core.encoding")
                            && matches!(
                                type_name.as_str(),
                                "EncodingLimits" | "EncodingCause" | "EncodingError"
                            )
                            && type_args.is_empty()
                        {
                            return in_own_frame(|| {
                                let tfields = fields
                                    .iter()
                                    .map(|(name, _, value)| {
                                        (name.clone(), lower_expr(value, cx, env), false)
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
                            });
                        }
                        if cx.core_import_module_for_function(&env.fn_name, alias)
                            == Some("core.encoding.cbor")
                            && matches!(type_name.as_str(), "CBOROptions" | "CBORError")
                            && type_args.is_empty()
                        {
                            return in_own_frame(|| {
                                let tfields = fields
                                    .iter()
                                    .map(|(name, _, value)| {
                                        (name.clone(), lower_expr(value, cx, env), false)
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
                            });
                        }
                        if cx.core_import_module_for_function(&env.fn_name, alias)
                            == Some("core.encoding.xml")
                            && matches!(
                                type_name.as_str(),
                                "XMLLimits"
                                    | "XMLParseOptions"
                                    | "XMLRenderOptions"
                                    | "XMLCanonical"
                                    | "XMLError"
                            )
                            && type_args.is_empty()
                        {
                            return in_own_frame(|| {
                                let tfields = fields
                                    .iter()
                                    .map(|(name, _, value)| {
                                        (name.clone(), lower_expr(value, cx, env), false)
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
                            });
                        }
                        if cx.core_import_module_for_function(&env.fn_name, alias)
                            == Some(crate::Syntax::CORE_EMAIL_MODULE)
                            && matches!(
                                type_name.as_str(),
                                "RecipientReport"
                                    | "SendReport"
                                    | "Limits"
                                    | "DkimConfig"
                                    | "SMTPConfig"
                            )
                        {
                            return in_own_frame(|| {
                                let tfields = fields
                                    .iter()
                                    .map(|(name, _, value)| {
                                        (name.clone(), lower_expr(value, cx, env), false)
                                    })
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
                            });
                        }
                        let tfields = fields
                            .iter()
                            .map(|(n, _, fe)| (n.clone(), lower_expr(fe, cx, env), false))
                            .collect();
                        let qualified = in_own_frame(|| {
                            if cx.code_modules.contains(alias) {
                                // Inline-module instance members are local generated nominals.
                                // Their qualified source spelling is never a foreign identity.
                                jet_foundation::Names::member_name(alias, type_name)
                            } else if let Some(identity) =
                                cx.foreign_type_identity(alias, type_name)
                            {
                                identity
                            } else {
                                // A bundle-backed context spells a dotted head one of exactly two
                                // ways: an inline-module member or an import. Failing both tables
                                // there is a genuine internal contradiction (I3).
                                jet_foundation::ice!(
                                None,
                                "foreign struct literal `{alias}.{type_name}` has no canonical nominal identity (I3)"
                            )
                            }
                        });
                        return TExpr {
                            ty: if type_args.is_empty() {
                                Type::Named(qualified)
                            } else {
                                Type::Apply {
                                    name: qualified,
                                    args: type_args.clone(),
                                }
                            },
                            kind: TExprKind::StructLit {
                                fields: tfields,
                                extra: None,
                                as_trait: None,
                            },
                        };
                    });
                }
                // c109 Phase 17: a PRELUDE struct literal (HTTPRequest/HTTPResponse).
                if net_handle_rust_type(type_name).is_some() {
                    return in_own_frame(|| {
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
                    });
                }
                // D-TEXTWIDTH1=B: `TextWidth.{ ambiguous: .Wide, controls: .Reject }` —
                // a plain dot-ctor core struct, `jet_std::TextWidth` head, no injected
                // extra field (unlike HTTPRequest's `params`).
                if type_name == Syntax::TYPE_ERR {
                    return in_own_frame(|| {
                        let tfields = fields
                            .iter()
                            .map(|(name, _, value)| {
                                (
                                    name.clone(),
                                    lower_default_err_field(name, value, cx, env),
                                    false,
                                )
                            })
                            .collect();
                        return TExpr {
                            ty: Type::Named(Syntax::TYPE_ERR.to_string()),
                            kind: TExprKind::StructLit {
                                fields: tfields,
                                extra: None,
                                as_trait: None,
                            },
                        };
                    });
                }
                if matches!(
                    type_name.as_str(),
                    "TextWidth" | "TerminalSize" | "TerminalPolicy"
                ) {
                    return in_own_frame(|| {
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
                    });
                }
                if type_name == "AsyncPolicy" {
                    return in_own_frame(|| {
                        let tfields = fields
                            .iter()
                            .map(|(name, _, value)| {
                                (name.clone(), lower_expr(value, cx, env), false)
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
                    });
                }
                // D-ENCSTREAM-SURFACE1=A: shared encoding value constructors.
                if matches!(
                    type_name.as_str(),
                    "EncodingLimits" | "EncodingCause" | "EncodingError"
                ) {
                    return in_own_frame(|| {
                        let tfields = fields
                            .iter()
                            .map(|(name, _, value)| {
                                (name.clone(), lower_expr(value, cx, env), false)
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
                    });
                }
                // D-VALIDATE1: `FieldError.{ path: …, reason: … }` — same shape,
                // separate jet_std Rust head.
                if type_name == "FieldError" {
                    return in_own_frame(|| {
                        let tfields = fields
                            .iter()
                            .map(|(name, _, value)| {
                                (name.clone(), lower_expr(value, cx, env), false)
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
                    });
                }
                if matches!(
                    type_name.as_str(),
                    "RecipientReport" | "SendReport" | "Limits" | "DkimConfig" | "SMTPConfig"
                ) {
                    return in_own_frame(|| {
                        let tfields = fields
                            .iter()
                            .map(|(name, _, value)| {
                                (name.clone(), lower_expr(value, cx, env), false)
                            })
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
                    });
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
                if let Some(base_name) = type_name.strip_suffix(".Patch") {
                    // TIR's entry-module shape table contains source structs, not
                    // sema's synthetic patch item. Reconstruct that one derived
                    // shape from the base fields, preserving the computed-field
                    // exclusion owned by the patchable sema pass.
                    let all = cx.struct_fields.get(base_name).map(|base_fields| {
                        base_fields
                            .iter()
                            .filter(|(name, _)| {
                                !cx.computed_fields
                                    .get(base_name)
                                    .is_some_and(|computed| computed.contains(name))
                            })
                            .map(|(name, field_ty)| {
                                (name.clone(), Type::Option(Box::new(field_ty.clone())))
                            })
                            .collect::<Vec<_>>()
                    });
                    if let Some(all) = all {
                        return in_own_frame(|| {
                            let provided: std::collections::HashMap<_, _> =
                                fields.iter().map(|(n, _, fe)| (n.as_str(), fe)).collect();
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
                        });
                    }
                }
                in_own_frame(|| {
                    // I3: sema already proved this literal names a real struct. A
                    // declaring-module local may carry the imported module's canonical
                    // identity; an entry-module local keeps its source spelling.
                    // The `jet run` TIR path lowers before the AOT context registers
                    // local structs, so an absent map entry remains ordinary.
                    let resolved_name = cx
                        .local_type_identities
                        .get(type_name)
                        .cloned()
                        .or_else(|| {
                            if cx.struct_fields.contains_key(type_name) {
                                Some(type_name.clone())
                            } else {
                                cx.foreign_type_identity("", type_name)
                            }
                        })
                        .unwrap_or_else(|| type_name.clone());
                    let resolved_ty = if type_args.is_empty() {
                        Type::Named(resolved_name.clone())
                    } else {
                        Type::Apply {
                            name: resolved_name.clone(),
                            args: type_args.clone(),
                        }
                    };
                    // c109: a self-referential field (`child: Tree?` on `Tree`) has Rust type
                    // `Box<…>` (`cx.boxed_edges`); resolve the `boxed` flag here (a total fact)
                    // so emit can wrap the value in `Box::new(…)`, exactly as `emit_struct_lit`.
                    let mut tfields = fields
                        .iter()
                        .map(|(n, _, fe)| {
                            let boxed =
                                cx.boxed_edges.contains(&(resolved_name.clone(), n.clone()));
                            let mut value = lower_owned_expr(fe, cx, env);
                            // D-UNIONTYPE1=A: member → union inject at Codable/struct field sites.
                            if let Some(fty) = struct_field_type(cx, &resolved_ty, n) {
                                if matches!(&value.kind, TExprKind::Absent)
                                    && matches!(&fty, Type::Option(_))
                                {
                                    value.ty = fty.clone();
                                }
                                // A typed fixed-list literal is lowered without an
                                // expected type at this point. Reapply the field's
                                // concrete shape before emission, or a `[T#N]` field
                                // receives a `Vec<T>` and rustc reports an internal
                                // type error (I2).
                                value = preserve_typed_list_shape(value, &fty, cx);
                                value = crate::Codegen::TIR::maybe_widen_expr_to_union(value, &fty);
                                value = lower_atomic_initializer(value, &fty);
                            }
                            (n.clone(), value, boxed)
                        })
                        .collect::<Vec<_>>();
                    if cx.published_schemas.contains(&resolved_name) {
                        let holder_ty = Type::Named(Syntax::TYPE_DATA.to_string());
                        let holder = if env.fn_name == "decode" {
                            TExpr {
                                ty: holder_ty,
                                kind: TExprKind::Local(TLocal::user("tree")),
                            }
                        } else {
                            core_call_expr(
                                holder_ty,
                                "core.encoding",
                                "__published_schema_empty",
                                Vec::new(),
                                *span,
                                Vec::new(),
                            )
                        };
                        tfields.push((Syntax::PUBLISHED_UNKNOWN_FIELDS.to_string(), holder, false));
                    }
                    // c109 Phase 30: a trait-coerced literal's value type is the trait object (so a
                    // list of them types `[Shape]`); an uncoerced literal keeps its struct type.
                    let ty = match as_trait {
                        Some(t) => Type::TraitObject(vec![t.clone()]),
                        None => resolved_ty,
                    };
                    TExpr {
                        ty,
                        kind: TExprKind::StructLit {
                            fields: tfields,
                            extra: None,
                            as_trait: trait_coerce,
                        },
                    }
                })
            })
        }
        // c109 Phase 3: a struct field read in borrow position. Resolve the field
        // type ONCE here from the receiver's resolved struct type (totality). A
        // covered function never reaches here with a non-struct receiver (sema
        // guarantees field reads target struct values).
        Expr::Field(receiver, member, span) => {
            in_own_frame(|| {
                if core_module_path_from_receiver(receiver, cx, env).as_deref() == Some("core.math")
                {
                    // Module fields, not calls. A 0-arg CoreCall left resident
                    // JIT looking for unregistered `jet_std_math_pi`. The
                    // Prelude function is `f64::consts::PI`; keep that one
                    // constant here (I9).
                    if let Some(value) = match member.as_str() {
                        "pi" => Some(std::f64::consts::PI),
                        "e" => Some(std::f64::consts::E),
                        "tau" => Some(std::f64::consts::TAU),
                        "infinity" => Some(f64::INFINITY),
                        "nan" => Some(f64::NAN),
                        _ => None,
                    } {
                        return TExpr {
                            ty: Type::Float,
                            kind: TExprKind::FloatLit(value),
                        };
                    }
                }
                // D-LAYOUT-FACTS1=B: derive bodies bind their type parameter as a
                // local type fact for that binding. Keep `@layout` on the field
                // path so `T.@layout` is not mistaken for an enum literal.
                let compiler_fact_receiver = match receiver.as_ref() {
                    // A qualified type path lowers as a field chain. The final
                    // segment is still the type name (`module.Packet`), while a
                    // value path such as `info.layout` remains lowercase.
                    Expr::Ident(name, _) => {
                        name.chars().next().is_some_and(char::is_uppercase)
                            || matches!(
                                env.ty_of(name),
                                Some(Type::Named(type_name))
                                    if matches!(
                                        type_name.as_str(),
                                        "FieldInfo" | "MethodInfo" | "TypeParamInfo"
                                    )
                            )
                    }
                    Expr::Field(_, name, _) => name.chars().next().is_some_and(char::is_uppercase),
                    _ => false,
                };
                if compiler_fact_receiver
                    && (Syntax::compiler_fact_member(member).is_some()
                        || jet_foundation::Registry::fact_read(member).is_some())
                {
                    return in_own_frame(|| {
                        let recv = lower_expr(receiver, cx, env);
                        return TExpr {
                            ty: compiler_fact_type(member),
                            kind: TExprKind::Field {
                                recv: Box::new(recv),
                                field: member.clone(),
                                boxed: false,
                            },
                        };
                    });
                }
                if let Some((enum_type, variant)) =
                    grouped_enum_unit_variant(cx, receiver, member, |name| {
                        env.ty_of(name).is_some()
                    })
                {
                    return in_own_frame(|| {
                        return TExpr {
                            ty: Type::Named(enum_type.clone()),
                            kind: TExprKind::EnumLit {
                                enum_type,
                                variant,
                                payload: TEnumPayload::Unit,
                            },
                        };
                    });
                }
                // c109 Phase 4: a *unit* enum literal (`Light.Yellow`) reaches codegen as
                // a `Field` whose receiver is the enum-name ident (sema re-types but does
                // not rewrite the node). The gate proved this is a covered enum + unit
                // variant; emit `__jet_<Enum>::__jet_<variant>` (the AST path's form).
                if let Expr::Ident(enum_name, _) = receiver.as_ref() {
                    let resolved_enum = cx
                        .core_qualified_rust_type_name(enum_name)
                        .unwrap_or(enum_name.as_str());
                    if env.ty_of(enum_name).is_none()
                        && matches!(
                            enum_name.as_str(),
                            "Overflow"
                                | "FailurePolicy"
                                | "DispatchState"
                                | "HookPolicy"
                                | "HookDecision"
                                | "HookOutcome"
                        )
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named(enum_name.clone()),
                                kind: TExprKind::EnumLit {
                                    enum_type: enum_name.clone(),
                                    variant: member.clone(),
                                    payload: TEnumPayload::Unit,
                                },
                            };
                        });
                    }
                    if env.ty_of(enum_name).is_none()
                        && enum_name == "DataEvent"
                        && matches!(
                            member.as_str(),
                            "Null" | "ArrayStart" | "ArrayEnd" | "ObjectStart" | "ObjectEnd"
                        )
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named("DataEvent".to_string()),
                                kind: TExprKind::EnumLit {
                                    enum_type: "DataEvent".to_string(),
                                    variant: member.clone(),
                                    payload: TEnumPayload::Unit,
                                },
                            };
                        });
                    }
                    // D-LISTREMOVE1/F: `RemoveBy.Val` / `RemoveBy.Slot` is a
                    // built-in enum with a registered type fact, so it does not
                    // pass through the user-enum field path above.
                    if enum_name == crate::Syntax::TYPE_REMOVE_BY
                        && matches!(member.as_str(), "Val" | "Slot")
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named(crate::Syntax::TYPE_REMOVE_BY.to_string()),
                                kind: TExprKind::EnumLit {
                                    enum_type: crate::Syntax::TYPE_REMOVE_BY.to_string(),
                                    variant: member.clone(),
                                    payload: TEnumPayload::Unit,
                                },
                            };
                        });
                    }
                    if enum_name == "EncodingErrorKind"
                        && matches!(
                            member.as_str(),
                            "Syntax" | "Truncated" | "Unsupported" | "Limit" | "IO" | "State"
                        )
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named(enum_name.clone()),
                                kind: TExprKind::EnumLit {
                                    enum_type: enum_name.clone(),
                                    variant: member.clone(),
                                    payload: TEnumPayload::Unit,
                                },
                            };
                        });
                    }
                    if enum_name == "EncodingFormat"
                        && matches!(
                            member.as_str(),
                            "JSON" | "JSONL" | "CSV" | "TOML" | "YAML" | "XML" | "CBOR"
                        )
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named(enum_name.clone()),
                                kind: TExprKind::EnumLit {
                                    enum_type: enum_name.clone(),
                                    variant: member.clone(),
                                    payload: TEnumPayload::Unit,
                                },
                            };
                        });
                    }
                    if env.ty_of(enum_name).is_none()
                        && matches!(
                            resolved_enum,
                            "SMTPSecurity" | "RecipientPolicy" | "SMTPAuth" | "TLSTrust"
                        )
                        && ((resolved_enum == "SMTPSecurity"
                            && matches!(member.as_str(), "StartTls" | "TLS"))
                            || (resolved_enum == "RecipientPolicy"
                                && matches!(member.as_str(), "RequireAll" | "DeliverAccepted"))
                            || (resolved_enum == "SMTPAuth" && member == "None")
                            || (resolved_enum == "TLSTrust" && member == "System"))
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named(resolved_enum.to_string()),
                                kind: TExprKind::EnumLit {
                                    enum_type: resolved_enum.to_string(),
                                    variant: member.clone(),
                                    payload: TEnumPayload::Unit,
                                },
                            };
                        });
                    }
                    // Core unit enums reach codegen as Field (`NetReadyInterest.Write`).
                    if env.ty_of(enum_name).is_none()
                        && ((resolved_enum == "NetReadyInterest"
                            && matches!(member.as_str(), "Read" | "Write" | "ReadWrite"))
                            || (resolved_enum == "NetShutdown"
                                && matches!(member.as_str(), "Read" | "Write" | "Both")))
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named(resolved_enum.to_string()),
                                kind: TExprKind::EnumLit {
                                    enum_type: resolved_enum.to_string(),
                                    variant: member.clone(),
                                    payload: TEnumPayload::Unit,
                                },
                            };
                        });
                    }
                    if env.ty_of(enum_name).is_none()
                        && cx.variant_owner.get(member).map(String::as_str)
                            == Some(enum_name.as_str())
                    {
                        return in_own_frame(|| {
                            // c109 Phase 24: a FOREIGN enum's unit literal (`NoteType.User` in
                            // search.jet) qualifies with the module path, exactly as `emit_expr`'s
                            // `Field` arm (Expression.rs ~L232): `{root}{mod}::__jet_<Enum>::<V>`.
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
                        });
                    }
                    if env.ty_of(enum_name).is_none() {
                        if let Some(owner) = cx
                            .variant_owner
                            .get(member)
                            .filter(|owner| owner.rsplit("::").next() == Some(enum_name))
                        {
                            return in_own_frame(|| {
                                return TExpr {
                                    ty: Type::Named(owner.clone()),
                                    kind: TExprKind::EnumLit {
                                        enum_type: owner.clone(),
                                        variant: member.clone(),
                                        payload: TEnumPayload::Unit,
                                    },
                                };
                            });
                        }
                    }
                    // D-ENC-DYN1=A+: `Data.Null` → `{root}jet_std::DataTree::Null` (a unit
                    // construction reaching codegen as a `Field`, the gate proved it).
                    if env.ty_of(enum_name).is_none()
                        && is_json_type_name(enum_name)
                        && member == "Null"
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named(Syntax::TYPE_DATA.to_string()),
                                kind: TExprKind::JSONLit {
                                    variant: "Null".to_string(),
                                    arg: None,
                                },
                            };
                        });
                    }
                    // D-DBDRIVER1: `DBValue.Null` — same no-arg-`Field` shape as `Data.Null`.
                    if env.ty_of(enum_name).is_none()
                        && is_db_value_type_name(enum_name)
                        && member == "Null"
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named(Syntax::TYPE_DB_VALUE.to_string()),
                                kind: TExprKind::DBValueLit {
                                    variant: "Null".to_string(),
                                    arg: None,
                                },
                            };
                        });
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
                                return in_own_frame(|| {
                                    return TExpr {
                                        ty: nt.clone(),
                                        kind: TExprKind::HostCall(Box::new(
                                            crate::Codegen::TIR::THostCall::NumericBounds {
                                                ty: nt.clone(),
                                                member: member.to_string(),
                                            },
                                        )),
                                    };
                                });
                            }
                        }
                    }
                    // User unit-enum construction (`Light.Green`) reaches codegen as
                    // a Field on an unbound type name. Fragment Cx may omit enum
                    // items, so `variant_owner` is empty; the spelling is still a
                    // unit variant when both segments are type-case and unbound.
                    if env.ty_of(enum_name).is_none()
                        && enum_name.chars().next().is_some_and(char::is_uppercase)
                        && member.chars().next().is_some_and(char::is_uppercase)
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named(resolved_enum.to_string()),
                                kind: TExprKind::EnumLit {
                                    enum_type: resolved_enum.to_string(),
                                    variant: member.clone(),
                                    payload: TEnumPayload::Unit,
                                },
                            };
                        });
                    }
                }
                // D-SOA1 / D-SOA-TIER1=A: a fused `xs[i].field` where `xs` is a
                // columnar list reads that field's column directly through the
                // shared store — the cache-friendly path, no whole-`S` gather.
                // The result is the same owned, bounds-checked field value the
                // array-of-structs form would produce.
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
                                // The column index comes from the ONE stored-field
                                // order every tier reads, so a fused read and the
                                // emitted column set can never disagree. A member
                                // that is not a stored column (a computed field)
                                // is not a fused read at all and falls through to
                                // the ordinary getter path below.
                                if let Type::Named(elem_name) = elem.as_ref() {
                                    if let Some(column) =
                                        cx.columnar_column_index(elem_name, member)
                                    {
                                        if let Some(field_ty) = struct_field_type(cx, elem, member)
                                        {
                                            let index_t = lower_expr(index, cx, env);
                                            let line = crate::Diagnostics::span_line_col(
                                                &cx.src, span.start,
                                            )
                                            .0;
                                            return TExpr {
                                                ty: field_ty,
                                                kind: TExprKind::ColumnarColumnRead {
                                                    owner: elem_name.clone(),
                                                    base: Box::new(base_t),
                                                    index: Box::new(index_t),
                                                    field: member.clone(),
                                                    column,
                                                    line,
                                                },
                                            };
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let recv = lower_expr(receiver, cx, env);
                if member == "value" {
                    if let Type::Apply { name, args } = recv.ty.clone() {
                        if name == Syntax::TYPE_SHARED_GUARD && args.len() == 1 {
                            return in_own_frame(|| {
                                return TExpr {
                                    ty: args[0].clone(),
                                    kind: TExprKind::SharedGuardValue {
                                        guard: Box::new(recv),
                                        editable: false,
                                    },
                                };
                            });
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
                                    return in_own_frame(|| {
                                        return TExpr {
                                            ty: args[0].clone(),
                                            kind: TExprKind::SharedGuardValue {
                                                guard: Box::new(recv),
                                                editable: matches!(
                                                    marker,
                                                    crate::AST::TagMarker::Internal(
                                                        crate::AST::InternalTag::SharedGuardEdit
                                                    )
                                                ),
                                            },
                                        };
                                    });
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
                        let call = TExpr {
                            ty: Type::Result {
                                ok: Box::new(field_ty.clone()),
                                err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
                            },
                            kind: TExprKind::MethodCall {
                                recv: Box::new(recv),
                                method: crate::Codegen::TIR::TMethodRef::inherent(member),
                                type_args: Vec::new(),
                                args: vec![],
                                source_first_string_literal: None,
                                operator_line: None,
                            },
                        };
                        let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
                        // D-SERDE2: a raw Encode method cannot propagate the
                        // getter's Result with `?`; consume it at the protocol
                        // boundary, matching the legacy field_self_read path.
                        let convert = if env.raw_protocol_return {
                            TTryConvert::ProtocolExit
                        } else {
                            TTryConvert::None
                        };
                        return TExpr {
                            ty: field_ty,
                            kind: TExprKind::Try {
                                inner: Box::new(call),
                                note: None,
                                convert,
                                file: escape_rust_str(&cx.file),
                                line,
                                fn_name: escape_rust_str(&env.fn_name),
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
                // The `Int` here is a LAST RESORT, never a default: it is the
                // guess that emitted `jet_int_to_string(<String>)` for
                // `child.output` and made rustc reject Jet's own output (card
                // 2021, an I2 internal compiler error). `struct_field_type` now
                // answers from the table sema declared the field in, so the
                // guess is unreachable for every user struct and every CORE
                // struct. Anything still reaching it is a receiver sema itself
                // could not name — do not widen it, and never make print's
                // integer fast path (`emit/expressions.rs`, `Type::Int`) key on
                // a type that came from here.
                let field_ty = struct_field_type(cx, &recv.ty, member).unwrap_or(Type::Int);
                // Preserve the checked field label; the native adapter owns its Rust spelling.
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
            })
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
            leading_dot,
            span,
            ..
        } => {
            if *leading_dot && type_name.is_empty() {
                if let Some(lowered) = in_own_frame(|| {
                    lower_raw_contextual_result_variant(variant, args, *span, cx, env)
                }) {
                    return lowered;
                }
            }
            in_own_frame(|| {
                let resolved_type = cx
                    .local_type_identities
                    .get(type_name)
                    .map(String::as_str)
                    .or_else(|| cx.core_qualified_rust_type_name(type_name))
                    .unwrap_or(type_name.as_str());
                let payload = in_own_frame(|| {
                    if args.is_empty() {
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
                        // Keep checked source labels through TIR; MIRRust owns Rust-name mangling.
                        let named = args
                            .iter()
                            .map(|a| match a {
                                EnumLitArg::Named { label, expr } => {
                                    let edge = format!("{}.{}", variant, label);
                                    (
                                        label.clone(),
                                        lower_enum_arg(
                                            resolved_type,
                                            variant,
                                            &edge,
                                            expr,
                                            cx,
                                            env,
                                        ),
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
                    }
                });
                TExpr {
                    ty: Type::Named(resolved_type.to_string()),
                    kind: TExprKind::EnumLit {
                        enum_type: resolved_type.to_string(),
                        variant: variant.clone(),
                        payload,
                    },
                }
            })
        }
        // c109 Phase 5: a list literal. Lowers each element as-is (mirrors the AST
        // `vec![…]` form — no clone/coercion at the literal site). The result type
        // is `[E]` with `E` taken from the first element; an empty `[]` has no
        // element to read, so its element type is unresolved (`Int` placeholder),
        // but the emitted `vec![]` is type-inferred by Rust from the binding context.
        Expr::ListLit(elems, _) => lower_list_lit(elems, cx, env),
        // c109 Phase 23: a named-tuple literal. The gate guaranteed `ty` is
        // `Some(Type::Tuple)`. The CANONICAL field order + struct name come from
        // the type; each canonical field's value is taken from the literal (by
        // name) and lowered. Fields keep their checked names: MIR keys a tuple
        // field by `<instance>::<name>` against the `Type::Tuple` shape, and
        // each backend mangles from its own field row.
        Expr::TupleLit(lit_fields, _, ty) => {
            in_own_frame(|| {
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
                        (n.clone(), v)
                    })
                    .collect();
                TExpr {
                    ty: ty.clone().unwrap_or_else(|| Type::Tuple(Vec::new())),
                    kind: TExprKind::TupleLit {
                        struct_name,
                        fields,
                    },
                }
            })
        }
        // A map literal is one typed value. Keep every lowered pair on the node so
        // nested maps stay values of the outer map instead of sharing a synthetic
        // mutable-map local with it.
        Expr::MapLit(entries, _) => in_own_frame(|| {
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
            TExpr {
                ty: map_ty,
                kind: TExprKind::MapLit(tentries),
            }
        }),
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
            in_own_frame(|| {
                // D-LAYOUT-FACTS1=B: select the checked layout field through
                // the typed `fields.find(...).unwrap()` path. The exact-name
                // predicate preserves the checked selector meaning: an absent
                // field stays absent and is not replaced with another field.
                if let IndexKind::LayoutField(field_name) = kind {
                    let layout_field_ty = Type::Named(Syntax::TYPE_LAYOUT_FIELD.to_string());
                    let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
                    let fields = TExpr {
                        ty: Type::List(Box::new(layout_field_ty.clone())),
                        kind: TExprKind::Field {
                            recv: Box::new(lower_expr(base, cx, env)),
                            field: "fields".to_string(),
                            boxed: false,
                        },
                    };
                    let field_param = TExpr {
                        ty: layout_field_ty.clone(),
                        kind: TExprKind::Local(TLocal::user("layout_field").through_ref()),
                    };
                    let predicate_body = TExpr {
                        ty: Type::Bool,
                        kind: TExprKind::Binary {
                            op: BinOp::Eq,
                            overflow: false,
                            line: line as u32,
                            lhs: Box::new(TExpr {
                                ty: Type::String,
                                kind: TExprKind::Field {
                                    recv: Box::new(field_param),
                                    field: "name".to_string(),
                                    boxed: false,
                                },
                            }),
                            rhs: Box::new(TExpr {
                                ty: Type::String,
                                kind: TExprKind::StrLit(vec![TStrPart::Lit(field_name.clone())]),
                            }),
                        },
                    };
                    let callback = TExpr {
                        ty: Type::Fn {
                            params: vec![layout_field_ty.clone()],
                            ret: Some(Box::new(Type::Bool)),
                            effect_bound: None,
                            param_contract: None,
                            call_metadata: None,
                            return_view_provenance: None,
                        },
                        kind: TExprKind::Lambda(Box::new(TLambda {
                            source_params: vec!["layout_field".to_string()],
                            param_types: vec![layout_field_ty.clone()],
                            ret: Some(Type::Bool),
                            failure_carrier: TFailureCarrier::Infallible,
                            executable: TLambdaBody::Expr(Box::new(predicate_body)),
                            source_span: *span,
                            frame_schedule: None,
                            frame_schedule_derivation: None,
                            capture_facts: crate::Codegen::TIR::TCaptureFacts::default(),
                            effects: crate::Codegen::TIR::TEffectFacts::default(),
                            jit_name: String::new(),
                            is_move: true,
                            boxed: false,
                            rc: false,
                            arc: false,
                            captures: Vec::new(),
                            materialized_captures: Vec::new(),
                            frozen_captures: Vec::new(),
                            uses_stack_sentry: false,
                        })),
                    };
                    let found = TExpr {
                        ty: Type::Option(Box::new(layout_field_ty.clone())),
                        kind: TExprKind::ClosureMethod {
                            recv: Box::new(fields),
                            op: crate::Codegen::TIR::TClosureOp::Find,
                            args: vec![callback],
                        },
                    };
                    return TExpr {
                        ty: layout_field_ty,
                        kind: TExprKind::HostCall(Box::new(THostCall::OptionProbe {
                            inner: Box::new(found),
                            kind: TOptionProbe::Unwrap,
                        })),
                    };
                }
                let base_t = lower_expr(base, cx, env);
                // `IndexKind` is a total sema fact. An unresolved kind is an
                // invariant violation now that fragment evaluation is gone.
                debug_assert!(
                    !matches!(kind, IndexKind::Unknown),
                    "sema-to-TIR handoff violated: unresolved index kind"
                );
                let index_t = lower_expr(index, cx, env);
                let base_ty = base_t.ty.without_user_tags();
                let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
                if matches!(kind, IndexKind::Range) {
                    return in_own_frame(|| {
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
                    });
                }
                // D-SIMD2: `v[i]` lane access on a SIMD lane type → a bounds-checked lane
                // read. The result is the lane scalar; sema resolved `IndexKind::Lane`.
                if let IndexKind::Lane(lane_ty) = kind {
                    return in_own_frame(|| {
                        return TExpr {
                            ty: crate::Sema::math_scalar_ty(lane_ty),
                            kind: TExprKind::MathLaneIndex {
                                lane_ty: lane_ty.clone(),
                                base: Box::new(base_t),
                                index: Box::new(index_t),
                                line: line as u32,
                            },
                        };
                    });
                }
                if let IndexKind::User(type_name) = kind {
                    return in_own_frame(|| {
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
                    });
                }
                // D-MEM1 S6 (D-POOLID-API1=A): `pool[id]` read — a generation-checked
                // clone of `T` via `jet_pool_get` (panics on a stale `id`, mirroring the
                // array-oob panic precedent). `ConstInline` is the pragmatic vehicle: no
                // new `TExprKind` needed for a single free-function call, same as the
                // `SQL.raw` escape in `lower_method_call` below.
                if matches!(kind, IndexKind::Pool) {
                    let elem_ty = match base_ty {
                        Type::Apply { name, args } if name == "Pool" && !args.is_empty() => {
                            args[0].clone()
                        }
                        _ => Type::Int,
                    };
                    let src_line = cx
                        .src
                        .lines()
                        .nth(line.saturating_sub(1))
                        .unwrap_or_default()
                        .to_string();
                    return TExpr {
                        ty: elem_ty,
                        kind: TExprKind::PoolSlot {
                            pool: Box::new(base_t),
                            id: Box::new(index_t),
                            mutable: false,
                            field: None,
                            line,
                            src_line,
                        },
                    };
                }
                if matches!(kind, IndexKind::FixedListProof) {
                    let elem_ty = match base_ty {
                        Type::FixedList { elem, .. } => (**elem).clone(),
                        _ => Type::Int,
                    };
                    return TExpr {
                        ty: elem_ty,
                        kind: TExprKind::HostCall(Box::new(
                            crate::Codegen::TIR::THostCall::FixedListIndex {
                                base: Box::new(base_t),
                                index: Box::new(index_t),
                                line: line as u32,
                            },
                        )),
                    };
                }
                let result_ty = match base_ty {
                    Type::List(elem) => (**elem).clone(),
                    Type::Map { value, .. } => (**value).clone(),
                    Type::FixedList { elem, .. } => (**elem).clone(),
                    // D-DYNARRAY1: `window[i]` on a `View<T>`.
                    Type::Apply { name, args }
                        if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut")
                            && args.len() == 1 =>
                    {
                        args[0].clone()
                    }
                    _ => Type::Int,
                };
                // D-SOA1: `xs[i]` on a columnar list gathers the logical `S` from the
                // columns. (A fused `xs[i].field` is handled in the `Field` arm before
                // this point — that path reads a single column directly.)
                if let Type::List(elem) = base_ty {
                    if cx.columnar_list_type(elem).is_some() {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: result_ty,
                                kind: TExprKind::ColumnarGather {
                                    base: Box::new(base_t),
                                    index: Box::new(index_t),
                                    line,
                                },
                            };
                        });
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
            })
        }
        // Owned slicing lowers here. Place contexts are handled above and use
        // ViewNew/ViewMutNew, preserving the owner's storage.
        Expr::Slice {
            base,
            start,
            end,
            range,
            span,
        } => in_own_frame(|| {
            let base_t = lower_expr(base, cx, env);
            let start_t = lower_expr(start, cx, env);
            let end_t = lower_expr(end, cx, env);
            let range_t = range
                .as_ref()
                .map(|range| Box::new(lower_expr(range, cx, env)));
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
        }),
        // D-TAINT1: `#Tainted expr` — the value-fact tag is **erased in codegen**
        // (I3). Lower the inner expression unchanged; taint exists only as a
        // compile-time sema proof, never a runtime value.
        Expr::Tainted(inner, _, _) => lower_expr(inner, cx, env),
        // c109 Phase 8: `value(x)` → `Some(x)`. The result type is `T?` where `T` is
        // the inner's resolved type (totality). The constructor owns its payload,
        // so resource locals must transfer through `ResourceTake`, not dereference
        // the cleanup guard. Mirrors `Expr::Present`.
        Expr::Present(inner, _) => in_own_frame(|| {
            let mut t = lower_owned_expr(inner, cx, env);
            if let Some(Type::Option(want)) = &env.ret_ty {
                t = preserve_typed_list_shape(t, want, cx);
            }
            TExpr {
                ty: Type::Option(Box::new(t.ty.clone())),
                kind: TExprKind::Present(Box::new(t)),
            }
        }),
        // c109 Phase 8: bare `null` → `None`. The element type is unresolved here
        // (`Int` placeholder) — like an empty `vec![]`, Rust infers it from the
        // binding/return context. Mirrors `Expr::Absent`.
        Expr::Absent(_) => TExpr {
            ty: Type::Option(Box::new(Type::Int)),
            kind: TExprKind::Absent,
        },
        // D-FAIL-BREACH1=A: a `#Todo` typed goal becomes the E3011 Prelude stop.
        // The enclosing checked return type lives on TExpr.ty; MIR interns that
        // type directly instead of carrying or reparsing a display string.
        Expr::Todo { span, .. } => in_own_frame(|| {
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: env.ret_ty.clone().unwrap_or_else(unit_type),
                kind: TExprKind::Todo { line },
            }
        }),
        // Card #1440: the dead end of an else-less exhaustive dispatch. Sema
        // proved coverage (E0307); like Todo, the result `ty` is never
        // load-bearing — the node diverges on every tier.
        Expr::NoElse(span) => in_own_frame(|| {
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: Type::Named("Unit".to_string()),
                kind: TExprKind::Unreachable { line },
            }
        }),
        Expr::Ok(inner, _) => in_own_frame(|| {
            let mut t = lower_owned_expr(inner, cx, env);
            // Sema uses `Ok(...)` to lift an unwrapped value-expected tail into
            // the callable's effective Result carrier. A function-value call
            // already has that carrier, so lifting it again would emit
            // `Ok(Result<...>)` and leave rustc with a nested result. Preserve
            // the existing carrier and wrap only a raw success value.
            if env.ret_ty.as_ref() == Some(&t.ty) {
                return t;
            }
            if let Some(Type::Result { ok, .. }) = &env.ret_ty {
                t = preserve_typed_list_shape(t, ok, cx);
                t = crate::Codegen::TIR::maybe_widen_expr_to_union(t, ok);
            }
            TExpr {
                ty: Type::Result {
                    ok: Box::new(t.ty.clone()),
                    err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
                },
                kind: TExprKind::Ok(Box::new(t)),
            }
        }),
        // c109 Phase 8: `Err(e)` → `Err(e)`. The err type is the inner's; the ok type
        // is unresolved here (inferred from the function return context).
        Expr::Err(inner, _) => in_own_frame(|| {
            let mut t = lower_owned_expr(inner, cx, env);
            let ok_ty = match env.ret_ty.as_ref() {
                Some(Type::Result { ok, .. }) => (**ok).clone(),
                _ => Type::Int,
            };
            if let Some(Type::Result { err, .. }) = &env.ret_ty {
                t = preserve_typed_list_shape(t, err, cx);
                t = crate::Codegen::TIR::maybe_widen_expr_to_union(t, err);
            }
            TExpr {
                ty: Type::Result {
                    ok: Box::new(ok_ty),
                    err: Box::new(t.ty.clone()),
                },
                kind: TExprKind::Err(Box::new(t)),
            }
        }),
        // c109 Phase 8: the `?` propagation operator. The `TryConvert` decision is the
        // total sema fact — reproduce it exactly (including Typed source/target). The result
        // type is the inner `Result`'s ok type (the `?` unwraps it). The trace-frame
        // location is resolved here so emit never reads `cx.current_fn`/`cx.src`.
        Expr::Try(inner, span, convert, note) => {
            in_own_frame(|| {
                // A sema-elaborated `?` owns the same carrier-preserving
                // subject boundary as `??`: keep the inner call's effective
                // Result/Option visible until this outer Try consumes it.
                // Isolate the worklist cache so nested argument lowering does
                // not replay values from the surrounding normal-value pass.
                let inner_t = {
                    let _try_subject_cache_scope = ExprCacheScope::enter();
                    let fallback_subject = env.fallback_subject;
                    env.fallback_subject = true;
                    let inner_t = lower_expr(inner, cx, env);
                    env.fallback_subject = fallback_subject;
                    inner_t
                };
                if matches!(&inner_t.ty, Type::Named(name) if name == Syntax::TYPE_NEVER) {
                    return inner_t;
                }
                let note_t = note
                    .as_ref()
                    .map(|note| Box::new(lower_expr(note, cx, env)));
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
                    TryConvert::Never => TTryConvert::Never,
                    TryConvert::DefaultErr => TTryConvert::DefaultErr,
                    TryConvert::Typed {
                        fn_name,
                        source,
                        target,
                    } => TTryConvert::Typed {
                        fn_name: fn_name.clone(),
                        source: source.clone(),
                        target: target.clone(),
                    },
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
                        note: note_t,
                        convert: tconvert,
                        file: escape_rust_str(&cx.file),
                        line,
                        fn_name: escape_rust_str(&env.fn_name),
                    },
                }
            })
        }
        // c109 Phase 8: the `??` fallback operator. D-FAIL-CARRIER1=A: one carrier,
        // so the value type alone gives the payload type. Mirrors `emit_or_fallback`.
        Expr::OrFallback {
            value, fallback, ..
        } => lower_or_fallback(value, fallback, cx, env),
        // c109 Phase 8: optional chaining `base?.member`. The `flatten` fact is total
        // (from sema): true → `.and_then`, false → `.map`. Resolve the projected
        // optional type in canonical TIR so every backend sees the same carrier.
        Expr::OptField {
            base,
            member,
            flatten,
            ..
        } => in_own_frame(|| {
            let base_t = lower_expr(base, cx, env);
            let ty = match &base_t.ty {
                Type::Option(payload_ty) => struct_field_type(cx, payload_ty, member)
                    .map(|field_ty| {
                        if *flatten {
                            match field_ty {
                                Type::Option(inner) => Type::Option(inner),
                                other => Type::Option(Box::new(other)),
                            }
                        } else {
                            Type::Option(Box::new(field_ty))
                        }
                    })
                    .unwrap_or_else(|| base_t.ty.clone()),
                _ => base_t.ty.clone(),
            };
            TExpr {
                ty,
                kind: TExprKind::OptField {
                    base: Box::new(base_t),
                    member: member.to_string(),
                    flatten: *flatten,
                },
            }
        }),
        // c109 Phase 11: a lambda/closure literal. The gate proved the body is
        // in-subset; lower it via `lower_lambda` (capture/escape facts total from
        // `Lambda.meta`). The result type is the closure's fn type — rarely
        // load-bearing in emit (a closure is consumed in arg position), so carry a
        // placeholder `Fn` type; the binding/arg context supplies the real Rust type.
        Expr::Lambda(lam) => in_own_frame(|| {
            let tl = lower_lambda(lam, cx, env);
            let params = tl.param_types.clone();
            let ret = tl.ret.clone().map(Box::new);
            TExpr {
                ty: Type::Fn {
                    params,
                    ret,
                    effect_bound: None,
                    param_contract: None,
                    call_metadata: None,
                    return_view_provenance: lam.meta.return_view_provenance.clone(),
                },
                kind: TExprKind::Lambda(Box::new(tl)),
            }
        }),
        // c109 Phase 18: `mem.Ptr<T>.from_addr(addr)` (S58). The result type is
        // `Ptr<elem>` (`ptr_type`), total from the node's `elem`. The element's Rust type
        // is resolved here (`cx.rust_type`) so emit makes no decision (I3). The cast is
        // safe Rust (no `unsafe`).
        Expr::PtrFromAddr { elem, addr, .. } => in_own_frame(|| {
            let taddr = lower_expr(addr, cx, env);
            TExpr {
                ty: crate::Sema::ptr_type(elem.clone()),
                kind: TExprKind::PtrFromAddr {
                    elem: elem.clone(),
                    addr: Box::new(taddr),
                },
            }
        }),
        // These nodes are compile-time syntax or condition-only shapes. Sema
        // consumes them before value lowering; retaining an explicit typed
        // violation prevents a checked node from disappearing into a fallback.
        Expr::StrMatchLit(_, span) => invariant_violation_expr(*span, "StrMatchLit"),
        Expr::BinMatchLit(_, span) => invariant_violation_expr(*span, "BinMatchLit"),
        Expr::MemberSpread { span, .. } => invariant_violation_expr(*span, "MemberSpread"),
        Expr::Spread(_, span) => invariant_violation_expr(*span, "Spread"),
        Expr::ReduceMarker(_, span) => invariant_violation_expr(*span, "ReduceMarker"),
        Expr::Paren(inner, _) => lower_expr(inner, cx, env),

        // D-UNITLIT1 / D-TYPE2-IMAG1=A: Comptime/MirBridge lower the raw AST,
        // so sema's unit-literal rewrite has not run yet. Reproduce it and
        // lower the result. An unsupported suffix is an impossible checked
        // shape, represented explicitly instead of as a successful Todo.
        Expr::UnitLit { suffix, span, .. } => match unit_lit_elaborated(e, cx) {
            Some(rewritten) => lower_expr(&rewritten, cx, env),
            None => invariant_violation_expr(*span, format!("UnitLit `{suffix}`")),
        },

        // Comptime/MirBridge can evaluate function bodies before sema elaborates
        // `Type.{ … }` (eval_comptime_items runs early). Mirror elaborate_typed_lit.
        // Sema also keeps the head on an EMPTY list/map literal (`[U8]{}`,
        // `[String:Int]{}`) on purpose: the head is the only source of the
        // element type, and this arm carries it into `t.ty` in every position.
        Expr::TypedLit { head, body, span } => {
            in_own_frame(|| {
                let Some(head) = head.clone() else {
                    return invariant_violation_expr(*span, "TypedLit without head");
                };
                if let Type::Named(type_name) = &head {
                    if let Some(lowered) = lower_boundary_typed_lit(type_name, body, cx, env) {
                        return lowered;
                    }
                }
                if head == Type::Named(Syntax::TYPE_REGEX.to_string()) {
                    if let TypedLitBody::Value(pattern) = body {
                        return in_own_frame(|| {
                            return core_call_expr(
                                head,
                                "core.regex",
                                "literal",
                                vec![lower_expr(pattern, cx, env)],
                                *span,
                                vec![false],
                            );
                        });
                    }
                }
                let rewritten = match (head.clone(), body.clone()) {
                    (Type::List(_) | Type::FixedList { .. }, TypedLitBody::Empty) => {
                        Expr::ListLit(Vec::new(), *span)
                    }
                    (Type::List(_) | Type::FixedList { .. }, TypedLitBody::Elements(elems)) => {
                        Expr::ListLit(elems, *span)
                    }
                    (Type::List(_) | Type::FixedList { .. }, TypedLitBody::ByteText(parts)) => {
                        let Some(elems) = byte_text_exprs(&parts, *span) else {
                            return invariant_violation_expr(*span, "TypedLit byte text");
                        };
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
                        retag_numeric_width(
                            &mut t,
                            &head,
                            crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32,
                        );
                        return t;
                    }
                    (_, TypedLitBody::Elements(elems)) if elems.len() == 1 => {
                        let mut t = lower_expr(&elems[0], cx, env);
                        retag_numeric_width(
                            &mut t,
                            &head,
                            crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32,
                        );
                        return t;
                    }
                    _ => {
                        return invariant_violation_expr(
                            *span,
                            format!("typed literal body vs head `{}`", head.name()),
                        );
                    }
                };
                in_own_frame(|| {
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
                            | Type::InlineRange { .. }
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
                        if matches!(elem.as_ref(), Type::Float | Type::Float32) {
                            let line =
                                crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32;
                            if let TExprKind::ListLit(elems) = &mut t.kind {
                                for el in elems.iter_mut() {
                                    retag_numeric_width(el, elem, line);
                                }
                            }
                        }
                    }
                    t
                })
            })
        }
        Expr::PatternTest {
            subject, pattern, ..
        } if is_binding_free_user_variant_pattern_test(pattern, cx) => {
            lower_binding_free_variant_pattern_test(subject, pattern, cx, env)
        }
        // D-FAIL-CARRIER1: sema represents expression-position `value == None`
        // as an absent pattern test.  Probe the same carrier used by TIR `if`
        // conditions, then negate its presence.  The evaluator and resident JIT
        // marshal this existing probe; the emitted program uses JetOptionalView.
        Expr::PatternTest {
            subject,
            pattern: Pattern::Absent(_),
            ..
        } => {
            let subject = lower_expr(subject, cx, env);
            let present = TExpr {
                ty: Type::Bool,
                kind: TExprKind::HostCall(Box::new(THostCall::OptionProbe {
                    inner: Box::new(subject),
                    kind: TOptionProbe::IsSome,
                })),
            };
            TExpr {
                ty: Type::Bool,
                kind: TExprKind::Unary {
                    op: UnOp::Not,
                    operand: Box::new(present),
                },
            }
        }
        // A PatternTest that reaches value lowering with any other pattern is
        // a condition-only shape consumed by the if-condition lowerer.
        Expr::PatternTest { span, .. } => invariant_violation_expr(*span, "PatternTest"),
    }
}

/// Retag a lowered scalar typed-literal body with the head's numeric width.
/// Nested float unary/binary operands inherit the same width so MirBridge
/// doesn't mix F32/F64 in `F32.{ -0.0 }` / `F32.{ max + max }`.
fn retag_numeric_width(expr: &mut TExpr, head: &Type, line: u32) {
    let mut work = TirWorklist::new();
    work.push(expr);
    while let Some(expr) = work.pop() {
        if matches!(head, Type::Float | Type::Float32)
            && matches!(expr.ty, Type::Int | Type::IntN { .. })
        {
            let source_signed = !matches!(expr.ty, Type::IntN { signed: false, .. });
            let source = std::mem::replace(
                expr,
                TExpr {
                    ty: head.clone(),
                    kind: TExprKind::Unit,
                },
            );
            expr.kind = TExprKind::NumericMethod {
                recv: Box::new(source),
                op: TNumericOp::CheckedIntToFloat {
                    source_signed,
                    target_f32: *head == Type::Float32,
                    line,
                },
            };
            continue;
        }
        expr.ty = head.clone();
        if !matches!(head, Type::Float | Type::Float32) {
            continue;
        }
        // The raw-fragment path deliberately lowers an untyped decimal token
        // through `Decimal.from_str`. A scalar `Float{ ... }` or `Float32{ ... }`
        // supplies the missing context only here, after the body has been
        // lowered, so convert that carrier back to the machine-float literal
        // before evaluation or AOT emission.
        let raw_float = match &expr.kind {
            TExprKind::PreciseBuiltin {
                type_name,
                func,
                args,
            } if type_name == Syntax::TYPE_DECIMAL && func == "from_str" => match args.as_slice() {
                [TExpr {
                    kind: TExprKind::StrLit(parts),
                    ..
                }] => match parts.as_slice() {
                    [TStrPart::Lit(raw)] => raw.replace('_', "").parse::<f64>().ok(),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        if let Some(value) = raw_float {
            expr.kind = TExprKind::FloatLit(value);
            continue;
        }
        match &mut expr.kind {
            TExprKind::Unary { operand, .. } => work.push(operand),
            TExprKind::Binary { lhs, rhs, .. } => {
                work.push(rhs);
                work.push(lhs);
            }
            TExprKind::Clone(inner)
            | TExprKind::ExplicitCopy(inner)
            | TExprKind::MaterializeView(inner) => work.push(inner),
            _ => {}
        }
    }
}

/// D-UNITLIT1 / D-TYPE2-IMAG1=A: the rewrite sema's `Expr::UnitLit` arm
/// performs (`Sema/CheckerInfer/expr.rs`), reproduced for the raw AST that
/// comptime items and MirBridge fragments lower before sema elaborates them.
/// An in-scope unit member wins over the imaginary suffix here too, so a user
/// `#UnitFamily` member named `i` shadows it exactly like any other unit.
///
/// A canonical Time literal (`500ms`) is absent on purpose: sema resolves it
/// against the Time family's literal facts, which a lowering `Cx` does not
/// carry, so a member of that family keeps refusing here instead of being
/// mistaken for an ordinary unit member and folded to the wrong value.
fn unit_lit_elaborated(e: &Expr, cx: &Cx) -> Option<Expr> {
    let Expr::UnitLit {
        int,
        float,
        suffix,
        suffix_span,
        span,
        ..
    } = e
    else {
        return None;
    };
    let value = float.unwrap_or_else(|| int.unwrap_or(0) as f64);
    let read_arg = |expr: Expr| CallArg {
        convention: AccessConvention::Read,
        expr,
        span: *span,
        flags: crate::AST::CallArgFlags::default(),
        label: None,
        spread: false,
    };
    let type_name = crate::AST::UnitFamilyDef::type_name(suffix);
    // Sema's `unit_literal("Time", suffix)` lookup, from the one fact a
    // lowering `Cx` does carry: the minted type's family name.
    let canonical_time = cx
        .unit_facts
        .get(&type_name)
        .is_some_and(|fact| fact.family == "Time");
    if !canonical_time
        && cx
            .distinct_types
            .get(&type_name)
            .is_some_and(|(base, numeric)| *numeric && *base == Type::Float)
    {
        return Some(Expr::MethodCall {
            receiver: Box::new(Expr::Ident(type_name, *suffix_span)),
            method: Syntax::numeric_conversion_method("Float")
                .expect("Float has a canonical conversion method")
                .to_string(),
            method_span: *suffix_span,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
            args: vec![read_arg(Expr::Float(value, *span, false, None))],
            recv_type: None,
            resolved_ret: None,
            operator_rhs: None,
            checked_widen: false,
        });
    }
    if suffix != Syntax::UNIT_SUFFIX_IMAGINARY {
        return None;
    }
    // `4i` is a pure imaginary value: zero real part, the literal imaginary.
    Some(Expr::Call(Call {
        name: Syntax::TYPE_COMPLEX.to_string(),
        name_span: *suffix_span,
        type_args: Vec::new(),
        args: vec![
            read_arg(Expr::Float(0.0, *span, false, None)),
            read_arg(Expr::Float(value, *span, false, None)),
        ],
        resolved_ret: None,
        range_checked: false,
        widen_approx: false,
    }))
}

/// D-TYPE2-IMAG1=A: `Checker::complexize_operand` for lowered operands. Only
/// the four arithmetic operators the precise `Complex` rule defines promote; a
/// comparison stays untouched so it keeps refusing instead of inventing an
/// operation sema rejects.
fn complexize_operands(op: BinOp, lhs: TExpr, rhs: TExpr, cx: &Cx) -> (TExpr, TExpr) {
    if !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
        || cx.type_names.contains(Syntax::TYPE_COMPLEX)
    {
        return (lhs, rhs);
    }
    let is_complex = |ty: &Type| matches!(ty, Type::Named(name) if name == Syntax::TYPE_COMPLEX);
    let is_scalar = |ty: &Type| {
        matches!(
            ty,
            Type::Int | Type::IntN { .. } | Type::Float | Type::Float32
        )
    };
    match (is_complex(&lhs.ty), is_complex(&rhs.ty)) {
        (true, false) if is_scalar(&rhs.ty) => (lhs, complex_from_scalar(rhs)),
        (false, true) if is_scalar(&lhs.ty) => (complex_from_scalar(lhs), rhs),
        _ => (lhs, rhs),
    }
}

/// The shared `Complex` carrier is two `f64` parts on every tier, so an integer
/// or `F32` operand widens through the same numeric conversion op
/// `Float.from_int(…)` lowers to before it becomes the real part.
fn complex_from_scalar(value: TExpr) -> TExpr {
    let real = if matches!(value.ty, Type::Float) {
        value
    } else {
        let source = value.ty.name();
        let op = crate::Codegen::TIR::resolve_numeric_conversion_op("Float", &source)
            .expect("a complex scalar is a registered numeric conversion source");
        TExpr {
            ty: Type::Float,
            kind: TExprKind::NumericMethod {
                recv: Box::new(value),
                op,
            },
        }
    };
    TExpr {
        ty: Type::Named(Syntax::TYPE_COMPLEX.to_string()),
        kind: TExprKind::PreciseBuiltin {
            type_name: Syntax::TYPE_COMPLEX.to_string(),
            func: "from_parts".to_string(),
            args: vec![
                real,
                TExpr {
                    ty: Type::Float,
                    kind: TExprKind::FloatLit(0.0),
                },
            ],
        },
    }
}

/// D-FAILURE-FOUNDATION1=A: sema types a call to a plain helper (`fn f() T`)
/// as its declared success value — `call.resolved_ret` is `T`, not the
/// carrier — and never wraps it in `Try` (a plain helper does not enter the
/// failure rail). The callable still returns the shared default route
/// `Result<T, Err>`, which is the type `lower_expr` gives the call. Consume
/// that route here through the one `Try` node sema itself elaborates for a
/// fallible callee, so every backend sees `T` in value position and the
/// default route still propagates. A `??` subject keeps the carrier: the
/// fallback consumes it itself.
fn consume_plain_helper_route(call: &Call, lowered: TExpr, cx: &Cx, env: &LowerEnv) -> TExpr {
    if env.fallback_subject
        || !call
            .resolved_ret
            .as_ref()
            .is_some_and(|declared| !declared.is_fallible())
    {
        return lowered;
    }
    let Type::Result { ok, err } = &lowered.ty else {
        return lowered;
    };
    let convert = if matches!(err.as_ref(), Type::Named(name) if name == Syntax::TYPE_NEVER) {
        TTryConvert::Never
    } else if env.raw_protocol_return {
        TTryConvert::ProtocolExit
    } else {
        TTryConvert::None
    };
    let line = crate::Diagnostics::span_line_col(&cx.src, call.name_span.start).0;
    TExpr {
        ty: (**ok).clone(),
        kind: TExprKind::Try {
            inner: Box::new(lowered),
            note: None,
            convert,
            file: escape_rust_str(&cx.file),
            line,
            fn_name: escape_rust_str(&env.fn_name),
        },
    }
}

/// Lower an expression whose result is stored or returned as an owned value.
/// A `Read`/`Write` non-scalar parameter is represented by a dereferenced Rust
/// borrow; moving that place would leak E0507 from rustc. Jet generic functions
/// record the clone's type so generic emission adds the required bound, then
/// materialize the owned value at this semantic boundary.
pub(crate) fn lower_owned_expr(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    fn reads_borrowed_place(e: &Expr, env: &LowerEnv) -> bool {
        let mut current = e;
        loop {
            match current {
                Expr::Ident(name, _) => return env.is_borrowed(name),
                Expr::Field(base, _, _) | Expr::Index { base, .. } | Expr::Paren(base, _) => {
                    current = base
                }
                _ => return false,
            }
        }
    }

    let lowered = lower_expr(e, cx, env);
    if matches!(e, Expr::Ident(name, _) if env.is_resource(name)) {
        let Expr::Ident(name, _) = e else {
            unreachable!()
        };
        TExpr {
            ty: lowered.ty,
            kind: TExprKind::ResourceTake(env.resource_take_place(name)),
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
    let has_default = args
        .iter()
        .any(|arg| arg.flags.binder_slot.is_some() && arg.flags.source_index.is_none());
    if !has_default && !args.iter().any(|arg| arg.flags.source_index.is_some()) {
        return None;
    }
    let mut slots: Vec<usize> = (0..args.len())
        .filter(|slot| args[*slot].flags.source_index.is_some())
        .collect();
    slots.sort_by_key(|slot| args[*slot].flags.source_index);
    if has_default {
        let mut defaults: Vec<usize> = (0..args.len())
            .filter(|slot| {
                args[*slot].flags.binder_slot.is_some() && args[*slot].flags.source_index.is_none()
            })
            .collect();
        defaults.sort_by_key(|slot| args[*slot].flags.binder_slot);
        slots.extend(defaults);
    }
    Some(slots)
}

/// D-BOUND-UNDO1=A: turn a proven foreign undo contract into the existing
/// transaction rollback hook. The foreign expression remains the only bridge
/// call; this helper only captures its arguments and registers an ordinary
/// `on_rollback` closure before evaluating it.
pub(crate) fn wrap_foreign_undo(
    lowered: TExpr,
    inverse: Option<&str>,
    site: u32,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    let Some(inverse) = inverse else {
        return lowered;
    };
    let Some(handle) = env.txn_handle.clone() else {
        return lowered;
    };
    let result_ty = lowered.ty.clone();
    let (mut prefix, foreign) = match lowered.kind {
        TExprKind::InlineBlock(mut stmts) => {
            let Some(last) = stmts.pop() else {
                return TExpr {
                    ty: result_ty,
                    kind: TExprKind::InlineBlock(stmts),
                };
            };
            let TStmt::ExprStmt(expr) = last else {
                stmts.push(last);
                return TExpr {
                    ty: result_ty,
                    kind: TExprKind::InlineBlock(stmts),
                };
            };
            (stmts, expr)
        }
        kind => (
            Vec::new(),
            TExpr {
                ty: result_ty.clone(),
                kind,
            },
        ),
    };
    enum Forward {
        Extern {
            symbol: String,
            c_abi: bool,
        },
        Module {
            form: TModuleCallForm,
            target_return: Option<Type>,
            type_args: Vec<Type>,
        },
    }
    enum Arg {
        Extern(TExternArg),
        Module(TCallArg),
    }
    let (forward, args) = match foreign.kind {
        TExprKind::ExternCall {
            symbol,
            c_abi,
            args,
        } => (
            Forward::Extern { symbol, c_abi },
            args.into_iter().map(Arg::Extern).collect::<Vec<_>>(),
        ),
        TExprKind::ModuleCall {
            form,
            target_return,
            type_args,
            args,
        } => (
            Forward::Module {
                form,
                target_return,
                type_args,
            },
            args.into_iter().map(Arg::Module).collect::<Vec<_>>(),
        ),
        kind => {
            prefix.push(TStmt::ExprStmt(TExpr {
                ty: result_ty.clone(),
                kind,
            }));
            return TExpr {
                ty: result_ty,
                kind: TExprKind::InlineBlock(prefix),
            };
        }
    };

    if let Some(flag) = &env.txn_undo_needed {
        flag.set(true);
    }
    let inverse_params = cx.sigs.get(inverse).cloned().unwrap_or_default();
    let mut forward_extern_args = Vec::with_capacity(args.len());
    let mut forward_module_args = Vec::with_capacity(args.len());
    let mut inverse_args = Vec::with_capacity(args.len());
    let mut captures = Vec::with_capacity(args.len());
    for (index, arg) in args.into_iter().enumerate() {
        let (value, module_arg, extern_mut_borrow) = match arg {
            Arg::Extern(arg) => (arg.value, None, arg.mut_borrow),
            Arg::Module(arg) => {
                let TCallArg {
                    value,
                    borrow,
                    mut_borrow,
                    fn_coerce,
                    widen_to_vec,
                    widen_to_union,
                    box_as_trait,
                    ..
                } = arg;
                (
                    value,
                    Some((
                        borrow,
                        mut_borrow,
                        fn_coerce,
                        widen_to_vec,
                        widen_to_union,
                        box_as_trait,
                    )),
                    false,
                )
            }
        };
        let temp = jet_name_format!("{name_prefix}undo_arg_{site}_{index}");
        let ty = value.ty.clone();
        env.note_clone(&ty);
        captures.push((
            temp.clone(),
            crate::Codegen::TIR::local_place(&temp),
            ty.clone(),
        ));
        prefix.push(TStmt::Let {
            name: temp.clone(),
            kw: "let",
            let_ty: crate::Codegen::TIR::TLetTy::inferred(),
            init: TExpr {
                ty: ty.clone(),
                kind: TExprKind::Clone(Box::new(value)),
            },
            gc_promotion: None,
            gc_transferred: false,
        });
        let local_for_forward = TExpr {
            ty: ty.clone(),
            kind: TExprKind::Local(TLocal::user(temp.clone())),
        };
        match module_arg {
            Some((borrow, mut_borrow, fn_coerce, widen_to_vec, widen_to_union, box_as_trait)) => {
                // The capture owns the snapshot. Keep the module-call's boundary
                // conversions, but do not clone a second time before the forward call.
                forward_module_args.push(TCallArg {
                    value: local_for_forward,
                    template_items: None,
                    borrow,
                    mut_borrow,
                    clone: false,
                    arc_clone: false,
                    fn_coerce,
                    widen_to_vec,
                    widen_to_union,
                    box_as_trait,
                });
            }
            None => {
                // A non-scalar foreign parameter is already passed as a clone by the
                // normal FFI lowering. Clone the captured slot for the forward call so
                // the rollback closure keeps its copy alive.
                forward_extern_args.push(TExternArg {
                    value: local_for_forward,
                    clone: !ty.is_scalar(),
                    mut_borrow: extern_mut_borrow,
                });
            }
        }
        let (convention, inverse_ty) = inverse_params
            .get(index)
            .cloned()
            .unwrap_or((AccessConvention::Read, ty.clone()));
        inverse_args.push(TCallArg {
            value: TExpr {
                ty: ty.clone(),
                kind: TExprKind::Local(TLocal::user(temp)),
            },
            template_items: None,
            borrow: convention == AccessConvention::Read && !inverse_ty.is_scalar(),
            mut_borrow: convention == AccessConvention::Write,
            clone: false,
            arc_clone: false,
            fn_coerce: None,
            widen_to_vec: false,
            widen_to_union: None,
            box_as_trait: None,
        });
    }
    let inverse_expr = TExpr {
        ty: call_return_type(cx, inverse),
        kind: TExprKind::Call {
            name: inverse.to_string(),
            type_args: Vec::new(),
            args: inverse_args,
        },
    };
    let inverse_ret = (!matches!(
        &inverse_expr.ty,
        Type::Named(name) if name == "Unit"
    ))
    .then(|| inverse_expr.ty.clone());
    let inverse_failure = TFailureCarrier::from_checked_type(&inverse_expr.ty);
    let capture_facts = crate::Codegen::TIR::TCaptureFacts {
        escapes: true,
        moved: captures.iter().map(|(name, _, _)| name.clone()).collect(),
        ..Default::default()
    };
    let lambda = TLambda {
        executable: TLambdaBody::Expr(Box::new(inverse_expr)),
        source_span: Span::new(site as usize, site as usize),
        frame_schedule: None,
        frame_schedule_derivation: None,
        capture_facts,
        failure_carrier: inverse_failure,
        effects: crate::Codegen::TIR::TEffectFacts::default(),
        source_params: Vec::new(),
        jit_name: mangle_generated(&format!("undo_{site}")),
        param_types: Vec::new(),
        ret: inverse_ret,
        is_move: true,
        boxed: false,
        rc: false,
        arc: false,
        captures,
        materialized_captures: Vec::new(),
        frozen_captures: Vec::new(),
        uses_stack_sentry: false,
    };
    let registration = TExpr {
        ty: Type::Named("TransactionGuard".to_string()),
        kind: TExprKind::CoreClosureCall {
            kind: TCoreClosureKind::OnRollback {
                handle_name: handle.name.clone(),
                executable: Box::new(lambda),
            },
        },
    };
    prefix.push(TStmt::ExprStmt(registration));
    let forward_kind = match forward {
        Forward::Extern { symbol, c_abi } => TExprKind::ExternCall {
            symbol,
            c_abi,
            args: forward_extern_args,
        },
        Forward::Module {
            form,
            target_return,
            type_args,
        } => TExprKind::ModuleCall {
            form,
            target_return,
            type_args,
            args: forward_module_args,
        },
    };
    prefix.push(TStmt::ExprStmt(TExpr {
        ty: result_ty.clone(),
        kind: forward_kind,
    }));
    TExpr {
        ty: result_ty,
        kind: TExprKind::InlineBlock(prefix),
    }
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
        | TExprKind::StaticCall { args, .. }
        | TExprKind::ModuleCall { args, .. }
        | TExprKind::FnValue {
            kind: crate::Codegen::TIR::TFnValueKind::Call { args, .. },
        } => bind_arg_temporaries(args, order, ast_arg_count, site),
        TExprKind::CoreCall { args, .. } => bind_arg_temporaries(args, order, ast_arg_count, site),
        TExprKind::ExternCall { args, .. } => {
            bind_arg_temporaries(args, order, ast_arg_count, site)
        }
        TExprKind::HandleMethod { args, .. } => {
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

trait OrderedArg {
    fn value(&self) -> &TExpr;
    /// A by-reference place must remain a place: moving it into a temporary
    /// changes ownership, while sema has already proved its read/write timing.
    /// Other arguments can be pinned in source order.
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
        if self.borrow || self.mut_borrow {
            value = TExpr {
                ty: value.ty.clone(),
                kind: TExprKind::Borrow {
                    place: Box::new(value),
                    mutable: self.mut_borrow,
                },
            };
            self.borrow = false;
            self.mut_borrow = false;
        }
        value
    }
}

impl OrderedArg for crate::Codegen::TIR::TExternArg {
    fn value(&self) -> &TExpr {
        &self.value
    }

    fn can_bind(&self) -> bool {
        !self.mut_borrow
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
        if self.mut_borrow {
            value = TExpr {
                ty: value.ty.clone(),
                kind: TExprKind::Borrow {
                    place: Box::new(value),
                    mutable: true,
                },
            };
            self.mut_borrow = false;
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
        // move.
        self.ty.is_scalar()
            || matches!(
                &self.kind,
                TExprKind::Call { .. }
                    | TExprKind::MethodCall { .. }
                    | TExprKind::StaticCall { .. }
                    | TExprKind::ModuleCall { .. }
                    | TExprKind::FnValue { .. }
                    | TExprKind::CoreCall { .. }
                    | TExprKind::ExternCall { .. }
                    | TExprKind::HostCall(_)
                    | TExprKind::InlineBlock(_)
                    | TExprKind::Clone(_)
                    | TExprKind::StrLit(_)
                    | TExprKind::Print(_)
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
    let mut order: Vec<usize> = order.iter().map(|slot| slot + offset).collect();
    if offset > 0 {
        // `#Root` dot calls keep the receiver in TIR slot zero even though
        // sema stripped it from the AST argument list. Materialize that slot
        // before the written arguments: later defaults may refer to the root
        // parameter, and the reference must read the same once-evaluated temp.
        order.splice(0..0, 0..offset);
    }
    // Every bindable slot in the binder's source order is evaluated exactly
    // once. A borrowed place stays in the call, where its access wrapper is
    // emitted against the original place instead of moving it into a temp.
    if order.is_empty() {
        return Vec::new();
    }
    let bindable: Vec<usize> = order
        .into_iter()
        .filter(|slot| args.get(*slot).is_some_and(OrderedArg::can_bind))
        .collect();
    if bindable.is_empty() {
        return Vec::new();
    }
    let mut stmts: Vec<TStmt> = Vec::with_capacity(bindable.len() + 1);
    for slot in bindable {
        let arg = args
            .get_mut(slot)
            .expect("binder source-order slot must have a lowered argument");
        // The name has to be unique across nesting: a nested reordered call is
        // lowered as the initialiser of one of these very temporaries, and the
        // interpreter shares one scope with it. `site` is the call's source
        // offset, so two calls can never collide and the name stays stable
        // across runs.
        let ast_slot = slot.saturating_sub(offset);
        let temp_slot = if offset > 0 { slot } else { ast_slot };
        let temp = jet_format!("{jet_prefix}arg{site}_{temp_slot}");
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
        f if f == Syntax::COMPILER_FACT_RANGE => Type::Named(Syntax::TYPE_RANGE.to_string()),
        f if f == Syntax::COMPILER_FACT_DIMENSION => Type::Named("DimensionInfo".to_string()),
        f if f == Syntax::COMPILER_FACT_STATES => {
            Type::List(Box::new(Type::Named("StateInfo".to_string())))
        }
        f if f == Syntax::COMPILER_FACT_EFFECTS => Type::Named("EffectInfo".to_string()),
        f if f == Syntax::COMPILER_BUILD_FACT_PROFILE => Type::String,
        _ => Type::Named(Syntax::TYPE_LAYOUT_INFO.to_string()),
    }
}

#[cfg(test)]
mod source_order_tests {
    use super::*;
    use crate::Codegen::TIR::TExternArg;
    use crate::AST::BinOp;

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
            TStmt::Let {
                init: TExpr {
                    kind: TExprKind::Binary { op: BinOp::Div, .. },
                    ..
                },
                ..
            }
        ));
        assert!(matches!(
            &stmts[1],
            TStmt::Let {
                init: TExpr {
                    kind: TExprKind::Print(_),
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn core_call_keeps_panicking_arithmetic_in_written_order() {
        // The first source expression sits in slot 1 and widens at the call
        // site. Its raw value must still be evaluated first.
        let call = core_call_expr(
            unit_type(),
            "core.units",
            "from",
            vec![print(), division()],
            Span::new(0, 1),
            vec![false, true],
        );
        assert_division_then_print(preserve_source_arg_order(call, &[1, 0], 2, 7));
    }

    #[test]
    fn extern_call_uses_the_same_written_order_wrapper() {
        let call = TExpr {
            ty: unit_type(),
            kind: TExprKind::ExternCall {
                symbol: "ordered".to_string(),
                c_abi: false,
                args: vec![
                    TExternArg {
                        value: print(),
                        clone: false,
                        mut_borrow: false,
                    },
                    TExternArg {
                        value: division(),
                        clone: false,
                        mut_borrow: false,
                    },
                ],
            },
        };
        assert_division_then_print(preserve_source_arg_order(call, &[1, 0], 2, 9));
    }

    #[test]
    fn core_call_pins_a_local_read_after_an_earlier_call() {
        let call = core_call_expr(
            unit_type(),
            "core.units",
            "from",
            vec![
                TExpr {
                    ty: Type::Int,
                    kind: TExprKind::Local(TLocal::user("x")),
                },
                bump(),
            ],
            Span::new(0, 1),
            vec![false, false],
        );
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
            TStmt::Let {
                init: TExpr {
                    kind: TExprKind::Local(_),
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn core_call_does_not_move_a_read_borrowed_string_place() {
        let call = core_call_expr(
            unit_type(),
            "core.units",
            "from",
            vec![
                TExpr {
                    ty: Type::String,
                    kind: TExprKind::Local(TLocal::user("key")),
                },
                bump(),
            ],
            Span::new(0, 1),
            vec![false, false],
        );
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
