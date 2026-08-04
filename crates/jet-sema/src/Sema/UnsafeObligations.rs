//! D-UNSAFE-OBLIG1=A: policy-resolved, typed unsafe operation obligations.
//!
//! Assertions are parser-only pseudo calls. This sema pass validates and removes
//! them before TIR, keeping codegen dumb and preserving AOT/dev parity.

use crate::AST::{EnumLitArg, Expr, Func, Item, LValue, OrFallback, ProgramBundle, Stmt, StrPart};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Policy::{self, PolicyDeclaration, PolicyKey, PolicyScope, PolicyValue};
use crate::Syntax;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct UnsafeInspection { pub gates: Vec<UnsafeGateInspection>, pub diagnostics: Vec<UnsafeDiagnostic> }

#[derive(Debug, Clone)]
pub struct UnsafeDiagnostic {
    pub source: String,
    pub diagnostic: Diagnostic,
}

#[derive(Debug, Clone)]
pub struct UnsafeGateInspection {
    pub source: String,
    pub span: Span,
    pub reason: Option<String>,
    pub mode: String,
    pub provenance: Vec<String>,
    pub operations: Vec<UnsafeOperationInspection>,
}

#[derive(Debug, Clone)]
pub struct UnsafeOperationInspection {
    pub kind: String,
    pub span: Span,
    pub required: Vec<String>,
    pub asserted: Vec<String>,
    pub discharged: bool,
}

pub fn inspect(bundle: &ProgramBundle) -> UnsafeInspection {
    let mut result = UnsafeInspection { gates: Vec::new(), diagnostics: Vec::new() };
    for module in &bundle.modules {
        for item in &module.items {
            visit_item(
                item,
                &module.display,
                &module.policy_declarations,
                &module.rule_facts,
                &mut result,
            );
        }
    }
    result.gates.sort_by(|a, b| (&a.source, a.span.start, a.span.end).cmp(&(&b.source, b.span.start, b.span.end)));
    result
}

pub(crate) fn check_and_strip(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    let result = inspect(bundle);
    for module in &mut bundle.modules {
        for item in &mut module.items { strip_item(item); }
    }
    result.diagnostics.into_iter().map(|entry| entry.diagnostic).collect()
}

fn push_diagnostic(result: &mut UnsafeInspection, source: &str, diagnostic: Diagnostic) {
    result.diagnostics.push(UnsafeDiagnostic { source: source.to_string(), diagnostic });
}

fn visit_item(
    item: &Item,
    source: &str,
    declarations: &[PolicyDeclaration],
    rule_facts: &[crate::AST::AppliedRuleApplication],
    result: &mut UnsafeInspection,
) {
    match item {
        Item::Func(function) => visit_function(function, source, declarations, rule_facts, result),
        Item::Struct(definition) => {
            for function in &definition.methods { visit_function(function, source, declarations, rule_facts, result); }
            for implementation in &definition.trait_impls {
                for function in &implementation.methods { visit_function(function, source, declarations, rule_facts, result); }
            }
        }
        Item::Enum(definition) => {
            for function in &definition.methods { visit_function(function, source, declarations, rule_facts, result); }
            for implementation in &definition.trait_impls {
                for function in &implementation.methods { visit_function(function, source, declarations, rule_facts, result); }
            }
        }
        Item::Impl(implementation) => for function in &implementation.methods { visit_function(function, source, declarations, rule_facts, result); },
        Item::Test(test) => visit_plain_body(&test.body, source, declarations, result),
        Item::Bench(bench) => visit_plain_body(&bench.body, source, declarations, result),
        Item::CodeModule(module) => {
            if let Some(items) = &module.body {
                for item in items { visit_item(item, source, declarations, rule_facts, result); }
            }
        }
        _ => {}
    }
}

fn visit_function(
    function: &Func,
    source: &str,
    declarations: &[PolicyDeclaration],
    rule_facts: &[crate::AST::AppliedRuleApplication],
    result: &mut UnsafeInspection,
) {
    if function.is_unsafe {
        let source_has_reason = rule_facts.iter().any(|application| {
            application.target == Some(function.span)
                && application.marker.name == Syntax::KW_UNSAFE
                && !application.marker.args.is_empty()
        });
        visit_gate(
            function.span,
            function.unsafe_span.unwrap_or(function.span),
            true,
            function.unsafe_reason.as_deref(),
            source_has_reason,
            &function.body,
            source,
            declarations,
            result,
        );
    } else {
        visit_plain_body(&function.body, source, declarations, result);
    }
}

fn visit_plain_body(body: &[Stmt], source: &str, declarations: &[PolicyDeclaration], result: &mut UnsafeInspection) {
    visit_nested_gates(body, source, declarations, result);
    reject_assertions_outside_gate(body, source, result);
}

fn visit_nested_gates(body: &[Stmt], source: &str, declarations: &[PolicyDeclaration], result: &mut UnsafeInspection) {
    for statement in body {
        match statement {
            Stmt::Unsafe {
                audit,
                audit_expr,
                body,
                span,
            } => visit_gate(
                *span,
                *span,
                false,
                audit.as_deref(),
                audit_expr.is_some(),
                body,
                source,
                declarations,
                result,
            ),
            _ => for nested in nested_bodies(statement) { visit_nested_gates(nested, source, declarations, result); },
        }
    }
}

fn visit_gate(
    target: Span,
    gate_span: Span,
    is_function: bool,
    reason: Option<&str>,
    source_has_reason: bool,
    body: &[Stmt],
    source: &str,
    declarations: &[PolicyDeclaration],
    result: &mut UnsafeInspection,
) {
    let outer = declarations.iter().filter(|declaration| {
        declaration.key == PolicyKey::Unsafe && matches!(declaration.scope, PolicyScope::Organization | PolicyScope::Package)
    }).cloned().collect::<Vec<_>>();
    let site = declarations.iter().filter(|declaration| declaration.key == PolicyKey::Unsafe && declaration.target == Some(target)).cloned().collect::<Vec<_>>();
    let outer_policy = Policy::resolve(PolicyKey::Unsafe, outer.clone()).ok().flatten();
    let mut chain = outer;
    chain.extend(site.clone());
    let effective = match Policy::resolve(PolicyKey::Unsafe, chain) {
        Ok(policy) => policy,
        Err(error) => {
            push_diagnostic(result, source, Diagnostic::error("E0355", "invalid effective `unsafe` policy".to_string(), format!("the organization, package, and gate declarations do not form a valid tightening chain: {error:?}"), "remove the weaker declaration or track the required obligations".to_string(), Some(gate_span)));
            None
        }
    };
    let mut value = effective.as_ref().map(|policy| policy.value).unwrap_or(PolicyValue::UnsafeDefault);
    if matches!(value, PolicyValue::UnsafePerSite) {
        push_diagnostic(result, source, Diagnostic::error("E3106", "this unsafe gate must choose an obligation mode".to_string(), "the effective `.PerSite` policy requires every gate to select `.Track` or `.Skip`".to_string(), "add `obligations: .Track` (or `.Skip` when organization policy permits it)".to_string(), Some(gate_span)));
        value = PolicyValue::UnsafeTrack;
    }
    if site.iter().any(|declaration| declaration.value == PolicyValue::UnsafeSkip)
        && !matches!(outer_policy.as_ref().map(|policy| policy.value), Some(PolicyValue::UnsafePerSite))
    {
        push_diagnostic(result, source, Diagnostic::error("E3106", "this unsafe gate cannot skip obligations".to_string(), "`.Skip` is site control for an effective `.PerSite` package policy; it is not ambient authorization".to_string(), "set package policy to `.PerSite`, or remove `.Skip`".to_string(), Some(gate_span)));
        value = PolicyValue::UnsafeTrack;
    }
    if value == PolicyValue::UnsafeForbid {
        push_diagnostic(result, source, Diagnostic::error("E3105", "organization or package policy forbids unsafe code".to_string(), "a lexical `#Unsafe` gate cannot widen the effective safety floor".to_string(), "remove the low-level operation or change the outer policy through its owner".to_string(), Some(gate_span)));
    } else if reason.is_none() && !source_has_reason && value != PolicyValue::UnsafeRelaxed {
        let (what, why, fix) = if is_function {
            ("this `#Unsafe` function has no reason", "every gated function records why callers can rely on its unsafe contract", "add the reason: `#Unsafe(\"why this is safe\") fn ...`")
        } else {
            ("this `#Unsafe` block has no reason", "every gated region records why it can't break memory safety", "add the reason: `#Unsafe(\"why this is safe\") { … }`")
        };
        push_diagnostic(result, source, Diagnostic::error("E3112", what.to_string(), why.to_string(), fix.to_string(), Some(gate_span)));
    }

    let mut gate = UnsafeGateInspection {
        source: source.to_string(), span: gate_span, reason: reason.map(str::to_string),
        mode: mode_name(value).to_string(),
        provenance: effective.as_ref().map(|policy| policy.provenance.iter().map(|declaration| format!("{}:{}:{}..{}={}", declaration.scope.name(), declaration.source, declaration.span.start, declaration.span.end, declaration.value.display())).collect()).unwrap_or_default(),
        operations: Vec::new(),
    };
    scan_sequence(body, requires_obligations(value), &mut gate, source, &mut result.diagnostics);
    result.gates.push(gate);
    visit_nested_gates(body, source, declarations, result);
}

fn mode_name(value: PolicyValue) -> &'static str {
    match value {
        PolicyValue::UnsafeObligations | PolicyValue::UnsafeTrack => "Obligations",
        PolicyValue::UnsafeRelaxed => "Relaxed",
        PolicyValue::UnsafePerSite => "PerSite",
        PolicyValue::UnsafeSkip | PolicyValue::UnsafeDefault | PolicyValue::UnsafeGateOnly => "GateOnly",
        PolicyValue::UnsafeForbid => "Forbid",
        _ => "GateOnly",
    }
}

fn requires_obligations(value: PolicyValue) -> bool { matches!(value, PolicyValue::UnsafeObligations | PolicyValue::UnsafeTrack) }

fn scan_sequence(body: &[Stmt], track: bool, gate: &mut UnsafeGateInspection, source: &str, diagnostics: &mut Vec<UnsafeDiagnostic>) {
    let mut pending_operations = Vec::new();
    for statement in body {
        if let Some((names, span)) = assertion(statement) {
            if pending_operations.is_empty() {
                diagnostics.push(UnsafeDiagnostic { source: source.to_string(), diagnostic: Diagnostic::error("E3108", "this unsafe assertion has no preceding operation".to_string(), "typed obligations attach only to the immediately preceding low-level operation statement".to_string(), "move the assertion directly after the operation it proves".to_string(), Some(span)) });
                continue;
            }
            let mut asserted = BTreeSet::new();
            for name in names {
                if matches!(name.as_str(), Syntax::UNSAFE_OBLIGATION_VALID_PTR | Syntax::UNSAFE_OBLIGATION_ALIGNED | Syntax::UNSAFE_OBLIGATION_NO_ALIAS) {
                    asserted.insert(name);
                } else {
                    diagnostics.push(UnsafeDiagnostic { source: source.to_string(), diagnostic: Diagnostic::error("E3108", format!("`{name}` is not an unsafe obligation"), "unsafe proofs use the closed typed set `valid_ptr`, `aligned`, and `no_alias`".to_string(), "fix the obligation name".to_string(), Some(span)) });
                }
            }
            discharge_operations(&pending_operations, &asserted, track, gate, source, diagnostics);
            pending_operations.clear();
            continue;
        }
        discharge_operations(&pending_operations, &BTreeSet::new(), track, gate, source, diagnostics);
        pending_operations.clear();
        if matches!(statement, Stmt::Unsafe { .. }) { continue; }
        let mut operations = Vec::new();
        collect_shallow_operations(statement, &mut operations);
        pending_operations.extend(operations);
        for nested in nested_bodies(statement) { scan_sequence(nested, track, gate, source, diagnostics); }
    }
    discharge_operations(&pending_operations, &BTreeSet::new(), track, gate, source, diagnostics);
}

fn discharge_operations(operations: &[(&'static str, Span, Vec<&'static str>)], asserted: &BTreeSet<String>, track: bool, gate: &mut UnsafeGateInspection, source: &str, diagnostics: &mut Vec<UnsafeDiagnostic>) {
    for (kind, span, required) in operations {
        let missing = required.iter().filter(|name| !asserted.contains(**name)).copied().collect::<Vec<_>>();
        let discharged = !track || missing.is_empty();
        if track && !missing.is_empty() {
            diagnostics.push(UnsafeDiagnostic { source: source.to_string(), diagnostic: Diagnostic::error("E3107", format!("`{kind}` is missing unsafe obligations: {}", missing.join(", ")), "the effective `.Obligations` policy requires a typed proof immediately after each low-level operation".to_string(), format!("add `assert {}` immediately after this operation", required.join(", ")), Some(*span)) });
        }
        gate.operations.push(UnsafeOperationInspection { kind: (*kind).to_string(), span: *span, required: required.iter().map(|name| (*name).to_string()).collect(), asserted: asserted.iter().cloned().collect(), discharged });
    }
}

fn assertion(statement: &Stmt) -> Option<(Vec<String>, Span)> {
    let Stmt::Expr(Expr::Call(call)) = statement else { return None };
    if call.name != Syntax::INTERNAL_UNSAFE_ASSERT { return None; }
    Some((call.args.iter().filter_map(|argument| match &argument.expr { Expr::Ident(name, _) => Some(name.clone()), _ => None }).collect(), call.name_span))
}

fn collect_shallow_operations(statement: &Stmt, out: &mut Vec<(&'static str, Span, Vec<&'static str>)>) {
    match statement {
        Stmt::Expr(expression) | Stmt::Return(Some(expression), _) => collect_expr_operations(expression, out),
        Stmt::Val(binding) => collect_expr_operations(&binding.init, out),
        Stmt::Assign { target, value, .. } => { collect_lvalue_operations(target, out); collect_expr_operations(value, out); }
        Stmt::While { cond, .. } => collect_expr_operations(cond, out),
        Stmt::CountedLoop { init, cond, step, .. } => { collect_expr_operations(&init.init, out); collect_expr_operations(cond, out); if let Some(step) = step { collect_shallow_operations(step, out); } }
        Stmt::For { kind, .. } => match kind {
            crate::AST::ForKind::Range { start, end, step, exclusive: _ } => { collect_expr_operations(start, out); collect_expr_operations(end, out); if let Some(step) = step { collect_expr_operations(step, out); } }
            crate::AST::ForKind::In { collection, step } => {
                collect_expr_operations(collection, out);
                if let Some(step) = step { collect_expr_operations(step, out); }
            }
        },
        Stmt::Switch { subject, .. } => collect_expr_operations(subject, out),
        Stmt::ComptimeIf { cond, .. } => collect_expr_operations(cond, out),
        Stmt::ComptimeSwitch { subject, .. } => collect_expr_operations(subject, out),
        Stmt::ContextBlock { fields, .. } => for (_, value, _) in fields { collect_expr_operations(value, out); },
        Stmt::ScopeMember { args, .. } => for argument in args { collect_expr_operations(argument, out); },
        Stmt::Yield(value, _) => collect_expr_operations(value, out),
        _ => {}
    }
}

fn collect_lvalue_operations(value: &LValue, out: &mut Vec<(&'static str, Span, Vec<&'static str>)>) {
    match value {
        LValue::Local { .. } => {}
        LValue::Index { base, index, .. } => { collect_expr_operations(base, out); collect_expr_operations(index, out); }
        LValue::Field { base, .. } => collect_expr_operations(base, out),
    }
}

fn collect_expr_operations(expression: &Expr, out: &mut Vec<(&'static str, Span, Vec<&'static str>)>) {
    match expression {
        Expr::PtrFromAddr { addr, span, .. } => { out.push(("pointer_from_address", *span, vec![Syntax::UNSAFE_OBLIGATION_VALID_PTR, Syntax::UNSAFE_OBLIGATION_ALIGNED])); collect_expr_operations(addr, out); }
        Expr::Deref(inner, span) => { out.push(("dereference", *span, vec![Syntax::UNSAFE_OBLIGATION_VALID_PTR, Syntax::UNSAFE_OBLIGATION_ALIGNED])); collect_expr_operations(inner, out); }
        Expr::RawOf(inner, span) => { out.push(("raw_pointer", *span, vec![Syntax::UNSAFE_OBLIGATION_NO_ALIAS])); collect_expr_operations(inner, out); }
        Expr::MethodCall { receiver, method, args, method_span, .. } => {
            let required = match method.as_str() {
                "volatile_read" => Some(("volatile_read", vec![Syntax::UNSAFE_OBLIGATION_VALID_PTR, Syntax::UNSAFE_OBLIGATION_ALIGNED])),
                "volatile_write" => Some(("volatile_write", vec![Syntax::UNSAFE_OBLIGATION_VALID_PTR, Syntax::UNSAFE_OBLIGATION_ALIGNED, Syntax::UNSAFE_OBLIGATION_NO_ALIAS])),
                "cast_ptr" => Some(("pointer_cast", vec![Syntax::UNSAFE_OBLIGATION_VALID_PTR, Syntax::UNSAFE_OBLIGATION_ALIGNED])),
                _ => None,
            };
            if let Some((kind, obligations)) = required { out.push((kind, *method_span, obligations)); }
            collect_expr_operations(receiver, out);
            for argument in args { collect_expr_operations(&argument.expr, out); }
        }
        Expr::Call(call) => for argument in &call.args { collect_expr_operations(&argument.expr, out); },
        Expr::Str(parts, _) => for part in parts { if let StrPart::Interp(value, _) = part { collect_expr_operations(value, out); } },
        Expr::Binary(_, left, right, _) => { collect_expr_operations(left, out); collect_expr_operations(right, out); }
        Expr::CompareChain { operands, .. } | Expr::ListLit(operands, _) => for operand in operands { collect_expr_operations(operand, out); },
        Expr::Index { base, index, .. } => { collect_expr_operations(base, out); collect_expr_operations(index, out); }
        Expr::Slice { base, start, end, range, .. } => {
            collect_expr_operations(base, out);
            if let Some(range) = range {
                collect_expr_operations(range, out);
            } else {
                collect_expr_operations(start, out);
                collect_expr_operations(end, out);
            }
        }
        Expr::Range { start, end, .. } => { collect_expr_operations(start, out); collect_expr_operations(end, out); }
        Expr::Unary(_, inner, _) | Expr::Copy(inner, _) | Expr::Place(inner, _, _) | Expr::Field(inner, _, _) | Expr::OptField { base: inner, .. } | Expr::Paren(inner, _) | Expr::Spread(inner, _) => collect_expr_operations(inner, out),
        Expr::StructLit { fields, .. } => for (_, _, value) in fields { collect_expr_operations(value, out); },
        Expr::TypedLit { body, .. } => body.for_each_expr(|value| collect_expr_operations(value, out)),
        Expr::EnumLit { args, .. } => for argument in args { match argument { EnumLitArg::Positional(value) | EnumLitArg::Named { expr: value, .. } => collect_expr_operations(value, out) } },
        Expr::MapLit(entries, _) => for (key, value) in entries { collect_expr_operations(key, out); collect_expr_operations(value, out); },
        Expr::TupleLit(fields, _, _) => for (_, value) in fields { collect_expr_operations(value, out); },
        Expr::CallValue { callee, args, .. } => { collect_expr_operations(callee, out); for argument in args { collect_expr_operations(&argument.expr, out); } }
        Expr::Tainted(inner, _, _) | Expr::Present(inner, _) | Expr::Ok(inner, _) | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) | Expr::IncDec { operand: inner, .. } => collect_expr_operations(inner, out),
        Expr::PatternTest { subject, .. } => collect_expr_operations(subject, out),
        Expr::OrFallback { value, fallback, .. } => {
            collect_expr_operations(value, out);
            match fallback {
                OrFallback::Value(value) | OrFallback::Return(Some(value), _) => collect_expr_operations(value, out),
                OrFallback::Panic { args, .. } => for argument in args { collect_expr_operations(&argument.expr, out); },
                OrFallback::Return(None, _)
                | OrFallback::Break(_)
                | OrFallback::Continue(_)
                | OrFallback::BreakLabel(..)
                | OrFallback::ContinueLabel(..) => {}
            }
        }
        Expr::If { cond, then_value, else_value, .. } => { collect_expr_operations(cond, out); collect_expr_operations(then_value, out); collect_expr_operations(else_value, out); }
        _ => {}
    }
}

pub(crate) fn nested_bodies(statement: &Stmt) -> Vec<&[Stmt]> {
    match statement {
        Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::Loop { body, .. } | Stmt::CountedLoop { body, .. }
        | Stmt::Impure { body, .. } | Stmt::Reactive { body, .. } | Stmt::Shield { body, .. } | Stmt::Off { body, .. }
        | Stmt::DebugOnly { body, .. } | Stmt::Region { body, .. } | Stmt::Policy { body, .. } | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. } | Stmt::Caps { body, .. } | Stmt::Grant { body, .. } | Stmt::Transact { body, .. }
        | Stmt::ContextBlock { body, .. } | Stmt::Live { body, .. } | Stmt::AssumeDet { body, .. } => vec![body],
        Stmt::Switch { arms, else_body, .. } => arms.iter().map(|arm| arm.body.as_slice()).chain(else_body.iter().map(Vec::as_slice)).collect(),
        Stmt::ComptimeIf { then_body, else_body, .. } => std::iter::once(then_body.as_slice()).chain(else_body.iter().map(Vec::as_slice)).collect(),
        Stmt::ComptimeSwitch { arms, else_body, .. } => arms.iter().map(|arm| arm.body.as_slice()).chain(else_body.iter().map(Vec::as_slice)).collect(),
        Stmt::ComptimeBlock { body, .. } | Stmt::ScopeMember { body, .. } => vec![body],
        _ => Vec::new(),
    }
}

fn reject_assertions_outside_gate(body: &[Stmt], source: &str, result: &mut UnsafeInspection) {
    for statement in body {
        if let Some((_, span)) = assertion(statement) {
            push_diagnostic(result, source, Diagnostic::error("E3108", "unsafe obligations can only be asserted inside `#Unsafe`".to_string(), "the assertion discharges one tracked low-level operation and has no ambient meaning".to_string(), "move it immediately after an operation inside the audited gate".to_string(), Some(span)));
        }
        if !matches!(statement, Stmt::Unsafe { .. }) { for nested in nested_bodies(statement) { reject_assertions_outside_gate(nested, source, result); } }
    }
}

fn strip_item(item: &mut Item) {
    match item {
        Item::Func(function) => strip_body(&mut function.body),
        Item::Struct(definition) => { for function in &mut definition.methods { strip_body(&mut function.body); } for implementation in &mut definition.trait_impls { for function in &mut implementation.methods { strip_body(&mut function.body); } } }
        Item::Enum(definition) => { for function in &mut definition.methods { strip_body(&mut function.body); } for implementation in &mut definition.trait_impls { for function in &mut implementation.methods { strip_body(&mut function.body); } } }
        Item::Impl(implementation) => for function in &mut implementation.methods { strip_body(&mut function.body); },
        Item::Test(test) => strip_body(&mut test.body),
        Item::Bench(bench) => strip_body(&mut bench.body),
        Item::CodeModule(module) => {
            if let Some(items) = &mut module.body {
                for item in items { strip_item(item); }
            }
        }
        _ => {}
    }
}

fn strip_body(body: &mut Vec<Stmt>) {
    body.retain(|statement| assertion(statement).is_none());
    for statement in body {
        for nested in nested_bodies_mut(statement) {
            strip_body(nested);
        }
    }
}

fn nested_bodies_mut(statement: &mut Stmt) -> Vec<&mut Vec<Stmt>> {
    match statement {
        Stmt::Unsafe { body, .. } | Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::Loop { body, .. }
        | Stmt::CountedLoop { body, .. } | Stmt::Impure { body, .. } | Stmt::Reactive { body, .. } | Stmt::Shield { body, .. }
        | Stmt::Off { body, .. } | Stmt::DebugOnly { body, .. } | Stmt::Region { body, .. } | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. } | Stmt::Layout { body, .. } | Stmt::Caps { body, .. } | Stmt::Grant { body, .. }
        | Stmt::Transact { body, .. } | Stmt::ContextBlock { body, .. } | Stmt::Live { body, .. } | Stmt::AssumeDet { body, .. } => vec![body],
        Stmt::Switch { arms, else_body, .. } => arms.iter_mut().map(|arm| &mut arm.body).chain(else_body.iter_mut()).collect(),
        Stmt::ComptimeIf { then_body, else_body, .. } => std::iter::once(then_body).chain(else_body.iter_mut()).collect(),
        Stmt::ComptimeSwitch { arms, else_body, .. } => arms.iter_mut().map(|arm| &mut arm.body).chain(else_body.iter_mut()).collect(),
        Stmt::ComptimeBlock { body, .. } | Stmt::ScopeMember { body, .. } => vec![body],
        _ => Vec::new(),
    }
}
