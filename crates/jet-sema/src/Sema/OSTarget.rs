//! D-OSTARGET1=A (ratified 2026-07-01, c134): native OS platform gating.
//! Mirrors `WebPartition.rs`'s shape — one structural check, one signature
//! check — for the second, mutually-exclusive `#Target(OS.*)` axis of the
//! same `#Target(...)` marker family.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax::{self, OSTarget as OS};
use crate::AST::{Expr, Func, Item, LambdaBody, Pattern, ProgramBundle, Stmt, SwitchArm, Type};
use std::collections::HashMap;

/// D-OSTARGET2=B (ratified 2026-07-03): fold every `comptime if build.os == {
/// .Linux -> … .MacOS -> … .Windows -> … [else -> …] }` switch to the arm
/// matching this build's active OS (`bundle.active_os`), discarding the rest.
///
/// Runs as the very first step of `check_bundle`, before any other sema pass or
/// codegen sees a body — so the OS-gating checks, type-checker, and codegen
/// only ever meet the *taken* arm (constructing an OS-gated type inside it is
/// legal; the dead arms never trip `E-OSTARGET-UNMATCHED-CALL` and never reach
/// rustc). The rewrite lowers each switch into a chain of `Stmt::ComptimeIf`
/// whose arm conditions are the compile-time constants `build.os == .OS`
/// (emitted here as a `Bool` literal), so all the existing `comptime if`
/// machinery — arm selection, dropped-arm name resolution (D-WHEN2), codegen —
/// handles it unchanged. `build.os` is meaningful only as this switch's
/// subject; anywhere else `build` is an ordinary identifier (unknown at
/// runtime → E0107), never a magic runtime value.
pub fn desugar_os_switches(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    let active = bundle.active_os;
    let mut diags = Vec::new();
    for module in &mut bundle.modules {
        for item in &mut module.items {
            match item {
                Item::Func(f) => desugar_stmts(&mut f.body, active, &mut diags),
                Item::Struct(s) => {
                    for m in &mut s.methods {
                        desugar_stmts(&mut m.body, active, &mut diags);
                    }
                    for b in &mut s.trait_impls {
                        for m in &mut b.methods {
                            desugar_stmts(&mut m.body, active, &mut diags);
                        }
                    }
                }
                Item::Enum(e) => {
                    for m in &mut e.methods {
                        desugar_stmts(&mut m.body, active, &mut diags);
                    }
                }
                Item::Impl(i) => {
                    for m in &mut i.methods {
                        desugar_stmts(&mut m.body, active, &mut diags);
                    }
                }
                _ => {}
            }
        }
    }
    diags
}

/// Recurse through every body-bearing statement, rewriting `ComptimeSwitch` in
/// place. Also descends into lambda block bodies and value-position `if`
/// branches so a switch nested in an expression is still folded.
fn desugar_stmts(stmts: &mut Vec<Stmt>, active: OS, diags: &mut Vec<Diagnostic>) {
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
                        desugar_stmts(&mut arm.body, active, diags);
                    }
                    if let Some(eb) = else_body {
                        desugar_stmts(eb, active, diags);
                    }
                }
                let taken = std::mem::replace(stmt, Stmt::Break(Span::new(0, 0)));
                *stmt = fold_switch(taken, active, diags);
            }
            _ => desugar_child_blocks(stmt, active, diags),
        }
    }
}

/// Descend into every statement body a non-switch statement carries.
fn desugar_child_blocks(stmt: &mut Stmt, active: OS, diags: &mut Vec<Diagnostic>) {
    match stmt {
        Stmt::Expr(e) | Stmt::Yield(e, _) => desugar_expr(e, active, diags),
        Stmt::Val(b) => desugar_expr(&mut b.init, active, diags),
        Stmt::Assign { value, .. } => desugar_expr(value, active, diags),
        Stmt::Return(Some(e), _) => desugar_expr(e, active, diags),
        Stmt::If(ifs) => desugar_if(ifs, active, diags),
        Stmt::While { body, .. } | Stmt::For { body, .. } => desugar_stmts(body, active, diags),
        Stmt::Switch {
            arms, else_body, ..
        } => {
            for arm in arms {
                desugar_stmts(&mut arm.body, active, diags);
            }
            if let Some(eb) = else_body {
                desugar_stmts(eb, active, diags);
            }
        }
        Stmt::CountedLoop { body, step, .. } => {
            if let Some(step) = step { desugar_child_blocks(step, active, diags); }
            desugar_stmts(body, active, diags);
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
        | Stmt::ContextBlock { body, .. } => desugar_stmts(body, active, diags),
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            desugar_stmts(then_body, active, diags);
            if let Some(eb) = else_body {
                desugar_stmts(eb, active, diags);
            }
        }
        _ => {}
    }
}

fn desugar_if(ifs: &mut crate::AST::IfStmt, active: OS, diags: &mut Vec<Diagnostic>) {
    desugar_stmts(&mut ifs.then_body, active, diags);
    match &mut ifs.else_branch {
        Some(crate::AST::ElseBranch::ElseIf(inner)) => desugar_if(inner, active, diags),
        Some(crate::AST::ElseBranch::Else(body)) => desugar_stmts(body, active, diags),
        None => {}
    }
}

/// Descend into the few expression shapes that can hold statement blocks
/// (lambda block bodies, value-position `if` branches).
fn desugar_expr(e: &mut Expr, active: OS, diags: &mut Vec<Diagnostic>) {
    match e {
        Expr::Lambda(l) => {
            if let LambdaBody::Block(body) = &mut l.body {
                desugar_stmts(body, active, diags);
            }
        }
        Expr::If {
            then_body,
            else_body,
            ..
        } => {
            desugar_stmts(then_body, active, diags);
            desugar_stmts(else_body, active, diags);
        }
        Expr::Paren(inner, _) => desugar_expr(inner, active, diags),
        _ => {}
    }
}

/// Validate one `comptime if build.os == { … }` switch and rewrite it into the
/// nested `ComptimeIf` chain. On a validation error, returns an empty block
/// (`ComptimeBlock` with no body) so the surrounding statements still check.
fn fold_switch(sw: Stmt, active: OS, diags: &mut Vec<Diagnostic>) -> Stmt {
    let Stmt::ComptimeSwitch {
        subject,
        arms,
        else_body,
        span,
    } = sw
    else {
        unreachable!("fold_switch only called on ComptimeSwitch");
    };

    // The subject must be exactly `build.os`.
    let subject_ok = matches!(
        &subject,
        Expr::Field(base, member, _)
            if matches!(base.as_ref(), Expr::Ident(n, _) if n == Syntax::BUILD_INFO)
                && member == Syntax::BUILD_INFO_OS
    );
    if !subject_ok {
        diags.push(Syntax::os_target_build_context(Some(subject.span())));
        return empty_stmt(span);
    }

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
pub fn check_os_target(bundle: &ProgramBundle) -> Vec<Diagnostic> {
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

    if gate_of_type.is_empty() {
        return diags;
    }

    for module in &bundle.modules {
        check_items_signatures(&module.items, &gate_of_type, &mut diags);
    }

    diags
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
