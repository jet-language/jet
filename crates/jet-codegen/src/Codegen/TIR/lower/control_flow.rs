use crate::AST::{BinOp, ElseBranch, Expr, IfStmt, PatSlot, Pattern, Stmt, StructPatField, SwitchArm, Type};
use crate::Codegen::Cx;
use crate::Codegen::emit_if_let_pattern;
use crate::Codegen::emit_match_pattern;
use crate::Codegen::escape_rust_str;
use crate::Codegen::is_json_variant;
use crate::Codegen::is_key_variant;
use crate::Codegen::mangle;
use crate::Codegen::TIR::arm_fallible_pattern;
use crate::Codegen::TIR::arm_head_range;
use crate::Codegen::TIR::arm_is_plain_cond;
use crate::Codegen::TIR::arm_bin_match_pattern;
use crate::Codegen::TIR::arm_str_match_pattern;
use crate::Codegen::TIR::arm_struct_pattern;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::fork_panic;
use crate::Codegen::TIR::lower::bool_and_chain;
use crate::Codegen::TIR::lower_enum_match;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_fallible_match;
use crate::Codegen::TIR::lower::lower_str_match_pattern_bindings;
use crate::Codegen::TIR::lower::lower_bin_match_pattern_bindings;
use crate::Codegen::TIR::lower_range_switch;
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::lower::str_match_pattern_cond_expr;
use crate::Codegen::TIR::lower::bin_match_pattern_cond_expr;
use crate::Codegen::TIR::lower::struct_pattern_field_type;
use crate::Codegen::TIR::lower::struct_pattern_subject_field_expr;
use crate::Codegen::TIR::static_call_type_name_unchecked;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TForInMethod;
use crate::Codegen::TIR::TIfCond;
use crate::Codegen::TIR::tir_recv_jet_ty;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::variant_binding_types;
use crate::Diagnostics::Span;
use crate::Syntax;

pub(super) fn tracked_float_origin(b: &crate::AST::Binding, ty: &Type, cx: &Cx) -> Option<String> {
    if !b.track || !matches!(ty, Type::Float) {
        return None;
    }
    let (line, col) = crate::Diagnostics::span_line_col(&cx.src, b.name_span.start);
    let snippet = cx
        .src
        .lines()
        .nth(line.saturating_sub(1))
        .unwrap_or("")
        .trim();
    Some(format!(
        "tracked `{}` at {}:{}:{}: {}",
        b.name, cx.file, line, col, snippet
    ))
}

pub(super) fn static_call_type_name_lower(receiver: &Expr, env: &LowerEnv) -> Option<String> {
    let name = static_call_type_name_unchecked(receiver)?;
    match receiver {
        Expr::Ident(n, _) if env.locals.contains_key(n) => None,
        Expr::Field(base, _, _) => {
            if let Expr::Ident(prefix, _) = base.as_ref() {
                if env.locals.contains_key(prefix) {
                    return None;
                }
            }
            Some(name)
        }
        _ => Some(name),
    }
}

/// Pull the bare label name out of an `@name` loop label, dropping the span. The
/// emitter renders it as `'jet_<name>:` (mirroring `loop_label_prefix`).
pub(crate) fn label_name(label: &Option<(String, Span)>) -> Option<String> {
    label.as_ref().map(|(n, _)| n.clone())
}

/// c109 Phase 22: resolve a `loop x in <coll>` collection into its emitted Rust
/// string + (for a method-call collection) the iteration form, reproducing
/// `emit_for_in`'s branch selection (Source/Codegen/Statement.rs) byte-for-byte.
/// For `chars`/`lines` the returned string is the *receiver* (the form emits
/// `({recv}).chars()` / `BufRead::lines(&mut ({recv}).inner)`); for the plain form
/// (incl. a non-special method call routed to `.iter().cloned()`) it is the whole
/// collection. The FileReader-vs-stdin `lines` split mirrors the AST's
/// `expr_jet_ty(receiver)` / inline-`io.stdin()` test exactly.
pub(crate) fn lower_forin_collection(
    collection: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> (String, Option<TForInMethod>) {
    if let Expr::MethodCall {
        receiver, method, ..
    } = collection
    {
        match method.as_str() {
            "chars" => {
                let recv = emit_tir_expr(&lower_expr(receiver, cx, env), cx);
                return (recv, Some(TForInMethod::Chars));
            }
            "lines" => {
                // FileReader streaming vs stdin streaming — the AST tests
                // `expr_jet_ty(receiver)` (reproduced by `tir_recv_jet_ty`) for the
                // FileReader case, then a `StdinHandle` type OR an inline `io.stdin()`
                // receiver for the stdin case. Checked in the SAME order as
                // `emit_for_in` (FileReader first).
                let recv = emit_tir_expr(&lower_expr(receiver, cx, env), cx);
                if matches!(tir_recv_jet_ty(receiver, env), Some(Type::Named(n)) if n == "FileReader")
                {
                    return (recv, Some(TForInMethod::LinesFile));
                }
                // stdin: a `StdinHandle`-typed receiver OR an inline `io.stdin()` call.
                let is_stdin = matches!(tir_recv_jet_ty(receiver, env), Some(Type::Named(n)) if n == "StdinHandle")
                    || matches!(receiver.as_ref(), Expr::MethodCall { method: m, .. } if m == "stdin");
                if is_stdin {
                    return (recv, Some(TForInMethod::LinesStdin));
                }
                // D-PROCESS1=A: `child.stdout.lines()` / `child.stderr.lines()` — the
                // receiver is a `Field` read (`tir_recv_jet_ty` never resolves a
                // `Field`, by design — mirrors `expr_jet_ty`), so test its BASE's
                // type instead: `child` (or any `ProcessChild`-typed expr) `.stdout`
                // / `.stderr`.
                let is_process_stream = matches!(receiver.as_ref(), Expr::Field(base, member, _)
                    if (member == "stdout" || member == "stderr")
                        && matches!(tir_recv_jet_ty(base, env), Some(Type::Named(n)) if n == "ProcessChild"));
                if is_process_stream {
                    return (recv, Some(TForInMethod::LinesProcessStream));
                }
                // A `.lines()` on neither (unreachable in valid Jet — sema E2502
                // restricts `.lines()` to a FileReader/StdinHandle loop position) would
                // fall to the AST `else` default; reproduce that for totality.
                let coll = emit_tir_expr(&lower_expr(collection, cx, env), cx);
                (coll, None)
            }
            _ => {
                // The `.iter().cloned()` default: emit the WHOLE method call as the
                // collection value (e.g. a `.split(…)` builtin returning a `[String]`).
                let coll = emit_tir_expr(&lower_expr(collection, cx, env), cx);
                (coll, None)
            }
        }
    } else {
        let coll = emit_tir_expr(&lower_expr(collection, cx, env), cx);
        (coll, None)
    }
}

/// c109 Phase 22: lower an `if` condition into a `TIfCond`, plus the optional
/// then-branch binding the condition introduces (name, rust place, resolved type).
/// Reproduces `emit_if`'s condition handling (Source/Codegen/Statement.rs):
///  - `x == null` (`Pattern::Absent`) → `IsNone` (no binding);
///  - `value(b)`/`Ok(b)`/`Err(b)` → `IfLet` with the Rust pattern from
///    `emit_if_let_pattern`, the binding's type resolved off the subject's lowered
///    `Option`/`Result` (mirroring `add_pattern_bindings`);
///  - binding-free user enum variant/group tests (`d == .Fire`) → `Matches`;
///  - anything else → `Plain`.
pub(super) fn is_binding_free_user_variant_pattern_test(pattern: &Pattern, cx: &Cx) -> bool {
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            bindings.is_empty()
                && !is_json_variant(variant)
                && !is_key_variant(variant)
                && cx.variant_owner.contains_key(variant)
        }
        _ => false,
    }
}

pub(super) fn lower_binding_free_variant_pattern_test(
    subject: &Expr,
    pattern: &Pattern,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    let subj = lower_expr(subject, cx, env);
    let variant = match pattern {
        Pattern::Variant { variant, .. } => variant,
        _ => unreachable!("binding-free variant gate admitted non-variant"),
    };
    let subject_enum = match &subj.ty {
        Type::Named(name) | Type::Apply { name, .. } => {
            let resolved = cx
                .core_qualified_rust_type_name(name)
                .unwrap_or(name.as_str());
            cx.enum_variants
                .get(resolved)
                .is_some_and(|variants| {
                    variants
                        .iter()
                        .any(|(candidate, _)| candidate == variant)
                })
                .then_some(resolved)
        }
        _ => None,
    };
    let enum_type = subject_enum.or_else(|| cx.variant_owner.get(variant).map(String::as_str));
    let pat_str = match enum_type {
        Some(type_name) if cx.enum_variants.get(type_name).is_some_and(|variants| {
            variants.iter().any(|(candidate, _)| candidate == variant)
        }) => crate::Codegen::TIR::tir_enum_lit_prefix(cx, type_name, variant),
        _ => emit_match_pattern(cx, pattern, enum_type),
    };
    TExpr {
        ty: Type::Bool,
        kind: TExprKind::PatternMatches {
            subj: Box::new(subj),
            pat_str,
        },
    }
}

pub(crate) fn lower_if_cond(
    cond: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> (TIfCond, Option<(String, String, Option<Type>)>, Vec<TStmt>) {
    if let Expr::PatternTest {
        subject,
        pattern: Pattern::Absent(_),
        ..
    } = cond
    {
        let subj = lower_expr(subject, cx, env);
        return (TIfCond::IsNone { subj }, None, Vec::new());
    }
    // D-ENC-DYN1=A+: a dynamic `Data` variant if-let (`if data == Object(entries)` /
    // `if n == Int(v)`). The Rust if-let pattern is `{root}jet_std::DataTree::<Variant>(…)`;
    // the binding's type comes from `core_json_pattern_types`. Scalars/`Array` bind their
    // inner field directly. `Object` is special: `DataTree::Object` is ordered
    // `Vec<(String, DataTree)>`, but the user-facing payload is a `Map<String, Data>`, so
    // the pattern binds the pairs to a temp and a then-body prefix `let` collects them into
    // a `BTreeMap` (the value the body sees).
    if let Expr::PatternTest {
        subject,
        pattern:
            pattern @ Pattern::Variant {
                variant,
                bindings,
                span: pat_span,
            },
        ..
    } = cond
    {
        if is_json_variant(variant) {
            if let Some(PatSlot::Bind(name)) = bindings.first() {
                let subj = lower_expr(subject, cx, env);
                let ty = crate::Sema::core_json_pattern_types(variant)
                    .and_then(|ts| ts.into_iter().next());
                let place = mangle(name);
                if variant == "Object" {
                    let obj_tmp = format!("__jet_obj{}", pat_span.start);
                    let pat_str =
                        format!("{}jet_std::DataTree::Object({})", cx.root_prefix, obj_tmp);
                    let map_ty = ty.clone().unwrap_or(Type::Map {
                        key: Box::new(Type::String),
                        key_span: None,
                        value: Box::new(Type::Named(Syntax::TYPE_DATA.to_string())),
                    });
                    let prefix = TStmt::Let {
                        name: name.clone(),
                        kw: "let",
                        ty_clause: format!(": {}", cx.rust_type(&map_ty)),
                        init: TExpr {
                            ty: map_ty.clone(),
                            kind: TExprKind::ConstInline(format!(
                                "{}.into_iter().collect()",
                                obj_tmp
                            )),
                        },
                        track_origin: None,
                gc_promotion: None,
                gc_transferred: false,
                    };
                    return (
                        TIfCond::IfLet { pat_str, subj },
                        Some((name.clone(), place, Some(map_ty))),
                        vec![prefix],
                    );
                }
                let pat_str = emit_if_let_pattern(cx, pattern);
                return (
                    TIfCond::IfLet { pat_str, subj },
                    Some((name.clone(), place, ty)),
                    Vec::new(),
                );
            }
        }
        // c109 (B4): a USER-enum variant if-let (`if m == Ping(n)`). The Rust if-let
        // pattern is the same `emit_if_let_pattern` (`user_E::user_V(user_b)`), and the
        // binding's type is the variant's first payload type from `variant_binding_types`
        // (the same total fact `add_pattern_bindings` reads on the AST path).
        if !is_json_variant(variant) {
            if let Some(PatSlot::Bind(name)) = bindings.first() {
                let subj = lower_expr(subject, cx, env);
                let pat_str = emit_if_let_pattern(cx, pattern);
                let ty = variant_binding_types(cx, variant).and_then(|ts| ts.into_iter().next());
                let place = mangle(name);
                return (
                    TIfCond::IfLet { pat_str, subj },
                    Some((name.clone(), place, ty)),
                    Vec::new(),
                );
            }
            // c109 (D-PATW): a WILDCARD payload slot (`if w == Some(_)`). `_` binds
            // nothing, so the if-let introduces NO then-branch binding; the pattern
            // renders the slot as `_` (`emit_if_let_pattern`), byte-for-byte the AST.
            if let Some(PatSlot::Wildcard) = bindings.first() {
                let subj = lower_expr(subject, cx, env);
                let pat_str = emit_if_let_pattern(cx, pattern);
                return (TIfCond::IfLet { pat_str, subj }, None, Vec::new());
            }
        }
    }
    if let Expr::PatternTest {
        subject, pattern, ..
    } = cond
    {
        if matches!(
            pattern,
            Pattern::Present { .. } | Pattern::Ok { .. } | Pattern::Err { .. }
        ) {
            let subj = lower_expr(subject, cx, env);
            let pat_str = emit_if_let_pattern(cx, pattern);
            // The bound name + its inner type, off the subject's resolved Option/Result
            // (totality — never re-inferred). Mirrors `add_pattern_bindings`.
            let binding = match pattern {
                Pattern::Present { binding, .. } => {
                    let ty = match &subj.ty {
                        Type::Option(inner) => Some((**inner).clone()),
                        _ => None,
                    };
                    (binding.clone(), ty)
                }
                Pattern::Ok { binding, .. } => {
                    let ty = match &subj.ty {
                        Type::Result { ok, .. } => Some((**ok).clone()),
                        _ => None,
                    };
                    (binding.clone(), ty)
                }
                Pattern::Err { binding, .. } => {
                    let ty = match &subj.ty {
                        Type::Result { err, .. } => Some((**err).clone()),
                        _ => None,
                    };
                    (binding.clone(), ty)
                }
                _ => unreachable!("checked above"),
            };
            let (name, ty) = binding;
            let place = mangle(&name);
            return (
                TIfCond::IfLet { pat_str, subj },
                Some((name, place, ty)),
                Vec::new(),
            );
        }
    }
    if let Expr::PatternTest {
        subject, pattern, ..
    } = cond
    {
        if is_binding_free_user_variant_pattern_test(pattern, cx) {
            let subj = lower_expr(subject, cx, env);
            let enum_type = match pattern {
                Pattern::Variant { variant, .. } => {
                    cx.variant_owner.get(variant).map(String::as_str)
                }
                _ => None,
            };
            let pat_str = emit_match_pattern(cx, pattern, enum_type);
            return (TIfCond::Matches { pat_str, subj }, None, Vec::new());
        }
    }
    (TIfCond::Plain(lower_expr(cond, cx, env)), None, Vec::new())
}

pub(crate) fn lower_if(ifs: &IfStmt, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    // c109 Phase 22: classify the condition (plain / if-let / is_none), reproducing
    // `emit_if`'s three head shapes. The if-let form binds its name into the
    // then-branch scope (mirroring `add_pattern_bindings`).
    let (cond, then_binding, then_prefix) = lower_if_cond(&ifs.cond, cx, env);
    // Each branch gets its own lexical scope, so its bindings are available to panic
    // context inside the branch but not after the `if`.
    let then_body = {
        let mut branch = if then_binding.is_some() {
            fork_panic(env)
        } else {
            clone_env(env)
        };
        if let Some((name, place, ty)) = then_binding {
            branch.bind(&name, place, ty);
        }
        // D-ENC-DYN1=A+: a `Data` `Object(entries)` if-let prepends a `let` that
        // collects the matched `Vec<(String, DataTree)>` pairs into the `BTreeMap` the
        // body sees. Emitted before the source body statements.
        let mut body = then_prefix;
        body.extend(lower_stmts(&ifs.then_body, cx, &mut branch));
        body
    };
    let (else_body, else_is_elseif) = match &ifs.else_branch {
        None => (None, false),
        Some(ElseBranch::Else(body)) => {
            let mut branch = clone_env(env);
            (Some(lower_stmts(body, cx, &mut branch)), false)
        }
        // `else if` nests as an else-body holding a single `If`; the flag marks it so
        // emit renders `} else if …` (an explicit `else { if … }` block does NOT).
        Some(ElseBranch::ElseIf(next)) => {
            let mut branch = clone_env(env);
            (Some(vec![lower_if(next, cx, &mut branch)]), true)
        }
    };
    TStmt::If {
        cond,
        then_body,
        else_body,
        else_is_elseif,
    }
}

/// c109 Phase 4: lower a `when`/match. The gate (`switch_in_subset`) has already
/// proved one of the two covered shapes; pick the matching lowering.
pub(crate) fn lower_switch(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // Shape B: all arm-head ranges + else → if/else chain (`emit_mixed_switch`).
    if else_body.is_some()
        && arms
            .iter()
            .all(|a| arm_head_range(cx, &a.cond, subject).is_some())
    {
        return lower_range_switch(subject, arms, else_body, cx, env);
    }
    // Shape C (c109 Phase 8): all arms are fallible/optional patterns → a Rust match
    // over the subject's Result/Option (`Ok(..)`/`Err(..)`/`Some(..)`/`None`).
    if arms
        .iter()
        .all(|a| arm_fallible_pattern(cx, &a.cond, subject).is_some())
    {
        return lower_fallible_match(subject, arms, else_body, cx, env);
    }
    // Shape D (c109 Phase 15): all arms are plain comparison/Bool conds — or D-IF3 range
    // heads / D-DESTRUCT1 struct-pattern heads mixed in — → the general mixed
    // `if/else if … else` chain. (An all-range + else switch already routed to shape B
    // above; this catches the value+range mix.)
    if arms.iter().all(|a| {
        arm_is_plain_cond(cx, &a.cond, subject)
            || arm_head_range(cx, &a.cond, subject).is_some()
            || arm_struct_pattern(cx, &a.cond, subject).is_some()
            || arm_str_match_pattern(cx, &a.cond, subject).is_some()
            || arm_bin_match_pattern(cx, &a.cond, subject).is_some()
    }) {
        return lower_mixed_switch(subject, arms, else_body, cx, env);
    }
    // Shape A: exhaustive enum match (`emit_pattern_match_switch`).
    lower_enum_match(subject, arms, else_body, cx, env)
}

/// D-IF3: `subject >= lo && subject <= hi` as a lowered bool expression.
fn range_inclusive_cond(subject: &Expr, lo: i64, hi: i64, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let lo_e = TExpr {
        ty: Type::Int,
        kind: TExprKind::IntLit(lo, None),
    };
    let hi_e = TExpr {
        ty: Type::Int,
        kind: TExprKind::IntLit(hi, None),
    };
    let lhs_ge = lower_expr(subject, cx, env);
    let lhs_le = lower_expr(subject, cx, env);
    let ge = TExpr {
        ty: Type::Bool,
        kind: TExprKind::Binary {
            op: BinOp::Ge,
            overflow: false,
            line: 0,
            lhs: Box::new(lhs_ge),
            rhs: Box::new(lo_e),
        },
    };
    let le = TExpr {
        ty: Type::Bool,
        kind: TExprKind::Binary {
            op: BinOp::Le,
            overflow: false,
            line: 0,
            lhs: Box::new(lhs_le),
            rhs: Box::new(hi_e),
        },
    };
    TExpr {
        ty: Type::Bool,
        kind: TExprKind::Binary {
            op: BinOp::And,
            overflow: false,
            line: 0,
            lhs: Box::new(ge),
            rhs: Box::new(le),
        },
    }
}

/// c109 Phase 15: lower a MIXED comparison/Bool `when` switch (shape D) to a
/// `TStmt::MixedSwitch`, reproducing `emit_mixed_switch` (Source/Codegen/Statement.rs).
/// The subject is bound once to `_jet_switch_subject = &(subject)` (emitted for parity);
/// each arm's PLAIN condition is resolved to a Rust string at lowering (`emit_expr`); the
/// arm bodies + `else` are lowered in separate lexical environments.
pub(crate) fn lower_mixed_switch(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    let subject_expr = lower_expr(subject, cx, env);
    let subject_ty = subject_expr.ty.clone();
    let subject_str = emit_tir_expr(&subject_expr, cx);
    let mut tarms = Vec::new();
    for arm in arms {
        let struct_pat = arm_struct_pattern(cx, &arm.cond, subject);
        let str_match_pat = arm_str_match_pattern(cx, &arm.cond, subject);
        let bin_match_pat = arm_bin_match_pattern(cx, &arm.cond, subject);
        let cond_expr = if let Some((lo, hi)) = arm_head_range(cx, &arm.cond, subject) {
            range_inclusive_cond(subject, lo, hi, cx, env)
        } else if let Some(pattern) = struct_pat.as_ref() {
            struct_pattern_cond_expr(pattern, &subject_ty, cx, env)
        } else if let Some(pattern) = str_match_pat.as_ref() {
            str_match_pattern_cond_expr(pattern, cx)
        } else if let Some(pattern) = bin_match_pat.as_ref() {
            bin_match_pattern_cond_expr(pattern, cx)
        } else {
            lower_expr(&arm.cond, cx, env)
        };
        // Each arm body has its own lexical bindings.
        let mut branch = clone_env(env);
        let mut body = if let Some(pattern) = struct_pat.as_ref() {
            lower_struct_pattern_bindings(pattern, &subject_ty, cx, &mut branch)
        } else if let Some(pattern) = str_match_pat.as_ref() {
            lower_str_match_pattern_bindings(pattern, cx, &mut branch)
        } else if let Some(pattern) = bin_match_pat.as_ref() {
            lower_bin_match_pattern_bindings(pattern, cx, &mut branch)
        } else {
            Vec::new()
        };
        body.extend(lower_stmts(&arm.body, cx, &mut branch));
        tarms.push((cond_expr, body));
    }
    let else_lowered = else_body.as_ref().map(|body| {
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    });
    TStmt::MixedSwitch {
        subject_str,
        arms: tarms,
        else_body: else_lowered,
    }
}

/// D-DESTRUCT1: value tests in `.{ field: value, ... }` become equality checks
/// against the borrowed `_jet_switch_subject` that `MixedSwitch` emits.
fn struct_pattern_cond_expr(
    pattern: &Pattern,
    subject_ty: &Type,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    let mut tests = Vec::new();
    if let Pattern::Struct { fields, .. } = pattern {
        for field in fields {
            let StructPatField::Value { field, value, .. } = field else {
                continue;
            };
            let fty = struct_pattern_field_type(cx, subject_ty, field).unwrap_or(Type::Int);
            let lhs = TExpr {
                ty: fty.clone(),
                kind: TExprKind::ConstInline(struct_pattern_subject_field_expr(
                    cx, subject_ty, field,
                )),
            };
            let rhs = lower_expr(value, cx, env);
            tests.push(TExpr {
                ty: Type::Bool,
                kind: TExprKind::Binary {
                    op: BinOp::Eq,
                    overflow: false,
                    line: 0,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            });
        }
    }
    bool_and_chain(tests)
}

/// D-DESTRUCT1: bind field locals before an arm body runs. Each binding clones from
/// the borrowed switch subject, matching the existing binding-position destructure.
fn lower_struct_pattern_bindings(
    pattern: &Pattern,
    subject_ty: &Type,
    cx: &Cx,
    env: &mut LowerEnv,
) -> Vec<TStmt> {
    let mut out = Vec::new();
    let Pattern::Struct { fields, .. } = pattern else {
        return out;
    };
    for field in fields {
        let StructPatField::Bind { field, local, .. } = field else {
            continue;
        };
        let fty = struct_pattern_field_type(cx, subject_ty, field).unwrap_or(Type::Int);
        let local_rust = mangle(local);
        env.bind(local, local_rust.clone(), Some(fty.clone()));
        out.push(TStmt::Let {
            name: local.clone(),
            kw: "let",
            ty_clause: format!(": {}", cx.rust_type(&fty)),
            init: TExpr {
                ty: fty,
                kind: TExprKind::ConstInline(format!(
                    "{}.clone()",
                    struct_pattern_subject_field_expr(cx, subject_ty, field)
                )),
            },
            track_origin: None,
                gc_promotion: None,
                gc_transferred: false,
        });
    }
    out
}

/// D-PARSESTR1: build the Rust closure body that scans the subject against a
/// str-match pattern's literal anchors and holes, returning
/// `Option<(T1, T2, ...)>` (`None` on any anchor miss or failed typed read).
/// Holes are non-greedy and anchored: since E0147 already rejected adjacent
/// holes at parse time, the next literal anchor (or end-of-string) always
/// exists between/after every hole, so the scan never backtracks — each step
/// is `starts_with`/`find` from the current cursor. Thin wrapper over
/// `str_match_scan_closure_ex` in full-match mode (the `if == {}` shape).
pub(super) fn str_match_scan_closure(pattern: &Pattern, cx: &Cx) -> (String, Vec<(String, Type)>) {
    let Pattern::StrMatch { parts, .. } = pattern else {
        return ("(|| -> Option<()> { Some(()) })()".to_string(), Vec::new());
    };
    str_match_scan_closure_ex(parts, cx, "_jet_switch_subject.as_str()", true)
}

/// D-PARSESTR1 (extended for D-SHIFT1's `Cursor.take_pattern` consume mode —
/// I8, one matcher engine, not a second one): the same scan engine as above,
/// generalized over WHERE the subject text comes from (`subject_src`, a raw
/// Rust expression) and whether the WHOLE subject must be consumed
/// (`require_full_match`). The `if == {}` shape uses `_jet_switch_subject`
/// and requires full consumption; `take_pattern` scans a local `__jet_tail`
/// slice and only needs a PREFIX match — the caller advances a cursor by the
/// consumed byte count, which (when `require_full_match` is false) is
/// appended as one extra trailing `usize` in the returned Rust tuple (not
/// reflected in the returned `holes` list — callers reconstruct the same
/// trailing slot themselves, `lower_cursor_take_pattern` below does exactly
/// that).
pub(crate) fn str_match_scan_closure_ex(
    parts: &[crate::AST::StrMatchPart],
    cx: &Cx,
    subject_src: &str,
    require_full_match: bool,
) -> (String, Vec<(String, Type)>) {
    let mut holes: Vec<(String, Type)> = Vec::new();
    let mut body = String::new();
    body.push_str(&format!("let mut __jet_sm: &str = {};\n", subject_src));
    if !require_full_match {
        body.push_str("let __jet_sm_orig_len: usize = __jet_sm.len();\n");
    }
    let mut i = 0;
    while i < parts.len() {
        match &parts[i] {
            crate::AST::StrMatchPart::Lit(lit) => {
                // A literal that immediately FOLLOWS a hole was already consumed as
                // that hole's trailing anchor (the `find` step below) — skip it here
                // to avoid double-consuming. Every other literal (leading, or between
                // two other literals — never actually produced by the lexer, but
                // handled uniformly) is a prefix check at the current cursor.
                let already_consumed_by_prior_hole =
                    i > 0 && matches!(parts[i - 1], crate::AST::StrMatchPart::Hole { .. });
                if !already_consumed_by_prior_hole {
                    body.push_str(&format!(
                        "if !__jet_sm.starts_with({lit}) {{ return None; }} __jet_sm = &__jet_sm[{lit}.len()..];\n",
                        lit = escape_rust_str(lit)
                    ));
                }
                i += 1;
            }
            crate::AST::StrMatchPart::Hole { name, ty, .. } => {
                let var = format!("__jet_sm_{}", mangle(name));
                // Find the boundary: the next literal anchor's text (non-greedy —
                // the FIRST occurrence from the cursor), or end-of-string if this
                // hole is the last part.
                match parts.get(i + 1) {
                    Some(crate::AST::StrMatchPart::Lit(next_lit)) => {
                        body.push_str(&format!(
                            "let {var}: &str = match __jet_sm.find({next_lit}) {{ Some(__jet_i) => {{ let __jet_h = &__jet_sm[..__jet_i]; __jet_sm = &__jet_sm[__jet_i + {next_lit}.len()..]; __jet_h }}, None => return None }};\n",
                            var = var,
                            next_lit = escape_rust_str(next_lit)
                        ));
                    }
                    _ => {
                        // Last part, or a hole followed by another hole (rejected at
                        // parse time by E0147) — take the rest of the string.
                        body.push_str(&format!(
                            "let {var}: &str = __jet_sm; __jet_sm = \"\";\n",
                            var = var
                        ));
                    }
                }
                let bound_ty = ty.clone().unwrap_or(Type::String);
                match &bound_ty {
                    Type::String => {
                        body.push_str(&format!(
                            "let {var}: String = {var}.to_string();\n",
                            var = var
                        ));
                    }
                    Type::Int => {
                        body.push_str(&format!(
                            "let {var}: i64 = match {var}.trim().parse::<i64>() {{ Ok(__jet_v) => __jet_v, Err(_) => return None }};\n",
                            var = var
                        ));
                    }
                    Type::Float => {
                        body.push_str(&format!(
                            "let {var}: f64 = match {var}.trim().parse::<f64>() {{ Ok(__jet_v) => __jet_v, Err(_) => return None }};\n",
                            var = var
                        ));
                    }
                    Type::Bool => {
                        body.push_str(&format!(
                            "let {var}: bool = match {var}.trim().parse::<bool>() {{ Ok(__jet_v) => __jet_v, Err(_) => return None }};\n",
                            var = var
                        ));
                    }
                    // sema already rejected any other type (E0305) before codegen runs.
                    _ => {}
                }
                holes.push((name.clone(), bound_ty));
                i += 1;
            }
        }
    }
    // A full-match pattern must consume the WHOLE subject, not just a
    // prefix. A trailing hole (no literal after it) already takes the rest
    // of the string by construction; a trailing literal only checked a
    // prefix, so require nothing is left over. `take_pattern`'s consume mode
    // (`!require_full_match`) skips this — the caller only wanted a PREFIX
    // matched, and reads the leftover tail via the consumed-length count.
    let ends_in_lit = matches!(parts.last(), Some(crate::AST::StrMatchPart::Lit(_)));
    if require_full_match && ends_in_lit {
        body.push_str("if !__jet_sm.is_empty() { return None; }\n");
    }
    let mut tuple_vars: Vec<String> = holes
        .iter()
        .map(|(n, _)| format!("__jet_sm_{}", mangle(n)))
        .collect();
    let mut tuple_tys: Vec<String> = holes.iter().map(|(_, t)| cx.rust_type(t)).collect();
    if !require_full_match {
        body.push_str("let __jet_consumed: usize = __jet_sm_orig_len - __jet_sm.len();\n");
        tuple_vars.push("__jet_consumed".to_string());
        tuple_tys.push("usize".to_string());
    }
    body.push_str(&format!("Some(({}))\n", tuple_join(&tuple_vars)));
    let closure = format!(
        "(|| -> Option<({})> {{\n{}}})()",
        tuple_join(&tuple_tys),
        body
    );
    (closure, holes)
}

/// D-BINPAT1 (card #506): build the Rust closure that scans a `[U8]` subject
/// against a binary pattern with a sequential MSB-first bit cursor, returning
/// `Option<(T1, T2, …)>` (`None` on any short read, byte mismatch, or a
/// pattern with no trailing rest that doesn't consume the whole subject). This
/// is the byte-mode sibling of `str_match_scan_closure` — one matcher engine
/// (I8) — and must read bit-for-bit identically to the tier-0 interpreter's
/// `Pattern::BinMatch` arm (R12 parity). Thin wrapper over
/// `bin_match_scan_closure_ex` in full-match mode (the `switch`-arm shape).
pub(crate) fn bin_match_scan_closure(pattern: &Pattern, cx: &Cx) -> (String, Vec<(String, Type)>) {
    let Pattern::BinMatch { parts, .. } = pattern else {
        return ("(|| -> Option<()> { Some(()) })()".to_string(), Vec::new());
    };
    bin_match_scan_closure_ex(parts, cx, "(_jet_switch_subject).as_slice()", true)
}

/// D-BINPAT1 (card #506 follow-up): the same bit-scan engine, generalized
/// over WHERE the subject bytes come from (`subject_src`, a raw Rust
/// expression yielding `&[u8]`) and whether the WHOLE subject must be
/// consumed (`require_full_match`) — the byte-mode sibling of
/// `str_match_scan_closure_ex`. `switch` uses the whole-buffer subject and
/// full consumption; `Reader.take_pattern` scans a local `__jet_tail` slice
/// and only needs a PREFIX match, ending on a byte boundary (a `Reader`
/// advances by whole bytes) — the caller advances the reader's position by
/// the consumed byte count, appended as one extra trailing `usize` in the
/// returned Rust tuple (not reflected in the returned `holes` list — callers
/// reconstruct the same trailing slot themselves, `lower_reader_take_pattern`
/// does exactly that, mirroring `lower_cursor_take_pattern`).
pub(crate) fn bin_match_scan_closure_ex(
    parts: &[crate::AST::BinMatchPart],
    cx: &Cx,
    subject_src: &str,
    require_full_match: bool,
) -> (String, Vec<(String, Type)>) {
    use crate::AST::{BinEndian, BinMatchPart, BinSpec};
    let mut holes: Vec<(String, Type)> = Vec::new();
    let mut body = String::new();
    body.push_str(&format!("let __jet_bm: &[u8] = {};\n", subject_src));
    body.push_str("let __jet_total: usize = __jet_bm.len().saturating_mul(8);\n");
    body.push_str("let mut __jet_pos: usize = 0;\n");
    let ends_in_rest = matches!(
        parts.last(),
        Some(BinMatchPart::Hole { spec: BinSpec::Rest, .. })
    );
    for part in parts {
        match part {
            BinMatchPart::Lit(bytes) => {
                let arr = bytes
                    .iter()
                    .map(|b| format!("{}u8", b))
                    .collect::<Vec<_>>()
                    .join(", ");
                body.push_str("if __jet_pos % 8 != 0 { return None; }\n");
                body.push_str(&format!(
                    "{{ let __s = __jet_pos / 8; let __lit: &[u8] = &[{arr}]; if __s + __lit.len() > __jet_bm.len() || &__jet_bm[__s..__s + __lit.len()] != __lit {{ return None; }} __jet_pos += __lit.len() * 8; }}\n",
                ));
            }
            BinMatchPart::Hole { name, spec, .. } => match spec {
                BinSpec::Rest => {
                    let var = format!("__jet_bm_{}", mangle(name));
                    body.push_str("if __jet_pos % 8 != 0 { return None; }\n");
                    body.push_str(&format!(
                        "let {var}: Vec<u8> = __jet_bm[__jet_pos / 8..].to_vec(); __jet_pos = __jet_total;\n"
                    ));
                    holes.push((
                        name.clone(),
                        Type::List(Box::new(Type::IntN { signed: false, bits: 8 })),
                    ));
                }
                BinSpec::Bits { width, endian } => {
                    let var = format!("__jet_bm_{}", mangle(name));
                    let ty = cx.rust_type(&bin_bits_type(*width));
                    let w = *width as usize;
                    body.push_str(&format!("if __jet_pos + {w} > __jet_total {{ return None; }}\n"));
                    body.push_str(&format!(
                        "let {var}: {ty} = {{ let mut __v: u64 = 0; let mut __k = 0usize; while __k < {w} {{ let __p = __jet_pos + __k; __v = (__v << 1) | ((__jet_bm[__p / 8] >> (7 - (__p % 8))) & 1) as u64; __k += 1; }} __jet_pos += {w}; "
                    ));
                    if matches!(endian, BinEndian::Little) {
                        let nb = w / 8;
                        body.push_str(&format!(
                            "{{ let mut __sw: u64 = 0; let mut __i = 0usize; while __i < {nb} {{ __sw |= ((__v >> (8 * __i)) & 0xff) << (8 * ({nb} - 1 - __i)); __i += 1; }} __v = __sw; }} "
                        ));
                    }
                    body.push_str(&format!("__v as {ty} }};\n"));
                    holes.push((name.clone(), bin_bits_type(*width)));
                }
            },
        }
    }
    // A full-match pattern (`switch`) must consume the WHOLE subject, not
    // just a prefix — a trailing `{...}` rest hole already takes the rest by
    // construction, so only a non-rest ending needs the check. A
    // `take_pattern` consume mode (`!require_full_match`) skips this — the
    // caller only wanted a PREFIX matched — but a `Reader` only advances by
    // whole bytes, so the match point must still land on a byte boundary.
    if require_full_match {
        if !ends_in_rest {
            body.push_str("if __jet_pos != __jet_total { return None; }\n");
        }
    } else {
        body.push_str("if __jet_pos % 8 != 0 { return None; }\n");
    }
    let mut tuple_vars: Vec<String> = holes
        .iter()
        .map(|(n, _)| format!("__jet_bm_{}", mangle(n)))
        .collect();
    let mut tuple_tys: Vec<String> = holes.iter().map(|(_, t)| cx.rust_type(t)).collect();
    if !require_full_match {
        body.push_str("let __jet_consumed: usize = __jet_pos / 8;\n");
        tuple_vars.push("__jet_consumed".to_string());
        tuple_tys.push("usize".to_string());
    }
    body.push_str(&format!("Some(({}))\n", tuple_join(&tuple_vars)));
    let closure = format!("(|| -> Option<({})> {{\n{}}})()", tuple_join(&tuple_tys), body);
    (closure, holes)
}

/// D-BINPAT1: the unsigned Rust integer type a fixed-width bit hole reads into
/// — matches sema's `bin_bits_type`.
pub(super) fn bin_bits_type(width: u8) -> Type {
    let bits = if width <= 8 {
        8
    } else if width <= 16 {
        16
    } else if width <= 32 {
        32
    } else {
        64
    };
    Type::IntN { signed: false, bits }
}

/// A single-element Rust tuple needs a trailing comma (`(x,)`); zero or 2+
/// elements render as the ordinary comma-joined list.
pub(crate) fn tuple_join(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => format!("{},", items[0]),
        _ => items.join(", "),
    }
}
