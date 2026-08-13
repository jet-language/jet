use crate::jet_generated_format as jet_format;
use crate::AST::{BinOp, Expr, PatSlot, Pattern, Stmt, SwitchArm, Type, VariantPayload};
use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::mangle_generated;
use crate::Codegen::mangle_path;
use crate::Codegen::TIR::arm_fallible_pattern;
use crate::Codegen::TIR::arm_head_range;
use crate::Codegen::TIR::arm_variant_pattern;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::fork_panic;
use crate::Codegen::TIR::lower::{deferred_stmt, LowerBody, LowerStmtPlan};
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower::str_match_scan_closure;
use crate::Codegen::TIR::struct_field_type;
use crate::Codegen::TIR::TEnumArg;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::THandleOp;
use crate::Codegen::TIR::TMatchArm;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TPattern;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::TIR::unit_type;
use crate::Codegen::TIR::variant_pattern_enum;
use crate::Codegen::{variant_binding_types, variant_binding_types_for_enum};

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
) -> TExpr {
    let recv_t = lower_expr(receiver, cx, env);
    let canonical: Vec<(String, Type)> = parts
        .iter()
        .filter_map(|p| match p {
            crate::AST::StrMatchPart::Hole { name, ty, .. } => {
                Some((name.clone(), ty.clone().unwrap_or(Type::String)))
            }
            crate::AST::StrMatchPart::Lit(_) => None,
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
            op: THandleOp::CursorTakePattern {
                parts: parts.to_vec(),
                canonical,
            },
            args: vec![],
        },
    }
}

/// D-BINPAT1 (card #506 follow-up): lower `reader.take_pattern(b"…")` — the
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
) -> TExpr {
    use crate::AST::{BinMatchPart, BinSpec};
    let recv_t = lower_expr(receiver, cx, env);
    let canonical: Vec<(String, Type)> = parts
        .iter()
        .filter_map(|p| match p {
            BinMatchPart::Hole { name, spec, .. } => {
                let ty = match spec {
                    BinSpec::Rest => Type::List(Box::new(Type::IntN { signed: false, bits: 8 })),
                    BinSpec::Bits { width, .. } => crate::Codegen::TIR::lower::bin_bits_type(*width),
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
/// pattern appears in an if-table with no fallback).
pub(super) fn str_match_pattern_cond_expr(pattern: &Pattern, _cx: &Cx) -> TExpr {
    let Pattern::StrMatch { parts, .. } = pattern else {
        return TExpr {
            ty: Type::Bool,
            kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::StrMatchScan {
                parts: Vec::new(),
                probe: crate::Codegen::TIR::TMatchProbe::IsSome,
            })),
        };
    };
    TExpr {
        ty: Type::Bool,
        kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::StrMatchScan {
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
pub(super) fn lower_str_match_pattern_bindings(pattern: &Pattern, cx: &Cx, env: &mut LowerEnv) -> Vec<TStmt> {
    let (_, holes) = str_match_scan_closure(pattern, cx);
    if holes.is_empty() {
        return Vec::new();
    }
    let parts = match pattern {
        Pattern::StrMatch { parts, .. } => parts.clone(),
        _ => Vec::new(),
    };
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
                parts,
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
pub(super) fn bin_match_pattern_cond_expr(pattern: &Pattern, _cx: &Cx) -> TExpr {
    let parts = match pattern {
        Pattern::BinMatch { parts, .. } => parts.clone(),
        _ => Vec::new(),
    };
    TExpr {
        ty: Type::Bool,
        kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::BinMatchScan {
            parts,
            probe: crate::Codegen::TIR::TMatchProbe::IsSome,
        })),
    }
}

/// D-BINPAT1: bind each hole locally before the arm body runs — re-invokes the
/// bit-scan closure (pure/cheap), binds the whole result tuple to one temp,
/// then projects each hole out by index. Mirrors `lower_str_match_pattern_bindings`.
pub(super) fn lower_bin_match_pattern_bindings(
    pattern: &Pattern,
    cx: &Cx,
    env: &mut LowerEnv,
) -> Vec<TStmt> {
    let (_, holes) = crate::Codegen::TIR::lower::bin_match_scan_closure(pattern, cx);
    if holes.is_empty() {
        return Vec::new();
    }
    let parts = match pattern {
        Pattern::BinMatch { parts, .. } => parts.clone(),
        _ => Vec::new(),
    };
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
                parts,
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
    // The subject's resolved type carries the ok/err/present payload types. Lower the
    // subject once to get both its emitted string and its total type.
    let subject_t = lower_expr(subject, cx, env);
    let subject_ty = subject_t.ty.clone();
    // A by-reference enum param is cloned in the enum-match path; a fallible/optional
    // subject in-subset is never a deref'd slot (it is a fn-call value or an owned
    // local), so the scrutinee is the plain emitted form — matching the AST path,
    // whose `subj` clone branch only fires for a deref'd `Ident`.
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
    let mut tarms = Vec::new();
    let mut bodies = Vec::new();
    for arm in arms {
        let pattern =
            arm_fallible_pattern(cx, &arm.cond, subject).expect("gate proved fallible arm");
        // An arm body is a CLONED env in `emit_pattern_match_switch` (no leak) — fork.
        let mut body_env = fork_panic(env);
        tir_add_fallible_binding(&pattern, &mut body_env, &subject_ty);
        let tir_pattern = if matches!(&subject_ty, Type::Option(_)) {
            TPattern::option_binding(pattern)
        } else {
            TPattern::binding(pattern)
        };
        tarms.push(tir_pattern);
        bodies.push(LowerBody::scoped(&arm.body, body_env));
    }
    let has_else = else_body.is_some();
    if let Some(body) = else_body {
        let branch = clone_env(env);
        bodies.push(LowerBody::scoped(body, branch));
    }
    deferred_stmt(bodies, move |mut lowered| {
        let else_lowered = has_else.then(|| lowered.pop().unwrap());
        let mut lowered = lowered.into_iter();
        let tarms = tarms
            .into_iter()
            .map(|pattern| TMatchArm {
                pattern,
                body: lowered.next().expect("fallible match body was deferred"),
            })
            .collect();
        // No explicit `else` → the AST path (`emit_pattern_match_switch`) appends
        // `_ => unreachable!(…)` so rustc sees a complete match (sema proved E0307).
        let fallthrough = !has_else;
        TStmt::EnumMatch {
            scrutinee,
            clone_subject,
            arms: tarms,
            else_body: else_lowered,
            fallthrough,
        }
    })
}

/// c109 Phase 8: bind the ok/err/present payload to its resolved type, read from the
/// subject's Result/Option type. Mirrors `add_pattern_bindings`'s Ok/Err/Present
/// arms (the binding's `jet_ty` is the inner type so any arithmetic on it traps
/// exactly as the AST path; `null` binds nothing).
pub(crate) fn tir_add_fallible_binding(pattern: &Pattern, env: &mut LowerEnv, subject_ty: &Type) {
    let (binding, ty) = match (pattern, subject_ty) {
        (Pattern::Ok { binding, .. }, Type::Result { ok, .. }) => {
            (binding.clone(), Some((**ok).clone()))
        }
        (Pattern::Err { binding, .. }, Type::Result { err, .. }) => {
            (binding.clone(), Some((**err).clone()))
        }
        (Pattern::Present { binding, .. }, Type::Option(inner)) => {
            (binding.clone(), Some((**inner).clone()))
        }
        // The subject type didn't resolve to the expected shape (impossible for a
        // covered subject — sema validated it); bind with no type (matches the AST
        // path's `jet_ty: None` fallback).
        (Pattern::Ok { binding, .. }, _)
        | (Pattern::Err { binding, .. }, _)
        | (Pattern::Present { binding, .. }, _) => (binding.clone(), None),
        // `null` (Absent) binds nothing.
        _ => return,
    };
    env.bind(&binding, TLocal::user(&binding), ty);
}

pub(crate) fn lower_enum_match<'a>(
    subject: &'a Expr,
    arms: &'a [SwitchArm],
    else_body: &'a Option<Vec<Stmt>>,
    cx: &'a Cx,
    env: &mut LowerEnv,
) -> LowerStmtPlan<'a> {
    // The match owns the value. Mirror `emit_pattern_match_switch`: a by-reference
    // subject (a deref'd enum param) is cloned — the borrow itself is cloned, NOT
    // the deref'd place, so the scrutinee carries the slot *without* its deref and
    // the clone is recorded as a fact.
    let (scrutinee, clone_subject) = match subject {
        Expr::Ident(name, _) if env.is_borrowed(name) => {
            let slot = env.local_of(name);
            let ty = env.ty_of(name).unwrap_or(Type::Int);
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
    // Resolve the owning enum once — drives the Rust variant prefix in patterns.
    // Variant names are not unique (`Closed`, for example), so prefer the checked
    // subject type over the lossy variant-name fallback.
    let subject_ty = scrutinee.ty.clone();
    let subject_enum = match &subject_ty {
        Type::Union(members) => Some(crate::AST::union_enum_name(members)),
        Type::Named(name) | Type::Apply { name, .. } => {
            let resolved = cx
                .core_qualified_rust_type_name(name)
                .unwrap_or(name.as_str());
            cx.enum_variants
                .contains_key(resolved)
                .then(|| resolved.to_string())
        }
        _ => None,
    };
    let enum_type = subject_enum.or_else(|| {
        arms.iter().find_map(|a| {
            arm_variant_pattern(cx, &a.cond, subject).and_then(|p| variant_pattern_enum(cx, &p))
        })
    });
    let mut patterns = Vec::new();
    let mut bodies = Vec::new();
    for arm in arms {
        let pattern = arm_variant_pattern(cx, &arm.cond, subject).expect("gate proved variant arm");
        // The arm body sees the variant's payload bindings, typed from the layout. The
        // arm body is a CLONED env in `emit_pattern_match_switch` (no leak) — fork.
        let mut body_env = fork_panic(env);
        tir_add_pattern_bindings(cx, &pattern, &mut body_env, Some(&subject_ty));
        patterns.push(TPattern::arm(pattern, enum_type.clone()));
        bodies.push(LowerBody::scoped(&arm.body, body_env));
    }
    let has_else = else_body.is_some();
    if let Some(body) = else_body {
        let branch = clone_env(env);
        bodies.push(LowerBody::scoped(body, branch));
    }
    deferred_stmt(bodies, move |mut lowered| {
        let else_lowered = has_else.then(|| lowered.pop().unwrap());
        let mut lowered = lowered.into_iter();
        let tarms = patterns
            .into_iter()
            .map(|pattern| TMatchArm {
                pattern,
                body: lowered.next().expect("enum match body was deferred"),
            })
            .collect();
        // No explicit `else` → the AST path appends `_ => unreachable!(…)` so rustc
        // sees a complete match (sema already proved exhaustiveness — E0307).
        TStmt::EnumMatch {
            scrutinee,
            clone_subject,
            arms: tarms,
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
    let mut ranges = Vec::new();
    let mut bodies = Vec::new();
    for arm in arms {
        let (lo, hi) = arm_head_range(cx, &arm.cond, subject).expect("gate proved range arm");
        ranges.push((lo, hi));
        bodies.push(LowerBody::scoped(&arm.body, clone_env(env)));
    }
    let else_body = else_body.as_ref().expect("range switch requires else (gate)");
    bodies.push(LowerBody::scoped(else_body, clone_env(env)));
    deferred_stmt(bodies, move |mut lowered| {
        let else_lowered = lowered.pop().unwrap();
        let mut lowered = lowered.into_iter();
        let arms = ranges
            .into_iter()
            .map(|(lo, hi)| {
                (
                    lo,
                    hi,
                    lowered.next().expect("range switch body was deferred"),
                )
            })
            .collect();
        TStmt::RangeSwitch {
            subject: subject_expr,
            arms,
            else_body: else_lowered,
        }
    })
}

/// TIR-local reproduction of codegen's `emit_range_guard` (Statement.rs): a payload
/// range slot becomes `__jet_range_i >= lo && __jet_range_i <= hi`. `None` when no
/// slot is a range. Or-patterns reuse the first alt's ranges (all alts bind alike).
pub(crate) fn tir_range_guard(pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Variant { bindings, .. } => {
            let guards: Vec<String> = bindings
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    if let PatSlot::Range { lo, hi } = s {
                        Some(jet_format!(
                            "{jet_prefix}range_{} >= {} && {jet_prefix}range_{} <= {}",
                            i, lo, i, hi
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            if guards.is_empty() {
                None
            } else {
                Some(guards.join(" && "))
            }
        }
        Pattern::Or(alts, _) => alts.first().and_then(tir_range_guard),
        _ => None,
    }
}

/// TIR-local reproduction of codegen's `add_pattern_bindings`/`variant_binding_types`
/// for the user-enum case: bind each `Bind` slot to its payload field type, read
/// from the resolved enum layout (`cx.enum_variants`). Wildcard/Range slots bind
/// nothing. Or-patterns bind the first alt's names (all alts bind alike — E0317).
pub(crate) fn tir_add_pattern_bindings(
    cx: &Cx,
    pattern: &Pattern,
    env: &mut LowerEnv,
    subject_ty: Option<&Type>,
) {
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            let hook_payload = match (subject_ty, variant.as_str()) {
                (Some(Type::Apply { name, args }), "Continue" | "Transform")
                    if matches!(name.as_str(), "HookOutcome" | "HookDecision") => args.first().cloned(),
                (Some(Type::Apply { name, args }), "Fail")
                    if matches!(name.as_str(), "HookOutcome" | "HookDecision") => args.get(1).cloned(),
                _ => None,
            };
            // D-UNIONTYPE1=A: union arm binds the matching member type.
            let union_payload = match subject_ty {
                Some(Type::Union(members)) => members
                    .iter()
                    .find(|m| crate::AST::union_member_tag(m) == *variant)
                    .map(|m| vec![m.clone()]),
                _ => None,
            };
            let tys = hook_payload
                .map(|ty| vec![ty])
                .or(union_payload)
                .or_else(|| variant_payload_types(cx, variant, subject_ty));
            for (i, slot) in bindings.iter().enumerate() {
                if let PatSlot::Bind { name, .. } = slot {
                    // Payload types are scalar/Char (the enum is covered), so the
                    // binding is a by-value local; default to Int if unresolved
                    // (impossible for a covered enum — sema validated the access).
                    let ty = tys
                        .as_ref()
                        .and_then(|ts| ts.get(i).cloned())
                        .unwrap_or(Type::Int);
                    env.bind(name, TLocal::user(name), Some(ty));
                }
            }
        }
        Pattern::Or(alts, _) => {
            if let Some(first) = alts.first() {
                tir_add_pattern_bindings(cx, first, env, subject_ty);
            }
        }
        _ => {}
    }
}

/// The payload field types a variant binds, from the resolved enum layout. c109
/// Phase 24: DELEGATES to the AST `variant_binding_types` (made `pub(crate)`), which
/// handles the JSON enum (`core_json_pattern_types`) AND user/foreign enums
/// (`cx.variant_owner` → `cx.enum_variants`) — pure table lookups, no env/inference —
/// so the bound payload type is byte-parity-faithful for every covered enum (e.g. a
/// foreign `ParseError.NoFrontmatter(p)` binds `p: String`).
pub(crate) fn variant_payload_types(
    cx: &Cx,
    variant: &str,
    subject_ty: Option<&Type>,
) -> Option<Vec<Type>> {
    let typed = subject_ty.and_then(|ty| {
        let name = match ty {
            Type::Named(name) | Type::Apply { name, .. } => name,
            _ => return None,
        };
        let resolved = cx
            .core_qualified_rust_type_name(name)
            .unwrap_or(name.as_str());
        variant_binding_types_for_enum(cx, resolved, variant)
    });
    typed.or_else(|| variant_binding_types(cx, variant))
}

/// c109 Phase 24: the Rust enum-literal head `{prefix}::{mangle(variant)}` for a payload
/// or named enum literal, reproducing `emit_enum_lit`'s `type_prefix` (Expression.rs): a
/// FOREIGN (imported) enum → `{root}{mod}::__jet_<T>::__jet_<V>`, a local enum →
/// `__jet_<T>::__jet_<V>`. Keyed on the ENUM name in `cx.foreign_types`, byte-for-byte.
pub(crate) fn tir_enum_lit_prefix(cx: &Cx, type_name: &str, variant: &str) -> String {
    // D-UNIONTYPE1=A: compiler-generated union enums use bare member-type tags.
    if type_name.starts_with("__JetUnion_") {
        return format!("{}::{variant}", mangle_path(type_name));
    }
    // D-TERM1 (ratified 2026-06-22): `Key` is a prelude enum; its Rust name is `JetKey`.
    // Variant names are not mangled (Char, Enter, …).
    if type_name == crate::Syntax::TYPE_KEY {
        return format!("{}JetKey::{}", cx.root_prefix, variant);
    }
    if type_name == crate::Syntax::TYPE_REMOVE_BY
        || type_name == mangle(crate::Syntax::TYPE_REMOVE_BY)
    {
        let variant = variant
            .strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX)
            .unwrap_or(variant);
        return format!("{}JetRemoveBy::{}", cx.root_prefix, variant);
    }
    if type_name == crate::Syntax::TYPE_TASK_FAILURE {
        return format!("{}jet_std::JetTaskFailure::{}", cx.root_prefix, variant);
    }
    if type_name == "DataEvent" {
        return format!("{}jet_std::DataEvent::{}", cx.root_prefix, variant);
    }
    if matches!(type_name, "EncodingFormat" | "EncodingErrorKind") {
        return format!("{}jet_std::{}::{}", cx.root_prefix, type_name, variant);
    }
    if matches!(type_name, "XMLEncoding" | "XMLLexicalPolicy" | "XMLCanonicalMode") {
        return format!("{}jet_std::{}::{}", cx.root_prefix, type_name, variant);
    }
    // D-PROCESS1=A: `ProcessStreamMode` is a core dot-literal enum (`.Stream`/
    // `.Inherit`/`.Capture`) — its Rust type lives in `jet_std`, plain variant names.
    if type_name == "ProcessStreamMode" {
        return format!("{}jet_std::ProcessStreamMode::{}", cx.root_prefix, variant);
    }
    if type_name == crate::Syntax::TYPE_TERMINAL_MODE {
        return format!("{}jet_std::TerminalMode::{}", cx.root_prefix, variant);
    }
    // D-TEXTWIDTH1=B: `TextWidth`'s two field enums — same shape.
    if matches!(type_name, "TextWidthAmbiguous" | "TextWidthControls") {
        return format!("{}jet_std::{}::{}", cx.root_prefix, type_name, variant);
    }
    if type_name == crate::Syntax::DURATION_UNIT_TYPE {
        return format!("{}jet_std::DurationUnit::{}", cx.root_prefix, variant);
    }
    if type_name == "Overflow" {
        return format!("{}jet_std::JetEventOverflow::{}", cx.root_prefix, variant);
    }
    if type_name == "FailurePolicy" {
        return format!("{}jet_std::JetFailurePolicy::{}", cx.root_prefix, variant);
    }
    if type_name == "DispatchState" {
        return format!("{}jet_std::JetDispatchState::{}", cx.root_prefix, variant);
    }
    if type_name == "HookPolicy" {
        return format!("{}jet_std::JetHookPolicy::{}", cx.root_prefix, variant);
    }
    if type_name == "HookDecision" {
        return format!("{}jet_std::JetHookDecision::{}", cx.root_prefix, variant);
    }
    if type_name == "HookOutcome" {
        return format!("{}jet_std::JetHookOutcome::{}", cx.root_prefix, variant);
    }
    if matches!(type_name, "NetShutdown" | "NetReadyInterest") {
        return format!("{}Jet{}::{}", cx.root_prefix, type_name, variant);
    }
    if matches!(type_name, "NetError" | "NetDnsError") {
        return format!("{}Jet{}::{}", cx.root_prefix, type_name, variant);
    }
    if type_name == "TLSClientTrust" {
        return format!("{}JetTLSTrust::{}", cx.root_prefix, variant);
    }
    if type_name == "TLSVersion" {
        return format!("{}JetTLSVersion::{}", cx.root_prefix, variant);
    }
    if matches!(type_name, "IOError" | "IOOperation") {
        return format!("{}jet_std::{}::{}", cx.root_prefix, if type_name == "IOError" { "IOError" } else { "IOOperation" }, variant);
    }
    if matches!(type_name, "HTTPError" | "HTTPOperation" | "HTTPProxy" | "HTTPCorsOrigins" | "HTTPRedirectPolicy" | "HTTPRetryPolicy" | "HTTPCookieJar" | "HTTPCompressEncoding") {
        return format!("{}Jet{}::{}", cx.root_prefix, type_name, variant);
    }
    if matches!(type_name, "SMTPSecurity" | "RecipientPolicy" | "EmailError" | "SMTPAuth" | "TLSTrust") {
        let rust = if type_name == "EmailError" { "Error" } else { type_name };
        return format!("{}jet_email::{}::{}", cx.root_prefix, rust, variant);
    }
    if type_name == "AuthError" {
        return format!("{}JetAuthError::{}", cx.root_prefix, variant);
    }
    if type_name == "ServiceReceipt" {
        return format!("{}JetServiceReceipt::{}", cx.root_prefix, variant);
    }
    if type_name == "ServiceError" {
        return format!("{}JetServiceError::{}", cx.root_prefix, variant);
    }
    let foreign_identity = if cx.foreign_types.contains_key(type_name) {
        Some(type_name.to_string())
    } else {
        cx.foreign_type_identity("", type_name)
    };
    let type_prefix = match foreign_identity {
        Some(identity) => {
            let rust_mod = cx
                .foreign_types
                .get(&identity)
                .expect("foreign identity must have a Rust module");
            let leaf = identity.rsplit("::").next().unwrap_or(&identity);
            format!("{}{}::{}", cx.root_prefix, rust_mod, mangle_path(leaf))
        }
        None => mangle_path(type_name),
    };
    format!("{}::{}", type_prefix, mangle_path(variant))
}

/// c109 Phase 16: the single-payload type of `(type_name, edge)`, mirroring the AST
/// `enum_variant_payload_type` (Expression.rs). `edge` is the VARIANT name for a
/// positional arg, or `"Variant.label"` for a named arg — the latter never matches a
/// variant name, so it returns `None` (the AST never clones a named-payload arg), as
/// `enum_variant_payload_type` does. Only `Single(t)` / single-field `Named` resolve.
pub(crate) fn enum_variant_payload_type<'a>(
    cx: &'a Cx,
    type_name: &str,
    edge: &str,
) -> Option<&'a Type> {
    let variants = cx.enum_variants.get(type_name)?;
    let (_, payload) = variants.iter().find(|(v, _)| v == edge)?;
    match payload {
        VariantPayload::Single(t, _) => Some(t),
        VariantPayload::Named(fs) if fs.len() == 1 => Some(&fs[0].ty),
        _ => None,
    }
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
    let payload_ty = enum_variant_payload_type(cx, type_name, edge);
    let borrowed = matches!(e, Expr::Ident(name, _) if env.is_borrowed(name));
    let clone = payload_ty.is_some_and(|t| !t.is_scalar()) && borrowed;
    let boxed = cx
        .boxed_edges
        .contains(&(type_name.to_string(), edge.to_string()));
    let _ = variant;
    let mutable_view_payload = matches!(
        payload_ty,
        Some(Type::Apply { name, .. })
            if matches!(name.as_str(), "ViewMut" | "ComputeViewMut")
    );
    let mutable_place;
    let payload_expr = if mutable_view_payload {
        match e {
            Expr::Place(inner, _, span) => {
                mutable_place = Expr::Place(
                    inner.clone(),
                    crate::AST::PlaceAccess::Write,
                    *span,
                );
                &mutable_place
            }
            Expr::Slice { span, .. } => {
                mutable_place = Expr::Place(
                    Box::new(e.clone()),
                    crate::AST::PlaceAccess::Write,
                    *span,
                );
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
