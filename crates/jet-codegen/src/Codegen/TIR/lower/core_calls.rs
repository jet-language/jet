use crate::AST::{Expr, Type};
use crate::Codegen::Cx;
use crate::Codegen::TIR::core_closure_call_return_ty;
use crate::Codegen::TIR::lower_lambda_expecting_value;
use crate::Codegen::TIR::lambda_body_ty;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_lambda;
use crate::Codegen::TIR::lower_spawn_lambda_for_jit;
use crate::Codegen::TIR::render_lambda_str;
use crate::Codegen::TIR::render_lambda_str_expecting_value;
use crate::Codegen::TIR::render_spawn_lambda;
use crate::Codegen::TIR::TCoreClosureKind;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::unit_type;
use crate::Diagnostics::Span;
use std::collections::HashMap;

/// c109 Phase 13: lower a closure-taking core call (`tasks.spawn`/`http.serve`/
/// `scope.guard`) into a bespoke `CoreClosureCall` node, reproducing `emit_core_call`
/// (Source/Codegen/Expression.rs) byte-for-byte. Returns `None` when `(module,
/// method)` isn't one of the three (so the caller falls through to the plain
/// `CoreCall`). The gate (`core_closure_call_in_subset`) already proved a literal
/// in-subset lambda in the closure-arg position.
pub(super) fn core_module_path_from_receiver(
    receiver: &Expr,
    imports: &HashMap<String, String>,
    env: &LowerEnv,
) -> Option<String> {
    match receiver {
        Expr::Ident(alias, _) if !env.locals.contains_key(alias) => imports.get(alias).cloned(),
        Expr::Field(base, leaf, _) => {
            let module = core_module_path_from_receiver(base, imports, env)?;
            let submodule = format!("{module}.{leaf}");
            crate::Syntax::is_known_core_module(&submodule).then_some(submodule)
        }
        _ => None,
    }
}

pub(crate) fn lower_core_closure_call(
    module: &str,
    method: &str,
    source_span: Span,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    let lam_at = |i: usize| match args.get(i).map(|a| &a.expr) {
        Some(Expr::Lambda(lam)) => Some(lam),
        _ => None,
    };
    let data_err = || Type::Named("DataError".to_string());
    let data_checked = jet_foundation::PackageEdition::edition_at_least(&cx.package_edition, "2027");
    let wrap_data = |ok: Type| -> Type {
        if data_checked {
            Type::Result {
                ok: Box::new(ok),
                err: Box::new(data_err()),
            }
        } else {
            ok
        }
    };
    let data_row_ty = |ty: &Type| -> Type {
        match ty {
            Type::List(inner) => (**inner).clone(),
            Type::Apply { name, args } if name == "DataStream" && args.len() == 1 => args[0].clone(),
            _ => Type::Int,
        }
    };
    let kind = match (module, method) {
        ("core.tasks", "spawn") => {
            let lam = lam_at(0)?;
            // The spawned body's type (the lambda's return) is the Task's element type.
            let body_ty = lambda_body_ty(lam, cx, env);
            let jit_lambda = lower_spawn_lambda_for_jit(lam, cx, env);
            cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
            let spawn_closure = render_spawn_lambda(lam, cx, env);
            return Some(TExpr {
                ty: core_closure_call_return_ty(module, method, body_ty),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::Spawn { spawn_closure },
                },
            });
        }
        ("jet.http", "serve") => {
            let lam = lam_at(1)?;
            let addr = lower_expr(&args[0].expr, cx, env);
            let closure = render_lambda_str_expecting_value(
                lam,
                cx,
                env,
                &[Type::Named("HttpRequest".to_string())],
            );
            TCoreClosureKind::Serve {
                addr: Box::new(addr),
                closure,
            }
        }
        ("core.scope", "guard") => {
            let lam = lam_at(0)?;
            let executable = Box::new(lower_lambda(lam, cx, env));
            let closure = render_lambda_str(lam, cx, env);
            TCoreClosureKind::Guard {
                closure,
                executable,
            }
        }
        ("core.data", "filter" | "sort_by") => {
            let rows = lower_expr(&args[0].expr, cx, env);
            let row_ty = data_row_ty(&rows.ty);
            let pred = lower_lambda_expecting_value(lam_at(1)?, cx, env, &[row_ty.clone()]);
            let list = Type::List(Box::new(row_ty));
            return Some(TExpr {
                ty: if method == "sort_by" { wrap_data(list) } else { list },
                kind: TExprKind::CoreCall {
                    module: module.to_string(),
                    method: method.to_string(),
                    args: vec![
                        rows,
                        TExpr {
                            ty: Type::Named("Unit".to_string()),
                            kind: TExprKind::Lambda(Box::new(pred)),
                        },
                    ],
                    source_span,
                    widen_to_vec: vec![false, false],
                },
            });
        }
        ("core.data", "group_count") => {
            let rows = lower_expr(&args[0].expr, cx, env);
            let row_ty = data_row_ty(&rows.ty);
            let key = lower_lambda_expecting_value(lam_at(1)?, cx, env, &[row_ty]);
            return Some(TExpr {
                ty: wrap_data(Type::List(Box::new(Type::Named("DataGroup".to_string())))),
                kind: TExprKind::CoreCall {
                    module: module.to_string(),
                    method: method.to_string(),
                    args: vec![
                        rows,
                        TExpr {
                            ty: Type::Named("Unit".to_string()),
                            kind: TExprKind::Lambda(Box::new(key)),
                        },
                    ],
                    source_span,
                    widen_to_vec: vec![false, false],
                },
            });
        }
        ("core.data", "group_sum" | "group_mean") => {
            let rows = lower_expr(&args[0].expr, cx, env);
            let row_ty = data_row_ty(&rows.ty);
            let key = lower_lambda_expecting_value(lam_at(1)?, cx, env, &[row_ty.clone()]);
            let value = lower_lambda_expecting_value(lam_at(2)?, cx, env, &[row_ty]);
            return Some(TExpr {
                ty: wrap_data(Type::List(Box::new(Type::Named("DataGroup".to_string())))),
                kind: TExprKind::CoreCall {
                    module: module.to_string(),
                    method: method.to_string(),
                    args: vec![
                        rows,
                        TExpr {
                            ty: Type::Named("Unit".to_string()),
                            kind: TExprKind::Lambda(Box::new(key)),
                        },
                        TExpr {
                            ty: Type::Named("Unit".to_string()),
                            kind: TExprKind::Lambda(Box::new(value)),
                        },
                    ],
                    source_span,
                    widen_to_vec: vec![false, false, false],
                },
            });
        }
        ("core.data", "inner_join" | "left_join") => {
            let left = lower_expr(&args[0].expr, cx, env);
            let right = lower_expr(&args[1].expr, cx, env);
            let left_ty = match &left.ty {
                Type::List(inner) => (**inner).clone(),
                _ => Type::Int,
            };
            let right_ty = match &right.ty {
                Type::List(inner) => (**inner).clone(),
                _ => Type::Int,
            };
            let left_key = lower_lambda_expecting_value(lam_at(2)?, cx, env, &[left_ty.clone()]);
            let right_key = lower_lambda_expecting_value(lam_at(3)?, cx, env, &[right_ty.clone()]);
            let joined_right = if method == "left_join" {
                Type::Option(Box::new(right_ty))
            } else {
                right_ty
            };
            return Some(TExpr {
                ty: wrap_data(Type::List(Box::new(Type::Apply {
                    name: "DataJoin".to_string(),
                    args: vec![left_ty, joined_right],
                }))),
                kind: TExprKind::CoreCall {
                    module: module.to_string(),
                    method: method.to_string(),
                    args: vec![
                        left,
                        right,
                        TExpr {
                            ty: Type::Named("Unit".to_string()),
                            kind: TExprKind::Lambda(Box::new(left_key)),
                        },
                        TExpr {
                            ty: Type::Named("Unit".to_string()),
                            kind: TExprKind::Lambda(Box::new(right_key)),
                        },
                    ],
                    source_span,
                    widen_to_vec: vec![false, false, false, false],
                },
            });
        }
        ("core.data", "pivot_sum") => {
            let rows = lower_expr(&args[0].expr, cx, env);
            let row_ty = data_row_ty(&rows.ty);
            let row_key = lower_lambda_expecting_value(lam_at(1)?, cx, env, &[row_ty.clone()]);
            let col_key = lower_lambda_expecting_value(lam_at(2)?, cx, env, &[row_ty.clone()]);
            let value = lower_lambda_expecting_value(lam_at(3)?, cx, env, &[row_ty]);
            let cell = if data_checked {
                Type::Named("DataPivotCell".to_string())
            } else {
                Type::Named("DataGroup".to_string())
            };
            return Some(TExpr {
                ty: wrap_data(Type::List(Box::new(cell))),
                kind: TExprKind::CoreCall {
                    module: module.to_string(),
                    method: method.to_string(),
                    args: vec![
                        rows,
                        TExpr {
                            ty: Type::Named("Unit".to_string()),
                            kind: TExprKind::Lambda(Box::new(row_key)),
                        },
                        TExpr {
                            ty: Type::Named("Unit".to_string()),
                            kind: TExprKind::Lambda(Box::new(col_key)),
                        },
                        TExpr {
                            ty: Type::Named("Unit".to_string()),
                            kind: TExprKind::Lambda(Box::new(value)),
                        },
                    ],
                    source_span,
                    widen_to_vec: vec![false, false, false, false],
                },
            });
        }
        ("core.data", "lazy_filter" | "lazy_sort_by") => {
            let frame = lower_expr(&args[0].expr, cx, env);
            let row_ty = match &frame.ty {
                Type::Apply { name, args } if name == "LazyFrame" && args.len() == 1 => {
                    args[0].clone()
                }
                _ => Type::Int,
            };
            let closure = lower_lambda_expecting_value(lam_at(1)?, cx, env, &[row_ty.clone()]);
            return Some(TExpr {
                ty: Type::Apply {
                    name: "LazyFrame".to_string(),
                    args: vec![row_ty],
                },
                kind: TExprKind::CoreCall {
                    module: module.to_string(),
                    method: method.to_string(),
                    args: vec![
                        frame,
                        TExpr {
                            ty: Type::Named("Unit".to_string()),
                            kind: TExprKind::Lambda(Box::new(closure)),
                        },
                    ],
                    source_span,
                    widen_to_vec: vec![false, false],
                },
            });
        }
        // D-REACT1=B: the `derived` closure's body type is the `Derived<T>` element.
        ("jet.reactive", "derived") => {
            let lam = lam_at(0)?;
            let body_ty = lambda_body_ty(lam, cx, env);
            let closure = render_lambda_str(lam, cx, env);
            let executable = Box::new(lower_lambda(lam, cx, env));
            // Captured signals need the spawn-lambda ABI (explicit capture params).
            let jit_lambda = lower_spawn_lambda_for_jit(lam, cx, env);
            cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
            return Some(TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_DERIVED.to_string(),
                    args: vec![body_ty],
                },
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::ReactiveDerived {
                        closure,
                        executable,
                    },
                },
            });
        }
        ("jet.reactive", "effect") => {
            let lam = lam_at(0)?;
            let closure = render_lambda_str(lam, cx, env);
            let executable = Box::new(lower_lambda(lam, cx, env));
            let jit_lambda = lower_spawn_lambda_for_jit(lam, cx, env);
            cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
            TCoreClosureKind::ReactiveEffect { closure, executable }
        }
        // D-SIGNAL1: `computed` is a canonical alias for `derived`.
        ("jet.reactive", "computed") => {
            let lam = lam_at(0)?;
            let body_ty = lambda_body_ty(lam, cx, env);
            let closure = render_lambda_str(lam, cx, env);
            let executable = Box::new(lower_lambda(lam, cx, env));
            let jit_lambda = lower_spawn_lambda_for_jit(lam, cx, env);
            cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
            return Some(TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_COMPUTED.to_string(),
                    args: vec![body_ty],
                },
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::ReactiveDerived {
                        closure,
                        executable,
                    },
                },
            });
        }
        // D-RENDERTGT2=A (c133 M2): reactive UI render loop through the backend seam.
        ("core.ui", "reactive_render") => {
            let lam = lam_at(0)?;
            let closure = render_lambda_str(lam, cx, env);
            let executable = Box::new(lower_lambda(lam, cx, env));
            let jit_lambda = lower_spawn_lambda_for_jit(lam, cx, env);
            cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
            TCoreClosureKind::UiReactiveRender { closure, executable }
        }
        _ => return None,
    };
    Some(TExpr {
        ty: core_closure_call_return_ty(module, method, unit_type()),
        kind: TExprKind::CoreClosureCall { kind },
    })
}
