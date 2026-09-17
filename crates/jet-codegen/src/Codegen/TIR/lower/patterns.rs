use crate::Codegen::mangle_generated;
use crate::Codegen::Cx;
use crate::Codegen::TIR::arm_fallible_pattern;
use crate::Codegen::TIR::arm_head_range;
use crate::Codegen::TIR::arm_variant_pattern;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::fork_panic;
use crate::Codegen::TIR::lower::{deferred_stmt, LowerBody, LowerStmtPlan};
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::struct_field_type;
use crate::Codegen::TIR::unit_type;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::TEnumArg;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::THandleOp;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TMatchArm;
use crate::Codegen::TIR::TPattern;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::variant_binding_types_for_enum;
use crate::AST::{BinOp, Expr, PatSlot, Pattern, Stmt, SwitchArm, Type, VariantPayload};

/// D-SHIFT1 (c7shift): lower `cursor.take_pattern("…")`. Builds the
/// `(name, type)` canonical hole list the SAME way sema did when it set this
/// call's `resolved_ret` (untyped hole binds `String`), so the
/// `JetTup_<hash>` struct `collect_tuple_shapes_from_expr` already
/// registered from `resolved_ret` matches the one this op constructs
/// (`tuple_struct_name` is a pure hash of the (name, type) list — same
/// input, same name, both computed independently rather than threaded
/// through, matching how `handle_method_return_ty` independently re-derives
/// sema's tables elsewhere in this file per the project's I3 design).
pub(super) fn lower_cursor_take_pattern(
    receiver: &Expr,
    parts: &[crate::AST::StrMatchPart],
    cx: &Cx,
    env: &mut LowerEnv,
    lowered_receiver: Option<TExpr>,
) -> TExpr {
    let recv_t = lowered_receiver.unwrap_or_else(|| lower_expr(receiver, cx, env));
    let mut canonical = Vec::new();
    for part in parts {
        let crate::AST::StrMatchPart::Hole { name, ty, span } = part else {
            continue;
        };
        let ty = match ty {
            None => Type::String,
            Some(t @ (Type::Int | Type::Float | Type::Bool | Type::String | Type::InlineRange { .. })) => {
                t.clone()
            }
            Some(_) => {
                return invariant_expr(
                    "cursor.take_pattern hole type",
                    *span,
                    Type::Result {
                        ok: Box::new(unit_type()),
                        err: Box::new(Type::String),
                    },
                );
            }
        };
        canonical.push((name.clone(), ty));
    }
    let ok_ty = if canonical.is_empty() {
        unit_type()
    } else {
        Type::Tuple(
            canonical
                .iter()
                .map(|(n, t)| (n.clone(), Box::new(t.clone())))
                .collect(),
        )
    };
    TExpr {
        ty: Type::Result {
            ok: Box::new(ok_ty),
            err: Box::new(Type::String),
        },
        kind: TExprKind::HandleMethod {
            recv: Box::new(recv_t),
            op: THandleOp::CursorTakePattern {
                parts: parts.to_vec(),
                canonical,
            },
            args: vec![],
        },
    }
}

/// D-BINPAT1 (card #506 follow-up): lower `reader.take_pattern([U8]{"…"})` — the
/// byte-mode sibling of `lower_cursor_take_pattern` immediately above. Builds
/// the SAME canonical hole list the same way sema did when it set this call's
/// `resolved_ret` (`bin_match_hole_types`), so the `JetTup_<hash>` struct
/// `collect_tuple_shapes_from_expr` already registered matches the one this
/// op constructs.
pub(super) fn lower_reader_take_pattern(
    receiver: &Expr,
    parts: &[crate::AST::BinMatchPart],
    cx: &Cx,
    env: &mut LowerEnv,
    lowered_receiver: Option<TExpr>,
) -> TExpr {
    use crate::AST::{BinMatchPart, BinSpec};
    let recv_t = lowered_receiver.unwrap_or_else(|| lower_expr(receiver, cx, env));
    let canonical: Vec<(String, Type)> = parts
        .iter()
        .filter_map(|p| match p {
            BinMatchPart::Hole { name, spec, .. } => {
                let ty = match spec {
                    BinSpec::Rest => Type::List(Box::new(Type::IntN {
                        signed: false,
                        bits: 8,
                    })),
                    BinSpec::Bits { width, .. } => {
                        crate::Codegen::TIR::lower::bin_bits_type(*width)
                    }
                };
                Some((name.clone(), ty))
            }
            BinMatchPart::Lit(_) => None,
        })
        .collect();
    let ok_ty = if canonical.is_empty() {
        unit_type()
    } else {
        Type::Tuple(
            canonical
                .iter()
                .map(|(n, t)| (n.clone(), Box::new(t.clone())))
                .collect(),
        )
    };
    TExpr {
        ty: Type::Result {
            ok: Box::new(ok_ty),
            err: Box::new(Type::String),
        },
        kind: TExprKind::HandleMethod {
            recv: Box::new(recv_t),
            op: THandleOp::ReaderTakePattern {
                parts: parts.to_vec(),
                canonical,
            },
            args: vec![],
        },
    }
}

/// D-PARSESTR1: the bool test for a str-match arm head — whether the scan
/// closure succeeds. Always refutable (E0148 requires an `else` whenever this
/// pattern appears in an if-table, sema requires an `else`).
pub(super) fn str_match_pattern_cond_expr(pattern: &Pattern, subject: TExpr, _cx: &Cx) -> TExpr {
    let Pattern::StrMatch { parts, .. } = pattern else {
        return invariant_expr(
            "string-match condition pattern",
            pattern.span(),
            Type::Bool,
        );
    };
    TExpr {
        ty: Type::Bool,
        kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::StrMatchScan {
            subject: Box::new(subject),
            parts: parts.clone(),
            probe: crate::Codegen::TIR::TMatchProbe::IsSome,
        })),
    }
}

/// D-PARSESTR1: bind each hole locally before the arm body runs. Re-invokes
/// the same scan closure text as the cond (pure/cheap — `starts_with`/`find`/
/// `.parse()` — matching how struct-pattern value tests and bind fields are
/// independently re-derived rather than shared), binds the whole result tuple
/// to one temp, then projects each hole out of it by field index.
pub(super) fn lower_str_match_pattern_bindings(
    pattern: &Pattern,
    subject: TExpr,
    _cx: &Cx,
    env: &mut LowerEnv,
) -> Vec<TStmt> {
    let Pattern::StrMatch { parts, .. } = pattern else {
        return vec![TStmt::InvariantViolation {
            construct: "string-match binding pattern".to_string(),
            span: pattern.span(),
        }];
    };
    let holes: Vec<(String, Type)> = parts
        .iter()
        .filter_map(|part| match part {
            crate::AST::StrMatchPart::Hole { name, ty, .. } => {
                Some((name.clone(), ty.clone().unwrap_or(Type::String)))
            }
            crate::AST::StrMatchPart::Lit(_) => None,
        })
        .collect();
    if holes.is_empty() {
        return Vec::new();
    }
    let tuple_local = mangle_generated("sm_tuple");
    let tuple_ty = Type::Tuple(
        holes
            .iter()
            .map(|(n, t)| (n.clone(), Box::new(t.clone())))
            .collect(),
    );
    let mut out = vec![TStmt::Let {
        name: tuple_local.to_string(),
        kw: "let",
        let_ty: crate::Codegen::TIR::let_ty_tuple(holes.iter().map(|(_, t)| t.clone()).collect()),
        init: TExpr {
            ty: tuple_ty.clone(),
            kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::StrMatchScan {
                subject: Box::new(subject),
                parts: parts.clone(),
                probe: crate::Codegen::TIR::TMatchProbe::Unwrap,
            })),
        },
        gc_promotion: None,
        gc_transferred: false,
    }];
    for (i, (name, ty)) in holes.iter().enumerate() {
        env.bind(name, TLocal::user(name), Some(ty.clone()));
        out.push(TStmt::Let {
            name: name.clone(),
            kw: "let",
            let_ty: crate::Codegen::TIR::TLetTy::plain(ty.clone()),
            init: TExpr {
                ty: ty.clone(),
                kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::TupleIndex {
                    base: Box::new(TExpr {
                        ty: tuple_ty.clone(),
                        kind: TExprKind::Local(TLocal::user(&tuple_local)),
                    }),
                    index: i,
                })),
            },
            gc_promotion: None,
            gc_transferred: false,
        });
    }
    out
}

/// D-BINPAT1 (card #506): the bool test for a binary-pattern arm head —
/// whether the bit-scan closure succeeds. Always refutable (E0148).
pub(super) fn bin_match_pattern_cond_expr(pattern: &Pattern, subject: TExpr, _cx: &Cx) -> TExpr {
    let Pattern::BinMatch { parts, .. } = pattern else {
        return invariant_expr(
            "binary-match condition pattern",
            pattern.span(),
            Type::Bool,
        );
    };
    TExpr {
        ty: Type::Bool,
        kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::BinMatchScan {
            subject: Box::new(subject),
            parts: parts.clone(),
            probe: crate::Codegen::TIR::TMatchProbe::IsSome,
        })),
    }
}
/// D-BINPAT1: bind each hole locally before the arm body runs — re-invokes the
/// bit-scan closure (pure/cheap), binds the whole result tuple to one temp,
/// then projects each hole out by index. Mirrors `lower_str_match_pattern_bindings`.
pub(super) fn lower_bin_match_pattern_bindings(
    pattern: &Pattern,
    subject: TExpr,
    _cx: &Cx,
    env: &mut LowerEnv,
) -> Vec<TStmt> {
    let Pattern::BinMatch { parts, .. } = pattern else {
        return vec![TStmt::InvariantViolation {
            construct: "binary-match binding pattern".to_string(),
            span: pattern.span(),
        }];
    };
    let holes: Vec<(String, Type)> = parts
        .iter()
        .filter_map(|part| match part {
            crate::AST::BinMatchPart::Hole { name, spec, .. } => {
                let ty = match spec {
                    crate::AST::BinSpec::Rest => Type::List(Box::new(Type::IntN {
                        signed: false,
                        bits: 8,
                    })),
                    crate::AST::BinSpec::Bits { width, .. } => super::bin_bits_type(*width),
                };
                Some((name.clone(), ty))
            }
            crate::AST::BinMatchPart::Lit(_) => None,
        })
        .collect();
    if holes.is_empty() {
        return Vec::new();
    }
    let tuple_local = mangle_generated("bm_tuple");
    let tuple_ty = Type::Tuple(
        holes
            .iter()
            .map(|(n, t)| (n.clone(), Box::new(t.clone())))
            .collect(),
    );
    let mut out = vec![TStmt::Let {
        name: tuple_local.to_string(),
        kw: "let",
        let_ty: crate::Codegen::TIR::let_ty_tuple(holes.iter().map(|(_, t)| t.clone()).collect()),
        init: TExpr {
            ty: tuple_ty.clone(),
            kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::BinMatchScan {
                subject: Box::new(subject),
                parts: parts.clone(),
                probe: crate::Codegen::TIR::TMatchProbe::Unwrap,
            })),
        },
        gc_promotion: None,
        gc_transferred: false,
    }];
    for (i, (name, ty)) in holes.iter().enumerate() {
        env.bind(name, TLocal::user(name), Some(ty.clone()));
        out.push(TStmt::Let {
            name: name.clone(),
            kw: "let",
            let_ty: crate::Codegen::TIR::TLetTy::plain(ty.clone()),
            init: TExpr {
                ty: ty.clone(),
                kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::TupleIndex {
                    base: Box::new(TExpr {
                        ty: tuple_ty.clone(),
                        kind: TExprKind::Local(TLocal::user(&tuple_local)),
                    }),
                    index: i,
                })),
            },
            gc_promotion: None,
            gc_transferred: false,
        });
    }
    out
}

pub(super) fn struct_pattern_field_type(cx: &Cx, subject_ty: &Type, field: &str) -> Option<Type> {
    match subject_ty {
        Type::Apply { name, .. } => cx
            .struct_fields
            .get(name)?
            .iter()
            .find(|(f, _)| f == field)
            .map(|(_, t)| t.clone()),
        _ => struct_field_type(cx, subject_ty, field),
    }
}

pub(super) fn bool_and_chain(mut tests: Vec<TExpr>) -> TExpr {
    let Some(mut acc) = tests.pop() else {
        return TExpr {
            ty: Type::Bool,
            kind: TExprKind::BoolLit(true),
        };
    };
    while let Some(next) = tests.pop() {
        acc = TExpr {
            ty: Type::Bool,
            kind: TExprKind::Binary {
                op: BinOp::And,
                overflow: false,
                line: 0,
                lhs: Box::new(next),
                rhs: Box::new(acc),
            },
        };
    }
    acc
}

fn invariant_expr(
    construct: impl Into<String>,
    span: crate::Diagnostics::Span,
    ty: Type,
) -> TExpr {
    TExpr {
        ty,
        kind: TExprKind::InvariantViolation {
            construct: construct.into(),
            span,
        },
    }
}

fn invariant_stmt(construct: impl Into<String>, span: crate::Diagnostics::Span) -> TStmt {
    TStmt::InvariantViolation {
        construct: construct.into(),
        span,
    }
}

pub(super) fn resolved_enum_subject_type(cx: &Cx, subject_ty: &Type) -> Result<String, &'static str> {
    match subject_ty.without_user_tags() {
        Type::Union(members) if !members.is_empty() => Ok(crate::AST::union_enum_name(members)),
        Type::Union(_) => Err("empty union enum subject"),
        Type::Named(name) | Type::Apply { name, .. } => {
            Ok(crate::Codegen::TIR::canonical_enum_owner(cx, name))
        }
        _ => Err("enum match subject is not an enum type"),
    }
}


/// c109 Phase 8: lower a fallible/optional pattern match (`when … { it == Ok(n) ->
/// … }`). Reuses the `EnumMatch` TStmt — the scrutinee is the subject's emitted form
/// (a covered fallible/optional value: a user fallible fn call, an optional local,
/// etc.; no by-reference clone arises since those subjects are not deref'd enum
/// params), and each arm's pattern is the Rust `Ok(b)`/`Err(b)`/`Some(b)`/`None`,
/// mirroring `emit_match_pattern`. Binding payload types come from the subject's
/// resolved Result/Option type (totality), reproducing `add_pattern_bindings`.
pub(crate) fn lower_fallible_match<'a>(
    subject: &'a Expr,
    arms: &'a [SwitchArm],
    else_body: &'a Option<Vec<Stmt>>,
    cx: &'a Cx,
    env: &mut LowerEnv,
) -> LowerStmtPlan<'a> {
    // Preserve a fallible core/helper call as its Result/Option carrier while
    // lowering the scrutinee. Normal value lowering consumes that carrier via
    // `Try`, but an outcome pattern needs to inspect it directly.
    let subject_t = {
        let _fallible_match_cache_scope = super::expressions::ExprCacheScope::enter();
        let fallback_subject = env.fallback_subject;
        env.fallback_subject = true;
        let subject_t = lower_expr(subject, cx, env);
        env.fallback_subject = fallback_subject;
        subject_t
    };
    let subject_ty = subject_t.ty.clone();
    let is_option = match subject_ty.without_user_tags() {
        Type::Option(_) => true,
        Type::Result { .. } => false,
        _ => {
            return LowerStmtPlan::ready(invariant_stmt(
                "fallible match subject type",
                subject.span(),
            ));
        }
    };
    let subject_span = subject.span();
    let (scrutinee, clone_subject) = match subject {
        Expr::Ident(name, _) if env.is_borrowed(name) => {
            let mut by_value = env.local_of(name);
            by_value.deref = false;
            (
                TExpr {
                    ty: subject_ty.clone(),
                    kind: TExprKind::Local(by_value),
                },
                true,
            )
        }
        _ => (subject_t, false),
    };
    if arms.is_empty() {
        return LowerStmtPlan::ready(invariant_stmt(
            "fallible match has no arms",
            subject_span,
        ));
    }
    let mut tarms = Vec::with_capacity(arms.len());
    let mut bodies = Vec::with_capacity(arms.len() + usize::from(else_body.is_some()));
    for arm in arms {
        let Some(pattern) = arm_fallible_pattern(cx, &arm.cond, subject) else {
            return LowerStmtPlan::ready(invariant_stmt(
                "fallible match arm pattern",
                arm.span,
            ));
        };
        let mut body_env = fork_panic(env);
        if let Err(reason) = tir_add_fallible_binding(&pattern, &mut body_env, &subject_ty) {
            return LowerStmtPlan::ready(invariant_stmt(reason, pattern.span()));
        }
        let tir_pattern = if is_option {
            TPattern::option_binding_with_values(pattern, |value| lower_expr(value, cx, env))
        } else {
            TPattern::binding_with_values(pattern, |value| lower_expr(value, cx, env))
        };
        tarms.push(tir_pattern);
        bodies.push(LowerBody::scoped(&arm.body, body_env));
    }
    let has_else = else_body.is_some();
    if let Some(body) = else_body {
        let branch = clone_env(env);
        bodies.push(LowerBody::scoped(body, branch));
    }
    deferred_stmt(bodies, move |lowered| {
        let expected = tarms.len() + if has_else { 1 } else { 0 };
        if lowered.len() != expected {
            return invariant_stmt("fallible match body count", subject_span);
        }
        let mut lowered = lowered.into_iter();
        let mut match_arms = Vec::with_capacity(tarms.len());
        for pattern in tarms {
            let Some(body) = lowered.next() else {
                return invariant_stmt("fallible match body order", subject_span);
            };
            match_arms.push(TMatchArm { pattern, body });
        }
        let else_lowered = if has_else {
            let Some(body) = lowered.next() else {
                return invariant_stmt("fallible match else body", subject_span);
            };
            Some(body)
        } else {
            None
        };
        if lowered.next().is_some() {
            return invariant_stmt("fallible match extra body", subject_span);
        }
        TStmt::EnumMatch {
            scrutinee,
            clone_subject,
            arms: match_arms,
            else_body: else_lowered,
            fallthrough: !has_else,
        }
    })
}

/// c109 Phase 8: bind the ok/err/present payload to its resolved type, read from the
/// subject's Result/Option type. Mirrors `add_pattern_bindings`'s Ok/Err/Present
/// arms (the binding's `jet_ty` is the inner type so any arithmetic on it traps
/// exactly as the AST path; `null` binds nothing).
pub(crate) fn tir_add_fallible_binding(
    pattern: &Pattern,
    env: &mut LowerEnv,
    subject_ty: &Type,
) -> Result<(), &'static str> {
    let subject_ty = subject_ty.without_user_tags();
    let (binding, ty) = match (pattern, subject_ty) {
        (Pattern::Ok { binding, .. }, Type::Result { ok, .. }) => {
            (binding.clone(), (**ok).clone())
        }
        (Pattern::Err { binding, .. }, Type::Result { err, .. }) => {
            (binding.clone(), (**err).clone())
        }
        (Pattern::Present { binding, .. }, Type::Option(inner)) => {
            (binding.clone(), (**inner).clone())
        }
        (Pattern::Absent(_), Type::Option(_)) => return Ok(()),
        (Pattern::Ok { .. }, _) => return Err("Ok pattern does not match fallible subject type"),
        (Pattern::Err { .. }, _) => return Err("Err pattern does not match fallible subject type"),
        (Pattern::Present { .. }, _) => {
            return Err("present pattern does not match optional subject type")
        }
        (Pattern::Absent(_), _) => return Err("absent pattern does not match optional subject type"),
        _ => return Err("unsupported fallible binding pattern"),
    };
    let slot = if ty.is_allocator_view() {
        TLocal::user(&binding).through_ref()
    } else {
        TLocal::user(&binding)
    };
    env.bind(&binding, slot, Some(ty));
    Ok(())
}

pub(crate) fn lower_enum_match<'a>(
    subject: &'a Expr,
    arms: &'a [SwitchArm],
    else_body: &'a Option<Vec<Stmt>>,
    cx: &'a Cx,
    env: &mut LowerEnv,
) -> LowerStmtPlan<'a> {
    let subject_span = subject.span();
    let (scrutinee, clone_subject) = match subject {
        Expr::Ident(name, _) if env.is_borrowed(name) => {
            let slot = env.local_of(name);
            let Some(ty) = env.ty_of(name) else {
                return LowerStmtPlan::ready(invariant_stmt(
                    "enum match borrowed subject type",
                    subject_span,
                ));
            };
            let mut by_value = slot.clone();
            by_value.deref = false;
            (
                TExpr {
                    ty,
                    kind: TExprKind::Local(by_value),
                },
                true,
            )
        }
        _ => (lower_expr(subject, cx, env), false),
    };
    let subject_ty = scrutinee.ty.clone();
    let enum_type = match resolved_enum_subject_type(cx, &subject_ty) {
        Ok(name) => Some(name),
        Err(reason) => {
            return LowerStmtPlan::ready(invariant_stmt(reason, subject_span));
        }
    };
    if arms.is_empty() {
        return LowerStmtPlan::ready(invariant_stmt(
            "enum match has no arms",
            subject_span,
        ));
    }
    let mut patterns = Vec::with_capacity(arms.len());
    let mut bodies = Vec::with_capacity(arms.len() + usize::from(else_body.is_some()));
    for arm in arms {
        let Some(pattern) = arm_variant_pattern(cx, &arm.cond, subject) else {
            return LowerStmtPlan::ready(invariant_stmt(
                "enum match arm pattern",
                arm.span,
            ));
        };
        let mut body_env = fork_panic(env);
        if let Err(reason) =
            tir_add_pattern_bindings(cx, &pattern, &mut body_env, Some(&subject_ty))
        {
            return LowerStmtPlan::ready(invariant_stmt(reason, pattern.span()));
        }
        patterns.push(TPattern::arm_with_values(
            pattern,
            enum_type.clone(),
            |value| lower_expr(value, cx, env),
        ));
        bodies.push(LowerBody::scoped(&arm.body, body_env));
    }
    let has_else = else_body.is_some();
    if let Some(body) = else_body {
        let branch = clone_env(env);
        bodies.push(LowerBody::scoped(body, branch));
    }
    deferred_stmt(bodies, move |lowered| {
        let expected = patterns.len() + if has_else { 1 } else { 0 };
        if lowered.len() != expected {
            return invariant_stmt("enum match body count", subject_span);
        }
        let mut lowered = lowered.into_iter();
        let mut match_arms = Vec::with_capacity(patterns.len());
        for pattern in patterns {
            let Some(body) = lowered.next() else {
                return invariant_stmt("enum match body order", subject_span);
            };
            match_arms.push(TMatchArm { pattern, body });
        }
        let else_lowered = if has_else {
            let Some(body) = lowered.next() else {
                return invariant_stmt("enum match else body", subject_span);
            };
            Some(body)
        } else {
            None
        };
        if lowered.next().is_some() {
            return invariant_stmt("enum match extra body", subject_span);
        }
        TStmt::EnumMatch {
            scrutinee,
            clone_subject,
            arms: match_arms,
            else_body: else_lowered,
            fallthrough: !has_else,
        }
    })
}

pub(crate) fn lower_range_switch<'a>(
    subject: &'a Expr,
    arms: &'a [SwitchArm],
    else_body: &'a Option<Vec<Stmt>>,
    cx: &'a Cx,
    env: &mut LowerEnv,
) -> LowerStmtPlan<'a> {
    let subject_expr = lower_expr(subject, cx, env);
    let subject_span = subject.span();
    if !matches!(
        subject_expr.ty.without_user_tags(),
        Type::Int | Type::InlineRange { .. } | Type::Char
    ) {
        return LowerStmtPlan::ready(invariant_stmt(
            "range switch subject type",
            subject_span,
        ));
    }
    if arms.is_empty() {
        return LowerStmtPlan::ready(invariant_stmt(
            "range switch has no arms",
            subject_span,
        ));
    }
    let mut ranges = Vec::with_capacity(arms.len());
    let mut bodies = Vec::with_capacity(arms.len() + 1);
    for arm in arms {
        let Some((lo, hi)) = arm_head_range(cx, &arm.cond, subject) else {
            return LowerStmtPlan::ready(invariant_stmt(
                "range switch arm pattern",
                arm.span,
            ));
        };
        if lo > hi {
            return LowerStmtPlan::ready(invariant_stmt(
                "range switch arm bounds",
                arm.span,
            ));
        }
        ranges.push((lo, hi));
        bodies.push(LowerBody::scoped(&arm.body, clone_env(env)));
    }
    let Some(else_body) = else_body.as_ref() else {
        return LowerStmtPlan::ready(invariant_stmt(
            "range switch requires else",
            subject_span,
        ));
    };
    bodies.push(LowerBody::scoped(else_body, clone_env(env)));
    deferred_stmt(bodies, move |lowered| {
        let expected = ranges.len() + 1;
        if lowered.len() != expected {
            return invariant_stmt("range switch body count", subject_span);
        }
        let mut lowered = lowered.into_iter();
        let mut match_arms = Vec::with_capacity(ranges.len());
        for (lo, hi) in ranges {
            let Some(body) = lowered.next() else {
                return invariant_stmt("range switch body order", subject_span);
            };
            match_arms.push((lo, hi, body));
        }
        let Some(else_lowered) = lowered.next() else {
            return invariant_stmt("range switch else body", subject_span);
        };
        if lowered.next().is_some() {
            return invariant_stmt("range switch extra body", subject_span);
        }
        TStmt::RangeSwitch {
            subject: subject_expr,
            arms: match_arms,
            else_body: else_lowered,
        }
    })
}

/// TIR-local reproduction of codegen's `emit_range_guard` (Statement.rs): a payload
/// range slot becomes `__jet_range_i >= lo && __jet_range_i <= hi`. `None` when no
/// slot is a range. Or-patterns reuse the first alt's ranges (all alts bind alike).


/// TIR-local reproduction of codegen's `add_pattern_bindings`: bind each `Bind`
/// slot to its exact payload type from the resolved subject. Wildcard/Range slots
/// bind nothing. Or-pattern alternatives are checked for the same names and types.
pub(crate) fn tir_add_pattern_bindings(
    cx: &Cx,
    pattern: &Pattern,
    env: &mut LowerEnv,
    subject_ty: Option<&Type>,
) -> Result<(), &'static str> {
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            let owner = match subject_ty {
                Some(ty) => Some(resolved_enum_subject_type(cx, ty)?),
                None => None,
            };
            let is_group = owner.as_deref().is_some_and(|owner| {
                !crate::Codegen::group_leaves(cx, Some(owner), variant).is_empty()
            });
            if is_group {
                return if bindings.is_empty() {
                    Ok(())
                } else {
                    Err("enum group pattern cannot bind payload slots")
                };
            }
            let tys = variant_payload_types(cx, variant, subject_ty)?;
            let range_slots_forbidden = subject_ty.is_some_and(|ty| {
                match ty.without_user_tags() {
                    Type::Named(name) | Type::Apply { name, .. } => {
                        let resolved = crate::Codegen::TIR::canonical_enum_owner(cx, name);
                        crate::Codegen::is_json_type_name(name)
                            || resolved == "DataTree"
                            || resolved == crate::Syntax::TYPE_KEY
                    }
                    _ => false,
                }
            });
            if bindings.len() != tys.len() {
                return Err("enum pattern payload slot count");
            }
            let payload = owner.as_deref().and_then(|owner| {
                cx.enum_variants
                    .get(owner)?
                    .iter()
                    .find(|(name, _)| name == variant)
                    .map(|(_, payload)| payload)
            });
            for (i, slot) in bindings.iter().enumerate() {
                let ty = tys.get(i).ok_or("enum pattern payload type")?;
                match slot {
                    PatSlot::Bind { name, .. } => {
                        let edge = match payload {
                            Some(VariantPayload::Single(..)) => Some(variant.clone()),
                            Some(VariantPayload::Named(fields)) => {
                                let Some(field) = fields.get(i) else {
                                    return Err("enum named payload field");
                                };
                                Some(format!("{variant}.{}", field.name))
                            }
                            Some(VariantPayload::Unit) | None => None,
                        };
                        let boxed = owner
                            .as_ref()
                            .zip(edge.as_ref())
                            .is_some_and(|(owner, edge)| {
                                cx.boxed_edges.contains(&(owner.clone(), edge.clone()))
                            });
                        let local = if boxed {
                            TLocal::user(name).through_ref()
                        } else {
                            TLocal::user(name)
                        };
                        env.bind(name, local, Some(ty.clone()));
                    }
                    PatSlot::Wildcard => {}
                    PatSlot::Range { lo, hi } => {
                        if range_slots_forbidden
                            || lo > hi
                            || !matches!(
                                ty.without_user_tags(),
                                Type::Int | Type::InlineRange { .. } | Type::Char
                            )
                        {
                            return Err("enum range payload type or bounds");
                        }
                    }
                }
            }
            Ok(())
        }
        Pattern::Or(alts, _) => {
            let Some(first) = alts.first() else {
                return Err("empty enum or-pattern");
            };
            tir_add_pattern_bindings(cx, first, env, subject_ty)?;
            let first_names = first.binding_names();
            let expected: Vec<(String, Option<Type>)> = first_names
                .iter()
                .map(|binding| {
                    (
                        binding.local_name().to_string(),
                        env.ty_of(binding.local_name()),
                    )
                })
                .collect();
            for alternative in alts.iter().skip(1) {
                let mut alternative_env = clone_env(env);
                tir_add_pattern_bindings(cx, alternative, &mut alternative_env, subject_ty)?;
                let names = alternative.binding_names();
                if names.len() != expected.len() {
                    return Err("enum or-pattern binding count");
                }
                for (binding, (expected_name, expected_ty)) in names.iter().zip(&expected) {
                    if binding.local_name() != expected_name
                        || alternative_env.ty_of(binding.local_name()).as_ref()
                            != expected_ty.as_ref()
                    {
                        return Err("enum or-pattern binding shape");
                    }
                }
            }
            Ok(())
        }
        _ => Err("unsupported enum binding pattern"),
    }
}

/// The payload field types a variant binds, from the resolved subject type and
/// canonical enum layout. Missing or inconsistent layout data is a compiler
/// invariant.
///
/// Core dynamic enums use their canonical payload tables; user and foreign enums
/// use the exact layout selected by the already-resolved subject type.
pub(crate) fn variant_payload_types(
    cx: &Cx,
    variant: &str,
    subject_ty: Option<&Type>,
) -> Result<Vec<Type>, &'static str> {
    let Some(subject_ty) = subject_ty else {
        return Err("enum payload subject type");
    };
    let subject_ty = subject_ty.without_user_tags();
    if let Type::Apply { name, args } = subject_ty {
        if matches!(
            name.as_str(),
            crate::Syntax::TYPE_HOOK_OUTCOME | crate::Syntax::TYPE_HOOK_DECISION
        ) {
            return match (name.as_str(), variant) {
                ("HookOutcome", "Continue") | ("HookDecision", "Continue" | "Transform") => args
                    .first()
                    .cloned()
                    .map(|ty| vec![ty])
                    .ok_or("hook payload type"),
                ("HookOutcome", "Fail") | ("HookDecision", "Fail") => args
                    .get(1)
                    .cloned()
                    .map(|ty| vec![ty])
                    .ok_or("hook payload type"),
                ("HookOutcome", "Cancel") | ("HookDecision", "Cancel") => Ok(Vec::new()),
                _ => Err("unknown hook enum variant"),
            };
        }
    }
    match subject_ty {
        Type::Union(members) => members
            .iter()
            .find(|member| crate::AST::union_member_tag(member) == variant)
            .cloned()
            .map(|member| vec![member])
            .ok_or("union variant payload type"),
        Type::Named(name) | Type::Apply { name, .. } => {
            let resolved = crate::Codegen::TIR::canonical_enum_owner(cx, name);
            if (crate::Codegen::is_json_type_name(name) || resolved == "DataTree")
                && crate::Codegen::is_json_variant(variant)
            {
                return match variant {
                    "Null" => Ok(Vec::new()),
                    "Bool" => Ok(vec![Type::Bool]),
                    "Int" => Ok(vec![Type::Int]),
                    "Float" => Ok(vec![Type::Float]),
                    "Text" => Ok(vec![Type::String]),
                    "Array" => Ok(vec![Type::List(Box::new(Type::Named(
                        crate::Syntax::TYPE_DATA.to_string(),
                    )))]),
                    "Object" => Ok(vec![Type::Map {
                        key: Box::new(Type::String),
                        key_span: None,
                        value: Box::new(Type::Named(crate::Syntax::TYPE_DATA.to_string())),
                    }]),
                    "Number" => Ok(vec![Type::String]),
                    _ => Err("unknown data enum variant"),
                };
            }
            if resolved == crate::Syntax::TYPE_KEY {
                return match variant {
                    "Char" | "Ctrl" => Ok(vec![Type::Char]),
                    "F" => Ok(vec![Type::Int]),
                    v if crate::Codegen::is_key_variant(v) => Ok(Vec::new()),
                    _ => Err("unknown key enum variant"),
                };
            }
            if resolved == "DataEvent" {
                return match variant {
                    "Bool" => Ok(vec![Type::Bool]),
                    "Int" => Ok(vec![Type::Int]),
                    "Float" => Ok(vec![Type::Float]),
                    "Text" | "Key" => Ok(vec![Type::String]),
                    "Bytes" => Ok(vec![Type::List(Box::new(Type::Int))]),
                    "Null" | "ArrayStart" | "ArrayEnd" | "ObjectStart" | "ObjectEnd" => {
                        Ok(Vec::new())
                    }
                    _ => Err("unknown data-event enum variant"),
                };
            }
            variant_binding_types_for_enum(cx, &resolved, variant)
                .ok_or("enum variant payload type")
        }
        _ => Err("enum payload subject type"),
    }
}

/// The Rust type path a Jet enum name spells, plus whether its variants are
/// spelled RAW. `true` = a Rust-defined enum (Prelude/host) whose variants are
/// plain Rust identifiers; `false` = a Jet-declared enum whose variants are
/// mangled.
///
/// This is the ONE table. `tir_enum_lit_prefix` (enum literals and bare
/// variant-path tests) and `emit_match_pattern` (match-arm heads, Statement.rs)
/// both read it, so a Prelude enum can never be spelled `jet_std::WatchDomain`
/// as a value and `__jet_WatchDomain` as a pattern. A second copy of this table
/// is how `WatchDomain`/`WatchKind` reached rustc as E0433 in generated code
/// (I2): the copy in `emit_match_pattern` had never heard of them, so every
/// core enum the copy was missing mangled into a nonexistent local type.


/// The Rust variant name under a `tir_enum_rust_path` head. A raw (Rust-defined)
/// variant keeps its own identifier — a mangled spelling reaching it means the
/// caller carried the generated prefix, which is not part of the Rust name.


/// c109 Phase 24: the Rust enum-literal head `{prefix}::{mangle(variant)}` for a payload
/// or named enum literal, reproducing `emit_enum_lit`'s `type_prefix` (Expression.rs): a
/// FOREIGN (imported) enum → `{root}{mod}::__jet_<T>::__jet_<V>`, a local enum →
/// `__jet_<T>::__jet_<V>`. Keyed on the ENUM name in `cx.foreign_types`, byte-for-byte.


/// c109 Phase 16: the single-payload type of `(type_name, edge)`, mirroring the AST
/// `enum_variant_payload_type` (Expression.rs). `edge` is the VARIANT name for a
/// positional arg, or `"Variant.label"` for a named arg — the latter is explicitly
/// non-applicable to the single-payload clone decision. Missing checked owner/variant
/// layout is an invariant, not a payload-less variant.
pub(crate) fn enum_variant_payload_type<'a>(
    cx: &'a Cx,
    type_name: &str,
    edge: &str,
) -> Result<Option<&'a Type>, &'static str> {
    let type_name = crate::Codegen::TIR::canonical_enum_owner(cx, type_name);
    if let Some((variant, label)) = edge.split_once('.') {
        let variants = cx
            .enum_variants
            .get(&type_name)
            .ok_or("enum payload owner layout")?;
        let (_, payload) = variants
            .iter()
            .find(|(candidate, _)| candidate == variant)
            .ok_or("enum payload variant layout")?;
        return match payload {
            VariantPayload::Named(fields)
                if fields.iter().any(|field| field.name == label) =>
            {
                Ok(None)
            }
            VariantPayload::Named(_) => Err("enum named payload field layout"),
            VariantPayload::Unit | VariantPayload::Single(..) => {
                Err("enum named payload variant layout")
            }
        };
    }
    let variants = cx
        .enum_variants
        .get(&type_name)
        .ok_or("enum payload owner layout")?;
    let (_, payload) = variants
        .iter()
        .find(|(v, _)| v == edge)
        .ok_or("enum payload variant layout")?;
    Ok(match payload {
        VariantPayload::Single(t, _) => Some(t),
        VariantPayload::Named(fs) if fs.len() == 1 => Some(&fs[0].ty),
        VariantPayload::Unit => return Err("enum payload argument for unit variant"),
        VariantPayload::Named(_) => return Err("enum positional payload requires one field"),
    })
}

/// c109 Phase 16: lower one enum-literal payload arg, resolving the `clone`/`boxed`
/// decisions as TOTAL facts, reproducing `emit_boxed_enum_arg` (Expression.rs)
/// byte-for-byte. `edge` is the variant name (positional) or `"Variant.label"`
/// (named). A non-scalar single-payload type whose arg is a borrowed-in-env ident
/// gets `(…).clone()`; a recursive (`boxed_edge`) edge gets `Box::new(…)`.
pub(crate) fn lower_enum_arg(
    type_name: &str,
    variant: &str,
    edge: &str,
    e: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TEnumArg {
    let payload_ty = match enum_variant_payload_type(cx, type_name, edge) {
        Ok(payload_ty) => payload_ty,
        Err(reason) => {
            let value = lower_expr(e, cx, env);
            let value_ty = value.ty.clone();
            return TEnumArg {
                value: invariant_expr(
                    format!("enum payload `{type_name}.{variant}`: {reason}"),
                    e.span(),
                    value_ty,
                ),
                clone: false,
                boxed: false,
            };
        }
    };
    let borrowed = matches!(e, Expr::Ident(name, _) if env.is_borrowed(name));
    let clone = payload_ty.is_some_and(|t| !t.is_scalar()) && borrowed;
    let boxed = cx
        .boxed_edges
        .contains(&(type_name.to_string(), edge.to_string()));
    let mutable_view_payload = matches!(
        payload_ty,
        Some(Type::Apply { name, .. })
            if matches!(name.as_str(), "ViewMut" | "ComputeViewMut")
    );
    let mutable_place;
    let payload_expr = if mutable_view_payload {
        match e {
            Expr::Place(inner, _, span) => {
                mutable_place = Expr::Place(inner.clone(), crate::AST::PlaceAccess::Write, *span);
                &mutable_place
            }
            Expr::Slice { span, .. } => {
                mutable_place =
                    Expr::Place(Box::new(e.clone()), crate::AST::PlaceAccess::Write, *span);
                &mutable_place
            }
            _ => e,
        }
    } else {
        e
    };
    let mut value = lower_expr(payload_expr, cx, env);
    if let Some(want) = payload_ty {
        value = crate::Codegen::TIR::maybe_widen_expr_to_union(value, want);
    }
    TEnumArg {
        value,
        clone,
        boxed,
    }
}
