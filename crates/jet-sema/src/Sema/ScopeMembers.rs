//! D-DOTSCOPE1: contextual scope-member validation.
//!
//! A statement-position `.name { … }` / `.name(args) { … }` (parsed context-free
//! as `Stmt::ScopeMember`) is legal only as a **direct** statement of a marker
//! block that declares a vocabulary (`Syntax::scope_members`). Today the only
//! such marker is `#Test`, whose members are `.setup` / `.expect_fail` /
//! `.timeout` / `.skip`. This pass is the single owner of the member rules:
//!
//! - E0614 — unknown member (lists the vocabulary), or a member used inside a
//!   marker that declares none (e.g. `#Bench`).
//! - E0615 — a member statement outside any member-declaring marker block.
//! - E0616 — `.setup` is not the first statement.
//! - E0617 — wrong argument shape (`.timeout` needs one duration; `.setup` /
//!   `.expect_fail` takes an optional stop code; `.skip` takes an optional reason
//!   string).
//! - E0618 — a member nested inside another member or a control block (they must
//!   stay flat, at the top level of the marker body).
//!
//! Everything here is compile-time only (I3): the checker recurses into member
//! bodies for ordinary type-checking, and codegen lowers the members into the
//! `jet test` harness. `#Test`/`#Bench` bodies are validated only under their
//! own modes (they are not compiled otherwise, mirroring the rest of sema).

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{
    EnumLitArg, Expr, ForKind, Item, LambdaBody, OrFallback, Pattern, Stmt, StrPart,
    StructPatField, TraitImplBlock,
};

/// Validate every scope-member statement in the program. Runs in every mode
/// (not just `jet test`): a malformed member is a structural error that `jet
/// check` / `jet run` should surface too, even though the members only *run*
/// under `jet test`. The check needs no type information.
pub fn check(items: &[Item]) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    check_dsl_items(items, &mut diags);
    check_assertionless_tests(items, &mut diags);
    for item in items {
        match item {
            Item::Test(t) => check_marker_body(Syntax::KW_TEST, &t.body, &mut diags),
            Item::Bench(b) => check_marker_body(Syntax::KW_BENCH, &b.body, &mut diags),
            Item::Func(f) => reject_all(&f.body, &mut diags),
            Item::Impl(i) => {
                for m in &i.methods {
                    reject_all(&m.body, &mut diags);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    reject_all(&m.body, &mut diags);
                }
                reject_in_trait_impls(&s.trait_impls, &mut diags);
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    reject_all(&m.body, &mut diags);
                }
                reject_in_trait_impls(&e.trait_impls, &mut diags);
            }
            _ => {}
        }
    }
    diags
}

fn check_assertionless_tests(items: &[Item], diags: &mut Vec<Diagnostic>) {
    for item in items {
        let Item::Test(test) = item else {
            continue;
        };
        if !statements_have_assertion(&test.body) {
            diags.push(Diagnostic::lint(
                "L2901",
                "This #Test block has no assertions.".to_string(),
                "A test with no assertions cannot find bugs because it always passes."
                    .to_string(),
                "Add at least one assertion, or remove the test if it only exercises compilation."
                    .to_string(),
                Some(test.span),
            ));
        }
    }
}

fn statements_have_assertion(statements: &[Stmt]) -> bool {
    statements.iter().any(statement_has_assertion)
}

fn statement_has_assertion(statement: &Stmt) -> bool {
    let direct = match statement {
        Stmt::Expr(expr) | Stmt::Yield(expr, _) => expr_has_assertion(expr),
        Stmt::Val(binding) => expr_has_assertion(&binding.init),
        Stmt::Assign { value, .. } => expr_has_assertion(value),
        Stmt::Return(Some(expr), _)
        | Stmt::BreakValue(expr, _)
        | Stmt::BreakLabelValue(_, _, expr, _) => expr_has_assertion(expr),
        Stmt::While { cond, .. } => expr_has_assertion(cond),
        Stmt::For { kind, .. } => for_kind_has_assertion(kind),
        Stmt::Switch {
            subject, arms, ..
        }
        | Stmt::ComptimeSwitch {
            subject, arms, ..
        } => {
            expr_has_assertion(subject)
                || arms.iter().any(|arm| {
                    expr_has_assertion(&arm.cond) || statements_have_assertion(&arm.body)
                })
        }
        Stmt::CountedLoop {
            init, cond, step, ..
        } => {
            expr_has_assertion(&init.init)
                || expr_has_assertion(cond)
                || step
                    .as_deref()
                    .is_some_and(statement_has_assertion)
        }
        Stmt::Unsafe { audit_expr, .. } | Stmt::Impure {
            reason_expr: audit_expr,
            ..
        } => audit_expr
            .as_ref()
            .is_some_and(expr_has_assertion),
        Stmt::TaskGroup { limit, .. } => limit.as_ref().is_some_and(expr_has_assertion),
        Stmt::ComptimeIf { cond, .. } => expr_has_assertion(cond),
        Stmt::ContextBlock { fields, .. } => fields
            .iter()
            .any(|(_, value, _)| expr_has_assertion(value)),
        Stmt::AssumeDet { reason_expr, .. } => expr_has_assertion(reason_expr),
        Stmt::ScopeMember { args, .. } => args.iter().any(expr_has_assertion),
        Stmt::Return(None, _)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(_, _)
        | Stmt::ContinueLabel(_, _)
        | Stmt::Loop { .. }
        | Stmt::Reactive { .. }
        | Stmt::Shield { .. }
        | Stmt::Switched { .. }
        | Stmt::Region { .. }
        | Stmt::Policy { .. }
        | Stmt::Layout { .. }
        | Stmt::Caps { .. }
        | Stmt::Grant { .. }
        | Stmt::ComptimeBlock { .. }
        | Stmt::Live { .. }
        | Stmt::Transact { .. }
        => false,
    };
    direct || statement_bodies(statement)
        .iter()
        .any(|body| statements_have_assertion(body))
}

fn statement_bodies(statement: &Stmt) -> Vec<&[Stmt]> {
    match statement {
        Stmt::Switch {
            arms, else_body, ..
        }
        | Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => {
            let mut bodies: Vec<&[Stmt]> = arms.iter().map(|arm| arm.body.as_slice()).collect();
            if let Some(body) = else_body {
                bodies.push(body.as_slice());
            }
            bodies
        }
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            let mut bodies = vec![then_body.as_slice()];
            if let Some(body) = else_body {
                bodies.push(body.as_slice());
            }
            bodies
        }
        Stmt::While { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::CountedLoop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => vec![body.as_slice()],
        _ => Vec::new(),
    }
}

fn for_kind_has_assertion(kind: &ForKind) -> bool {
    match kind {
        ForKind::Range {
            start, end, step, ..
        } => {
            expr_has_assertion(start)
                || expr_has_assertion(end)
                || step.as_ref().is_some_and(expr_has_assertion)
        }
        ForKind::In { collection, step } => {
            expr_has_assertion(collection) || step.as_ref().is_some_and(expr_has_assertion)
        }
    }
}

fn expr_has_assertion(expr: &Expr) -> bool {
    match expr {
        Expr::Call(call) => {
            is_assertion_name(&call.name) || call.args.iter().any(|arg| expr_has_assertion(&arg.expr))
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            (method == Syntax::BUILTIN_SNAPSHOT && is_expect_call(receiver))
                || expr_has_assertion(receiver)
                || args.iter().any(|arg| expr_has_assertion(&arg.expr))
        }
        Expr::Str(parts, ..) => parts.iter().any(|part| match part {
            StrPart::Interp(value, _) => expr_has_assertion(value),
            StrPart::Lit(_) => false,
        }),
        Expr::ListLit(values, ..) => values.iter().any(expr_has_assertion),
        Expr::TupleLit(fields, ..) => fields
            .iter()
            .any(|(_, value)| expr_has_assertion(value)),
        Expr::MemberSpread { base, .. }
        | Expr::Spread(base, ..)
        | Expr::Deref(base, ..)
        | Expr::RawOf(base, ..)
        | Expr::Copy(base, ..)
        | Expr::Place(base, ..)
        | Expr::Field(base, ..)
        | Expr::Present(base, ..)
        | Expr::Ok(base, ..)
        | Expr::Err(base, ..)
        | Expr::Try(base, ..)
        | Expr::Paren(base, ..) => expr_has_assertion(base),
        Expr::MapLit(entries, ..) => entries
            .iter()
            .any(|(key, value)| expr_has_assertion(key) || expr_has_assertion(value)),
        Expr::Index { base, index, .. } => expr_has_assertion(base) || expr_has_assertion(index),
        Expr::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            expr_has_assertion(base)
                || expr_has_assertion(start)
                || expr_has_assertion(end)
                || range.as_ref().is_some_and(|value| expr_has_assertion(value))
        }
        Expr::Range { start, end, .. } => expr_has_assertion(start) || expr_has_assertion(end),
        Expr::Unary(_, value, ..) => expr_has_assertion(value),
        Expr::Binary(_, left, right, ..) => {
            expr_has_assertion(left) || expr_has_assertion(right)
        }
        Expr::CompareChain { operands, .. } => operands.iter().any(expr_has_assertion),
        Expr::OptField { base, .. } => expr_has_assertion(base),
        Expr::StructLit { fields, .. } => fields
            .iter()
            .any(|(_, _, value)| expr_has_assertion(value)),
        Expr::TypedLit { body, .. } => {
            let mut found = false;
            body.for_each_expr(|value| found |= expr_has_assertion(value));
            found
        }
        Expr::EnumLit { args, .. } => args.iter().any(enum_arg_has_assertion),
        Expr::Tainted(value, ..) => expr_has_assertion(value),
        Expr::PatternTest {
            subject, pattern, ..
        } => expr_has_assertion(subject) || pattern_has_assertion(pattern),
        Expr::OrFallback {
            value, fallback, ..
        } => expr_has_assertion(value) || fallback_has_assertion(fallback),
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            expr_has_assertion(cond)
                || statements_have_assertion(then_body)
                || expr_has_assertion(then_value)
                || statements_have_assertion(else_body)
                || expr_has_assertion(else_value)
        }
        Expr::Lambda(lambda) => match &lambda.body {
            LambdaBody::Expr(value) => expr_has_assertion(value),
            LambdaBody::Block(body) => statements_have_assertion(body),
        },
        Expr::CallValue { callee, args, .. } => {
            expr_has_assertion(callee)
                || args.iter().any(|arg| expr_has_assertion(&arg.expr))
        }
        Expr::PtrFromAddr { addr, .. } => expr_has_assertion(addr),
        Expr::IncDec { operand, .. } => expr_has_assertion(operand),
        Expr::StrMatchLit(..)
        | Expr::BinMatchLit(..)
        | Expr::Int(..)
        | Expr::Float(..)
        | Expr::Bool(..)
        | Expr::Char(..)
        | Expr::Ident(..)
        | Expr::UnitLit { .. }
        | Expr::Absent(..)
        | Expr::Todo { .. }
        | Expr::NoElse(..)
        | Expr::ReduceMarker(..)
        | Expr::ComptimeName { .. } => false,
    }
}

fn is_assertion_name(name: &str) -> bool {
    name == Syntax::BUILTIN_REQUIRE
        || name == Syntax::BUILTIN_REQUIRE_EQ
        || name == "assert"
        || name == "assert_eq"
}

fn is_expect_call(expr: &Expr) -> bool {
    match expr {
        Expr::Call(call) => call.name == Syntax::BUILTIN_EXPECT,
        Expr::Paren(inner, _) => is_expect_call(inner),
        _ => false,
    }
}

fn enum_arg_has_assertion(arg: &EnumLitArg) -> bool {
    match arg {
        EnumLitArg::Positional(value) | EnumLitArg::Named { expr: value, .. } => {
            expr_has_assertion(value)
        }
    }
}

fn fallback_has_assertion(fallback: &OrFallback) -> bool {
    match fallback {
        OrFallback::Value(value) | OrFallback::Return(Some(value), _) => {
            expr_has_assertion(value)
        }
        OrFallback::Block { body, value, .. } => {
            statements_have_assertion(body) || expr_has_assertion(value)
        }
        OrFallback::Panic { args, .. } => args.iter().any(|arg| expr_has_assertion(&arg.expr)),
        OrFallback::Return(None, _)
        | OrFallback::Break(_)
        | OrFallback::Continue(_)
        | OrFallback::BreakLabel(_, _)
        | OrFallback::ContinueLabel(_, _) => false,
    }
}

fn pattern_has_assertion(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Or(alternatives, _) => alternatives.iter().any(pattern_has_assertion),
        Pattern::Struct { fields, .. } => fields.iter().any(|field| match field {
            StructPatField::Value { value, .. } => expr_has_assertion(value),
            StructPatField::Bind { .. } => false,
        }),
        Pattern::Variant { .. }
        | Pattern::Present { .. }
        | Pattern::Absent(_)
        | Pattern::Ok { .. }
        | Pattern::Err { .. }
        | Pattern::Range { .. }
        | Pattern::StrMatch { .. }
        | Pattern::BinMatch { .. } => false,
    }
}

fn reject_in_trait_impls(blocks: &[TraitImplBlock], diags: &mut Vec<Diagnostic>) {
    for block in blocks {
        for m in &block.methods {
            reject_all(&m.body, diags);
        }
    }
}

/// The top level of a marker body: the only place members are legal. Each
/// direct-child member is validated against the marker's vocabulary; anything
/// deeper (nested in a member, an `if`, a loop, …) is E0618.
fn check_marker_body(marker: &str, body: &[Stmt], diags: &mut Vec<Diagnostic>) {
    for (i, s) in body.iter().enumerate() {
        match s {
            Stmt::ScopeMember {
                name,
                args,
                args_span,
                body: mbody,
                dot_span,
                span,
                ..
            } => {
                if is_registered_dsl_marker(name) {
                    continue;
                }
                validate_member(
                    marker,
                    name,
                    args,
                    args_span,
                    i == 0,
                    *dot_span,
                    *span,
                    diags,
                );
                // Members inside a member's body are nested → E0618.
                reject_nested(mbody, diags);
            }
            other => {
                // A member buried inside a control block is not at the top level.
                for child in child_bodies(other) {
                    reject_nested(child, diags);
                }
            }
        }
    }
}

/// Emit E0618 for every scope member found anywhere in `body` (used for the
/// interior of a marker block, where members must stay flat at the top level).
fn reject_nested(body: &[Stmt], diags: &mut Vec<Diagnostic>) {
    walk_members(body, &mut |name, dot_span| {
        if is_registered_dsl_marker(name) {
            return;
        }
        diags.push(Diagnostic::error(
            "E0618",
            "scope members can't be nested".to_string(),
            format!(
                "each `.{}`-style member is a top-level region of the block; nesting one inside another member or a control block has no meaning",
                name
            ),
            format!("move `.{} {{ … }}` out to the top level of the block", name),
            Some(dot_span),
        ));
    });
}

/// Emit E0615 for every scope member found anywhere in `body` (used for ordinary
/// function bodies, where a member statement is out of place entirely).
fn reject_all(body: &[Stmt], diags: &mut Vec<Diagnostic>) {
    walk_members(body, &mut |name, dot_span| {
        if is_registered_dsl_marker(name) {
            return;
        }
        diags.push(Diagnostic::error(
            "E0615",
            format!(
                "`.{}` only works inside a marker block that declares it",
                name
            ),
            "a leading-dot member statement resolves against the enclosing applied rule's vocabulary — here there is no such block".to_string(),
            format!(
                "move it inside a `#{}(\"…\") {{ … }}` block, or write an ordinary statement",
                Syntax::KW_TEST
            ),
            Some(dot_span),
        ));
    });
}

fn check_dsl_items(items: &[Item], diags: &mut Vec<Diagnostic>) {
    for item in items {
        match item {
            Item::Test(test) => check_dsl_body(&test.body, diags),
            Item::Bench(bench) => check_dsl_body(&bench.body, diags),
            Item::Func(func) => check_dsl_body(&func.body, diags),
            Item::Impl(implementation) => {
                for method in &implementation.methods {
                    check_dsl_body(&method.body, diags);
                }
            }
            Item::Struct(def) => {
                for method in &def.methods {
                    check_dsl_body(&method.body, diags);
                }
                for implementation in &def.trait_impls {
                    for method in &implementation.methods {
                        check_dsl_body(&method.body, diags);
                    }
                }
            }
            Item::Enum(def) => {
                for method in &def.methods {
                    check_dsl_body(&method.body, diags);
                }
                for implementation in &def.trait_impls {
                    for method in &implementation.methods {
                        check_dsl_body(&method.body, diags);
                    }
                }
            }
            _ => {}
        }
    }
}

fn check_dsl_body(body: &[Stmt], diags: &mut Vec<Diagnostic>) {
    for statement in body {
        match statement {
            Stmt::ScopeMember {
                name,
                args,
                args_span,
                body,
                span,
                ..
            } if is_registered_dsl_marker(name) => {
                validate_dsl_args(name, args, args_span, *span, diags);
                check_dsl_body(body, diags);
            }
            _ => {
                for child in child_bodies(statement) {
                    check_dsl_body(child, diags);
                }
            }
        }
    }
}

fn is_registered_dsl_marker(name: &str) -> bool {
    crate::Policy::applied_rule(name).is_some_and(|row| {
        matches!(row.status, crate::Policy::RuleStatus::Active)
            && row.sites.contains(&crate::Policy::RuleSite::Block)
    })
}

fn validate_dsl_args(
    name: &str,
    args: &[Expr],
    args_span: &Option<Span>,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) {
    let valid = crate::Policy::applied_rule(name).is_some_and(|row| {
        row.sites.contains(&crate::Policy::RuleSite::Block)
            && row
                .signature
                .argument_bindings(&vec![None; args.len()])
                .is_some()
    });
    if !valid {
        let at = args_span.unwrap_or(span);
        let expected = crate::Policy::applied_rule(name)
            .map(|row| format!("`#{name}{}`", row.signature.render()))
            .unwrap_or_else(|| format!("`#{name}(…)`"));
        diags.push(Diagnostic::error(
            "E0617",
            format!("`#{name}` has an invalid DSL header"),
            "a DSL marker uses the typed signature published by the applied-rule registry".to_string(),
            format!("write {expected}"),
            Some(at),
        ));
    }
}

/// Validate a single direct-child member against `marker`'s vocabulary.
fn validate_member(
    marker: &str,
    name: &str,
    args: &[Expr],
    args_span: &Option<Span>,
    is_first: bool,
    dot_span: Span,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) {
    let vocab = match Syntax::scope_members(marker) {
        Some(v) => v,
        None => {
            diags.push(Diagnostic::error(
                "E0614",
                format!("`#{}` blocks have no members", marker),
                format!(
                    "only some rule blocks declare a `.member {{ … }}` vocabulary; `#{}` isn't one of them",
                    marker
                ),
                format!("remove this `.{}` block", name),
                Some(dot_span),
            ));
            return;
        }
    };
    if !vocab.contains(&name) {
        diags.push(Diagnostic::error(
            "E0614",
            format!("`.{}` isn't a member of `#{}`", name, marker),
            format!(
                "a `#{}` block understands these members: {}",
                marker,
                vocab_list(vocab)
            ),
            format!("use one of {}, or remove this block", vocab_list(vocab)),
            Some(dot_span),
        ));
        return;
    }
    validate_args(name, args, args_span, span, diags);
    if name == Syntax::SCOPE_TEST_SETUP && !is_first {
        diags.push(Diagnostic::error(
            "E0616",
            "`.setup` must be the first statement in the test".to_string(),
            "`.setup` marks the test's initialization; anything before it would run first"
                .to_string(),
            "move `.setup { … }` to the top of the block".to_string(),
            Some(dot_span),
        ));
    }
}

/// D-DOTSCOPE1 argument shapes. The span used for the diagnostic is the arg
/// group when present, else the whole member.
fn validate_args(
    name: &str,
    args: &[Expr],
    args_span: &Option<Span>,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) {
    let at = args_span.unwrap_or(span);
    match name {
        n if n == Syntax::SCOPE_TEST_SETUP => {
            if !args.is_empty() {
                diags.push(Diagnostic::error(
                    "E0617",
                    format!("`.{}` takes no arguments", name),
                    format!("`.{}` marks a region; it has nothing to configure", name),
                    format!("write `.{} {{ … }}`", name),
                    Some(at),
                ));
            }
        }
        n if n == Syntax::SCOPE_TEST_EXPECT_FAIL => {
            let ok = args.is_empty()
                || (args.len() == 1
                    && matches!(
                        &args[0],
                        Expr::Ident(code, _) if code.starts_with("E30")
                            && jet_foundation::Registry::diagnostic(code).is_some()
                    ));
            if !ok {
                diags.push(Diagnostic::error(
                    "E0617",
                    "`.expect_fail` takes one optional runtime stop code".to_string(),
                    "the code names the E30xx stop the region must produce".to_string(),
                    "write `.expect_fail { … }` or `.expect_fail(E3010) { … }`".to_string(),
                    Some(at),
                ));
            }
        }
        n if n == Syntax::SCOPE_TEST_TIMEOUT => {
            let ok = args.len() == 1 && is_duration(&args[0]);
            if !ok {
                diags.push(Diagnostic::error(
                    "E0617",
                    "`.timeout` needs exactly one duration".to_string(),
                    "the region must complete within a time budget, written as a duration literal"
                        .to_string(),
                    "write `.timeout(500ms) { … }` (`ns`/`us`/`ms`/`s`)".to_string(),
                    Some(at),
                ));
            }
        }
        n if n == Syntax::SCOPE_TEST_SKIP => {
            let ok = args.is_empty() || (args.len() == 1 && matches!(args[0], Expr::Str(..)));
            if !ok {
                diags.push(Diagnostic::error(
                    "E0617",
                    "`.skip` takes at most one reason string".to_string(),
                    "`.skip` optionally records why the region is skipped".to_string(),
                    "write `.skip { … }` or `.skip(\"reason\") { … }`".to_string(),
                    Some(at),
                ));
            }
        }
        _ => {}
    }
}

/// A duration literal usable by `.timeout` — a bare unit literal whose suffix is
/// a recognized time unit (D-UNITLIT1; no `#UnitFamily` in scope required).
fn is_duration(e: &Expr) -> bool {
    matches!(e, Expr::UnitLit { suffix, .. } if Syntax::duration_suffix_nanos(suffix).is_some())
}

fn vocab_list(vocab: &[&str]) -> String {
    vocab
        .iter()
        .map(|m| format!("`.{}`", m))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Invoke `f(member_name, dot_span)` for every scope member anywhere in `stmts`,
/// recursing through control blocks and member bodies.
fn walk_members(stmts: &[Stmt], f: &mut impl FnMut(&str, Span)) {
    for s in stmts {
        if let Stmt::ScopeMember {
            name,
            dot_span,
            body,
            ..
        } = s
        {
            f(name, *dot_span);
            walk_members(body, f);
        } else {
            for child in child_bodies(s) {
                walk_members(child, f);
            }
        }
    }
}

/// Every `Vec<Stmt>` block body a statement carries. Leaf statements yield none.
fn child_bodies(s: &Stmt) -> Vec<&[Stmt]> {
    match s {
        Stmt::Switch {
            arms, else_body, ..
        } => {
            let mut v: Vec<&[Stmt]> = arms.iter().map(|a| a.body.as_slice()).collect();
            if let Some(eb) = else_body {
                v.push(eb.as_slice());
            }
            v
        }
        Stmt::While { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::CountedLoop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => vec![body.as_slice()],
        _ => Vec::new(),
    }
}
