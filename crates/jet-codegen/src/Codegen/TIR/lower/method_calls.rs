use crate::AST::{AccessConvention, Expr, Type};
use crate::Codegen::alloc_handle_rust_type;
use crate::Codegen::Cx;
use crate::Codegen::escape_rust_str;
use crate::Codegen::is_db_value_type_name;
use crate::Codegen::is_db_value_variant;
use crate::Codegen::is_json_type_name;
use crate::Codegen::is_json_variant;
use crate::Codegen::is_key_variant;
use crate::Codegen::mangle;
use crate::Codegen::TIR::alloc_new_type;
use crate::Codegen::TIR::builtin_result_ty;
use crate::Codegen::TIR::call_return_type;
use crate::Codegen::TIR::core_call_return_ty;
use crate::Codegen::TIR::duration_new_unit;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::fn_field_call_ty;
use crate::Codegen::TIR::game_static_type;
use crate::Codegen::TIR::handle_method_op;
use crate::Codegen::TIR::handle_method_return_ty;
use crate::Codegen::TIR::is_civil_time_method_name;
use crate::Codegen::TIR::is_concurrency_method_name;
use crate::Codegen::TIR::is_devserver_method_name;
use crate::Codegen::TIR::is_event_handle_type;
use crate::Codegen::TIR::is_event_method_name;
use crate::Codegen::TIR::is_http_method_name;
use crate::Codegen::TIR::is_http_type;
use crate::Codegen::TIR::is_loadable_method_name;
use crate::Codegen::TIR::is_measurement_method_name;
use crate::Codegen::TIR::is_reactive_method_name;
use crate::Codegen::TIR::is_sketch_method_name;
use crate::Codegen::TIR::is_sketch_type;
use crate::Codegen::TIR::is_ui_backend_method_name;
use crate::Codegen::TIR::is_watch_handle_type;
use crate::Codegen::TIR::is_watch_method_name;
use crate::Codegen::TIR::lambda_body_ty;
use crate::Codegen::TIR::lambda_body_ty_expecting;
use crate::Codegen::TIR::lower_core_closure_call;
use crate::Codegen::TIR::lower::core_module_path_from_receiver;
use crate::Codegen::TIR::lower_enum_arg;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_expr_as_mut_place;
use crate::Codegen::TIR::lower_owned_expr;
use crate::Codegen::TIR::lower_lambda;
use crate::Codegen::TIR::lower_lambda_expecting;
use crate::Codegen::TIR::lower_lambda_expecting_host_borrow;
use crate::Codegen::TIR::lower::lower_cursor_take_pattern;
use crate::Codegen::TIR::lower::lower_reader_take_pattern;
use crate::Codegen::TIR::lower_method_args;
use crate::Codegen::TIR::lower_module_args;
use crate::Codegen::TIR::lower_one_call_arg;
use crate::Codegen::TIR::lower_spawn_lambda_for_jit;
use crate::Codegen::TIR::lower::static_call_type_name_lower;
use crate::Codegen::TIR::pool_field_ty_hint;
use crate::Codegen::TIR::render_router_handler;
use crate::Codegen::TIR::render_safe_locals;
use crate::Codegen::TIR::render_spawn_lambda;
use crate::Codegen::TIR::resolve_builtin_op;
use crate::Codegen::TIR::resolve_closure_op;
use crate::Codegen::TIR::resolve_numeric_op;
use crate::Codegen::TIR::resolve_self_ty;
use crate::Codegen::TIR::solve_new_type;
use crate::Codegen::TIR::TBuiltinOp;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TCoreClosureKind;
use crate::Codegen::TIR::TEnumArg;
use crate::Codegen::TIR::TEnumPayload;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::THandleOp;
use crate::Codegen::TIR::tir_enum_lit_prefix;
use crate::Codegen::TIR::tir_recv_jet_ty;
use crate::Codegen::TIR::tir_src_line_at;
use crate::Codegen::TIR::TModuleCallForm;
use crate::Codegen::TIR::unit_type;
use crate::Diagnostics::Span;
use crate::Syntax;
use std::collections::HashSet;

fn builtin_arg_takes_ownership(op: &TBuiltinOp, index: usize) -> bool {
    match op {
        TBuiltinOp::Push
        | TBuiltinOp::Intersperse
        | TBuiltinOp::SetInsert
        | TBuiltinOp::SortedSetInsert
        | TBuiltinOp::BagAdd
        | TBuiltinOp::DequePushFront
        | TBuiltinOp::DequePushBack => index == 0,
        TBuiltinOp::InsertMap | TBuiltinOp::AddNewMap | TBuiltinOp::InsertList => index == 1,
        TBuiltinOp::LruPut | TBuiltinOp::LruAddNew => index < 2,
        _ => false,
    }
}

/// c109 Phase 6: lower a method call. The gate proved it is the synthetic `.clone()`
/// or a user instance method on a covered type; resolve every dispatch fact here.
pub(crate) fn lower_method_call(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    resolved_ret: Option<&Type>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    if let Expr::Ident(name, _) = receiver {
        if env.is_gc(name) {
            let root = env.place_of(name);
            let edge_args = match method {
                "insert" if args.len() > 1 => &args[1..],
                "remove" => &args[0..0],
                _ => args,
            };
            let mut edges = edge_args
                .iter()
                .flat_map(|arg| env.gc_edges_for_expr(&arg.expr, Some(name)))
                .collect::<Vec<_>>();
            edges.sort();
            edges.dedup();
            let mut lowered_args = args.to_vec();
            let index_temp = if matches!(method, "insert" | "remove") {
                args.first().and_then(|arg| {
                    let lowered = lower_expr(&arg.expr, cx, env);
                    if !lowered.ty.is_integer() {
                        return None;
                    }
                    let source_name = format!("__jet_gc_index_{}", method_span.start);
                    let rust_name = source_name.clone();
                    lowered_args[0].expr = Expr::Ident(source_name.clone(), arg.span);
                    env.bind(&source_name, rust_name.clone(), Some(lowered.ty.clone()));
                    Some((rust_name, lowered))
                })
            } else {
                None
            };
            let saved = env.locals.get(name).cloned();
            env.gc_locals.remove(name);
            env.bind(name, "(*__jet_value)".to_string(), saved.as_ref().and_then(|(_, ty)| ty.clone()));
            let inner = lower_method_call(
                receiver,
                method,
                method_span,
                &lowered_args,
                recv_type,
                resolved_ret,
                cx,
                env,
            );
            if let Some((place, ty)) = saved {
                env.bind(name, place, ty);
            }
            env.mark_gc(name);
            if let Some((temp, _)) = &index_temp {
                env.locals.remove(temp);
            }
            let ty = inner.ty.clone();
            let edit = emit_tir_expr(&inner, cx);
            let emitted = if method == "clear" {
                format!(
                    "jet_gc::runtime_or_exit({}.edit_clearing_edges(|__jet_value| {}))",
                    root, edit
                )
            } else if method == "pop" {
                format!(
                    "jet_gc::runtime_or_exit({}.edit_edge_slot_pop(\"collection\", |__jet_value| {}))",
                    root, edit
                )
            } else if method == "remove" && index_temp.is_some() {
                format!(
                    "jet_gc::runtime_or_exit({}.edit_edge_slot_remove(\"collection\", {} as usize, |__jet_value| {}))",
                    root,
                    index_temp.as_ref().map(|(temp, _)| temp.as_str()).unwrap_or("0"),
                    edit
                )
            } else if method == "insert" && index_temp.is_some() {
                format!(
                    "jet_gc::runtime_or_exit({}.edit_edge_slot_insert(\"collection\", {} as usize, &[{}], |__jet_value| {}))",
                    root,
                    index_temp.as_ref().map(|(temp, _)| temp.as_str()).unwrap_or("0"),
                    edges.join(", "),
                    edit
                )
            } else if method == "prepend" {
                format!(
                    "jet_gc::runtime_or_exit({}.edit_edge_slot_prepend(\"collection\", &[{}], |__jet_value| {}))",
                    root,
                    edges.join(", "),
                    edit
                )
            } else if matches!(method, "push" | "append") {
                format!(
                    "jet_gc::runtime_or_exit({}.edit_edge_slot_additive(\"collection\", &[{}], |__jet_value| {}))",
                    root,
                    edges.join(", "),
                    edit
                )
            } else if edges.is_empty() {
                format!(
                    "jet_gc::runtime_or_exit({}.edit(|__jet_value| {}))",
                    root, edit
                )
            } else {
                format!(
                    "jet_gc::runtime_or_exit({}.edit_edge_slot(\"method:{}\", &[{}], |__jet_value| {}))",
                    root,
                    method_span.start,
                    edges.join(", "),
                    edit
                )
            };
            let emitted = if let Some((temp, value)) = index_temp {
                format!(
                    "{{ let {} = {}; {} }}",
                    temp,
                    emit_tir_expr(&value, cx),
                    emitted
                )
            } else {
                emitted
            };
            return TExpr {
                ty,
                kind: TExprKind::ConstInline(emitted),
            };
        }
    }
    if recv_type.as_ref().is_some_and(|name| {
        cx.current_type_params.borrow().contains(name.as_str())
    }) && matches!(method, "read" | "write" | "write_all") {
        let arg_ty = if method == "read" {
            Type::Int
        } else {
            Type::List(Box::new(Type::IntN { signed: false, bits: 8 }))
        };
        let recv = lower_expr(receiver, cx, env);
        let targs = args.iter().map(|arg| {
            lower_one_call_arg(arg, Some((arg.convention, arg_ty.clone())), env, cx)
        }).collect();
        return TExpr {
            ty: resolved_ret.cloned().unwrap_or_else(unit_type),
            kind: TExprKind::MethodCall {
                recv: Box::new(recv),
                method_rust: method.to_string(),
                args: targs,
            },
        };
    }
    // D-NURSERY1/A: from `[Task<T>]` list type extract `T` (the joined result type).
    fn taskgroup_result_elem(tasks: &TExpr) -> Type {
        match &tasks.ty {
            Type::List(inner) => match inner.as_ref() {
                Type::Apply { name, args, .. } if name == "Task" && args.len() == 1 => {
                    args[0].clone()
                }
                other => (*other).clone(),
            },
            _ => unit_type(),
        }
    }
    let crypto_static = static_call_type_name_lower(receiver, env).and_then(|ty| {
        let helper = match (ty.as_str(), method) {
            ("Secret", "from_text") => "__secret_from_text",
            ("Secret", "from_bytes") => "__secret_from_bytes",
            ("SigningKey", "generate") => "__signing_generate",
            ("X25519SecretKey", "generate") => "__x25519_generate",
            ("VerifyKey", "from_bytes") => "__verify_key_from_bytes",
            ("X25519PublicKey", "from_bytes") => "__x25519_public_from_bytes",
            ("X25519PublicKey", "from_text") => "__x25519_public_from_text",
            ("Signature", "from_bytes") => "__signature_from_bytes",
            ("Sealed", "from_bytes") => "__sealed_from_bytes",
            ("WrappedKey", "from_bytes") => "__wrapped_from_bytes",
            ("PasswordHash", "parse") => "__password_parse",
            _ => return None,
        };
        Some(helper)
    });
    if let Some(helper) = crypto_static {
        return TExpr { ty: resolved_ret.cloned().unwrap_or_else(unit_type), kind: TExprKind::CoreCall {
            module: "jet.crypto".to_string(), method: helper.to_string(),
            args: args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect(),
        }};
    }
    if let Some(kind) = recv_type.as_deref() {
        let helper = match (kind, method) {
            ("SigningKey", "public_key") => Some("__signing_public"),
            ("X25519SecretKey", "public_key") => Some("__x25519_public"),
            ("VerifyKey", "bytes") => Some("__verify_key_bytes"),
            ("X25519PublicKey", "bytes") => Some("__x25519_public_bytes"),
            ("X25519PublicKey", "text") => Some("__x25519_public_text"),
            ("Signature", "bytes") => Some("__signature_bytes"),
            ("Sealed", "bytes") => Some("__sealed_bytes"),
            ("WrappedKey", "bytes") => Some("__wrapped_bytes"),
            ("Digest256", "bytes") => Some("__digest256_bytes"),
            ("Digest512", "bytes") => Some("__digest512_bytes"),
            ("Digest256", "hex") => Some("__digest256_hex"),
            ("Digest512", "hex") => Some("__digest512_hex"),
            ("PasswordHash", "text") => Some("__password_text"),
            _ => None,
        };
        if let Some(helper) = helper {
            let recv = lower_expr(receiver, cx, env);
            return TExpr { ty: resolved_ret.cloned().unwrap_or_else(unit_type), kind: TExprKind::CoreCall {
                module: "jet.crypto".to_string(), method: helper.to_string(), args: vec![recv],
            }};
        }
    }
    // D-TOOL4: `expect(x).snapshot()` — render the harness snapshot call. Test
    // bodies are `Result<(), String>`, so the trailing `?` propagates mismatch.
    if method == Syntax::BUILTIN_SNAPSHOT {
        if let Expr::Call(call) = receiver {
            if call.name == Syntax::BUILTIN_EXPECT && call.args.len() == 1 {
                let val = lower_expr(&call.args[0].expr, cx, env);
                let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0;
                let snap_path = format!(
                    "snapshots/{}_{}.snap",
                    cx.file.replace(['/', '\\', '.'], "_"),
                    line
                );
                let rendered = format!(
                    "jet_expect(format!(\"{{}}\", ({}).jet_show())).snapshot({snap_path:?})?",
                    emit_tir_expr(&val, cx)
                );
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::RequireStop {
                        rendered,
                        always_stops: false,
                    },
                };
            }
        }
    }
    // D-ERRCTX1=D: `<fallible>.context("…")` — lazily-evaluated (only formatted
    // if the error actually propagates): wrap the message in a zero-arg closure
    // and let `jet_context` call it only on the `Err` branch.
    if method == "context" && recv_type.as_deref() == Some("__Fallible__") {
        let recv = lower_expr(receiver, cx, env);
        let msg = lower_expr(&args[0].expr, cx, env);
        let code = format!(
            "{}jet_context({}, || {})",
            cx.root_prefix,
            emit_tir_expr(&recv, cx),
            emit_tir_expr(&msg, cx)
        );
        return TExpr {
            ty: recv.ty.clone(),
            kind: TExprKind::ConstInline(code),
        };
    }
    // D-TYPEDTEXT1=D: `Sql.raw("…")` / `Html.raw("…")` — the audited escape.
    // `Sql`/`Html` name the type here (sema already confirmed no local shadows
    // it), so `recv_type` was never set for this call; check the receiver
    // shape directly instead.
    if method == "raw" {
        if let Expr::Ident(n, _) = receiver {
            if n == "Sql" || n == "Html" || n == Syntax::TYPE_SH {
                let arg = lower_expr(&args[0].expr, cx, env);
                let code = if n == "Sql" {
                    format!("(({}).clone(), Vec::new())", emit_tir_expr(&arg, cx))
                } else if n == Syntax::TYPE_SH {
                    format!(
                        "({}).split_whitespace().map(|word| word.to_string()).collect::<Vec<String>>()",
                        emit_tir_expr(&arg, cx)
                    )
                } else {
                    format!("({}).clone()", emit_tir_expr(&arg, cx))
                };
                return TExpr {
                    ty: Type::Named(n.clone()),
                    kind: TExprKind::ConstInline(code),
                };
            }
        }
    }
    // D-TYPEDTEXT1=D: `.template()`/`.params()` split a checked `Sql` value;
    // `.text()` reads the escaped `Html` string.
    if recv_type.as_deref() == Some("Sql") && matches!(method, "template" | "params") {
        let recv = lower_expr(receiver, cx, env);
        let code = if method == "template" {
            format!("({}).0.clone()", emit_tir_expr(&recv, cx))
        } else {
            format!("({}).1.clone()", emit_tir_expr(&recv, cx))
        };
        return TExpr {
            ty: if method == "template" {
                Type::String
            } else {
                Type::List(Box::new(Type::String))
            },
            kind: TExprKind::ConstInline(code),
        };
    }
    if recv_type.as_deref() == Some("Html") && method == "text" {
        let recv = lower_expr(receiver, cx, env);
        return TExpr {
            ty: Type::String,
            kind: TExprKind::ConstInline(format!("({}).clone()", emit_tir_expr(&recv, cx))),
        };
    }
    // D-TASKSCOPE1=A / D-NURSERY1=A: structured taskgroup methods.
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_SPAWN_METHOD
    {
        if let Some(Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
            let body_ty = lambda_body_ty(lam, cx, env);
            let jit_lambda = lower_spawn_lambda_for_jit(lam, cx, env);
            cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
            let spawn_closure = render_spawn_lambda(lam, cx, env);
            return TExpr {
                ty: Type::Apply {
                    name: "Task".to_string(),
                    args: vec![body_ty],
                },
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::Spawn { spawn_closure },
                },
            };
        }
    }
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_ALL_METHOD
        && args.len() == 1
    {
        let tasks = lower_expr(&args[0].expr, cx, env);
        let elem = taskgroup_result_elem(&tasks);
        return TExpr {
            ty: Type::List(Box::new(elem)),
            kind: TExprKind::TaskGroupAll {
                tasks: Box::new(tasks),
            },
        };
    }
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_RACE_METHOD
        && args.len() == 1
    {
        let tasks = lower_expr(&args[0].expr, cx, env);
        let elem = taskgroup_result_elem(&tasks);
        return TExpr {
            ty: elem,
            kind: TExprKind::TaskGroupRace {
                tasks: Box::new(tasks),
            },
        };
    }
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_ANY_METHOD
        && args.len() == 1
    {
        let tasks = lower_expr(&args[0].expr, cx, env);
        let elem = taskgroup_result_elem(&tasks);
        return TExpr {
            ty: elem,
            kind: TExprKind::TaskGroupAny {
                tasks: Box::new(tasks),
            },
        };
    }
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_SELECT_METHOD
        && args.is_empty()
    {
        return TExpr {
            ty: Type::Apply {
                name: Syntax::TYPE_SELECT_BUILDER.to_string(),
                args: vec![],
            },
            kind: TExprKind::SelectStart,
        };
    }
    if recv_type
        .as_deref()
        .is_some_and(|rt| rt == Syntax::TYPE_SELECT_BUILDER || rt.starts_with("SelectBuilder<"))
    {
        let builder = lower_expr(receiver, cx, env);
        match method {
            Syntax::SELECT_RECV_METHOD if args.len() == 1 => {
                let channel = lower_expr(&args[0].expr, cx, env);
                let elem = match &builder.ty {
                    Type::Apply { name, args, .. }
                        if name == Syntax::TYPE_SELECT_BUILDER && args.len() == 1 =>
                    {
                        args[0].clone()
                    }
                    _ => unit_type(),
                };
                return TExpr {
                    ty: Type::Apply {
                        name: Syntax::TYPE_SELECT_BUILDER.to_string(),
                        args: vec![elem],
                    },
                    kind: TExprKind::SelectRecv {
                        builder: Box::new(builder),
                        channel: Box::new(channel),
                    },
                };
            }
            Syntax::SELECT_AFTER_METHOD if args.len() == 1 || args.len() == 2 => {
                let millis = lower_expr(&args[0].expr, cx, env);
                let value = args
                    .get(1)
                    .map(|arg| Box::new(lower_expr(&arg.expr, cx, env)));
                let ty = if let Some(value) = value.as_ref() {
                    match &builder.ty {
                        Type::Apply { name, args, .. }
                            if name == Syntax::TYPE_SELECT_BUILDER && args.len() == 1 =>
                        {
                            builder.ty.clone()
                        }
                        _ => Type::Apply {
                            name: Syntax::TYPE_SELECT_BUILDER.to_string(),
                            args: vec![value.ty.clone()],
                        },
                    }
                } else {
                    builder.ty.clone()
                };
                return TExpr {
                    ty,
                    kind: TExprKind::SelectAfter {
                        builder: Box::new(builder),
                        millis: Box::new(millis),
                        value,
                    },
                };
            }
            Syntax::SELECT_READ_METHOD if args.len() == 1 => {
                let stream = lower_expr(&args[0].expr, cx, env);
                return TExpr {
                    ty: builder.ty.clone(),
                    kind: TExprKind::SelectRead {
                        builder: Box::new(builder),
                        stream: Box::new(stream),
                    },
                };
            }
            Syntax::SELECT_WAIT_METHOD if args.is_empty() => {
                let ret = match &builder.ty {
                    Type::Apply { name, args, .. }
                        if name == Syntax::TYPE_SELECT_BUILDER && args.len() == 1 =>
                    {
                        args[0].clone()
                    }
                    _ => unit_type(),
                };
                return TExpr {
                    ty: ret,
                    kind: TExprKind::SelectWait {
                        builder: Box::new(builder),
                    },
                };
            }
            _ => {}
        }
    }
    // D-TXN3/D-TXN4: `<handle>.on_commit(() => { … })` on a `@Transact` handle.
    // The gate proved `recv_type == Some("Transaction")` and a single literal
    // zero-param lambda arg. Lower to `<handle>.on_commit(Box::new(move || { … }))`;
    // the Drop-backed LIFO-on-commit semantics live in the `JetTransaction` prelude
    // type. The receiver is the bound handle ident → its mangled Rust place.
    if method == Syntax::TXN_ON_COMMIT && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE) {
        // The handle is always a bound ident (sema typed it `Transaction` from a
        // `@Transact(name)` binding); its mangled place is `user_<name>`.
        let handle = match receiver {
            Expr::Ident(name, _) => mangle(name),
            // Defensive: a non-ident receiver can't be a transaction handle, but
            // lowering it keeps the place well-formed if one ever appears.
            other => emit_tir_expr(&lower_expr(other, cx, env), cx),
        };
        if let Some(crate::AST::Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
            // Build the closure directly (not via `render_lambda_str`, which may add
            // its own `Box::new(…)` wrapper). The hook is stored in the transaction's
            // `Vec<Box<dyn FnOnce()>>`, so it must be a `move` closure boxed exactly
            // once by the `TCoreClosureKind::OnCommit` emit (no double-box).
            let tl = lower_lambda(lam, cx, env);
            let inner = format!("move |{}| {}", tl.params.join(", "), tl.body);
            let closure = if tl.prep.is_empty() {
                inner
            } else {
                format!("{{ {} {} }}", tl.prep, inner)
            };
            return TExpr {
                ty: Type::Named("TransactionGuard".to_string()),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::OnCommit { handle, closure },
                },
            };
        }
    }
    // D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(() => { … })` on a `@Transact`
    // handle — the exact mirror of `on_commit`. Lower to
    // `<handle>.on_rollback(Box::new(move || { … }))`; the Drop-backed run-on-rollback
    // semantics live in the `JetTransaction` prelude type.
    if method == Syntax::TXN_ON_ROLLBACK && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE) {
        let handle = match receiver {
            Expr::Ident(name, _) => mangle(name),
            other => emit_tir_expr(&lower_expr(other, cx, env), cx),
        };
        if let Some(crate::AST::Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
            let tl = lower_lambda(lam, cx, env);
            let inner = format!("move |{}| {}", tl.params.join(", "), tl.body);
            let closure = if tl.prep.is_empty() {
                inner
            } else {
                format!("{{ {} {} }}", tl.prep, inner)
            };
            return TExpr {
                ty: Type::Named("TransactionGuard".to_string()),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::OnRollback { handle, closure },
                },
            };
        }
    }
    // D-CAP2 (D-MEM1/S4): a user-written `.clone()` MethodCall no longer reaches
    // here — sema never constructs one (unrecognized method, E0102/E0311), and
    // the compiler's own duplication rewrites build `Expr::Copy` instead (its
    // own `lower_expr` arm), which lowers straight to `TExprKind::Clone`.
    // c109 Phase 23: `.raw()` on a distinct type → `({recv}).0`. The receiver's resolved
    // type names the distinct; its base type (from `cx.distinct_types`) is the total
    // result type. Mirrors `emit_method_call`'s `METHOD_DISTINCT_RAW` early return.
    if method == Syntax::METHOD_DISTINCT_RAW {
        let recv = lower_expr(receiver, cx, env);
        let base = match &recv.ty {
            Type::Named(n) => cx
                .distinct_types
                .get(n)
                .map(|(b, _)| b.clone())
                .unwrap_or_else(unit_type),
            _ => unit_type(),
        };
        return TExpr {
            ty: base,
            kind: TExprKind::DistinctRaw(Box::new(recv)),
        };
    }
    // c109 Phase 27: a CALL THROUGH a fn-typed struct field — `w.step(4)`. The gate
    // proved `recv_type == Some(<CoveredStruct>)` and the named field is `Type::Fn`. The
    // AST `emit_method_call` (Expression.rs ~L1573) emits `(({recv}).{user_<field>})({args})`
    // with PLAIN args. Resolve the field's Rust name + the call's result type (the Fn's
    // return) here; emit just splices. (Tried before the JSON/core/user shapes, mirroring
    // the AST dispatch order — a fn-field check fires before user-method dispatch.)
    if let Some(Type::Fn { params, ret, .. }) = fn_field_call_ty(method, recv_type, cx) {
        let ret_ty = ret.as_deref().cloned().unwrap_or_else(unit_type);
        let recv = lower_expr(receiver, cx, env);
        let targs: Vec<TCallArg> = args
            .iter()
            .zip(params.iter())
            .map(|(a, ty)| {
                lower_one_call_arg(a, Some((AccessConvention::Read, ty.clone())), env, cx)
            })
            .collect();
        return TExpr {
            ty: ret_ty,
            kind: TExprKind::FnFieldCall {
                recv: Box::new(recv),
                field_rust: mangle(method),
                args: targs,
            },
        };
    }
    // D-ENC-DYN1=A+: a dynamic `Data` construction `Data.<Variant>(arg)` (the gate
    // proved the receiver is a `Data`/`Json`/… type-name ident and `method` is a `Data`
    // variant). Lower to `TExprKind::JsonLit`, carrying the payload's `implicit_clone`
    // flag as a total fact. The result type is `Data`.
    if let Expr::Ident(type_name, _) = receiver {
        if !env.locals.contains_key(type_name)
            && is_json_type_name(type_name)
            && is_json_variant(method)
        {
            let arg = args
                .first()
                .map(|a| Box::new((lower_expr(&a.expr, cx, env), a.flags.implicit_clone)));
            return TExpr {
                ty: Type::Named(Syntax::TYPE_DATA.to_string()),
                kind: TExprKind::JsonLit {
                    variant: method.to_string(),
                    arg,
                },
            };
        }
    }
    // D-DBDRIVER1: a `DbValue` construction `DbValue.Int(n)` / `.Float(f)` /
    // `.Text(s)` / `.Bool(b)` (the gate proved the receiver is `DbValue` and
    // `method` a `DbValue` variant). Same shape as the `Data` construction above.
    if let Expr::Ident(type_name, _) = receiver {
        if !env.locals.contains_key(type_name)
            && is_db_value_type_name(type_name)
            && is_db_value_variant(method)
        {
            let arg = args
                .first()
                .map(|a| Box::new((lower_expr(&a.expr, cx, env), a.flags.implicit_clone)));
            return TExpr {
                ty: Type::Named(Syntax::TYPE_DB_VALUE.to_string()),
                kind: TExprKind::DbValueLit {
                    variant: method.to_string(),
                    arg,
                },
            };
        }
    }
    // D-SHAPE-DURATION1=A: a bare `Duration.unit(value)` is a type-owned
    // checked constructor, not an instance/static user method.
    {
        let locals: HashSet<String> = env.locals.keys().cloned().collect();
        if let Some(unit) = duration_new_unit(receiver, method, &locals) {
            let value = lower_expr(&args[0].expr, cx, env);
            let float = matches!(value.ty, Type::Float | Type::Float32);
            return TExpr {
                ty: Type::Result {
                    ok: Box::new(Type::Named(Syntax::DURATION_TYPE.to_string())),
                    err: Box::new(Type::Named(
                        Syntax::DURATION_RANGE_ERROR_TYPE.to_string(),
                    )),
                },
                kind: TExprKind::HandleMethod {
                    recv: Box::new(value),
                    op: THandleOp::DurationNew { unit, float },
                    args: vec![],
                },
            };
        }
    }
    // c109 Phase 19: the arena allocator constructor `mem.Arena.new(…)` (D-ALLOC1). The
    // gate proved the receiver is `Field(Ident(mem-alias), <AllocType>)` + method `new`.
    // Render the whole ctor call HERE. Ordinary families call their runtime
    // constructors; Fixed.new carries an internal byte-count marker to the let
    // emitter so it can synthesize frame storage, and Fixed.over borrows its array.
    // result type is the allocator handle `Named(<AllocType>)` (`alloc_method_return`'s
    // `new` arm). The allocator's only `unsafe` lives in the vetted `jet_mem` prelude (I1).
    {
        let locals: HashSet<String> = env.locals.keys().cloned().collect();
        {
            if let Some(alloc_type) = alloc_new_type(receiver, method, cx, &locals) {
                let rust_type = alloc_handle_rust_type(alloc_type).unwrap_or("jet_mem::JetArena");
                let ctor = if alloc_type == "Fixed" && method == "new" {
                    let Expr::Int(size, _, _, _) = &args[0].expr else {
                        unreachable!("sema rewrites Fixed.new's comptime size to a literal")
                    };
                    format!("__JET_FIXED_INLINE:{size}")
                } else if alloc_type == "Fixed" && method == "over" {
                    let backing = emit_tir_expr(&lower_expr(&args[0].expr, cx, env), cx);
                    format!("{rust_type}::over(&mut {backing})")
                } else if args.is_empty() {
                    format!("{}::new()", rust_type)
                } else {
                    let ctor_fn = match alloc_type {
                        "Pool" => "with_slots",
                        _ => "with_capacity",
                    };
                    let a0 = emit_tir_expr(&lower_expr(&args[0].expr, cx, env), cx);
                    format!("{}::{}({} as usize)", rust_type, ctor_fn, a0)
                };
                return TExpr {
                    ty: Type::Named(alloc_type.to_string()),
                    kind: TExprKind::AllocNew { ctor },
                };
            }
        }
    }
    // D-SOLVER-LIB1=A: `solve.Solver.new(seed)` constructor. The receiver is a
    // core module sentinel (`solve.Solver`), so the seed arg becomes the lowered recv.
    {
        let locals: HashSet<String> = env.locals.keys().cloned().collect();
        if solve_new_type(receiver, method, cx, &locals).is_some() {
            let seed = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named(Syntax::SOLVER_TYPE.to_string()),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(seed),
                    op: THandleOp::SolverNew,
                    args: vec![],
                },
            };
        }
        if let Some(static_type) = game_static_type(receiver, method, cx, &locals) {
            let op = match (static_type, method) {
                ("Scene", "new") => THandleOp::GameSceneNew,
                ("Replay", "record") => THandleOp::GameReplayRecord,
                ("Backend", "headless") => THandleOp::GameBackendHeadless,
                _ => unreachable!("game_static_type admitted only stable game constructors"),
            };
            let ty = match static_type {
                "Scene" => Type::Named("GameScene".to_string()),
                "Replay" => Type::Named("GameReplay".to_string()),
                "Backend" => Type::Named("GameBackend".to_string()),
                _ => unit_type(),
            };
            let recv = if args.is_empty() {
                TExpr {
                    ty: unit_type(),
                    kind: TExprKind::ConstInline("()".to_string()),
                }
            } else {
                lower_expr(&args[0].expr, cx, env)
            };
            let rest = args
                .iter()
                .skip(1)
                .map(|a| lower_expr(&a.expr, cx, env))
                .collect();
            return TExpr {
                ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv),
                    op,
                    args: rest,
                },
            };
        }
    }
    // c109 Phase 16: an enum-variant CONSTRUCTION `Enum.Variant(args)` reaching codegen
    // as a `MethodCall` (sema never rewrites a payload variant to `Expr::EnumLit`). The
    // AST `emit_method_call` routes it to `emit_enum_lit` with all-positional args; we
    // reproduce that, resolving each arg's `clone`/`boxed` decisions via `lower_enum_arg`
    // (`emit_boxed_enum_arg` byte-for-byte). This is the construction half of the
    // string/struct/collection-payload + recursive (boxed) enum coverage.
    if recv_type.is_none() {
        if let Expr::Ident(type_name, _) = receiver {
            if !env.locals.contains_key(type_name) {
                // D-TERM1 (ratified 2026-06-22): `Key` is a prelude enum not in
                // `cx.enum_variants`; lower `Key.Variant(args)` to a TIR enum lit.
                let key_type = crate::Syntax::TYPE_KEY;
                if type_name == key_type && is_key_variant(method) {
                    let prefix = tir_enum_lit_prefix(cx, type_name, method);
                    let payload = if args.is_empty() {
                        TEnumPayload::Unit
                    } else {
                        let pos = args
                            .iter()
                            .map(|a| {
                                // Key payload args are always scalar/Char — no clone/box needed.
                                TEnumArg {
                                    value: lower_expr(&a.expr, cx, env),
                                    clone: false,
                                    boxed: false,
                                }
                            })
                            .collect();
                        TEnumPayload::Positional(pos)
                    };
                    return TExpr {
                        ty: Type::Named(type_name.clone()),
                        kind: TExprKind::EnumLit { prefix, payload },
                    };
                }
                if type_name == "DataEvent"
                    && matches!(method, "Bool" | "Int" | "Float" | "Text" | "Bytes" | "Key")
                {
                    let payload = TEnumPayload::Positional(
                        args.iter()
                            .map(|a| TEnumArg {
                                value: lower_expr(&a.expr, cx, env),
                                clone: false,
                                boxed: false,
                            })
                            .collect(),
                    );
                    return TExpr {
                        ty: Type::Named("DataEvent".to_string()),
                        kind: TExprKind::EnumLit {
                            prefix: format!("{}jet_std::DataEvent::{}", cx.root_prefix, method),
                            payload,
                        },
                    };
                }
                if let Some(variants) = cx.enum_variants.get(type_name) {
                    if variants.iter().any(|(v, _)| v == method) {
                        let prefix = tir_enum_lit_prefix(cx, type_name, method);
                        let payload = if args.is_empty() {
                            TEnumPayload::Unit
                        } else {
                            let pos = args
                                .iter()
                                .map(|a| {
                                    lower_enum_arg(type_name, method, method, &a.expr, cx, env)
                                })
                                .collect();
                            TEnumPayload::Positional(pos)
                        };
                        return TExpr {
                            ty: Type::Named(type_name.clone()),
                            kind: TExprKind::EnumLit { prefix, payload },
                        };
                    }
                }
            }
        }
    }
    // D-ENV-MUTATE1=A: current editions retain `env.set -> Void`, but invalid
    // runtime strings must produce existing E3001 at the Jet call span. Lower
    // this compatibility wrapper with all panic facts resolved before emit.
    if method == "set" && args.len() == 2 {
        if let Expr::Ident(alias, _) = receiver {
            if !env.locals.contains_key(alias)
                && cx.core_imports.get(alias).is_some_and(|module| module == "core.env")
            {
                let name = emit_tir_expr(&lower_expr(&args[0].expr, cx, env), cx);
                let value = emit_tir_expr(&lower_expr(&args[1].expr, cx, env), cx);
                let (src_line, line, col) = tir_src_line_at(&cx.src, method_span.start);
                let caret = (method_span.end - method_span.start) as u32;
                let locals = render_safe_locals(env);
                let rendered = format!(
                    "{{ let __jet_env_name = ({name}); let __jet_env_value = ({value}); if let Err(__jet_env_error) = {root}jet_std_env_set(&__jet_env_name, &__jet_env_value) {{ {cleanup} jet_panic_rich({file}, {line}, {fn_name}, {src_line}, {col}, {caret}, &format!(\"core.env.set: {{}}\", __jet_env_error.jet_show()), &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
                    cleanup = crate::Codegen::TIR::RESOURCE_CLEANUP_MARKER,
                    root = cx.root_prefix,
                    file = escape_rust_str(&cx.file),
                    fn_name = escape_rust_str(&env.fn_name),
                    src_line = escape_rust_str(src_line.trim_end()),
                );
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::RequireStop {
                        rendered,
                        always_stops: false,
                    },
                };
            }
        }
    }

    // c109 Phase 10: a core/stdlib module call `alias.method(args)`. The gate proved
    // `recv_type == None` + receiver is a core-import alias + `core_call_covered`.
    // Mirror `emit_core_call` (Source/Codegen/Expression.rs): resolve the module here
    // (total), lower args PLAINLY (no clone/borrow wrappers — `emit_core_call`'s
    // `arg(i)` is a raw `emit_expr`), and carry the return type from the authoritative
    // `core_fixed_sig` table. Tried BEFORE the builtin shape (a core method named
    // `get`/`split`/… must not be claimed by the receiver-keyed builtin op).
    if recv_type.is_none() {
        if matches!(receiver, Expr::Field(..)) {
            if let Some(submodule) = core_module_path_from_receiver(receiver, &cx.core_imports, env)
            {
                let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
                return TExpr {
                    ty: core_call_return_ty(&submodule, method),
                    kind: TExprKind::CoreCall {
                        module: submodule,
                        method: method.to_string(),
                        args: targs,
                    },
                };
            }
        }
        if let Expr::Ident(alias, _) = receiver {
            if !env.locals.contains_key(alias) {
                if let Some(module) = cx.core_imports.get(alias).cloned() {
                    // c109 Phase 13: a closure-taking core call (spawn/serve/guard).
                    // The gate proved a literal-lambda closure arg. Each renders its
                    // bespoke shape at lowering (lambda in subset — Phase 11).
                    if let Some(t) = lower_core_closure_call(&module, method, args, cx, env) {
                        return t;
                    }
                    let targs: Vec<TExpr> =
                        args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
                    // c109 Phase 18: the `core.mem` pointer ops carry a non-fixed return
                    // type. `address_of` is always `Int`; `volatile_read(p)` reads through
                    // the typed pointer, so its result is `ptr_elem(p.ty)` — the `T` of the
                    // `Ptr<T>` arg, recovered from the LOWERED arg's total `ty` (no emit-time
                    // inference, I3). `volatile_write(p, value)` returns `Unit`.
                    // A defensive `Unit` fallback (an ill-typed arg sema would already have
                    // rejected) keeps the fact total.
                    let ty = if module == "core.mem" {
                        match method {
                            "address_of" => Type::Int,
                            "volatile_read" => targs
                                .first()
                                .and_then(|a| crate::Sema::ptr_elem(&a.ty))
                                .unwrap_or_else(unit_type),
                            "volatile_write" => unit_type(),
                            _ => core_call_return_ty(&module, method),
                        }
                    } else if crate::Sema::is_polymorphic_core_special(&module, method) {
                        // c109 Phase 20: the polymorphic special's return type is NOT in
                        // `core_fixed_sig` — sema resolved it (arg-type dependent) and wrote
                        // it onto the node's `resolved_ret`. Read it totally (I3); a unit
                        // fallback (eprint/shuffle return nothing) keeps the fact total.
                        resolved_ret.cloned().unwrap_or_else(unit_type)
                    } else if module == "core.event"
                        && matches!(method, "new" | "with_policy" | "hook" | "async_result")
                    {
                        resolved_ret
                            .cloned()
                            .unwrap_or_else(|| core_call_return_ty(&module, method))
                    } else {
                        core_call_return_ty(&module, method)
                    };
                    return TExpr {
                        ty,
                        kind: TExprKind::CoreCall {
                            module,
                            method: method.to_string(),
                            args: targs,
                        },
                    };
                }
                // c109 Phase 14: a qualified cross-module call `alias.method(args)`.
                // The gate proved the alias is a re-export / import_mod / code_module.
                // Mirror `emit_method_call`'s arms IN ORDER (reexport, import_mods,
                // code_modules) — resolving the path pieces here so emit decides nothing.
                if let Some((real_mod, real_fn)) = cx
                    .reexport_calls
                    .get(&(alias.clone(), method.to_string()))
                    .cloned()
                {
                    let sig = cx
                        .import_sigs
                        .get(&(alias.clone(), method.to_string()))
                        .cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    let ret = cx
                        .import_rets
                        .get(&(alias.clone(), method.to_string()))
                        .cloned()
                        .flatten()
                        .unwrap_or_else(unit_type);
                    return TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod: real_mod,
                                rust_fn: mangle(&real_fn).to_string(),
                            },
                            args: targs,
                        },
                    };
                }
                if let Some(mod_name) = cx.import_mods.get(alias).cloned() {
                    let sig = cx
                        .import_sigs
                        .get(&(alias.clone(), method.to_string()))
                        .cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    let ret = cx
                        .import_rets
                        .get(&(alias.clone(), method.to_string()))
                        .cloned()
                        .flatten()
                        .unwrap_or_else(unit_type);
                    return TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod: mod_name,
                                rust_fn: mangle(method).to_string(),
                            },
                            args: targs,
                        },
                    };
                }
                if cx.code_modules.contains(alias.as_str()) {
                    let mangled_key = format!("{}__{}", alias, method);
                    let sig = cx.sigs.get(&mangled_key).cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    return TExpr {
                        ty: call_return_type(cx, &mangled_key),
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::InlineMangled {
                                mangled: mangled_key,
                            },
                            args: targs,
                        },
                    };
                }
            }
        }
    }
    // c109 Phase 9: a built-in collection/string method (`emit_builtin_method`). The
    // gate proved `recv_type == None` + a covered builtin name + an in-subset value
    // receiver. Resolve the Map-vs-List-vs-String emit branch HERE from the
    // receiver's type (reproducing `expr_jet_ty`, incl. its `None` partiality), so
    // emit makes no type decision (I3). The result type comes from the builtin's
    // sema return (`Collections::builtin_method_return`) for totality.
    if recv_type.is_none() {
        if let Some(op) =
            resolve_builtin_op(receiver, method, method_span, args, resolved_ret, env, cx)
        {
            // D-MEM1 S6: a mutating builtin (`.push()` etc.) on a place rooted in
            // a `Pool` index (`tree[root].children.push(child)`) needs the SAME
            // genuine-mutable-place treatment `LValue::Field`/`LValue::Index`
            // get above — the ordinary receiver lowering reads `pool[id]` as an
            // owned CLONE (`jet_pool_get`), so mutating it would silently edit a
            // throwaway copy instead of the real slot.
            let recv_ast_ty = tir_recv_jet_ty(receiver, env);
            let recv_mut_ty_hint = recv_ast_ty
                .clone()
                .or_else(|| pool_field_ty_hint(receiver, cx, env));
            let recv_t = if crate::Collections::builtin_needs_mut_receiver(
                recv_mut_ty_hint.as_ref().unwrap_or(&Type::Int),
                method,
            ) {
                lower_expr_as_mut_place(receiver, cx, env)
            } else {
                lower_expr(receiver, cx, env)
            };
            // D-HOLE1: `Option.zip`'s `b` type is heterogeneous (arg-dependent), so
            // the generic single-receiver-type table (`builtin_result_ty`) can't
            // resolve it; `resolve_builtin_op` already worked it out for the tuple
            // struct name above — reuse it here instead of guessing a placeholder.
            let result_ty = match &op {
                TBuiltinOp::OptionZip { elem_ty, .. } => Type::Option(Box::new(elem_ty.clone())),
                _ if resolved_ret.is_some() => resolved_ret.cloned().unwrap_or_else(unit_type),
                _ => builtin_result_ty(method, args.len(), recv_ast_ty.as_ref()),
            };
            // Builtins that store an argument need an owned value. A borrowed
            // generic parameter is a dereferenced Rust place, so materialize a
            // clone here instead of leaking rustc E0507. Lookup/compare args stay
            // borrowed and avoid needless copies.
            let targs = args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    if builtin_arg_takes_ownership(&op, i) {
                        lower_owned_expr(&a.expr, cx, env)
                    } else {
                        lower_expr(&a.expr, cx, env)
                    }
                })
                .collect();
            return TExpr {
                ty: result_ty,
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(recv_t),
                    op,
                    args: targs,
                },
            };
        }
    }
    // c109 Phase 19: `Stopwatch.elapsed_millis()` (gate shape d2). The gate proved
    // `recv_type == None` + the `elapsed_millis` name + an in-subset value receiver.
    // Lower to the existing `THandleOp::StopwatchElapsedMillis` (`{root}jet_stopwatch_
    // elapsed_millis(&(recv))`), the same node the Phase-13 handle shape uses — emit is
    // byte-identical to `emit_builtin_method`'s name-keyed `elapsed_millis` arm. The
    // result type is `Int` (`stopwatch_method_return`), kept total per the design.
    if recv_type.is_none() && method == "elapsed_millis" && args.is_empty() {
        let recv_t = lower_expr(receiver, cx, env);
        return TExpr {
            ty: Type::Int,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::StopwatchElapsedMillis,
                args: Vec::new(),
            },
        };
    }
    // c109 Phase 24: `Match.group(n)` (gate shape d4). The gate proved `recv_type ==
    // Some("Match")` + `group`/1 + an in-subset value receiver. Lower to `BuiltinMethod`/
    // `MatchGroup`, byte-for-byte `emit_builtin_method`'s `("Match", "group")` arm. The
    // result type is `String?`. Placed BEFORE the user-instance shape (also `recv_type ==
    // Some`) — `Match` is never a covered user struct/enum, so the two never collide.
    if recv_type.as_deref() == Some("Match") && method == "group" && args.len() == 1 {
        let recv_t = lower_expr(receiver, cx, env);
        let arg0 = lower_expr(&args[0].expr, cx, env);
        return TExpr {
            ty: Type::Option(Box::new(Type::String)),
            kind: TExprKind::BuiltinMethod {
                recv: Box::new(recv_t),
                op: TBuiltinOp::MatchGroup,
                args: vec![arg0],
            },
        };
    }
    // D-REACT1=B: a reactive `Signal`/`Derived` method (gate shape d5). The gate proved
    // `recv_type == Some("Signal"|"Derived")` + `get`/0 or `set`/1. Resolve the op +
    // result type HERE from the receiver's already-resolved `Apply<T>` slot (I3):
    // `Signal.get()`/`Derived.get()` → `T`; `Signal.set(v)` → Unit.
    if matches!(
        recv_type.as_deref(),
        Some("Signal") | Some("Derived") | Some("Computed")
    ) && is_reactive_method_name(method, args.len())
    {
        let recv_t = lower_expr(receiver, cx, env);
        let elem = match &recv_t.ty {
            Type::Apply { args, .. } => args.first().cloned(),
            _ => None,
        }
        .unwrap_or_else(unit_type);
        let (op, ty) = match method {
            "get" => (THandleOp::ReactiveGet, elem),
            "set" => (THandleOp::ReactiveSet, unit_type()),
            _ => unreachable!("is_reactive_method_name admitted only get/set"),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // D-EVENT1=D: Event/Hook/Subscription/EventScope/EventTrace methods.
    if is_event_handle_type(recv_type.as_deref()) && is_event_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        let unit = unit_type();
        let result_ty = match (recv_type.as_deref(), method) {
            (Some("Event"), "emit" | "emit_async") => Type::Named("EventTrace".to_string()),
            (Some("AsyncEvent"), "emit_async") => match &recv_t.ty {
                Type::Apply { args, .. } if args.len() >= 2 => Type::Apply {
                    name: "Task".to_string(),
                    args: vec![Type::Apply { name: "DispatchReport".to_string(), args: vec![args[1].clone()] }],
                },
                _ => Type::Named("Unknown".to_string()),
            },
            (Some("Event"), "on" | "once" | "on_priority")
            | (Some("AsyncEvent"), "on" | "once" | "on_priority")
            | (Some("Hook"), "on" | "once" | "on_priority") => {
                Type::Named("Subscription".to_string())
            }
            (Some("Hook"), "run") => match &recv_t.ty {
                Type::Apply { args, .. } if args.len() >= 2 => args[1].clone(),
                _ => Type::Named("Unknown".to_string()),
            },
            (Some("DispatchReport"), "trace") => Type::Named("EventTrace".to_string()),
            (_, "trace" | "summary") => Type::String,
            (
                _,
                "listener_count" | "queued_count" | "active_count" | "delivered" | "queued"
                | "dropped" | "running_count" | "blocked_count" | "delivered_handlers",
            ) => Type::Int,
            (_, "is_active" | "accepted") => Type::Bool,
            (Some("DispatchReport"), "state") => Type::Named("DispatchState".to_string()),
            _ => unit,
        };
        let expected_payload = match &recv_t.ty {
            Type::Apply { args, .. } => args.first().cloned(),
            _ => None,
        };
        let expected_hook_result = match &recv_t.ty {
            Type::Apply { name, args } if name == "AsyncEvent" && args.len() >= 2 => Some(Type::Result {
                ok: Box::new(Type::Named("Void".to_string())),
                err: Box::new(args[1].clone()),
            }),
            Type::Apply { args, .. } if args.len() >= 2 => args.get(1).cloned(),
            _ => None,
        };
        let targs: Vec<TExpr> = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let handler_idx = matches!(
                    (method, args.len(), i),
                    ("on" | "once", 2, 1) | ("on_priority", 3, 2)
                );
                if handler_idx {
                    if let Expr::Lambda(lam) = &a.expr {
                        let params = expected_payload.clone().into_iter().collect::<Vec<_>>();
                        let tl = lower_lambda_expecting(lam, cx, env, Some(params.as_slice()));
                        return TExpr {
                            ty: Type::Fn {
                                params,
                                ret: expected_hook_result.clone().map(Box::new),
                                effect_bound: None,
                            },
                            kind: TExprKind::Lambda(Box::new(tl)),
                        };
                    }
                }
                lower_expr(&a.expr, cx, env)
            })
            .collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::EventMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-WATCH-SCOPE1: WatchHandle/WatchSet methods. Callback lambdas receive
    // a WatchEvent payload, matching the shared Core event/callback model.
    if is_watch_handle_type(recv_type.as_deref()) && is_watch_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match (recv_type.as_deref(), method) {
            (_, "poll" | "events") => Type::List(Box::new(Type::Named("WatchEvent".to_string()))),
            (Some("WatchHandle"), "on" | "once") => Type::Named("Subscription".to_string()),
            (_, "summary") => Type::String,
            (_, "is_active") => Type::Bool,
            _ => unit_type(),
        };
        let targs: Vec<TExpr> = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                if matches!(
                    (recv_type.as_deref(), method, args.len(), i),
                    (Some("WatchHandle"), "on" | "once", 2, 1)
                ) {
                    if let Expr::Lambda(lam) = &a.expr {
                        let params = vec![Type::Named("WatchEvent".to_string())];
                        let tl = lower_lambda_expecting(lam, cx, env, Some(params.as_slice()));
                        return TExpr {
                            ty: Type::Fn {
                                params,
                                ret: None,
                                effect_bound: None,
                            },
                            kind: TExprKind::Lambda(Box::new(tl)),
                        };
                    }
                }
                lower_expr(&a.expr, cx, env)
            })
            .collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::WatchMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-PROCESS1: ProcessSpec/ProcessChild methods lower to explicit prelude
    // helpers; sema already proved arity and argument types.
    if matches!(
        recv_type.as_deref(),
        Some("ProcessSpec") | Some("ProcessChild")
    ) {
        let recv_t = lower_expr(receiver, cx, env);
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        let result_ty = match (recv_type.as_deref(), method) {
            (Some("ProcessSpec"), "run") => Type::Result {
                ok: Box::new(Type::Named("ProcessResult".to_string())),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            (Some("ProcessSpec"), "spawn") => Type::Result {
                ok: Box::new(Type::Named("ProcessChild".to_string())),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            (Some("ProcessSpec"), _) => Type::Named("ProcessSpec".to_string()),
            (Some("ProcessChild"), "id") => Type::Int,
            (Some("ProcessChild"), "wait") => Type::Result {
                ok: Box::new(Type::Named("ProcessResult".to_string())),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            (Some("ProcessChild"), _) => Type::Result {
                ok: Box::new(unit_type()),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            _ => unit_type(),
        };
        let op = if recv_type.as_deref() == Some("ProcessSpec") {
            THandleOp::ProcessSpecMethod {
                method: method.to_string(),
            }
        } else {
            THandleOp::ProcessChildMethod {
                method: method.to_string(),
            }
        };
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // D-PROCESS1=A: `.write(text)` on `child.stdin` — the receiver LOWERS to the
    // real `ProcessChild.stdin` Rust field (a writer handle), and the write goes
    // through the generic `jet_process_stdin_write` prelude helper.
    if recv_type.as_deref() == Some("ProcessStdin") && method == "write" {
        let recv_t = lower_expr(receiver, cx, env);
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: Type::Result {
                ok: Box::new(unit_type()),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::ProcessStdinWrite,
                args: targs,
            },
        };
    }
    // D-HONESTNUM1=A: a `Measurement<Float>` method (gate shape d6).
    if recv_type.as_deref() == Some("Measurement") && is_measurement_method_name(method, args.len())
    {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match method {
            "value" | "uncertainty" => Type::Float,
            _ => recv_t.ty.clone(),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::MeasurementMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-PENDING1=B: a `Loadable<T,E>` method (gate shape d7).
    if recv_type.as_deref() == Some("Loadable") && is_loadable_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match method {
            "is_loading" | "is_loaded" | "is_failed" | "is_idle" => Type::Bool,
            // .loaded() → Option<T> (extract T from Loadable<T,E>)
            "loaded" => match &recv_t.ty {
                Type::Apply { args: targs, .. } if !targs.is_empty() => {
                    Type::Option(Box::new(targs[0].clone()))
                }
                _ => Type::Option(Box::new(Type::Named("Unknown".to_string()))),
            },
            // .or_else(default) → T
            "or_else" => match &recv_t.ty {
                Type::Apply { args: targs, .. } if !targs.is_empty() => targs[0].clone(),
                _ => Type::Named("Unknown".to_string()),
            },
            _ => recv_t.ty.clone(),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::LoadableMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-TTLVAL1=A: Expiring<T> / Rotting<T> method calls.
    if matches!(recv_type.as_deref(), Some("Expiring" | "Rotting"))
        && matches!(method, "get" | "is_valid" | "force")
    {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match (recv_type.as_deref(), method) {
            (Some("Expiring") | Some("Rotting"), "get") => Type::Result {
                ok: Box::new(match &recv_t.ty {
                    Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
                    _ => Type::Named("Unknown".to_string()),
                }),
                err: Box::new(Type::Named("Expired".to_string())),
            },
            (Some(_), "is_valid") => Type::Bool,
            _ => recv_t.ty.clone(),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        let op = if recv_type.as_deref() == Some("Rotting") {
            THandleOp::RottingMethod {
                method: method.to_string(),
            }
        } else {
            THandleOp::ExpiringMethod {
                method: method.to_string(),
            }
        };
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // D-RENDERTGT2=A (c133 M1/M2): a UI backend method (gate shape d7b).
    if matches!(
        recv_type.as_deref(),
        Some("NullBackend" | "TuiBackend" | "GtkBackend")
    ) && is_ui_backend_method_name(recv_type.as_deref(), method, args.len())
    {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match method {
            "measure" => Type::Named("Size".to_string()),
            "on_event" => Type::Named("EventResult".to_string()),
            "commands" | "frame_lines" => Type::List(Box::new(Type::String)),
            "render_count" => Type::Int,
            // D-A11YGATE1=B (c134 Phase 6): keyboard focus routing.
            "focused_label" => Type::String,
            // D-UIDEVSHELL1=A (c134 Phase 8): native GTK4 widget handles.
            "label" | "button" => Type::Int,
            _ => unit_type(),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::UiBackendMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // c-devserver (owner-directed 2026-07-01): a DevServer builder method.
    if recv_type.as_deref() == Some("DevServer") && is_devserver_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match method {
            "serve" => unit_type(),
            _ => Type::Named("DevServer".to_string()),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::DevServerMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-NETDEP1=A / D-HTTPLIB1=A: an HTTP type method (gate shape d10).
    if is_http_type(recv_type.as_deref()) && is_http_method_name(recv_type.as_deref(), method) {
        let kind = recv_type.as_deref().unwrap_or("HttpClientReq").to_string();
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match (kind.as_str(), method) {
            ("HttpClientReq", "header")
            | ("HttpClientReq", "body")
            | ("HttpClientReq", "timeout")
            | ("HttpClientReq", "connect_timeout")
            | ("HttpClientReq", "read_timeout")
            | ("HttpClientReq", "total_timeout")
            | ("HttpClientReq", "redirects")
            | ("HttpClientReq", "proxy")
            | ("HttpClientReq", "cookie")
            | ("HttpClientReq", "form")
            | ("HttpClientReq", "multipart_text") => Type::Named("HttpClientReq".to_string()),
            ("HttpClientReq", "send") => Type::Result {
                ok: Box::new(Type::Named("HttpClientResp".to_string())),
                err: Box::new(Type::String),
            },
            ("HttpClientResp", "status") => Type::Int,
            ("HttpClientResp", "body") | ("HttpClientResp", "header") => Type::String,
            ("HttpClientResp", "cookies") => Type::List(Box::new(Type::String)),
            ("HttpMux", _) => unit_type(),
            ("HttpHandler", "handle") => Type::Named("HttpSrvResp".to_string()),
            ("HttpSrvReq", "method") | ("HttpSrvReq", "path") | ("HttpSrvReq", "body") => {
                Type::String
            }
            ("HttpSrvReq", "body_len") => Type::Int,
            ("HttpSrvReq", "under_limit") => Type::Bool,
            ("HttpSrvReq", "param") | ("HttpSrvReq", "header") => {
                Type::Option(Box::new(Type::String))
            }
            ("HttpSrvResp", "header") => Type::Named("HttpSrvResp".to_string()),
            ("HttpSrvResp", "status") => Type::Int,
            ("HttpSrvResp", "body") => Type::String,
            ("HttpServer", "local_addr") => Type::Result { ok: Box::new(Type::String), err: Box::new(Type::String) },
            ("HttpServer", "serve" | "shutdown") => Type::Result {
                ok: Box::new(Type::Named("HttpShutdownReport".to_string())),
                err: Box::new(Type::String),
            },
            _ => unit_type(),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        let op = if kind.starts_with("HttpServer")
            || kind == "HttpMux"
            || kind == "HttpHandler"
            || kind == "HttpSrvReq"
            || kind == "HttpSrvResp"
        {
            THandleOp::HttpServerMethod {
                kind,
                method: method.to_string(),
            }
        } else {
            THandleOp::HttpClientMethod {
                kind,
                method: method.to_string(),
            }
        };
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // D-TIMEDEPTH1=A: a civil-time method (gate shape d9).
    if matches!(
        recv_type.as_deref(),
        Some(
            "Date"
                | "LocalDate"
                | "LocalTime"
                | "DateTime"
                | "Instant"
                | "Period"
                | "Zone"
                | "ZonedDateTime"
        )
    ) && is_civil_time_method_name(recv_type.as_deref(), method)
    {
        let kind = recv_type.as_deref().unwrap_or("Date").to_string();
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match (kind.as_str(), method) {
            ("Date" | "LocalDate", "year")
            | ("Date" | "LocalDate", "month")
            | ("Date" | "LocalDate", "day")
            | ("Date" | "LocalDate", "diff_days")
            | ("Date" | "LocalDate", "weekday")
            | ("Date" | "LocalDate", "iso_weekday")
            | ("Date" | "LocalDate", "day_of_year")
            | ("Date" | "LocalDate", "iso_week") => Type::Int,
            ("Date" | "LocalDate", "add_days" | "add_months" | "add_period" | "truncate") => {
                Type::Named("LocalDate".to_string())
            }
            ("Date" | "LocalDate", "to_string" | "format") => Type::String,
            ("LocalTime", "hour" | "minute" | "second") => Type::Int,
            ("LocalTime", "to_string") => Type::String,
            ("DateTime", "hour")
            | ("DateTime", "minute")
            | ("DateTime", "second")
            | ("DateTime", "to_timestamp")
            | ("DateTime", "to_unix_ms") => Type::Int,
            ("DateTime", "date") => Type::Named("LocalDate".to_string()),
            ("DateTime", "time") => Type::Named("LocalTime".to_string()),
            ("DateTime", "plus_duration" | "truncate" | "round") => {
                Type::Named("DateTime".to_string())
            }
            ("DateTime", "in_zone") => Type::Named("ZonedDateTime".to_string()),
            ("DateTime", "to_string" | "format_rfc3339" | "format") => Type::String,
            ("Instant", "elapsed_millis") => Type::Int,
            ("Period", "to_string") => Type::String,
            ("Zone", "name") => Type::String,
            ("ZonedDateTime", "date") => Type::Named("LocalDate".to_string()),
            ("ZonedDateTime", "time") => Type::Named("LocalTime".to_string()),
            ("ZonedDateTime", "offset_seconds") => Type::Int,
            ("ZonedDateTime", "to_datetime") => Type::Named("DateTime".to_string()),
            ("ZonedDateTime", "zone") => Type::Named("Zone".to_string()),
            ("ZonedDateTime", "add_duration" | "add_period") => {
                Type::Named("ZonedDateTime".to_string())
            }
            ("ZonedDateTime", "to_string" | "format") => Type::String,
            _ => unit_type(),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::CivilTimeMethod {
                    kind,
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-APPROX1=A: a sketch method (gate shape d8).
    if is_sketch_type(recv_type.as_deref()) && is_sketch_method_name(recv_type.as_deref(), method) {
        let sketch = recv_type.as_deref().unwrap_or("").to_string();
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match (sketch.as_str(), method) {
            ("HyperLogLog", "add")
            | ("TDigest", "add")
            | ("CountMinSketch", "add")
            | ("ReservoirSampler", "add") => unit_type(),
            ("HyperLogLog", "count") | ("CountMinSketch", "count") => Type::Int,
            ("TDigest", "quantile") => Type::Float,
            ("ReservoirSampler", "sample") => Type::List(Box::new(Type::String)),
            _ => unit_type(),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::SketchMethod {
                    sketch,
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // c109 Phase 21 / D-TUPLE-DESTRUCT1: a Task/Receiver/Sender concurrency method
    // (gate shape d3). The gate proved `recv_type == None` + a disjoint concurrency
    // name+arity. Resolve the op + result type HERE (totality). The result type comes
    // from `Collections::builtin_method_return`'s `Type::Apply` arms
    // (Source/Collections.rs), read off the receiver's already-resolved type
    // `Task<T>`/`Receiver<T>`/`Sender<T>` (the LOWERED receiver's `.ty`, total from the
    // binding's annotated/inferred slot — never re-inferred in emit, I3): `join`/`wait`
    // → `T`; `detach`/`pause`/`resume`/`cancel`/`send` → Unit; `trace` → `String`;
    // `receive` → `Result<T, Closed>`. Args lowered PLAINLY (the AST
    // `emit_builtin_method`'s `arg(i)` is a raw `emit_expr`).
    if recv_type.is_none() && is_concurrency_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        // The element type `T` from the receiver's `Apply<T>` (the first type arg).
        let elem = match &recv_t.ty {
            Type::Apply { args, .. } => args.first().cloned(),
            _ => None,
        };
        let elem = elem.unwrap_or_else(unit_type);
        let (op, ty) = match method {
            "join" | "wait" => (THandleOp::TaskJoin, elem),
            "detach" => (THandleOp::TaskDetach, unit_type()),
            "pause" => (THandleOp::TaskPause, unit_type()),
            "resume" => (THandleOp::TaskResume, unit_type()),
            "cancel" => (THandleOp::TaskCancel, unit_type()),
            "trace" => (THandleOp::TaskTrace, Type::String),
            "receive" => (
                THandleOp::ChannelReceive,
                Type::Result {
                    ok: Box::new(elem),
                    err: Box::new(Type::Named("Closed".to_string())),
                },
            ),
            "send" => (THandleOp::SenderSend, unit_type()),
            _ => unreachable!("is_concurrency_method_name admitted only these names"),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // D-MEM1 S6 (D-POOLID-API1=A / D-SHARED-API1=A): `Pool<T>.add/remove/ids`
    // and `Shared<T>.read/edit`. Both lower to a PLAIN Rust method call on the
    // receiver (`(recv).add(val)`, …) — `add`/`remove`/`ids` are genuine
    // inherent methods on `JetPool<T>` and `read`/`edit` on `JetShared<T>`
    // (Prelude/CoreLib.rs), so there's no free-function indirection to model
    // (unlike `Pool`/`Shared` INDEXING, which needs a real mutable-place
    // helper — see the `Expr::Index`/`LValue::Field` arms). `ConstInline` is
    // the pragmatic vehicle, same as the concurrency/Sql/Html escapes nearby.
    {
        let recv_peek = tir_recv_jet_ty(receiver, env);
        // Sema sets `recv_type` to `"Pool"`/`"Shared"` explicitly for these calls
        // (unlike Task/Sender, whose method names alone are globally unambiguous —
        // `add`/`remove`/`ids`/`read`/`edit` collide with Set/List/Map names).
        let is_pool = recv_type.as_deref() == Some("Pool");
        let is_shared = recv_type.as_deref() == Some("Shared");
        if is_pool && matches!(method, "add" | "remove" | "ids") && args.len() <= 1 {
            let recv_t = lower_expr(receiver, cx, env);
            let elem = match &recv_t.ty {
                Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
                _ => Type::Int,
            };
            let id_ty = Type::Apply {
                name: "Id".to_string(),
                args: vec![elem.clone()],
            };
            let ty = match method {
                "add" => id_ty,
                "remove" => Type::Option(Box::new(elem)),
                "ids" => Type::List(Box::new(id_ty)),
                _ => unreachable!("matches! above admitted only these"),
            };
            let recv_s = emit_tir_expr(&recv_t, cx);
            let arg_s: Vec<String> = args
                .iter()
                .map(|a| emit_tir_expr(&lower_expr(&a.expr, cx, env), cx))
                .collect();
            return TExpr {
                ty,
                kind: TExprKind::ConstInline(format!(
                    "({}).{}({})",
                    recv_s,
                    method,
                    arg_s.join(", ")
                )),
            };
        }
        if is_shared && matches!(method, "read" | "edit") && args.len() == 1 {
            let inner = match &recv_peek {
                Some(Type::Shared(inner)) => (**inner).clone(),
                _ => Type::Int,
            };
            let recv_t = lower_expr(receiver, cx, env);
            let recv_s = emit_tir_expr(&recv_t, cx);
            let Expr::Lambda(lam) = &args[0].expr else {
                unreachable!("sema's finish_shared_read/finish_shared_edit require a lambda arg");
            };
            let expected = std::slice::from_ref(&inner);
            // `JetShared::read`/`edit` lend `&T`/`&mut T` directly. This host
            // borrow is not an unmarked function-value parameter and must not
            // receive another D-MEM-PARAM1 Read borrow.
            let tl = lower_lambda_expecting_host_borrow(
                lam,
                cx,
                env,
                expected,
                method == "edit",
            );
            // D-STM1=A (card #506): a `Shared<T>.edit(f)` inside a `@Transact` block
            // routes to the deferred `edit_txn` — the write is buffered and applied
            // atomically at the block's commit, so the call yields nothing (Unit; E0750
            // rejects a value-producing edit here). The closure is stored past the call,
            // so it must `move` its captures. A `.read` (or an `.edit` outside a
            // transaction) is unchanged.
            let (method_out, ty, force_move) =
                if method == "edit" && cx.in_stm_transact.get() {
                    cx.stm_touched.set(true);
                    ("edit_txn", Type::Tuple(vec![]), true)
                } else {
                    (
                        method,
                        lambda_body_ty_expecting(lam, cx, env, Some(expected)),
                        tl.is_move,
                    )
                };
            let move_kw = if force_move { "move " } else { "" };
            let raw = format!("{}|{}| {}", move_kw, tl.params.join(", "), tl.body);
            let closure = if tl.prep.is_empty() {
                raw
            } else {
                format!("{{ {} {} }}", tl.prep, raw)
            };
            return TExpr {
                ty,
                kind: TExprKind::ConstInline(format!("({}).{}({})", recv_s, method_out, closure)),
            };
        }
    }
    // c109 Phase 11: a closure-taking collection method (`map`/`filter`/`each`/…).
    // The gate proved `recv_type == None` + a closure-method name + a literal lambda
    // arg. Resolve the receiver-type + Fn-vs-FnMut dispatch HERE into a total
    // `TClosureOp` (reproducing `emit_builtin_method`'s closure arms, incl. its
    // `expr_jet_ty(receiver)` Map/trait-object branches), so emit makes no decision.
    if recv_type.is_none() && crate::Collections::is_closure_method(method) {
        let op = resolve_closure_op(receiver, method, args, env, cx);
        let recv_t = lower_expr(receiver, cx, env);
        let recv_ast_ty = tir_recv_jet_ty(receiver, env);
        let result_ty = builtin_result_ty(method, args.len(), recv_ast_ty.as_ref());
        // Collection helpers lend callback inputs (`&T`, or `&U, &T` for
        // folds). Lower that host borrow exactly once, including scalar
        // payloads. `Option.map` emits through `.as_ref()` for the same law.
        let mut callback_params = recv_ast_ty
            .as_ref()
            .and_then(|ty| crate::Collections::builtin_method_arg_types(ty, method))
            .and_then(|types| {
                types.into_iter().find_map(|ty| match ty {
                    Type::Fn { params, .. } => Some(params),
                    _ => None,
                })
            });
        if matches!(method, "reduce" | "fold" | "scan" | "par_fold") {
            if let Some(seed_ty) = args
                .first()
                .and_then(|arg| tir_recv_jet_ty(&arg.expr, env))
            {
                if let Some(first) = callback_params
                    .as_mut()
                    .and_then(|params| params.first_mut())
                {
                    *first = seed_ty;
                }
            }
        }
        let targs = args
            .iter()
            .map(|a| {
                if let (Expr::Lambda(lam), Some(params)) = (&a.expr, callback_params.as_ref()) {
                    let tl = lower_lambda_expecting_host_borrow(lam, cx, env, params, false);
                    return TExpr {
                        ty: Type::Fn {
                            params: params.clone(),
                            ret: None,
                            effect_bound: None,
                        },
                        kind: TExprKind::Lambda(Box::new(tl)),
                    };
                }
                lower_expr(&a.expr, cx, env)
            })
            .collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::ClosureMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // c109 Phase 12: a numeric predicate / bit-pop / width-conversion method
    // (`is_nan`/`count_ones`/`to_i32`/…). The gate proved `recv_type ==
    // Some(<numeric name>)` + a covered nullary numeric op. Resolve the receiver
    // width source/target + the widening-vs-narrowing branch HERE (reproducing
    // `numeric_conversion`/`conv_rust_target` from Expression.rs) into a total
    // `TNumericOp`, so emit makes no decision (I3). The result type comes from
    // `numeric_method_return` (the sema table), keyed on the receiver type recovered
    // from `recv_type` (the total width source — `src = recv_type.or_else(rty.name())`
    // on the AST side, where `recv_type` is always `Some` for these).
    if let Some(numeric_name) = recv_type {
        if let Some(recv_ty) = crate::AST::numeric_type_from_name(numeric_name) {
            if let Some(op) = resolve_numeric_op(method, numeric_name) {
                let recv_t = lower_expr(receiver, cx, env);
                let result_ty = builtin_result_ty(method, args.len(), Some(&recv_ty));
                return TExpr {
                    ty: result_ty,
                    kind: TExprKind::NumericMethod {
                        recv: Box::new(recv_t),
                        op,
                    },
                };
            }
        }
    }
    // c109 Phase 25: HttpRouter route registration `router.get/post/put/delete(path,
    // handler)` (D-ROUTE1=A). The gate (`router_register_in_subset`) proved the receiver
    // + path in-subset and the handler a named-fn/lambda. Render the handler closure HERE
    // (the `emit_router_handler` reproduction); emit assembles the register call. Result
    // is Unit (the registration is a statement effect).
    if recv_type.as_deref() == Some("HttpRouter")
        && matches!(method, "get" | "post" | "put" | "delete")
        && args.len() == 2
    {
        let verb = match method {
            "get" => "GET",
            "post" => "POST",
            "put" => "PUT",
            "delete" => "DELETE",
            _ => unreachable!(),
        };
        let recv_t = lower_expr(receiver, cx, env);
        let path_t = lower_expr(&args[0].expr, cx, env);
        let handler = render_router_handler(args, cx, env);
        let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0;
        return TExpr {
            ty: unit_type(),
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::HttpRouterRegister {
                    verb,
                    handler,
                    file: cx.file.clone(),
                    line,
                },
                args: vec![path_t],
            },
        };
    }
    // c109 Phase 13: a method ON a handle. The gate proved `recv_type ==
    // Some(<handle>)` + a covered handle op. Resolve the handle-receiver branch HERE
    // into a total `THandleOp` (reproducing the handle arms of `emit_builtin_method`),
    // so emit makes no type decision (I3). Args lowered PLAINLY (`arg(i)` = raw
    // `emit_expr`). The return type is the total sema handle-table fact.
    // D-SIMD2 / D-LINALG1: a method on a built-in math value type. Resolve the
    // reduce-op marker (which is NOT a lowerable expression) here so emit makes no
    // decision (I3). The return type is the total sema math-method fact.
    if let Some(handle) = recv_type {
        if crate::Sema::is_math_type(handle) && !cx.type_names.contains(handle) {
            let is_reduce = method == "reduce" && crate::Sema::is_simd_lane_type(handle);
            if is_reduce || crate::Sema::math_method_return(handle, method, args.len()).is_some() {
                let recv_t = lower_expr(receiver, cx, env);
                let (reduce_op, value_args): (Option<String>, Vec<TExpr>) = if is_reduce {
                    let op = match args.first().map(|a| &a.expr) {
                        Some(Expr::ReduceMarker(name, _)) => name.clone(),
                        _ => "Add".to_string(),
                    };
                    (Some(op), Vec::new())
                } else {
                    (
                        None,
                        args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect(),
                    )
                };
                let ty = if is_reduce {
                    crate::Sema::math_scalar_ty(handle)
                } else {
                    crate::Sema::math_method_return(handle, method, args.len())
                        .unwrap_or_else(unit_type)
                };
                return TExpr {
                    ty,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(recv_t),
                        op: THandleOp::MathMethod {
                            type_name: handle.to_string(),
                            method: method.to_string(),
                            reduce_op,
                        },
                        args: value_args,
                    },
                };
            }
        }
    }
    // D-SHIFT1 (c7shift): `cursor.take_pattern("…")` — argument-dependent
    // (the return shape comes from the pattern's holes), so it is resolved
    // directly instead of through a generic method-return table. The parser
    // already committed to `Expr::StrMatchLit`
    // for this argument (sema rejects any other shape), so it's always
    // present when this method name/receiver-type pair is reached.
    if let Some(handle) = recv_type {
        if handle == "Cursor" && method == "take_pattern" {
            if let Some(arg) = args.first() {
                if let Expr::StrMatchLit(parts, _) = &arg.expr {
                    return lower_cursor_take_pattern(receiver, parts, cx, env);
                }
            }
        }
    }
    // D-BINPAT1 (card #506 follow-up): `reader.take_pattern(b"…")` — the
    // byte-mode sibling, same reasoning. The parser already committed to
    // `Expr::BinMatchLit` for this argument (sema rejects any other shape).
    if let Some(handle) = recv_type {
        if handle == "Reader" && method == "take_pattern" {
            if let Some(arg) = args.first() {
                if let Expr::BinMatchLit(parts, _) = &arg.expr {
                    return lower_reader_take_pattern(receiver, parts, cx, env);
                }
            }
        }
    }
    // D-LAYOUT1 / D-LAYOUT-GATES1: a method on `LayoutHandle`/`Constraint`.
    // Every Jet method name IS the `jet_layout` Rust method name (pure
    // passthrough, no reduce-marker-style special casing needed).
    if let Some(handle) = recv_type {
        if crate::Sema::is_layout_type(handle) {
            if let Some(ret) = crate::Sema::layout_method_return(handle, method, args.len()) {
                let recv_t = lower_expr(receiver, cx, env);
                let value_args: Vec<TExpr> =
                    args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
                return TExpr {
                    ty: ret,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(recv_t),
                        op: THandleOp::LayoutMethod {
                            method: method.to_string(),
                        },
                        args: value_args,
                    },
                };
            }
        }
    }
    if let Some(handle) = recv_type {
        if handle == "__SerdeEncode__" && method == "encode" && args.is_empty() {
            return TExpr {
                ty: Type::Named(Syntax::TYPE_DATA.to_string()),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(lower_expr(receiver, cx, env)),
                    op: THandleOp::SerdeEncode,
                    args: Vec::new(),
                },
            };
        }
        if handle == Syntax::TYPE_DATA
            && method == Syntax::METHOD_DATATREE_DECODE
            && args.is_empty()
        {
            if let Some(Type::Result { ok, .. }) = resolved_ret {
                return TExpr {
                    ty: resolved_ret.cloned().unwrap_or_else(unit_type),
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(lower_expr(receiver, cx, env)),
                        op: THandleOp::DataTreeDecode((**ok).clone()),
                        args: Vec::new(),
                    },
                };
            }
        }
        if let Some(mut op) = handle_method_op(handle, method, args.len()) {
            if let THandleOp::DurationIn { unit } = &mut op {
                *unit = args.first().and_then(|arg| match &arg.expr {
                    Expr::EnumLit { variant, .. } => Syntax::DURATION_UNITS
                        .iter()
                        .copied()
                        .find(|candidate| *candidate == variant),
                    _ => None,
                });
            }
            let recv_t = lower_expr(receiver, cx, env);
            let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            // c109 Phase 19: an arena `alloc(v)` returns a `&mut T` view whose VALUE type is
            // the arg's type (sema's `alloc_method_return` returns a `__alloc_infer__`
            // sentinel, resolved from the arg). The result `ty` is rarely load-bearing (an
            // `arena_view` binding emits no type annotation), but kept total per the design —
            // recovered from the LOWERED arg's total `ty`, never re-inferred (I3).
            let ty = match &op {
                THandleOp::AllocAlloc => targs
                    .first()
                    .map(|a| a.ty.clone())
                    .unwrap_or_else(unit_type),
                THandleOp::AllocReset => unit_type(),
                _ => handle_method_return_ty(handle, method, args.len()),
            };
            return TExpr {
                ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op,
                    args: targs,
                },
            };
        }
    }
    // D-ENCSTREAM-SURFACE1=A: qualified shared type constructor.
    if method == "safe" && args.is_empty() {
        if let Expr::Field(base, leaf, _) = receiver {
            if leaf == "EncodingLimits"
                && core_module_path_from_receiver(base, &cx.core_imports, env).as_deref() == Some("core.encoding")
            {
                return TExpr {
                    ty: Type::Named("EncodingLimits".to_string()),
                    kind: TExprKind::StaticCall {
                        type_prefix: format!("{}jet_std::EncodingLimits", cx.root_prefix),
                        method_rust: "safe".to_string(),
                        args: vec![],
                    },
                };
            }
            if leaf == "CBOROptions"
                && core_module_path_from_receiver(base, &cx.core_imports, env).as_deref() == Some("core.encoding.cbor")
            {
                return TExpr {
                    ty: Type::Named("CBOROptions".to_string()),
                    kind: TExprKind::StaticCall {
                        type_prefix: format!("{}jet_std::CBOROptions", cx.root_prefix),
                        method_rust: "safe".to_string(),
                        args: vec![],
                    },
                };
            }
            if matches!(leaf.as_str(), "XMLLimits" | "XMLParseOptions" | "XMLRenderOptions")
                && core_module_path_from_receiver(base, &cx.core_imports, env).as_deref() == Some("core.encoding.xml")
            {
                return TExpr {
                    ty: Type::Named(leaf.clone()),
                    kind: TExprKind::StaticCall {
                        type_prefix: format!("{}jet_std::{leaf}", cx.root_prefix),
                        method_rust: "safe".to_string(),
                        args: vec![],
                    },
                };
            }
            if leaf == "Limits"
                && core_module_path_from_receiver(base, &cx.core_imports, env).as_deref() == Some("core.email")
            {
                return TExpr {
                    ty: Type::Named("Limits".to_string()),
                    kind: TExprKind::StaticCall {
                        type_prefix: format!("{}jet_email::Limits", cx.root_prefix),
                        method_rust: "safe".to_string(),
                        args: vec![],
                    },
                };
            }
        }
    }
    // c109 Phase 7: a STATIC method call `Type.make(args)`. The gate
    // (`static_method_call_in_subset`) proved the receiver is a covered type-name
    // ident and `method` is a registered static method. Mirror the AST path
    // (Expression.rs ~L1644): `user_<Type>::user_<method>(args)`.
    if let Some(type_name) = static_call_type_name_lower(receiver, env) {
        // D-PATHFS1: `Path.from(str)` → `jet_path_from(&(str_arg))`.
        // The string arg becomes the "receiver" slot of the PathFrom HandleMethod;
        // `Path` itself (a type-name ident) has no value.
        if type_name == "Path"
            && method == "from"
            && args.len() == 1
            && !cx.type_names.contains("Path")
        {
            let str_arg = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named("Path".to_string()),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(str_arg),
                    op: THandleOp::PathFrom,
                    args: vec![],
                },
            };
        }
        // D-SHIFT1 (c7shift): `Reader.over(bytes)` → `jet_reader_over(&(bytes_arg))`.
        // Same "arg becomes the recv slot" shape as `Path.from` above.
        if type_name == "Reader"
            && method == "over"
            && args.len() == 1
            && !cx.type_names.contains("Reader")
        {
            let bytes_arg = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named("Reader".to_string()),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(bytes_arg),
                    op: THandleOp::ReaderOver,
                    args: vec![],
                },
            };
        }
        // D-SHIFT1: `Cursor.over(s)` → `jet_cursor_over(&(s_arg))`.
        if type_name == "Cursor"
            && method == "over"
            && args.len() == 1
            && !cx.type_names.contains("Cursor")
        {
            let s_arg = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named("Cursor".to_string()),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(s_arg),
                    op: THandleOp::CursorOver,
                    args: vec![],
                },
            };
        }
        // D-FIDELITY-API1=A: `Perf.fidelity()` / `Perf.override_fidelity(v)?`
        // lower to the same core call shape as `use core.perf as Perf`.
        if type_name == "Perf" && !cx.type_names.contains("Perf") {
            let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            return TExpr {
                ty: core_call_return_ty("core.perf", method),
                kind: TExprKind::CoreCall {
                    module: "core.perf".to_string(),
                    method: method.to_string(),
                    args: targs,
                },
            };
        }
        // D-COLLBREADTH1=A: `Set.from([...])` → collect list into HashSet.
        // Lower the list arg as the recv of a SetFrom BuiltinMethod.
        if type_name == "Set" && method == "from" && args.len() == 1 {
            let list_arg = lower_expr(&args[0].expr, cx, env);
            let elem_ty = match &list_arg.ty {
                Type::List(inner) => *inner.clone(),
                _ => Type::Int,
            };
            return TExpr {
                ty: Type::Apply {
                    name: "Set".to_string(),
                    args: vec![elem_ty],
                },
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(list_arg),
                    op: TBuiltinOp::SetFrom,
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_SORTED_SET && method == "from" && args.len() == 1 {
            let list_arg = lower_expr(&args[0].expr, cx, env);
            let elem_ty = match &list_arg.ty {
                Type::List(inner) => *inner.clone(),
                _ => Type::Int,
            };
            return TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_SORTED_SET.to_string(),
                    args: vec![elem_ty],
                },
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(list_arg),
                    op: TBuiltinOp::SortedSetFrom,
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_SORTED_SET && method == "new" && args.is_empty() {
            let elem_ty = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => targs[0].clone(),
                _ => Type::Int,
            };
            let elem_rust = cx.rust_type(&elem_ty);
            return TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_SORTED_SET.to_string(),
                    args: vec![elem_ty],
                },
                kind: TExprKind::StaticCall {
                    type_prefix: format!("std::collections::BTreeSet::<{}>", elem_rust),
                    method_rust: "new".to_string(),
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_PRIORITY_QUEUE && method == "from" && args.len() == 1 {
            let list_arg = lower_expr(&args[0].expr, cx, env);
            let elem_ty = match &list_arg.ty {
                Type::List(inner) => *inner.clone(),
                _ => Type::Int,
            };
            return TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_PRIORITY_QUEUE.to_string(),
                    args: vec![elem_ty],
                },
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(list_arg),
                    op: TBuiltinOp::PriorityQueueFrom,
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_PRIORITY_QUEUE && method == "new" && args.is_empty() {
            let elem_ty = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => targs[0].clone(),
                _ => Type::Int,
            };
            let elem_rust = cx.rust_type(&elem_ty);
            return TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_PRIORITY_QUEUE.to_string(),
                    args: vec![elem_ty],
                },
                kind: TExprKind::StaticCall {
                    type_prefix: format!("std::collections::BinaryHeap::<{}>", elem_rust),
                    method_rust: "new".to_string(),
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_LRU && method == "new" && args.len() == 1 {
            let cap_arg = lower_expr(&args[0].expr, cx, env);
            let (key_ty, value_ty) = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if targs.len() >= 2 => {
                    (targs[0].clone(), targs[1].clone())
                }
                _ => (Type::String, Type::Int),
            };
            return TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_LRU.to_string(),
                    args: vec![key_ty, value_ty],
                },
                kind: TExprKind::StaticCall {
                    type_prefix: "JetLru".to_string(),
                    method_rust: "new".to_string(),
                    args: vec![TCallArg {
                        value: cap_arg,
                        borrow: false,
                        mut_borrow: false,
                        clone: false,
                        arc_clone: false,
                        fn_coerce: None,
                        widen_to_vec: false,
                    }],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_BIT_SET && method == "new" && args.is_empty() {
            return TExpr {
                ty: Type::Named(crate::Syntax::TYPE_BIT_SET.to_string()),
                kind: TExprKind::StaticCall {
                    type_prefix: "JetBitSet".to_string(),
                    method_rust: "new".to_string(),
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_BYTE_BUFFER && method == "new" && args.is_empty() {
            return TExpr {
                ty: Type::Named(crate::Syntax::TYPE_BYTE_BUFFER.to_string()),
                kind: TExprKind::StaticCall {
                    type_prefix: "JetByteBuffer".to_string(),
                    method_rust: "new".to_string(),
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_BYTE_BUFFER && method == "from" && args.len() == 1 {
            let bytes_arg = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named(crate::Syntax::TYPE_BYTE_BUFFER.to_string()),
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(bytes_arg),
                    op: TBuiltinOp::ByteBufferFrom,
                    args: vec![],
                },
            };
        }
        // D-COLLBREADTH1=A: `Deque.new()` → empty VecDeque with elem type from sema.
        // The element type comes from `resolved_ret` (sema filled it from the annotation).
        if type_name == "Deque" && method == "new" && args.is_empty() {
            let elem_ty = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => targs[0].clone(),
                _ => Type::Int,
            };
            let elem_rust = cx.rust_type(&elem_ty);
            let deque_ty = Type::Apply {
                name: "Deque".to_string(),
                args: vec![elem_ty],
            };
            return TExpr {
                ty: deque_ty,
                kind: TExprKind::StaticCall {
                    type_prefix: format!("std::collections::VecDeque::<{}>", elem_rust),
                    method_rust: "new".to_string(),
                    args: vec![],
                },
            };
        }
        // D-TAG1: `Bag.new()` → empty HashMap with elem type from sema.
        if type_name == "Bag" && method == "new" && args.is_empty() {
            let elem_ty = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => targs[0].clone(),
                _ => Type::Int,
            };
            let elem_rust = cx.rust_type(&elem_ty);
            let bag_ty = Type::Apply {
                name: "Bag".to_string(),
                args: vec![elem_ty],
            };
            return TExpr {
                ty: bag_ty,
                kind: TExprKind::StaticCall {
                    type_prefix: format!("std::collections::HashMap::<{}, usize>", elem_rust),
                    method_rust: "new".to_string(),
                    args: vec![],
                },
            };
        }
        // D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>.new()` → an empty `JetPool<T>`.
        // The element type comes from `resolved_ret` (sema filled it from the
        // call-site turbofish or the binding's annotation).
        if type_name == "Pool" && method == "new" && args.is_empty() {
            let elem_ty = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => targs[0].clone(),
                _ => Type::Int,
            };
            let elem_rust = cx.rust_type(&elem_ty);
            let pool_ty = Type::Apply {
                name: "Pool".to_string(),
                args: vec![elem_ty],
            };
            return TExpr {
                ty: pool_ty,
                kind: TExprKind::StaticCall {
                    type_prefix: format!("{}jet_std::JetPool::<{}>", cx.root_prefix, elem_rust),
                    method_rust: "new".to_string(),
                    args: vec![],
                },
            };
        }
        // D-MEM1 S6 (D-SHARED-API1=A): `Shared.new(x)` → a `JetShared<T>`
        // wrapping `x`; `T` is `x`'s own lowered type (no turbofish, no
        // annotation needed — the argument alone fixes it).
        if type_name == "Shared" && method == "new" && args.len() == 1 {
            let arg_t = lower_expr(&args[0].expr, cx, env);
            let elem_ty = arg_t.ty.clone();
            let elem_rust = cx.rust_type(&elem_ty);
            return TExpr {
                ty: Type::Shared(Box::new(elem_ty)),
                kind: TExprKind::StaticCall {
                    type_prefix: format!("{}jet_std::JetShared::<{}>", cx.root_prefix, elem_rust),
                    method_rust: "new".to_string(),
                    args: vec![TCallArg {
                        value: arg_t,
                        borrow: false,
                        mut_borrow: false,
                        clone: false,
                        arc_clone: false,
                        fn_coerce: None,
                        widen_to_vec: false,
                    }],
                },
            };
        }
        // D-HOLE1: `Option.lift2(f, a, b)` → apply `f` to both payloads only when
        // both are present. `a`/`b` lower plainly as values; `R` comes from sema's
        // `resolved_ret` (the arg-dependent return type, same mechanism the
        // polymorphic core specials use — see the AST `MethodCall.resolved_ret` doc).
        if type_name == "Option" && method == "lift2" && !cx.type_names.contains("Option") {
            let a_ty = match tir_recv_jet_ty(&args[1].expr, env) {
                Some(Type::Option(inner)) => (*inner).clone(),
                _ => Type::Int,
            };
            let b_ty = match tir_recv_jet_ty(&args[2].expr, env) {
                Some(Type::Option(inner)) => (*inner).clone(),
                _ => Type::Int,
            };
            // c142: a bare `f` lambda is called immediately here (inside the
            // `.zip().map(...)` emit below), not stored — but rustc still needs its
            // param types written out whenever the body resolves a trait method on a
            // param (e.g. interpolation's `.jet_display()`), the same reason
            // `lower_one_call_arg` annotates a lambda flowing into a fn-typed
            // parameter. Reuse that mechanism with `a`/`b`'s payload types as the
            // expected params.
            let f_t = match &args[0].expr {
                Expr::Lambda(lam) => {
                    let tl = lower_lambda_expecting(lam, cx, env, Some(&[a_ty, b_ty]));
                    TExpr {
                        ty: Type::Fn {
                            params: Vec::new(),
                            ret: None,
                            effect_bound: None,
                        },
                        kind: TExprKind::Lambda(Box::new(tl)),
                    }
                }
                _ => lower_expr(&args[0].expr, cx, env),
            };
            let a_t = lower_expr(&args[1].expr, cx, env);
            let b_t = lower_expr(&args[2].expr, cx, env);
            let ret_ty = match resolved_ret {
                Some(Type::Option(inner)) => (**inner).clone(),
                _ => Type::Int,
            };
            return TExpr {
                ty: Type::Option(Box::new(ret_ty)),
                kind: TExprKind::OptionLift2 {
                    f: Box::new(f_t),
                    a: Box::new(a_t),
                    b: Box::new(b_t),
                },
            };
        }
        // D-SIMD2 / D-LINALG1: a static method on a built-in math type → the prelude
        // free function `{root}jet_math_<T>_<method>(args)`.
        if crate::Sema::is_math_type(&type_name) && !cx.type_names.contains(&type_name) {
            if let Some(ret) = crate::Sema::math_static_return(&type_name, method, args.len()) {
                let bridge = crate::Sema::math_static_arg_ty(&type_name, method);
                let targs: Vec<TExpr> = args
                    .iter()
                    .map(|a| {
                        let mut t = lower_expr(&a.expr, cx, env);
                        // D-FIXARR1 bridge: a `[..]` literal arg to `from_array` lowered
                        // as a growable list; re-tag it to the `[T#N]` fixed array so emit
                        // produces `[e1, …]` (a Rust stack array), not `vec![…]`.
                        if let Some(fl @ Type::FixedList { .. }) = &bridge {
                            if matches!(t.ty, Type::List(_))
                                && matches!(t.kind, TExprKind::ListLit(_))
                            {
                                t.ty = fl.clone();
                            }
                        }
                        t
                    })
                    .collect();
                return TExpr {
                    ty: ret,
                    kind: TExprKind::MathBuiltin {
                        type_name: type_name.clone(),
                        func: method.to_string(),
                        args: targs,
                    },
                };
            }
        }
        let sig = cx
            .method_sigs
            .get(&(type_name.clone(), method.to_string()))
            .cloned()
            .unwrap_or_default();
        let targs = lower_method_args(args, &sig, env, cx);
        let ret_ty = cx
            .method_rets
            .get(&(type_name.clone(), method.to_string()))
            .cloned()
            .flatten()
            .map(|t| resolve_self_ty(&t, &type_name))
            .unwrap_or_else(unit_type);
        return TExpr {
            ty: ret_ty,
            kind: TExprKind::StaticCall {
                // The AST path uses `cx.type_prefix(type_name)` = `user_<T>`.
                type_prefix: cx.type_prefix(&type_name),
                method_rust: mangle(method),
                args: targs,
            },
        };
    }
    // c109 Phase 30: DYNAMIC dispatch on a TRAIT-OBJECT receiver (`s.name()`/`s.area()`,
    // `s: Box<dyn user_Shape>`). The gate proved `recv_type == Some(<trait>)` with the
    // trait in `cx.trait_names`. The AST `emit_method_call` (Expression.rs ~L1657) emits
    // `({recv}).{method}({args})` — the BARE (unmangled) method name (vtable dispatch),
    // node (`({recv}).{method_rust}({args})`) with the bare method name. Trait declarations
    // retain their full parameter/return facts in `Cx`, so dynamic calls use the same
    // convention-aware argument lowering as static calls.
    if let Some(ty) = recv_type {
        if cx.trait_names.contains(ty) {
            let key = (ty.clone(), method.to_string());
            let sig = cx.method_sigs.get(&key).cloned().unwrap_or_default();
            let ret_ty = cx
                .method_rets
                .get(&key)
                .cloned()
                .flatten()
                .unwrap_or_else(unit_type);
            let recv = lower_expr(receiver, cx, env);
            let targs = lower_method_args(args, &sig, env, cx);
            return TExpr {
                ty: ret_ty,
                kind: TExprKind::MethodCall {
                    recv: Box::new(recv),
                    method_rust: method.to_string(),
                    args: targs,
                },
            };
        }
    }
    // A user instance method on a covered type. `recv_type` is total (gate proved
    // `Some`). Resolve the param conventions from `method_sigs` and the Rust method
    // name (trait-impl methods keep their bare name; others get the `user_` mangle).
    let ty_name = recv_type.clone().expect("gate proved recv_type is Some");
    let sig = cx
        .method_sigs
        .get(&(ty_name.clone(), method.to_string()))
        .cloned()
        .unwrap_or_default();
    let recv = if matches!(
        cx.method_self_convs.get(&(ty_name.clone(), method.to_string())),
        Some(AccessConvention::Move)
    )
        && matches!(receiver, Expr::Ident(name, _) if env.is_resource(name))
    {
        lower_owned_expr(receiver, cx, env)
    } else {
        lower_expr(receiver, cx, env)
    };
    let targs = lower_method_args(args, &sig, env, cx);
    // S62: a trait-impl method is called by its bare name (the trait impl owns it);
    // a plain user method is `user_<method>`. This mirrors `emit_method_call`'s
    // `trait_methods` check exactly — decided here, total, never re-derived in emit.
    let method_rust = if cx
        .trait_methods
        .contains(&(ty_name.clone(), method.to_string()))
    {
        method.to_string()
    } else {
        mangle(method)
    };
    // The result type, read from the resolved method return (total fact). It is
    // rarely load-bearing in emit (a binding carries sema's `b.ty`; arithmetic on a
    // method result doesn't trap — matching the AST `expr_jet_ty`/`operand_is_integer`),
    // but the TIR keeps it total per the design principle.
    let ret_ty = cx
        .method_rets
        .get(&(ty_name.clone(), method.to_string()))
        .cloned()
        .flatten()
        .unwrap_or_else(unit_type);
    TExpr {
        ty: ret_ty,
        kind: TExprKind::MethodCall {
            recv: Box::new(recv),
            method_rust,
            args: targs,
        },
    }
}
