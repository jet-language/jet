//! D-VALIDATE1 (ratified 2026-07-12, card #506): `validate { … }` in-body
//! block. See docs/spec/syntax-decisions.md for the full ratified law.
//!
//! Runs pre-registration, same timing as
//! `CheckerFieldPolicy::process_computed_fields` (D-FIELDPOL1): for each
//! struct with a non-empty `validate_block`,
//!   1. check every rule statement's shape — it must be exactly
//!      `check(cond, at: field, "msg")` (E0353); `field` must name a real
//!      field of the struct (E0354);
//!   2. rewrite bare `Ident`s in `cond`/`msg` that name a sibling field to
//!      `value.<field>` — reuses `CheckerFieldPolicy::rewrite_field_refs`,
//!      the exact substitution D-FIELDPOL1 computed fields use for
//!      `self.<field>`, with the receiver renamed to `value`;
//!   3. synthesize `fn validate(value: Self) => Self ? [FieldError] { … }`
//!      as a plain (non-trait) `impl Type { }` block appended to the
//!      module, `is_pure: true` — the existing `pure fn` purity pass
//!      (S60/E3401) then enforces "a rule may reference only fields and
//!      pure calls" over the whole synthesized body for free, no separate
//!      purity pass needed.
//!
//! The in-body block + `Type.validate(value)` standalone entry point are
//! wired. Typed decoding and validation share the owner-ratified accumulated
//! `[FieldError]` result contract.

use super::*;
use crate::AST::{
    AccessConvention, Binding, CallArg, CallArgFlags, Expr, Func, ImplDef, Item, Param, Stmt,
    StrPart, StructDef, SwitchArm, Type,
};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use std::collections::HashSet;

/// Entry point, called once per module before registration (same timing as
/// `process_computed_fields`/`inject_patchable_types`).
pub(crate) fn process_validate_blocks(items: &mut Vec<Item>, diags: &mut Vec<Diagnostic>) {
    let mut generated = Vec::new();
    for item in items.iter() {
        let Item::Struct(s) = item else { continue };
        if s.validate_block.is_empty() {
            continue;
        }
        if let Some(imp) = synthesize_validate_impl(s, diags) {
            generated.push(Item::Impl(imp));
        }
    }
    items.extend(generated);
}

/// One accepted `check(cond, at: field, "msg")` rule.
struct ValidateRule {
    cond: Expr,
    field: String,
    msg: Expr,
}

fn synthesize_validate_impl(s: &StructDef, diags: &mut Vec<Diagnostic>) -> Option<ImplDef> {
    let field_names: HashSet<String> = s.fields.iter().map(|f| f.name.clone()).collect();
    let mut rules = Vec::new();
    let mut all_ok = true;
    for stmt in &s.validate_block {
        match extract_check_rule(stmt, &field_names) {
            Ok(rule) => rules.push(rule),
            Err(d) => {
                diags.push(d);
                all_ok = false;
            }
        }
    }
    if !all_ok {
        // Diagnostics already registered; don't synthesize a function whose
        // body sema never finished validating (I2: nothing malformed reaches
        // codegen).
        return None;
    }
    let span = s.validate_span.unwrap_or(s.name_span);
    Some(build_validate_impl(s, &rules, span))
}

fn extract_check_rule(stmt: &Stmt, field_names: &HashSet<String>) -> Result<ValidateRule, Diagnostic> {
    let bad_shape = |span: Span| e0353_bad_validate_rule(span);
    let Stmt::Expr(Expr::Call(call)) = stmt else {
        return Err(bad_shape(stmt.span()));
    };
    if call.name != Syntax::VALIDATE_CHECK_FN {
        return Err(bad_shape(call.name_span));
    }
    if call.args.len() != 3 {
        return Err(bad_shape(call.name_span));
    }
    let cond_arg = &call.args[0];
    let at_arg = &call.args[1];
    let msg_arg = &call.args[2];
    if cond_arg.label.is_some() {
        return Err(bad_shape(cond_arg.span));
    }
    let Some((label, _)) = &at_arg.label else {
        return Err(bad_shape(at_arg.span));
    };
    if label != "at" {
        return Err(bad_shape(at_arg.span));
    }
    if msg_arg.label.is_some() {
        return Err(bad_shape(msg_arg.span));
    }
    let Expr::Ident(field_name, field_span) = &at_arg.expr else {
        return Err(bad_shape(at_arg.span));
    };
    if !field_names.contains(field_name.as_str()) {
        return Err(e0354_unknown_validate_field(field_name, *field_span));
    }
    Ok(ValidateRule {
        cond: cond_arg.expr.clone(),
        field: field_name.clone(),
        msg: msg_arg.expr.clone(),
    })
}

fn e0353_bad_validate_rule(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0353",
        "a `validate` rule must be `check(condition, at: field, \"message\")`".to_string(),
        "D-VALIDATE1: every statement in a `validate { … }` block is a `check` call — the first argument is the rule's condition, `at:` names the field the failure is reported against, and the last argument is the message text".to_string(),
        "write: check(<condition>, at: <field>, \"<message>\")".to_string(),
        Some(span),
    )
}

fn e0354_unknown_validate_field(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0354",
        format!("`at: {name}` doesn't name a field on this struct"),
        "D-VALIDATE1: `check(…, at: field, …)`'s `at:` argument must be a bare field name declared on the same struct — it's how the reported `FieldError.path` is chosen".to_string(),
        "fix the field name, or add the field to the struct".to_string(),
        Some(span),
    )
}

fn ident(name: &str, span: Span) -> Expr {
    Expr::Ident(name.to_string(), span)
}

fn str_lit(text: &str, span: Span) -> Expr {
    Expr::Str(vec![StrPart::Lit(text.to_string())], span)
}

fn call_arg(expr: Expr, span: Span) -> CallArg {
    CallArg {
        convention: AccessConvention::Read,
        expr,
        span,
        flags: CallArgFlags::default(),
        label: None,
        spread: false,
    }
}

fn method_call(receiver: Expr, method: &str, args: Vec<CallArg>, span: Span) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: method.to_string(),
        method_span: span,
        owner_type_args: Vec::new(),
        type_args: Vec::new(),
        args,
        recv_type: None,
        resolved_ret: None,
    }
}

const ERRORS_VAR: &str = "__errors";
const VALUE_VAR: &str = "value";

/// `fn validate(value: Self) => Self ? [FieldError] { … }`.
fn build_validate_impl(s: &StructDef, rules: &[ValidateRule], span: Span) -> ImplDef {
    let field_names: HashSet<String> = s.fields.iter().map(|f| f.name.clone()).collect();

    let errors_binding = Stmt::Val(Binding {
        mutable: true,
        track: false,
        track_span: None,
                reactive_local: false,
                reactive_local_span: None,
                reactive_shared: false,
                reactive_shared_span: None,
                reactive_upgrade: false,
        meta: None,
        name: ERRORS_VAR.to_string(),
        name_span: span,
        pattern: None,
        ty: Some(Type::List(Box::new(field_error_ty()))),
        ty_span: Some(span),
        init: Expr::ListLit(Vec::new(), span),
        is_comptime: false,
        ct: None,
        uninit: false,
        arena_view: false,
                string_view: false,
                gc_promotion: None,
                gc_transferred: false,
    });

    let mut body = vec![errors_binding];
    for rule in rules {
        let mut cond = rule.cond.clone();
        rewrite_field_refs(&mut cond, &field_names, VALUE_VAR);
        let mut msg = rule.msg.clone();
        rewrite_field_refs(&mut msg, &field_names, VALUE_VAR);

        let field_error_lit = Expr::StructLit {
            type_name: "FieldError".to_string(),
            type_args: Vec::new(),
            import_ns: None,
            as_trait: None,
            fields: vec![
                ("path".to_string(), span, str_lit(&rule.field, span)),
                ("reason".to_string(), span, msg),
            ],
            inferred: false,
            span,
        };
        let push_stmt = Stmt::Expr(method_call(
            ident(ERRORS_VAR, span),
            "push",
            vec![call_arg(field_error_lit, span)],
            span,
        ));
        let not_cond = Expr::Unary(crate::AST::UnOp::Not, Box::new(cond), span);
        body.push(Stmt::Switch {
            subject: Expr::Bool(true, span),
            arms: vec![SwitchArm {
                span: not_cond.span(),
                cond: not_cond,
                body: vec![push_stmt],
            }],
            else_body: None,
            span,
        });
    }

    let has_errors = Expr::Unary(
        crate::AST::UnOp::Not,
        Box::new(method_call(ident(ERRORS_VAR, span), "is_empty", Vec::new(), span)),
        span,
    );
    let return_err = Stmt::Return(
        Some(Expr::Err(Box::new(ident(ERRORS_VAR, span)), span)),
        span,
    );
    body.push(Stmt::Switch {
        subject: Expr::Bool(true, span),
        arms: vec![SwitchArm {
            span: has_errors.span(),
            cond: has_errors,
            body: vec![return_err],
        }],
        else_body: None,
        span,
    });
    body.push(Stmt::Return(
        Some(Expr::Ok(
            Box::new(Expr::Copy(Box::new(ident(VALUE_VAR, span)), span)),
            span,
        )),
        span,
    ));

    let value_param = Param {
        // D-VALIDATE1: `Type.validate(value)` reads `value` (no `^` at the
        // call site, matching the ratified spelling) and hands back an
        // owned copy on success (`return Ok(copy value)`).
        convention: AccessConvention::Read,
        name: VALUE_VAR.to_string(),
        name_span: span,
        ty: Type::Named(s.name.clone()),
        ty_span: span,
        default: None,
        variadic: false,
        variadic_bound_list: None, declared_view_from_names: None,
    };

    let validate_func = Func {
        span,
        is_pub: s.is_pub,
        is_package_pub: s.is_package_pub,
        external_type: None,
        name: "validate".to_string(),
        name_span: span,
        meta: None,
        type_params: Vec::new(),
        params: vec![value_param],
        return_type: Some(Type::Result {
            ok: Box::new(Type::Named(s.name.clone())),
            err: Box::new(Type::List(Box::new(field_error_ty()))),
        }),
        return_type_span: Some(span),
        return_view_provenance: None,
        declared_return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
        is_unsafe: false,
        unsafe_reason: None,
        unsafe_span: None,
        // S60/E3401: enforces D-VALIDATE1's "cond/msg reference only fields
        // and pure calls" for the whole synthesized body, no separate pass.
        is_pure: true,
        is_reactive: false,
                reactive_upgrades: Vec::new(),
        is_replayable: false,
        replayable_span: None,
        is_task: false,
        task_span: None,
        every: None,
        task_metadata: None,
        is_must_use: false,
        must_use_span: None,
        maturity: None,
        maturity_span: None,
        kernel: None,
        is_inline: false,
        is_inline_always: false,
        inline_span: None,
        is_sanitizer: false,
        scrub_tag: None,
        declared_effects: None,
        effect_via: None,
        state_requires: None,
        state_transition: None,
        web_marker: None,
        pre: Vec::new(),
        post: Vec::new(),
        inline_foreign: None,
        body,
    };

    ImplDef {
        span,
        type_name: s.name.clone(),
        type_span: s.name_span,
        trait_name: None,
        trait_span: None,
        methods: vec![validate_func],
        delegation_field: None,
        assoc_type_impls: Vec::new(),
        is_generated_serde: false,
        os_target: None,
    }
}
