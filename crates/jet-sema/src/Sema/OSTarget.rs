//! D-OSTARGET1=A (ratified 2026-07-01, c134): native OS platform gating.
//! Mirrors `WebPartition.rs`'s shape — one structural check, one signature
//! check — for the second, mutually-exclusive `#Target(OS.*)` axis of the
//! same `#Target(...)` marker family.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax::{self, OSTarget as OS};
use crate::AST::{BinOp, Expr, Func, Item, LambdaBody, Pattern, ProgramBundle, Stmt, StrPart, SwitchArm, Type};
use std::collections::HashMap;

/// D-CONF-READ1=A: fold every `@if @build.os == {
/// .Linux -> … .MacOS -> … .Windows -> … [else -> …] }` switch to the arm
/// matching this build's active OS (`bundle.active_os`), discarding the rest.
///
/// Runs as the very first step of `check_bundle`, before any other sema pass or
/// codegen sees a body — so the OS-gating checks, type-checker, and codegen
/// only ever meet the *taken* arm (constructing an OS-gated type inside it is
/// legal; the dead arms never trip `E-OSTARGET-UNMATCHED-CALL` and never reach
/// rustc). The rewrite lowers each switch into a chain of `Stmt::ComptimeIf`
/// whose arm conditions are the compile-time constants `@build.os == .OS`
/// (emitted here as a `Bool` literal), so all the existing `@if`
/// machinery — arm selection, dropped-arm name resolution (D-WHEN2), codegen —
/// handles it unchanged. `@build.os` is a compile-time fact value; ordinary
/// `build` remains an ordinary identifier everywhere else.
pub fn desugar_os_switches(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    let active = bundle.active_os;
    let build_facts = &bundle.build_facts;
    let mut diags = Vec::new();
    for module in &mut bundle.modules {
        desugar_items(&mut module.items, active, build_facts, &mut diags);
    }
    diags
}

/// Walk nested code and generic modules before generic expansion. This keeps
/// `@build.os` dispatch in a template on the same pre-registration path as a
/// top-level function; expansion then copies the already-folded body.
fn desugar_items(
    items: &mut [Item],
    active: OS,
    build_facts: &jet_foundation::Facts::BuildFactSnapshot,
    diags: &mut Vec<Diagnostic>,
) {
    for item in items {
        match item {
            Item::Func(f) => desugar_stmts(&mut f.body, active, build_facts, diags),
            Item::Struct(s) => {
                for m in &mut s.methods {
                    desugar_stmts(&mut m.body, active, build_facts, diags);
                }
                for b in &mut s.trait_impls {
                    for m in &mut b.methods {
                        desugar_stmts(&mut m.body, active, build_facts, diags);
                    }
                }
            }
            Item::Enum(e) => {
                for m in &mut e.methods {
                    desugar_stmts(&mut m.body, active, build_facts, diags);
                }
            }
            Item::Impl(i) => {
                for m in &mut i.methods {
                    desugar_stmts(&mut m.body, active, build_facts, diags);
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &mut module.body {
                    desugar_items(body, active, build_facts, diags);
                }
            }
            Item::GenericModule(module) => desugar_items(&mut module.body, active, build_facts, diags),
            _ => {}
        }
    }
}

/// Recurse through every body-bearing statement, rewriting `ComptimeSwitch` in
/// place. Also descends into lambda block bodies and value-position `if`
/// branches so a switch nested in an expression is still folded.
fn desugar_stmts(
    stmts: &mut Vec<Stmt>,
    active: OS,
    build_facts: &jet_foundation::Facts::BuildFactSnapshot,
    diags: &mut Vec<Diagnostic>,
) {
    for stmt in stmts.iter_mut() {
        // Recurse into child blocks first, then fold this node if it is a switch.
        match stmt {
            Stmt::ComptimeSwitch { .. } => {
                // Fold nested switches inside the arms before rewriting this one.
                if let Stmt::ComptimeSwitch {
                    arms, else_body, ..
                } = stmt
                {
                    for arm in arms.iter_mut() {
                        desugar_stmts(&mut arm.body, active, build_facts, diags);
                    }
                    if let Some(eb) = else_body {
                        desugar_stmts(eb, active, build_facts, diags);
                    }
                }
                let taken = std::mem::replace(stmt, Stmt::Break(Span::new(0, 0)));
                *stmt = fold_switch(taken, active, build_facts, diags);
            }
            _ => desugar_child_blocks(stmt, active, build_facts, diags),
        }
    }
}

/// Descend into every statement body a non-switch statement carries.
fn desugar_child_blocks(
    stmt: &mut Stmt,
    active: OS,
    build_facts: &jet_foundation::Facts::BuildFactSnapshot,
    diags: &mut Vec<Diagnostic>,
) {
    match stmt {
        Stmt::Expr(e)
        | Stmt::Yield(e, _)
        | Stmt::DeferClose { close: e, .. } => desugar_expr(e, active, build_facts, diags),
        Stmt::Val(b) => desugar_expr(&mut b.init, active, build_facts, diags),
        Stmt::Assign { value, .. } => desugar_expr(value, active, build_facts, diags),
        Stmt::Return(Some(e), _) => desugar_expr(e, active, build_facts, diags),
        Stmt::While { body, .. } | Stmt::For { body, .. } => {
            desugar_stmts(body, active, build_facts, diags)
        }
        Stmt::Switch {
            arms, else_body, ..
        } => {
            for arm in arms {
                desugar_stmts(&mut arm.body, active, build_facts, diags);
            }
            if let Some(eb) = else_body {
                desugar_stmts(eb, active, build_facts, diags);
            }
        }
        Stmt::CountedLoop { body, step, .. } => {
            if let Some(step) = step { desugar_child_blocks(step, active, build_facts, diags); }
            desugar_stmts(body, active, build_facts, diags);
        }
        Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::ScopeMember { body, .. }
        | Stmt::ContextBlock { body, .. } => desugar_stmts(body, active, build_facts, diags),
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            desugar_stmts(then_body, active, build_facts, diags);
            if let Some(eb) = else_body {
                desugar_stmts(eb, active, build_facts, diags);
            }
        }
        _ => {}
    }
}

/// Descend into the few expression shapes that can hold statement blocks
/// (lambda block bodies, value-position `if` branches).
fn desugar_expr(
    e: &mut Expr,
    active: OS,
    build_facts: &jet_foundation::Facts::BuildFactSnapshot,
    diags: &mut Vec<Diagnostic>,
) {
    match e {
        Expr::Lambda(l) => {
            if let LambdaBody::Block(body) = &mut l.body {
                desugar_stmts(body, active, build_facts, diags);
            }
        }
        Expr::If {
            then_body,
            else_body,
            ..
        } => {
            desugar_stmts(then_body, active, build_facts, diags);
            desugar_stmts(else_body, active, build_facts, diags);
        }
        Expr::Paren(inner, _) => desugar_expr(inner, active, build_facts, diags),
        _ => {}
    }
}

/// Validate one `@if @build.os == { … }` switch and rewrite it into the
/// nested `ComptimeIf` chain. On a validation error, returns an empty block
/// (`ComptimeBlock` with no body) so the surrounding statements still check.
fn fold_switch(
    sw: Stmt,
    active: OS,
    build_facts: &jet_foundation::Facts::BuildFactSnapshot,
    diags: &mut Vec<Diagnostic>,
) -> Stmt {
    let Stmt::ComptimeSwitch {
        subject,
        arms,
        else_body,
        span,
    } = sw
    else {
        unreachable!("fold_switch only called on ComptimeSwitch");
    };

    // `@build.os` is the original platform switch. Typed settings use the
    // same dispatch shape and fold against the already-resolved snapshot.
    let subject_path = expr_path(&subject);
    if subject_path.as_deref() == Some("@build.os") {
        return fold_os_switch(subject, arms, else_body, span, active, diags);
    }
    let Some(key) = subject_path
        .as_deref()
        .and_then(jet_foundation::Registry::build_setting_key)
    else {
        diags.push(Syntax::os_target_build_context(Some(subject.span())));
        return empty_stmt(span);
    };
    let Some(setting) = build_facts.setting(key) else {
        diags.push(Diagnostic::error(
            "E0302",
            format!("`@build.settings.{key}` is undeclared"),
            "a setting must be declared with a type and default before it can be read",
            format!("add `{key}: Type = default` to the package `settings: .{{ … }}` block"),
            Some(subject.span()),
        ));
        return empty_stmt(span);
    };

    let mut selected: Option<Vec<Stmt>> = None;
    for arm in &arms {
        let Some(matches) = setting_arm_matches(&arm.cond, &subject, &setting.value) else {
            diags.push(Diagnostic::error(
                "E0302",
                format!("invalid arm for `@build.settings.{key}`"),
                "typed settings dispatch compares one setting with a literal value",
                format!("use a literal arm or compare `{key}` with a Bool, Int, Char, String, or enum value"),
                Some(arm.span),
            ));
            return empty_stmt(span);
        };
        if matches && selected.is_none() {
            selected = Some(arm.body.clone());
        }
    }
    Stmt::ComptimeBlock {
        body: selected.or(else_body).unwrap_or_default(),
        span,
    }
}

fn fold_os_switch(
    _subject: Expr,
    arms: Vec<SwitchArm>,
    else_body: Option<Vec<Stmt>>,
    span: Span,
    active: OS,
    diags: &mut Vec<Diagnostic>,
) -> Stmt {

    // Each arm head must be a bare, payload-free OS variant; collect them.
    let mut arm_os: Vec<(OS, Vec<Stmt>, Span)> = Vec::new();
    let mut seen: Vec<OS> = Vec::new();
    let mut had_error = false;
    for arm in arms {
        match arm_variant(&arm) {
            Some(os) if !seen.contains(&os) => {
                seen.push(os);
                arm_os.push((os, arm.body, arm.span));
            }
            Some(os) => {
                diags.push(Syntax::os_target_dispatch_arm(
                    &format!(".{}", os.name()),
                    Some(arm.span),
                ));
                had_error = true;
            }
            None => {
                diags.push(Syntax::os_target_dispatch_arm(
                    &arm_head_text(&arm.cond),
                    Some(arm.span),
                ));
                had_error = true;
            }
        }
    }
    if had_error {
        return empty_stmt(span);
    }

    // Exhaustiveness (build-independent): every OS covered, or an `else`.
    if else_body.is_none() {
        let missing: Vec<&str> = [OS::Linux, OS::MacOS, OS::Windows]
            .into_iter()
            .filter(|os| !seen.contains(os))
            .map(|os| os.name())
            .collect();
        if !missing.is_empty() {
            diags.push(Syntax::os_target_dispatch_exhaustive(&missing, Some(span)));
            return empty_stmt(span);
        }
    }

    // Build the nested `ComptimeIf` chain, arm 0 outermost. Each condition is
    // the compile-time constant `active == this arm's OS`, emitted as a `Bool`.
    let mut tail: Option<Vec<Stmt>> = else_body;
    for (os, body, arm_span) in arm_os.into_iter().rev() {
        let cond = Expr::Bool(os == active, arm_span);
        let node = Stmt::ComptimeIf {
            cond,
            cond_span: arm_span,
            then_body: body,
            else_body: tail,
            span: arm_span,
            selected_then: None,
        };
        tail = Some(vec![node]);
    }
    match tail.and_then(|mut v| v.pop()) {
        Some(chain) => chain,
        None => empty_stmt(span),
    }
}

fn expr_path(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name, _) | Expr::ComptimeName { name, .. } => Some(name.clone()),
        Expr::Field(base, member, _) => Some(format!("{}.{}", expr_path(base)?, member)),
        _ => None,
    }
}

fn setting_arm_matches(
    cond: &Expr,
    subject: &Expr,
    value: &jet_foundation::Facts::BuildFactValue,
) -> Option<bool> {
    match cond {
        Expr::Binary(op, lhs, rhs, _) if matches!(op, BinOp::Eq | BinOp::Ne) => {
            if expr_path(lhs) != expr_path(subject) {
                return None;
            }
            let literal = setting_literal(rhs)?;
            let equal = setting_values_equal(&literal, value)?;
            Some(if *op == BinOp::Eq { equal } else { !equal })
        }
        Expr::PatternTest {
            subject: lhs,
            pattern: Pattern::Variant {
                variant, bindings, ..
            },
            ..
        } if expr_path(lhs) == expr_path(subject) && bindings.is_empty() => match value {
            jet_foundation::Facts::BuildFactValue::Enum {
                variant: actual, ..
            } => Some(variant == actual),
            _ => None,
        },
        _ => setting_values_equal(&setting_literal(cond)?, value),
    }
}

fn setting_literal(expr: &Expr) -> Option<jet_foundation::Facts::BuildFactValue> {
    match expr {
        Expr::Bool(value, _) => Some(jet_foundation::Facts::BuildFactValue::Bool(*value)),
        Expr::Int(value, _, _, _) => Some(jet_foundation::Facts::BuildFactValue::Int(*value)),
        Expr::Char(value, _) => Some(jet_foundation::Facts::BuildFactValue::Char(*value)),
        Expr::Str(parts, _) if parts.iter().all(|part| matches!(part, StrPart::Lit(_))) => {
            let text = parts
                .iter()
                .map(|part| match part {
                    StrPart::Lit(text) => text.as_str(),
                    StrPart::Interp(..) => "",
                })
                .collect();
            Some(jet_foundation::Facts::BuildFactValue::Text(text))
        }
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } if args.is_empty() => Some(jet_foundation::Facts::BuildFactValue::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
        }),
        _ => None,
    }
}

fn setting_values_equal(
    left: &jet_foundation::Facts::BuildFactValue,
    right: &jet_foundation::Facts::BuildFactValue,
) -> Option<bool> {
    match (left, right) {
        (
            jet_foundation::Facts::BuildFactValue::Bool(left),
            jet_foundation::Facts::BuildFactValue::Bool(right),
        ) => Some(left == right),
        (
            jet_foundation::Facts::BuildFactValue::Int(left),
            jet_foundation::Facts::BuildFactValue::Int(right),
        ) => Some(left == right),
        (
            jet_foundation::Facts::BuildFactValue::Char(left),
            jet_foundation::Facts::BuildFactValue::Char(right),
        ) => Some(left == right),
        (
            jet_foundation::Facts::BuildFactValue::Text(left),
            jet_foundation::Facts::BuildFactValue::Text(right),
        ) => Some(left == right),
        (
            jet_foundation::Facts::BuildFactValue::Enum {
                type_name: left_type,
                variant: left,
            },
            jet_foundation::Facts::BuildFactValue::Enum {
                type_name: right_type,
                variant: right,
            },
        ) if left_type.is_empty() || right_type.is_empty() || left_type == right_type => {
            Some(left == right)
        }
        _ => None,
    }
}

/// The OS variant an arm head names, or `None` if it isn't a bare payload-free
/// OS variant (`.Linux`/`.MacOS`/`.Windows`).
fn arm_variant(arm: &SwitchArm) -> Option<OS> {
    let Expr::PatternTest { pattern, .. } = &arm.cond else {
        return None;
    };
    let Pattern::Variant {
        variant, bindings, ..
    } = pattern
    else {
        return None;
    };
    if !bindings.is_empty() {
        return None;
    }
    OS::parse(variant)
}

/// A short display of a bad arm head for the diagnostic.
fn arm_head_text(cond: &Expr) -> String {
    match cond {
        Expr::PatternTest { pattern, .. } => match pattern {
            Pattern::Variant { variant, .. } => format!(".{}", variant),
            _ => "this pattern".to_string(),
        },
        _ => "this arm head".to_string(),
    }
}

fn empty_stmt(span: Span) -> Stmt {
    Stmt::ComptimeBlock {
        body: Vec::new(),
        span,
    }
}

/// Walk the bundle: flag a `#Target(OS.*)`-gated impl whose enclosing file/
/// module also carries a web-bucket ceiling (`#Target(Wasm)`/`#Target(JS)`)
/// — a structural conflict between the two mutually-exclusive axes
/// (E-OSTARGET-MIXED-AXIS) — and flag a function/method that isn't itself
/// gated to match but takes or returns a value of a gated type
/// (E-OSTARGET-UNMATCHED-CALL): reachable from any build, it would call a
/// method the gated `impl` supplies — a Rust compile error (unresolved
/// method) on every OS but the gated one, since codegen strips that `impl`
/// entirely there (`Codegen/Imports.rs::emit_program_items`). Catching it in
/// sema turns that would-be rustc ICE (I2) into a Jet-level diagnostic.
///
/// Signature-level, not a full call-graph walk: mirrors how
/// `WebPartition::check_abi_export` also only inspects param/return types,
/// never call bodies. The existing effect call-graph (`fx_edges`) only
/// records bare-name calls (`CheckerInfer/calls.rs`), never method calls, so
/// it can't see a caller reaching a gated `impl`'s methods either way.
pub fn check_os_target(bundle: &ProgramBundle, freestanding: bool) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut gate_of_type: HashMap<String, OS> = HashMap::new();

    for module in &bundle.modules {
        for item in &module.items {
            let Item::Impl(i) = item else { continue };
            let Some(os) = i.os_target else { continue };
            gate_of_type.insert(i.type_name.clone(), os);
            if let Some(bucket) = module.web_target_ceiling {
                let label = match &i.trait_name {
                    Some(t) => format!("{}.{}", i.type_name, t),
                    None => i.type_name.clone(),
                };
                diags.push(Syntax::os_target_mixed_axis(
                    &label,
                    os,
                    bucket.name(),
                    Some(i.type_span),
                ));
            }
        }
    }

    if !gate_of_type.is_empty() {
        for module in &bundle.modules {
            check_items_signatures(&module.items, &gate_of_type, &mut diags);
        }
    }

    check_app_capabilities(bundle, freestanding, &mut diags);

    diags
}

/// D-APP-UNIFY1=B: one App type has target-sensitive methods, but target
/// resolution and rejection stay in sema so AOT, JIT, interpreter, and web
/// adapters all consume the same checked call shape.
fn check_app_capabilities(
    bundle: &ProgramBundle,
    freestanding: bool,
    diags: &mut Vec<Diagnostic>,
) {
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        let target = super::CheckerMarkers::resolved_app_target(bundle, module_idx, freestanding);
        check_app_items(&module.items, target, diags);
    }
}

fn check_app_items(items: &[Item], target: &str, diags: &mut Vec<Diagnostic>) {
    for item in items {
        match item {
            Item::Func(f) => check_app_body(
                &f.body,
                f.web_marker
                    .map(|marker| marker.bucket().name())
                    .unwrap_or(target),
                diags,
            ),
            Item::Impl(i) => {
                for method in &i.methods {
                    check_app_body(
                        &method.body,
                        method
                            .web_marker
                            .map(|marker| marker.bucket().name())
                            .unwrap_or(target),
                        diags,
                    );
                }
            }
            Item::Struct(s) => {
                for method in &s.methods {
                    check_app_body(
                        &method.body,
                        method
                            .web_marker
                            .map(|marker| marker.bucket().name())
                            .unwrap_or(target),
                        diags,
                    );
                }
                for trait_impl in &s.trait_impls {
                    for method in &trait_impl.methods {
                        check_app_body(
                            &method.body,
                            method
                                .web_marker
                                .map(|marker| marker.bucket().name())
                                .unwrap_or(target),
                            diags,
                        );
                    }
                }
            }
            Item::Enum(e) => {
                for method in &e.methods {
                    check_app_body(
                        &method.body,
                        method
                            .web_marker
                            .map(|marker| marker.bucket().name())
                            .unwrap_or(target),
                        diags,
                    );
                }
                for trait_impl in &e.trait_impls {
                    for method in &trait_impl.methods {
                        check_app_body(
                            &method.body,
                            method
                                .web_marker
                                .map(|marker| marker.bucket().name())
                                .unwrap_or(target),
                            diags,
                        );
                    }
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    check_app_items(
                        body,
                        module
                            .web_target
                            .map(|bucket| bucket.name())
                            .unwrap_or(target),
                        diags,
                    );
                }
            }
            Item::GenericModule(module) => check_app_items(&module.body, target, diags),
            _ => {}
        }
    }
}

fn check_app_body(body: &[Stmt], target: &str, diags: &mut Vec<Diagnostic>) {
    for original in body {
        let mut stmt = original.clone();
        stmt.for_each_expr_mut(|expr| {
            let Expr::MethodCall {
                recv_type: Some(receiver_type),
                method,
                method_span,
                ..
            } = expr
            else {
                return;
            };
            if receiver_type.as_str() != "App" {
                return;
            }
            let Some(required) = jet_foundation::App::app_capability_target(method.as_str()) else {
                return;
            };
            if jet_foundation::App::app_target_supports(required, target) {
                return;
            }
            diags.push(Diagnostic::from_row(
                "E-APP-TARGET-CAPABILITY",
                &[("capability", method.as_str()), ("required", required), ("target", target)],
                Some(*method_span),
            ));
        });
    }
}

fn check_items_signatures(
    items: &[Item],
    gate_of_type: &HashMap<String, OS>,
    diags: &mut Vec<Diagnostic>,
) {
    for item in items {
        match item {
            Item::Func(f) => check_func_sig(f, None, gate_of_type, diags),
            Item::Impl(i) => {
                for m in &i.methods {
                    check_func_sig(m, i.os_target, gate_of_type, diags);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    check_func_sig(m, None, gate_of_type, diags);
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    check_func_sig(m, None, gate_of_type, diags);
                }
            }
            _ => {}
        }
    }
}

fn check_func_sig(
    f: &Func,
    own_gate: Option<OS>,
    gate_of_type: &HashMap<String, OS>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut flag = |type_name: &str| {
        // `self`'s placeholder type is `Type::Named("")` (S27) — never a real
        // gated type name, so a method's own receiver never self-triggers.
        if type_name.is_empty() {
            return;
        }
        let Some(&os) = gate_of_type.get(type_name) else {
            return;
        };
        if own_gate == Some(os) {
            return;
        }
        diags.push(Syntax::os_target_unmatched_call(
            &f.name,
            type_name,
            os,
            Some(f.name_span),
        ));
    };
    for p in &f.params {
        if let Type::Named(n) = &p.ty {
            flag(n);
        }
    }
    if let Some(Type::Named(n)) = &f.return_type {
        flag(n);
    }
}
