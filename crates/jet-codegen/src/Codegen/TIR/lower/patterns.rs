use crate::AST::{BinOp, Expr, PatSlot, Pattern, Stmt, SwitchArm, Type, VariantPayload};
use crate::Codegen::Cx;
use crate::Codegen::emit_match_pattern;
use crate::Codegen::mangle;
use crate::Codegen::mangle_variant;
use crate::Codegen::TIR::arm_fallible_pattern;
use crate::Codegen::TIR::arm_head_range;
use crate::Codegen::TIR::arm_variant_pattern;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::core_struct_field_rust_name;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::expr_ast_jet_ty;
use crate::Codegen::TIR::fork_panic;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::lower::str_match_scan_closure;
use crate::Codegen::TIR::struct_field_type;
use crate::Codegen::TIR::TEnumArg;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::THandleOp;
use crate::Codegen::TIR::TMatchArm;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::TIR::tuple_join;
use crate::Codegen::TIR::unit_type;
use crate::Codegen::TIR::variant_pattern_enum;
use crate::Codegen::variant_binding_types;

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
pub(super) fn str_match_pattern_cond_expr(pattern: &Pattern, cx: &Cx) -> TExpr {
    let (closure, _) = str_match_scan_closure(pattern, cx);
    TExpr {
        ty: Type::Bool,
        kind: TExprKind::ConstInline(format!("({}).is_some()", closure)),
    }
}

/// D-PARSESTR1: bind each hole locally before the arm body runs. Re-invokes
/// the same scan closure text as the cond (pure/cheap — `starts_with`/`find`/
/// `.parse()` — matching how struct-pattern value tests and bind fields are
/// independently re-derived rather than shared), binds the whole result tuple
/// to one temp, then projects each hole out of it by field index.
pub(super) fn lower_str_match_pattern_bindings(pattern: &Pattern, cx: &Cx, env: &mut LowerEnv) -> Vec<TStmt> {
    let (closure, holes) = str_match_scan_closure(pattern, cx);
    if holes.is_empty() {
        return Vec::new();
    }
    let tuple_ty_str = tuple_join(
        &holes
            .iter()
            .map(|(_, t)| cx.rust_type(t))
            .collect::<Vec<_>>(),
    );
    // `TStmt::Let` always mangles its `name` at emission (Codegen/TIR/emit.rs),
    // so the tuple temp's REFERENCED name must go through the same `mangle`.
    let tuple_local = "__jet_sm_tuple";
    let tuple_rust = mangle(tuple_local);
    let mut out = vec![TStmt::Let {
        name: tuple_local.to_string(),
        kw: "let",
        ty_clause: format!(": ({})", tuple_ty_str),
        init: TExpr {
            ty: Type::Bool,
            kind: TExprKind::ConstInline(format!("({}).unwrap()", closure)),
        },
        track_origin: None,
                gc_promotion: None,
                gc_transferred: false,
    }];
    let single = holes.len() == 1;
    for (i, (name, ty)) in holes.iter().enumerate() {
        let local_rust = mangle(name);
        env.bind(name, local_rust, Some(ty.clone()));
        let project = if single {
            // A one-element Rust tuple is `(x,)`; its sole field is still `.0`.
            format!("{}.0", tuple_rust)
        } else {
            format!("{}.{}", tuple_rust, i)
        };
        out.push(TStmt::Let {
            name: name.clone(),
            kw: "let",
            ty_clause: format!(": {}", cx.rust_type(ty)),
            init: TExpr {
                ty: ty.clone(),
                kind: TExprKind::ConstInline(project),
            },
            track_origin: None,
                gc_promotion: None,
                gc_transferred: false,
        });
    }
    out
}

/// D-BINPAT1 (card #506): the bool test for a binary-pattern arm head —
/// whether the bit-scan closure succeeds. Always refutable (E0148).
pub(super) fn bin_match_pattern_cond_expr(pattern: &Pattern, cx: &Cx) -> TExpr {
    let (closure, _) = crate::Codegen::TIR::lower::bin_match_scan_closure(pattern, cx);
    TExpr {
        ty: Type::Bool,
        kind: TExprKind::ConstInline(format!("({}).is_some()", closure)),
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
    let (closure, holes) = crate::Codegen::TIR::lower::bin_match_scan_closure(pattern, cx);
    if holes.is_empty() {
        return Vec::new();
    }
    let tuple_ty_str = tuple_join(
        &holes
            .iter()
            .map(|(_, t)| cx.rust_type(t))
            .collect::<Vec<_>>(),
    );
    let tuple_local = "__jet_bm_tuple";
    let tuple_rust = mangle(tuple_local);
    let mut out = vec![TStmt::Let {
        name: tuple_local.to_string(),
        kw: "let",
        ty_clause: format!(": ({})", tuple_ty_str),
        init: TExpr {
            ty: Type::Bool,
            kind: TExprKind::ConstInline(format!("({}).unwrap()", closure)),
        },
        track_origin: None,
                gc_promotion: None,
                gc_transferred: false,
    }];
    let single = holes.len() == 1;
    for (i, (name, ty)) in holes.iter().enumerate() {
        let local_rust = mangle(name);
        env.bind(name, local_rust, Some(ty.clone()));
        let project = if single {
            format!("{}.0", tuple_rust)
        } else {
            format!("{}.{}", tuple_rust, i)
        };
        out.push(TStmt::Let {
            name: name.clone(),
            kw: "let",
            ty_clause: format!(": {}", cx.rust_type(ty)),
            init: TExpr {
                ty: ty.clone(),
                kind: TExprKind::ConstInline(project),
            },
            track_origin: None,
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

pub(super) fn struct_pattern_subject_field_expr(cx: &Cx, subject_ty: &Type, field: &str) -> String {
    let field_rust =
        core_struct_field_rust_name(cx, subject_ty, field).unwrap_or_else(|| mangle(field));
    format!("((*_jet_switch_subject).{})", field_rust)
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
pub(crate) fn lower_fallible_match(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // The subject's resolved type carries the ok/err/present payload types. Lower the
    // subject once to get both its emitted string and its total type.
    let subject_t = lower_expr(subject, cx, env);
    let subject_ty = subject_t.ty.clone();
    // A by-reference enum param is cloned in the enum-match path; a fallible/optional
    // subject in-subset is never a deref'd slot (it is a fn-call value or an owned
    // local), so the scrutinee is the plain emitted form — matching the AST path,
    // whose `subj` clone branch only fires for a deref'd `Ident`.
    let scrutinee = match subject {
        Expr::Ident(name, _) if env.is_borrowed(name) => {
            format!("({}).clone()", env.rust_name_of(name))
        }
        _ => emit_tir_expr(&subject_t, cx),
    };
    let mut tarms = Vec::new();
    for arm in arms {
        let pattern =
            arm_fallible_pattern(cx, &arm.cond, subject).expect("gate proved fallible arm");
        let pat = tir_fallible_pattern(&pattern);
        // An arm body is a CLONED env in `emit_pattern_match_switch` (no leak) — fork.
        let mut body_env = fork_panic(env);
        tir_add_fallible_binding(&pattern, &mut body_env, &subject_ty);
        let body = lower_stmts(&arm.body, cx, &mut body_env);
        tarms.push(TMatchArm {
            pattern: pat,
            guard: None,
            body,
        });
    }
    // The `else` arm has its own lexical bindings.
    let else_lowered = else_body.as_ref().map(|body| {
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    });
    // No explicit `else` → the AST path (`emit_pattern_match_switch`) appends
    // `_ => unreachable!(…)` so rustc sees a complete match (sema proved E0307).
    let fallthrough = else_body.is_none();
    TStmt::EnumMatch {
        scrutinee,
        arms: tarms,
        else_body: else_lowered,
        fallthrough,
    }
}

/// c109 Phase 8: the Rust match pattern for a fallible/optional pattern, mirroring
/// `emit_match_pattern`'s Ok/Err/Present/Absent arms (Statement.rs).
pub(crate) fn tir_fallible_pattern(pattern: &Pattern) -> String {
    match pattern {
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
        Pattern::Present { binding, .. } => format!("Some({})", mangle(binding)),
        Pattern::Absent(_) => "None".to_string(),
        _ => unreachable!("non-fallible pattern in fallible match (gate)"),
    }
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
    env.bind(&binding, mangle(&binding), ty);
}

pub(crate) fn lower_enum_match(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // The match owns the value. Mirror `emit_pattern_match_switch`: a by-reference
    // subject (a deref'd enum param) is cloned as `({rust_name}).clone()` — the
    // borrow itself is cloned, NOT the deref'd place. Any other subject emits its
    // plain form.
    let scrutinee = match subject {
        Expr::Ident(name, _) if env.is_borrowed(name) => {
            format!("({}).clone()", env.rust_name_of(name))
        }
        _ => emit_tir_expr(&lower_expr(subject, cx, env), cx),
    };
    // Resolve the owning enum once — drives the Rust variant prefix in patterns.
    let enum_type = arms.iter().find_map(|a| {
        arm_variant_pattern(cx, &a.cond, subject).and_then(|p| variant_pattern_enum(cx, &p))
    });
    // The subject's resolved Jet type carries the variant binding payload types.
    let subject_ty = expr_ast_jet_ty(subject, env);
    let mut tarms = Vec::new();
    for arm in arms {
        let pattern = arm_variant_pattern(cx, &arm.cond, subject).expect("gate proved variant arm");
        let pat = tir_match_pattern(cx, &pattern, enum_type.as_deref());
        let guard = tir_range_guard(&pattern);
        // The arm body sees the variant's payload bindings, typed from the layout. The
        // arm body is a CLONED env in `emit_pattern_match_switch` (no leak) — fork.
        let mut body_env = fork_panic(env);
        tir_add_pattern_bindings(cx, &pattern, &mut body_env, subject_ty.as_ref());
        let body = lower_stmts(&arm.body, cx, &mut body_env);
        tarms.push(TMatchArm {
            pattern: pat,
            guard,
            body,
        });
    }
    // The `else` arm has its own lexical bindings.
    let else_lowered = else_body.as_ref().map(|body| {
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    });
    // No explicit `else` → the AST path appends `_ => unreachable!(…)` so rustc
    // sees a complete match (sema already proved exhaustiveness — E0307).
    let fallthrough = else_body.is_none();
    TStmt::EnumMatch {
        scrutinee,
        arms: tarms,
        else_body: else_lowered,
        fallthrough,
    }
}

pub(crate) fn lower_range_switch(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // The subject's emitted string — used for the borrow binding and each range
    // condition, exactly as `emit_mixed_switch` re-emits the subject.
    let subject_str = emit_tir_expr(&lower_expr(subject, cx, env), cx);
    let mut tarms = Vec::new();
    for arm in arms {
        let (lo, hi) = arm_head_range(cx, &arm.cond, subject).expect("gate proved range arm");
        let mut branch = clone_env(env);
        let body = lower_stmts(&arm.body, cx, &mut branch);
        tarms.push((lo, hi, body));
    }
    let else_lowered = {
        let body = else_body
            .as_ref()
            .expect("range switch requires else (gate)");
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    };
    TStmt::RangeSwitch {
        subject_str,
        arms: tarms,
        else_body: else_lowered,
    }
}

/// TIR-local reproduction of codegen's `emit_match_pattern` for the enum-match (shape
/// A) case the subset covers. c109 Phase 24: this now DELEGATES to the AST
/// `emit_match_pattern` (made `pub(crate)`), which is PURE formatting (it takes only
/// `cx` + the pattern + the resolved enum type — no env, no inference), so reusing it is
/// byte-parity-safe and automatically handles the FOREIGN-enum (`{root}{mod}::user_<T>::
/// user_<V>`) and JSON (`{root}jet_std::Json::<Variant>`, non-mangled) variant prefixes
/// the subset now admits — the same reuse Phase 22 made for `emit_if_let_pattern`.
pub(crate) fn tir_match_pattern(cx: &Cx, pattern: &Pattern, enum_type: Option<&str>) -> String {
    emit_match_pattern(cx, pattern, enum_type)
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
                        Some(format!(
                            "__jet_range_{} >= {} && __jet_range_{} <= {}",
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
            let tys = hook_payload.map(|ty| vec![ty]).or_else(|| variant_payload_types(cx, variant));
            for (i, slot) in bindings.iter().enumerate() {
                if let PatSlot::Bind { name, .. } = slot {
                    // Payload types are scalar/Char (the enum is covered), so the
                    // binding is a by-value local; default to Int if unresolved
                    // (impossible for a covered enum — sema validated the access).
                    let ty = tys
                        .as_ref()
                        .and_then(|ts| ts.get(i).cloned())
                        .unwrap_or(Type::Int);
                    env.bind(name, mangle(name), Some(ty));
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
pub(crate) fn variant_payload_types(cx: &Cx, variant: &str) -> Option<Vec<Type>> {
    variant_binding_types(cx, variant)
}

/// c109 Phase 24: the Rust enum-literal head `{prefix}::{mangle(variant)}` for a payload
/// or named enum literal, reproducing `emit_enum_lit`'s `type_prefix` (Expression.rs): a
/// FOREIGN (imported) enum → `{root}{mod}::user_<T>::user_<V>`, a local enum →
/// `user_<T>::user_<V>`. Keyed on the ENUM name in `cx.foreign_types`, byte-for-byte.
pub(crate) fn tir_enum_lit_prefix(cx: &Cx, type_name: &str, variant: &str) -> String {
    // D-TERM1 (ratified 2026-06-22): `Key` is a prelude enum; its Rust name is `JetKey`.
    // Variant names are not mangled (Char, Enter, …).
    if type_name == crate::Syntax::TYPE_KEY {
        return format!("{}JetKey::{}", cx.root_prefix, variant);
    }
    if type_name == "DataEvent" {
        return format!("{}jet_std::DataEvent::{}", cx.root_prefix, variant);
    }
    if matches!(type_name, "XMLEncoding" | "XMLLexicalPolicy" | "XMLCanonicalMode") {
        return format!("{}jet_std::{}::{}", cx.root_prefix, type_name, variant);
    }
    // D-PROCESS1=A: `ProcessStreamMode` is a core dot-literal enum (`.Stream`/
    // `.Inherit`/`.Capture`) — its Rust type lives in `jet_std`, plain variant names.
    if type_name == "ProcessStreamMode" {
        return format!("{}jet_std::ProcessStreamMode::{}", cx.root_prefix, variant);
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
    if type_name == "TlsClientTrust" {
        return format!("{}JetTlsTrust::{}", cx.root_prefix, variant);
    }
    if type_name == "TlsVersion" {
        return format!("{}JetTlsVersion::{}", cx.root_prefix, variant);
    }
    if matches!(type_name, "IOError" | "IOOperation") {
        return format!("{}jet_std::{}::{}", cx.root_prefix, if type_name == "IOError" { "IoError" } else { "IoOperation" }, variant);
    }
    if matches!(type_name, "HttpError" | "HttpOperation" | "HttpProxy") {
        return format!("{}Jet{}::{}", cx.root_prefix, type_name, variant);
    }
    if matches!(type_name, "SmtpSecurity" | "RecipientPolicy" | "EmailError" | "SmtpAuth" | "TlsTrust") {
        let rust = if type_name == "EmailError" { "Error" } else { type_name };
        return format!("{}jet_email::{}::{}", cx.root_prefix, rust, variant);
    }
    if type_name == "AuthError" {
        return format!("{}JetAuthError::{}", cx.root_prefix, variant);
    }
    let type_prefix = match cx.foreign_types.get(type_name) {
        Some(rust_mod) => format!("{}{}::user_{}", cx.root_prefix, rust_mod, type_name),
        None => format!("user_{}", type_name),
    };
    format!("{}::{}", type_prefix, mangle_variant(variant))
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
    TEnumArg {
        value: lower_expr(e, cx, env),
        clone,
        boxed,
    }
}
