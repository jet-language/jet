use crate::Codegen::Cx;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::data_plan_for_core_call;
use crate::Codegen::TIR::TFailureCarrier;
use crate::Codegen::TIR::core_closure_call_return_ty;
use crate::Codegen::TIR::lambda_body_ty;
use crate::Codegen::TIR::lambda_body_ty_expecting;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_lambda;
use crate::Codegen::TIR::lower_lambda_expecting_callable;
use crate::Codegen::TIR::lower_lambda_expecting_value;
use crate::Codegen::TIR::lower_lambda_expecting_value_with_return;
use crate::Codegen::TIR::lower_spawn_lambda_for_jit;
use crate::Codegen::TIR::lower_spawn_lambda_for_jit_expecting;
use crate::Codegen::TIR::lower_owned_expr;
use crate::Codegen::TIR::fixed_list_elem_compatible;
use crate::Codegen::TIR::lower_spawn_lambda_for_jit_unit;
use crate::Codegen::TIR::spawn_body_carrier_ty;
use crate::Codegen::TIR::spawn_label;
use crate::Codegen::TIR::unit_type;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::TCoreClosureKind;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TJitSpawnLambda;
use crate::Codegen::TIR::TLambda;
use crate::Diagnostics::Span;
use crate::AST::{Expr, Lambda, Type};

fn invariant_violation_expr(span: Span, construct: impl Into<String>) -> TExpr {
    TExpr {
        ty: Type::Named(crate::Syntax::TYPE_NEVER.to_string()),
        kind: TExprKind::InvariantViolation {
            construct: construct.into(),
            span,
        },
    }
}

fn checked_core_record(
    module: &str,
    method: &str,
    arity: usize,
    span: Span,
) -> Result<&'static crate::Syntax::CoreCallRecord, TExpr> {
    crate::Syntax::core_call_projection(
        module,
        method,
        crate::Syntax::CoreCallCoverage::TIR_SUBSET,
        arity,
    )
    .map_err(|error| {
        invariant_violation_expr(
            span,
            format!(
                "checked Core call `{module}.{method}` has no canonical TIR projection ({error:?})"
            ),
        )
    })
}

fn lambda_from_expr(expr: &Expr) -> Option<&Lambda> {
    match expr {
        Expr::Paren(inner, _) => lambda_from_expr(inner),
        Expr::Lambda(lam) => Some(lam),
        _ => None,
    }
}

fn required_lambda<'a>(
    args: &'a [crate::AST::CallArg],
    index: usize,
    module: &str,
    method: &str,
    span: Span,
) -> Result<&'a Lambda, TExpr> {
    args.get(index)
        .and_then(|arg| lambda_from_expr(&arg.expr))
        .ok_or_else(|| {
            invariant_violation_expr(
                span,
                format!(
                    "checked Core call `{module}.{method}` has no lambda at argument {index}"
                ),
            )
        })
}
fn unit_callback_type() -> Type {
    Type::Fn {
        params: Vec::new(),
        ret: Some(Box::new(unit_type())),
        effect_bound: None,
        param_contract: None,
        call_metadata: None,
        return_view_provenance: None,
    }
}
fn ui_drop_callback_type() -> Type {
    Type::Fn {
        params: vec![Type::List(Box::new(Type::Named("UiDropItem".to_string())))],
        ret: Some(Box::new(unit_type())),
        effect_bound: None,
        param_contract: None,
        call_metadata: None,
        return_view_provenance: None,
    }
}
fn ui_preview_callback_type() -> Type {
    Type::Fn {
        params: Vec::new(),
        ret: Some(Box::new(Type::Named("UiNode".to_string()))),
        effect_bound: None,
        param_contract: None,
        call_metadata: None,
        return_view_provenance: None,
    }
}

fn lower_optional_core_arg(
    arg: &crate::AST::CallArg,
    expected: &Type,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    let option_ty = Type::Option(Box::new(expected.clone()));
    if matches!(arg.expr, Expr::Absent(_)) {
        TExpr {
            ty: option_ty,
            kind: TExprKind::Absent,
        }
    } else {
        let value = lower_owned_expr(&arg.expr, cx, env);
        TExpr {
            ty: option_ty,
            kind: TExprKind::Present(Box::new(value)),
        }
    }
}
fn typed_lambda_value(source: &Lambda, lowered: TLambda) -> TExpr {
    let ty = Type::Fn {
        params: lowered.param_types.clone(),
        ret: lowered.ret.clone().map(Box::new),
        effect_bound: None,
        param_contract: None,
        call_metadata: None,
        return_view_provenance: source.meta.return_view_provenance.clone(),
    };
    TExpr {
        ty,
        kind: TExprKind::Lambda(Box::new(lowered)),
    }
}
/// Return the checked raw return of a query callback whose native ABI fixes it.
/// Generic query callbacks keep their source-inferred return because that type
/// becomes the host generic; fixed slots must not inherit Jet's default carrier.
fn query_callback_return_type(method: &str, index: usize) -> Option<Type> {
    match method {
        "filter" => Some(Type::Bool),
        "sort_by" => Some(Type::String),
        "inner_join" | "left_join" if index >= 1 => Some(Type::String),
        _ => None,
    }
}

fn no_widening(args: &[TExpr]) -> Vec<bool> {
    vec![false; args.len()]
}

fn data_widening(args: &[TExpr], expected_lists: &[(usize, &Type)]) -> Vec<bool> {
    let mut widening = no_widening(args);
    for &(index, expected) in expected_lists {
        if let Some(arg) = args.get(index) {
            widening[index] = match &arg.ty {
                Type::FixedList { elem, .. } => fixed_list_elem_compatible(elem, expected),
                _ => false,
            };
        }
    }
    widening
}

/// The JIT spawn-lambda table index for one source callback.
///
/// One source lambda is ONE table entry. A lambda body is lowered once per
/// pass — the AOT closure text, the `executable` TIR, and the JIT spawn body —
/// and a callback nested in that body is reached again on every one of them.
/// Pushing per lowering would grow `jit_spawn_lambdas` while
/// `count_spawn_sites` still sees a single site, and the resident tier planner
/// requires those two to be equal (jet-jit `tiers.rs` `spawn_ok`). Keying by
/// `(enclosing fn, lambda span)` keeps that identity true by construction.
///
/// This is the ONE place a spawn-lambda table entry is minted. `lower` builds
/// the entry and runs only when the key is new, so a nested callback inside
/// this body takes the lower index and is itself minted exactly once.
pub(crate) fn jit_spawn_site_with(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    lower: impl FnOnce(&Lambda, &Cx, &LowerEnv) -> TJitSpawnLambda,
) -> usize {
    let key = (env.fn_name.clone(), lam.span.start, lam.span.end);
    let existing = cx.jit_spawn_sites.borrow().get(&key).copied();
    if let Some(site) = existing {
        return site;
    }
    let jit_lambda = lower(lam, cx, env);
    let site = cx.jit_spawn_site_base + cx.jit_spawn_lambdas.borrow().len();
    cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
    cx.jit_spawn_sites.borrow_mut().insert(key, site);
    site
}

/// [`jit_spawn_site_with`] for a callback that needs no parameter seeding.
pub(crate) fn jit_spawn_site(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> usize {
    jit_spawn_site_with(lam, cx, env, lower_spawn_lambda_for_jit)
}
/// [`jit_spawn_site_with`] for a zero-parameter unit-returning callback.
pub(crate) fn jit_spawn_site_unit(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> usize {
    jit_spawn_site_with(lam, cx, env, lower_spawn_lambda_for_jit_unit)
}

fn interrupt_callback_value(value: TExpr) -> TExpr {
    let ty = value.ty.clone();
    TExpr {
        ty,
        kind: TExprKind::FnValue {
            kind: crate::Codegen::TIR::TFnValueKind::Send {
                value: Box::new(value),
            },
        },
    }
}

fn normalize_interrupt_named_value(value: TExpr) -> TExpr {
    value
}

fn lower_interrupt_callback(expr: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    match expr {
        Expr::Paren(inner, _) => lower_interrupt_callback(inner, cx, env),
        Expr::Lambda(lam) => {
            let mut tl = lower_lambda_expecting_value(lam, cx, env, &[]);
            tl.arc = true;
            tl.rc = false;
            let ty = Type::Fn {
                params: tl.param_types.clone(),
                ret: tl.ret.clone().map(Box::new),
                effect_bound: None,
                param_contract: None,
                call_metadata: None,
                return_view_provenance: lam.meta.return_view_provenance.clone(),
            };
            interrupt_callback_value(TExpr {
                ty,
                kind: TExprKind::Lambda(Box::new(tl)),
            })
        }
        Expr::Ident(name, _)
            if !env.locals.contains_key(name) && !cx.consts.contains_key(name) =>
        {
            let Some(ty) = cx.fn_types.get(name).cloned() else {
                return invariant_violation_expr(
                    expr.span(),
                    "checked interrupt callback is missing its function type",
                );
            };
            if !matches!(ty, Type::Fn { .. }) {
                return invariant_violation_expr(
                    expr.span(),
                    "checked interrupt callback is not a function value",
                );
            }
            interrupt_callback_value(TExpr {
                ty: ty.clone(),
                kind: TExprKind::FnValue {
                    kind: crate::Codegen::TIR::TFnValueKind::NamedFn {
                        name: Some(name.clone()),
                        lambda: None,
                    },
                },
            })
        }
        _ => interrupt_callback_value(normalize_interrupt_named_value(lower_expr(expr, cx, env))),
    }
}

/// c109 Phase 13: lower a closure-taking core call (`tasks.spawn`, `http.serve`,
/// or `scope.guard`) into a bespoke `CoreClosureCall` node. Returns `None`
/// when `(module, method)` has no closure-specific lowering, including a
/// sema-approved alternate plain-call form such as `serve(addr, router)`.
/// Checked closure shapes that reach this function must lower completely or
/// become a typed `InvariantViolation`.
pub(super) fn core_module_path_from_receiver(
    receiver: &Expr,
    cx: &Cx,
    env: &LowerEnv,
) -> Option<String> {
    match receiver {
        Expr::Ident(alias, _) if !env.locals.contains_key(alias) => cx
            .core_import_module_for_function(&env.fn_name, alias)
            .map(str::to_owned),
        Expr::Field(base, leaf, _) => {
            let module = core_module_path_from_receiver(base, cx, env)?;
            let submodule = format!("{module}.{leaf}");
            crate::Syntax::is_known_core_module(&submodule).then_some(submodule)
        }
        _ => None,
    }
}


/// D-QUERY-RETAIN1=A: `Query<T>` / grouped-query receiver methods and the
/// checked-SQL list door `[T].query(SQL)` are projections onto the receiver
/// Core rows. The receiver is the first Core argument; each callback is
/// lowered against the checked row type so every tier receives one typed
/// callable. Returns `None` when the call is not a query receiver call.
pub(crate) fn lower_query_receiver_call(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    resolved_ret: Option<&Type>,
    cx: &Cx,
    env: &mut LowerEnv,
    lowered_receiver: Option<&TExpr>,
) -> Option<TExpr> {
    let (receiver_type, callback_row_index): (&str, usize) = match recv_type.as_deref() {
        Some("Query")
            if matches!(
                method,
                "filter"
                    | "sort_by"
                    | "map"
                    | "min"
                    | "max"
                    | "inner_join"
                    | "left_join"
                    | "collect"
                    | "plan"
                    | "group_by"
                    | "watch"
            ) =>
        {
            ("Query", 0)
        }
        Some("DataTracked")
            if matches!(method, "query" | "insert" | "replace" | "remove") =>
        {
            ("DataTracked", 0)
        }
        Some("DataWatch") if matches!(method, "get" | "status" | "cancel") => {
            ("DataWatch", 0)
        }
        Some("DataGroupedQuery") if matches!(method, "count" | "sum" | "mean") => {
            ("DataGroupedQuery", 0)
        }
        None if method == "query"
            && args.len() == 1
            && resolved_ret.is_some_and(|ty| matches!(ty, Type::Result { .. })) =>
        {
            (crate::Syntax::INTERNAL_LIST_QUERY_HANDLE, 0)
        }
        _ => return None,
    };
    let Some(record) = crate::Syntax::core_receiver_method(receiver_type, method) else {
        return Some(invariant_violation_expr(
            method_span,
            format!("checked query method `{method}` has no canonical receiver Core row"),
        ));
    };
    let Some(result_ty) = resolved_ret.cloned() else {
        return Some(invariant_violation_expr(
            method_span,
            format!("checked query method `{method}` has no resolved return type"),
        ));
    };
    let receiver = lowered_receiver
        .cloned()
        .unwrap_or_else(|| lower_expr(receiver, cx, env));
    let row_ty = match receiver.ty.without_user_tags() {
        Type::Apply { name, args }
            if (name == "Query" && args.len() == 1)
                || (name == "DataGroupedQuery" && args.len() == 2)
                || (name == "DataTracked" && args.len() == 2)
                || (name == "DataWatch" && args.len() == 1) =>
        {
            Some(args[callback_row_index].clone())
        }
        Type::List(inner) | Type::FixedList { elem: inner, .. } => Some((**inner).clone()),
        Type::Apply { name, args } if name == "DataStream" && args.len() == 1 => {
            Some(args[0].clone())
        }
        _ => None,
    };
    let Some(row_ty) = row_ty else {
        return Some(invariant_violation_expr(
            method_span,
            format!("checked query method `{method}` did not retain its row type"),
        ));
    };
    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(receiver);
    for (index, arg) in args.iter().enumerate() {
        let takes_callback = if matches!(method, "inner_join" | "left_join") {
            index >= 1
        } else {
            matches!(
                method,
                "filter"
                    | "sort_by"
                    | "map"
                    | "min"
                    | "max"
                    | "group_by"
                    | "sum"
                    | "mean"
            )
        };
        if takes_callback {
            let lam = match required_lambda(args, index, "core.data", method, method_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let callback_row = if matches!(method, "inner_join" | "left_join") && index == 2 {
                call_args
                    .get(1)
                    .and_then(|other| match other.ty.without_user_tags() {
                        Type::Apply { name, args } if name == "Query" && args.len() == 1 => {
                            Some(args[0].clone())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| row_ty.clone())
            } else {
                row_ty.clone()
            };
            let lowered = if let Some(callback_return) =
                query_callback_return_type(method, index)
            {
                lower_lambda_expecting_value_with_return(
                    lam,
                    cx,
                    env,
                    &[callback_row],
                    &callback_return,
                )
            } else {
                lower_lambda_expecting_value(lam, cx, env, &[callback_row])
            };
            call_args.push(typed_lambda_value(lam, lowered));
        } else {
            call_args.push(lower_expr(&arg.expr, cx, env));
        }
    }
    if !record.accepts_arity(call_args.len()) {
        return Some(invariant_violation_expr(
            method_span,
            format!(
                "checked query Core row `{method}` has wrong arity {}",
                call_args.len()
            ),
        ));
    }
    let data_plan = match data_plan_for_core_call(record, &call_args, &result_ty, method_span) {
        Ok(plan) => plan,
        Err(error) => return Some(invariant_violation_expr(method_span, error)),
    };
    let widen_to_vec = if receiver_type == crate::Syntax::INTERNAL_LIST_QUERY_HANDLE {
        data_widening(&call_args, &[(0, &row_ty)])
    } else {
        no_widening(&call_args)
    };
    Some(TExpr {
        ty: result_ty.clone(),
        kind: TExprKind::CoreCall {
            record,
            args: call_args,
            source_span: method_span,
            type_args: Vec::new(),
            widen_to_vec,
            data_plan,
            fallibility: TFailureCarrier::from_checked_type(&result_ty),
        },
    })
}


pub(crate) fn lower_core_closure_call(

    module: &str,
    method: &str,
    source_span: Span,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    if module == "core.term" && method == "progress" {
        let Some(first) = args.first() else {
            return Some(invariant_violation_expr(source_span, "checked progress call has no source"));
        };
        let mut source = lower_owned_expr(&first.expr, cx, env);
        let (member, result_ty) = match &source.ty {
            Type::String => {
                let Some((_, result)) = crate::Sema::core_call_semantic_signature(module, method) else {
                    return Some(invariant_violation_expr(source_span, "checked text progress has no signature"));
                };
                ("progress", result)
            }
            Type::List(elem) | Type::FixedList { elem, .. } => {
                let result = crate::Collections::iter_ty((**elem).clone());
                source = TExpr {
                    ty: result.clone(),
                    kind: TExprKind::BuiltinMethod {
                        recv: Box::new(source),
                        op: crate::Codegen::TIR::TBuiltinOp::ListLazy,
                        args: Vec::new(),
                    },
                };
                ("progress_iter", result)
            }
            Type::Apply { name, args } if name == crate::Syntax::TYPE_ITER && args.len() == 1 => {
                ("progress_iter", source.ty.clone())
            }
            _ => return Some(invariant_violation_expr(source_span, "checked progress source is not text or a sequence")),
        };
        let mut values = vec![source];
        values.extend(args.iter().skip(1).map(|arg| lower_owned_expr(&arg.expr, cx, env)));
        if member == "progress_iter" {
            while values.len() < 3 {
                values.push(TExpr {
                    ty: Type::String,
                    kind: TExprKind::StrLit(Vec::new()),
                });
            }
        }
        let record = match checked_core_record(module, member, values.len(), source_span) {
            Ok(record) => record,
            Err(error) => return Some(error),
        };
        return Some(TExpr {
            ty: result_ty.clone(),
            kind: TExprKind::CoreCall {
                record,
                args: values,
                source_span,
                type_args: Vec::new(),
                widen_to_vec: Vec::new(),
                data_plan: None,
                fallibility: TFailureCarrier::from_checked_type(&result_ty),
            },
        });
    }
    if module == "core.data" && method == "query" {
        if args.len() != 1 {
            return Some(invariant_violation_expr(
                source_span,
                "checked Core call `core.data.query` has the wrong arity",
            ));
        }
        let record = match checked_core_record(module, method, args.len(), source_span) {
            Ok(record) => record,
            Err(error) => return Some(error),
        };
        let rows = lower_expr(&args[0].expr, cx, env);
        let Some(row_ty) = (match &rows.ty {
            Type::List(inner) | Type::FixedList { elem: inner, .. } => Some((**inner).clone()),
            Type::Apply { name, args } if name == "DataStream" && args.len() == 1 => {
                Some(args[0].clone())
            }
            _ => None,
        }) else {
            return Some(invariant_violation_expr(
                source_span,
                "checked Core call `core.data.query` did not retain its row type",
            ));
        };
        let widen_to_vec = data_widening(std::slice::from_ref(&rows), &[(0, &row_ty)]);
        let call_args = vec![rows];
        let result_ty = Type::Apply {
            name: "Query".to_string(),
            args: vec![row_ty.clone()],
        };
        let data_plan = match data_plan_for_core_call(
            record,
            &call_args,
            &result_ty,
            source_span,
        ) {
            Ok(plan) => plan,
            Err(error) => return Some(invariant_violation_expr(source_span, error)),
        };
        return Some(TExpr {
            ty: result_ty.clone(),
            kind: TExprKind::CoreCall {
                record,
                args: call_args,
                source_span,
                type_args: Vec::new(),
                widen_to_vec,
                data_plan,
                fallibility: TFailureCarrier::from_checked_type(&result_ty),
            },
        });
    }

    if module == "core.rt" && method == "callback" {
        if args.len() != 3 {
            return Some(invariant_violation_expr(
                source_span,
                "checked Core call `core.rt.callback` has the wrong arity",
            ));
        }
        let lam = match required_lambda(args, 2, module, method, source_span) {
            Ok(lam) => lam,
            Err(error) => return Some(error),
        };
        let callback_fn = Type::Fn {
            params: vec![Type::List(Box::new(Type::Float))],
            ret: Some(Box::new(unit_type())),
            effect_bound: Some(vec![
                ("Mem.Alloc".to_string(), source_span),
                ("Time.Wait".to_string(), source_span),
            ]),
            return_view_provenance: None,
            param_contract: None,
            call_metadata: None,
        };
        return Some(TExpr {
            ty: Type::Named("RealtimeStream".to_string()),
            kind: TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::Realtime {
                    rate: Box::new(lower_expr(&args[0].expr, cx, env)),
                    frames: Box::new(lower_expr(&args[1].expr, cx, env)),
                    executable: Box::new(lower_lambda_expecting_callable(
                        lam, cx, env, &callback_fn,
                    )),
                },
            },
        });
    }
    if module == "core.tasks" && method == "spawn" {
        if args.len() != 1 {
            return Some(invariant_violation_expr(
                source_span,
                "checked Core call `core.tasks.spawn` has the wrong arity",
            ));
        }
        let lam = match required_lambda(args, 0, module, method, source_span) {
            Ok(lam) => lam,
            Err(error) => return Some(error),
        };
        let carrier_ty = spawn_body_carrier_ty(lam, cx, env);
        // Sema keeps Task<T> as source metadata; the TIR handle retains the
        // closure carrier so its join adapter can propagate the inner E.
        let mut spawn_env = clone_env(env);
        spawn_env.ret_ty = Some(carrier_ty.clone());
        let site = jit_spawn_site(lam, cx, env);
        let label = spawn_label(lam, cx, env);
        let executable = Box::new(lower_lambda_expecting_value_with_return(
            lam, cx, &spawn_env, &[], &carrier_ty,
        ));
        return Some(TExpr {
            ty: core_closure_call_return_ty(module, method, carrier_ty),
            kind: TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::Spawn {
                    group: None,
                    site,
                    label,
                    executable,
                },
            },
        });
    }
    let data_err = || Type::Named("DataError".to_string());
    let wrap_data = |ok: Type| -> Type {
        Type::Result {
            ok: Box::new(ok),
            err: Box::new(data_err()),
        }
    };
    let data_row_ty = |ty: &Type| -> Option<Type> {
        match ty {
            Type::List(inner) | Type::FixedList { elem: inner, .. } => Some((**inner).clone()),
            Type::Apply { name, args } if name == "DataStream" && args.len() == 1 => {
                Some(args[0].clone())
            }
            _ => None,
        }
    };
    let kind = match (module, method) {
        ("core.sys", "on_interrupt") => {
            if args.len() != 1 {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.sys.on_interrupt` has the wrong arity",
                ));
            }
            TCoreClosureKind::OnInterrupt {
                callback: Box::new(lower_interrupt_callback(&args[0].expr, cx, env)),
            }
        }
        ("core.http", "serve") => {
            if args.len() != 2 {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.http.serve` has the wrong arity",
                ));
            }
            let Some(lam) = args.get(1).and_then(|arg| lambda_from_expr(&arg.expr)) else {
                // `serve(addr, router)` is a valid alternate analytical form;
                // ordinary CoreCall lowering owns it.
                return None;
            };
            let Some(address_arg) = args.first() else {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.http.serve` is missing its address",
                ));
            };
            let addr = lower_expr(&address_arg.expr, cx, env);
            let executable = Box::new(lower_lambda_expecting_value(
                lam,
                cx,
                env,
                &[Type::Named("HTTPRequest".to_string())],
            ));
            TCoreClosureKind::Serve {
                addr: Box::new(addr),
                executable,
            }
        }
        ("core.mem.scope", "guard") => {
            if args.len() != 1 {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.mem.scope.guard` has the wrong arity",
                ));
            }
            let lam = match required_lambda(args, 0, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let tl = lower_lambda_expecting_callable(lam, cx, env, &unit_callback_type());
            TCoreClosureKind::Guard {
                executable: Box::new(tl),
            }
        }
        ("core.data", "track") => {
            let record = match checked_core_record(module, method, args.len(), source_span) {
                Ok(record) => record,
                Err(error) => return Some(error),
            };
            if args.len() != 2 {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.data.track` has the wrong arity",
                ));
            }
            let rows = lower_expr(&args[0].expr, cx, env);
            let Some(row_ty) = data_row_ty(&rows.ty) else {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.data.track` did not retain its row type",
                ));
            };
            let lam = match required_lambda(args, 1, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let key = lower_lambda_expecting_value(lam, cx, env, &[row_ty.clone()]);
            let key_ty = key
                .ret
                .clone()
                .unwrap_or_else(|| lambda_body_ty_expecting(lam, cx, env, Some(&[row_ty.clone()])));
            let tracked = Type::Apply {
                name: "DataTracked".to_string(),
                args: vec![row_ty.clone(), key_ty],
            };
            let result_ty = wrap_data(tracked);
            let key = typed_lambda_value(lam, key);
            let call_args = vec![rows, key];
            let widen_to_vec = data_widening(&call_args, &[(0, &row_ty)]);
            let data_plan = match data_plan_for_core_call(
                record,
                &call_args,
                &result_ty,
                source_span,
            ) {
                Ok(plan) => plan,
                Err(error) => return Some(invariant_violation_expr(source_span, error)),
            };
            return Some(TExpr {
                ty: result_ty.clone(),
                kind: TExprKind::CoreCall {
                    record,
                    args: call_args,
                    source_span,
                    type_args: Vec::new(),
                    widen_to_vec,
                    data_plan,
                    fallibility: TFailureCarrier::from_checked_type(&result_ty),
                },
            });
        }
        ("core.data", "inner_join" | "left_join") => {
            let record = match checked_core_record(module, method, args.len(), source_span) {
                Ok(record) => record,
                Err(error) => return Some(error),
            };
            let Some(left_arg) = args.first() else {
                return Some(invariant_violation_expr(
                    source_span,
                    format!("checked Core call `{module}.{method}` is missing its left table"),
                ));
            };
            let Some(right_arg) = args.get(1) else {
                return Some(invariant_violation_expr(
                    source_span,
                    format!("checked Core call `{module}.{method}` is missing its right table"),
                ));
            };
            let left = lower_expr(&left_arg.expr, cx, env);
            let right = lower_expr(&right_arg.expr, cx, env);
            let left_ty = match &left.ty {
                Type::List(inner) | Type::FixedList { elem: inner, .. } => (**inner).clone(),
                _ => {
                    return Some(invariant_violation_expr(
                        source_span,
                        format!(
                            "checked Core call `{module}.{method}` did not retain its left row type"
                        ),
                    ))
                }
            };
            let right_ty = match &right.ty {
                Type::List(inner) | Type::FixedList { elem: inner, .. } => (**inner).clone(),
                _ => {
                    return Some(invariant_violation_expr(
                        source_span,
                        format!(
                            "checked Core call `{module}.{method}` did not retain its right row type"
                        ),
                    ))
                }
            };
            let left_lam = match required_lambda(args, 2, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let right_lam = match required_lambda(args, 3, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let left_key =
                lower_lambda_expecting_value(left_lam, cx, env, &[left_ty.clone()]);
            let right_key =
                lower_lambda_expecting_value(right_lam, cx, env, &[right_ty.clone()]);
            let call_args = vec![
                left,
                right,
                typed_lambda_value(left_lam, left_key),
                typed_lambda_value(right_lam, right_key),
            ];
            let widen_to_vec = data_widening(&call_args, &[(0, &left_ty), (1, &right_ty)]);
            let joined_right = if method == "left_join" {
                Type::Option(Box::new(right_ty))
            } else {
                right_ty
            };
            let result_ty = wrap_data(Type::List(Box::new(Type::Apply {
                name: "DataJoin".to_string(),
                args: vec![left_ty, joined_right],
            })));
            return Some(TExpr {
                ty: result_ty.clone(),
                kind: TExprKind::CoreCall {
                    record,
                    args: call_args,
                    source_span,
                    type_args: Vec::new(),
                    widen_to_vec,
                    data_plan: None,
                    fallibility: TFailureCarrier::from_checked_type(&result_ty),
                },
            });
        }
        ("core.data", "pivot_sum") => {
            let record = match checked_core_record(module, method, args.len(), source_span) {
                Ok(record) => record,
                Err(error) => return Some(error),
            };
            let Some(rows_arg) = args.first() else {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.data.pivot_sum` is missing its table",
                ));
            };
            let rows = lower_expr(&rows_arg.expr, cx, env);
            let Some(row_ty) = data_row_ty(&rows.ty) else {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.data.pivot_sum` did not retain its row type",
                ));
            };
            let row_lam = match required_lambda(args, 1, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let col_lam = match required_lambda(args, 2, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let value_lam = match required_lambda(args, 3, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let row_key = lower_lambda_expecting_value(
                row_lam,
                cx,
                env,
                &[row_ty.clone()],
            );
            let col_key = lower_lambda_expecting_value(
                col_lam,
                cx,
                env,
                &[row_ty.clone()],
            );
            let value = lower_lambda_expecting_value(value_lam, cx, env, &[row_ty.clone()]);
            let cell = Type::Named("DataPivotCell".to_string());
            let call_args = vec![
                rows,
                typed_lambda_value(row_lam, row_key),
                typed_lambda_value(col_lam, col_key),
                typed_lambda_value(value_lam, value),
            ];
            let widen_to_vec = data_widening(&call_args, &[(0, &row_ty)]);
            let result_ty = wrap_data(Type::List(Box::new(cell)));
            return Some(TExpr {
                ty: result_ty.clone(),
                kind: TExprKind::CoreCall {
                    record,
                    args: call_args,
                    source_span,
                    type_args: Vec::new(),
                    widen_to_vec,
                    data_plan: None,
                    fallibility: TFailureCarrier::from_checked_type(&result_ty),
                },
            });
        }
        // D-REACT1=B: the `derived` closure's body type is the `Derived<T>` element.
        ("core.reactive", "derived") => {
            if args.len() != 1 {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.reactive.derived` has the wrong arity",
                ));
            }
            let lam = match required_lambda(args, 0, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let body_ty = lambda_body_ty(lam, cx, env);
            let executable = Box::new(lower_lambda(lam, cx, env));
            // Captured signals need the spawn-lambda ABI (explicit capture params).
            let site = jit_spawn_site(lam, cx, env);
            return Some(TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_DERIVED.to_string(),
                    args: vec![body_ty],
                },
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::ReactiveDerived {
                        executable,
                        site,
                    },
                },
            });
        }
        ("core.reactive", "effect") => {
            if args.len() != 1 {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.reactive.effect` has the wrong arity",
                ));
            }
            let lam = match required_lambda(args, 0, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let executable = Box::new(lower_lambda_expecting_callable(
                lam,
                cx,
                env,
                &unit_callback_type(),
            ));
            let site = jit_spawn_site_unit(lam, cx, env);
            TCoreClosureKind::ReactiveEffect {
                executable,
                site,
            }
        }
        ("core.ui", "reactive_render") => {
            if args.len() != 1 {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.ui.reactive_render` has the wrong arity",
                ));
            }
            let lam = match required_lambda(args, 0, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let executable = Box::new(lower_lambda_expecting_callable(
                lam,
                cx,
                env,
                &unit_callback_type(),
            ));
            let site = jit_spawn_site_unit(lam, cx, env);
            TCoreClosureKind::UiReactiveRender { executable, site }
        }
        // D-SIGNAL1: `computed` is a canonical alias for `derived`.
        ("core.reactive", "computed") => {
            if args.len() != 1 {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.reactive.computed` has the wrong arity",
                ));
            }
            let lam = match required_lambda(args, 0, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let body_ty = lambda_body_ty(lam, cx, env);
            let executable = Box::new(lower_lambda(lam, cx, env));
            let site = jit_spawn_site(lam, cx, env);
            return Some(TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_COMPUTED.to_string(),
                    args: vec![body_ty],
                },
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::ReactiveDerived {
                        executable,
                        site,
                    },
                },
            });
        }
        // D-UI-PREVIEW1=A: preview/playground carries a checked zero-argument
        // UiNode callback plus an optional viewport value. The callback may be
        // normalized before or after the labelled optional argument, so locate
        // it by syntax and keep only ordinary values in MIR.
        ("core.ui", "preview" | "playground") if (2..=3).contains(&args.len()) => {
            let callback_index = if args
                .get(1)
                .and_then(|arg| lambda_from_expr(&arg.expr))
                .is_some()
            {
                1
            } else if args
                .get(2)
                .and_then(|arg| lambda_from_expr(&arg.expr))
                .is_some()
            {
                2
            } else {
                return Some(invariant_violation_expr(
                    source_span,
                    format!("checked Core call `core.ui.{method}` has no callback"),
                ));
            };
            let name_arg = args.first().expect("preview arity includes name");
            let viewport_index = (args.len() == 3).then_some(if callback_index == 1 { 2 } else { 1 });
            let viewport = if let Some(index) = viewport_index {
                Box::new(lower_optional_core_arg(
                    &args[index],
                    &Type::Named("UiPreviewViewport".to_string()),
                    cx,
                    env,
                ))
            } else {
                Box::new(TExpr {
                    ty: Type::Option(Box::new(Type::Named(
                        "UiPreviewViewport".to_string(),
                    ))),
                    kind: TExprKind::Absent,
                })
            };
            let lam = match required_lambda(args, callback_index, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let executable = Box::new(lower_lambda_expecting_callable(
                lam,
                cx,
                env,
                &ui_preview_callback_type(),
            ));
            let site = jit_spawn_site(lam, cx, env);
            return Some(TExpr {
                ty: Type::Named("UiPreview".to_string()),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::UiPreview {
                        name: Box::new(lower_owned_expr(&name_arg.expr, cx, env)),
                        viewport: Some(viewport),
                        executable,
                        site,
                        playground: method == "playground",
                        source_file: cx.file.clone(),
                        source_span,
                        source_start_line: crate::Diagnostics::span_line_col(
                            &cx.src,
                            source_span.start,
                        )
                        .0 as u32,
                        source_start_column: crate::Diagnostics::span_line_col(
                            &cx.src,
                            source_span.start,
                        )
                        .1 as u32,
                        source_end_line: crate::Diagnostics::span_line_col(
                            &cx.src,
                            source_span.end,
                        )
                        .0 as u32,
                        source_end_column: crate::Diagnostics::span_line_col(
                            &cx.src,
                            source_span.end,
                        )
                        .1 as u32,
                        build_id: cx.preview_build_id.clone(),
                        revision: cx.preview_revision.clone(),
                    },
                },
            });
        }
        ("core.ui", "preview" | "playground") => {
            return Some(invariant_violation_expr(
                source_span,
                "checked Core preview call has the wrong arity",
            ));
        }
        // D-UI-CLOSURE1=A: one fixed four-word ABI
        // `(display, shortcut, accessible_label, closure)`.
        ("core.ui", "button") if args.len() == 4 => {
            let lam = match required_lambda(args, 3, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let Some(display_arg) = args.first() else {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.ui.button` is missing its display text",
                ));
            };
            let display = Box::new(lower_owned_expr(&display_arg.expr, cx, env));
            let shortcut = Box::new(lower_optional_core_arg(
                &args[1],
                &Type::Named("UiShortcut".to_string()),
                cx,
                env,
            ));
            let accessible_label = Box::new(lower_optional_core_arg(
                &args[2],
                &Type::String,
                cx,
                env,
            ));
            let executable = Box::new(lower_lambda_expecting_callable(
                lam,
                cx,
                env,
                &unit_callback_type(),
            ));
            let site = jit_spawn_site_unit(lam, cx, env);
            return Some(TExpr {
                ty: Type::Named("UiNode".to_string()),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::UiButtonOnClick {
                        label: display,
                        shortcut,
                        accessible_label,
                        executable,
                        site,
                    },
                },
            });
        }
        ("core.ui", "button") if args.len() == 1 => return None,
        ("core.ui", "text_input") if args.len() == 3 => {
            let lam = match required_lambda(args, 2, module, method, source_span) {
                Ok(lam) => lam,
                Err(error) => return Some(error),
            };
            let Some(state_arg) = args.first() else {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.ui.text_input` is missing its state",
                ));
            };
            let Some(ime_arg) = args.get(1) else {
                return Some(invariant_violation_expr(
                    source_span,
                    "checked Core call `core.ui.text_input` is missing its IME mode",
                ));
            };
            let callback_type = ui_drop_callback_type();
            let state = Box::new(lower_owned_expr(&state_arg.expr, cx, env));
            let ime = Box::new(lower_owned_expr(&ime_arg.expr, cx, env));
            let executable = Box::new(lower_lambda_expecting_callable(
                lam,
                cx,
                env,
                &callback_type,
            ));
            let site = jit_spawn_site_with(lam, cx, env, |lam, cx, env| {
                lower_spawn_lambda_for_jit_expecting(
                    lam,
                    cx,
                    env,
                    &[Type::List(Box::new(Type::Named("UiDropItem".to_string())))],
                )
            });
            return Some(TExpr {
                ty: Type::Named("UiNode".to_string()),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::UiTextInputOnDrop {
                        state,
                        ime,
                        executable,
                        site,
                    },
                },
            });
        }
        ("core.ui", "text_input") if args.len() == 2 => return None,
        ("core.ui", "button" | "text_input") => {
            return Some(invariant_violation_expr(
                source_span,
                "checked Core UI call has the wrong arity",
            ));
        }
        _ => return None,
    };
    Some(TExpr {
        ty: core_closure_call_return_ty(module, method, unit_type()),
        kind: TExprKind::CoreClosureCall { kind },
    })
}
