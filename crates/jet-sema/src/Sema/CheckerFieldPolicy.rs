//! D-FIELDPOL1 (card #181): computed fields — `name: T => expr` on a struct.
//! Never stored; every read recomputes `expr` against the struct's current
//! sibling fields (data fields or other computed fields — computed-from-
//! computed is fine, a cycle is E0338).
//!
//! This pass runs before registration (mirrors `inject_patchable_types`):
//! for each struct, in order,
//!   1. detect a cycle among computed fields (E0338), on the RAW parsed expr
//!      (bare `Ident`s still name sibling fields at this point);
//!   2. rewrite every `Ident` in a computed field's expr that names a sibling
//!      field (data or computed) to `self.<field>` — the ballot's "bound
//!      (read-only) to `self.<field>`" scoping, made real by substitution
//!      instead of a synthetic scope, so both sema and codegen resolve it
//!      through the ordinary `self.field` path (no new lowering path, I3);
//!   3. synthesize a `fn <field>(self) -> T { return <rewritten expr>; }`
//!      method per computed field and append it to `s.methods`, so it flows
//!      through the *exact* same registration / body-checking / TIR codegen
//!      pipeline as a hand-written method (S62 delegation-method precedent).
//!      Codegen (`Codegen::Context`/`Codegen::TIR::lower`) marks the field
//!      "computed" and routes every `Expr::Field` read of it to a call of
//!      this method instead of a struct member access.
//!
//! Known v1 gap (not a soundness issue — see below): the rewrite in step 2
//! walks every `Expr` shape `expr_refs_name` (Captures.rs) also walks,
//! EXCEPT it does not descend into a lambda body or an if-expression's
//! statement block. A sibling-field reference in one of those positions is
//! simply left as a bare `Ident` — sema's ordinary name resolution then
//! reports it as an unknown name (never a silent miscompile: nothing reaches
//! codegen that rustc could reject, I2 holds). Cycle detection has the same
//! boundary (`expr_refs_name`'s own `Lambda(_) => false`), so this can't hide
//! a cycle through a lambda either.

use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{
    AccessConvention, EnumLitArg, Expr, Field, Func, Item, OrFallback, Param, Pattern, StrPart,
    Stmt, StructDef, StructPatField, Type,
};
use std::collections::{HashMap, HashSet};

/// Entry point, called once per module before registration (same timing as
/// `inject_patchable_types`).
pub(crate) fn process_computed_fields(items: &mut Vec<Item>, diags: &mut Vec<Diagnostic>) {
    for item in items.iter_mut() {
        let Item::Struct(s) = item else { continue };
        if !s.fields.iter().any(|f| f.computed.is_some()) {
            continue;
        }
        let cyclic = check_computed_field_cycles(s, diags);
        rewrite_computed_field_bodies(s);
        for f in &s.fields {
            let Some(_) = &f.computed else { continue };
            // A field caught in a cycle still gets a synthesized getter (so
            // the rest of the pipeline sees a consistent struct) — the E0338
            // diagnostic already aborts the compile before codegen runs.
            let _ = cyclic;
            s.methods.push(synthesize_computed_field_getter(f));
        }
    }
}

/// E0338: a cycle among computed-field dependencies (including a field that
/// references itself). Returns `true` when at least one cycle was reported.
fn check_computed_field_cycles(s: &StructDef, diags: &mut Vec<Diagnostic>) -> bool {
    let computed: Vec<&Field> = s.fields.iter().filter(|f| f.computed.is_some()).collect();
    if computed.is_empty() {
        return false;
    }
    let names: Vec<&str> = computed.iter().map(|f| f.name.as_str()).collect();
    // field name -> other computed field names its expr mentions (self
    // included — a self-mention is a trivial one-node cycle).
    let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
    for f in &computed {
        let expr = f.computed.as_deref().expect("filtered to computed fields");
        let mut ds = Vec::new();
        for &other in &names {
            if expr_refs_name(expr, other) {
                ds.push(other);
            }
        }
        deps.insert(f.name.as_str(), ds);
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }
    let mut color: HashMap<&str, Color> =
        names.iter().map(|&n| (n, Color::White)).collect();
    let mut reported: HashSet<&str> = HashSet::new();
    let span_of: HashMap<&str, Span> =
        computed.iter().map(|f| (f.name.as_str(), f.name_span)).collect();

    fn dfs<'a>(
        node: &'a str,
        deps: &HashMap<&'a str, Vec<&'a str>>,
        color: &mut HashMap<&'a str, Color>,
        span_of: &HashMap<&'a str, Span>,
        struct_name: &str,
        reported: &mut HashSet<&'a str>,
        diags: &mut Vec<Diagnostic>,
    ) {
        color.insert(node, Color::Gray);
        if let Some(neighbors) = deps.get(node) {
            for &nb in neighbors {
                match color.get(nb) {
                    Some(Color::Gray) => {
                        if reported.insert(node) {
                            diags.push(e0338_computed_field_cycle(
                                struct_name,
                                node,
                                nb,
                                span_of[node],
                            ));
                        }
                    }
                    Some(Color::White) => {
                        dfs(nb, deps, color, span_of, struct_name, reported, diags)
                    }
                    _ => {}
                }
            }
        }
        color.insert(node, Color::Black);
    }

    for &n in &names {
        if color[n] == Color::White {
            dfs(n, &deps, &mut color, &span_of, &s.name, &mut reported, diags);
        }
    }
    !reported.is_empty()
}

fn e0338_computed_field_cycle(
    struct_name: &str,
    field: &str,
    via: &str,
    span: Span,
) -> Diagnostic {
    let what = if field == via {
        format!(
            "computed field `{}.{}` references itself",
            struct_name, field
        )
    } else {
        format!(
            "computed field `{}.{}` forms a cycle through `{}`",
            struct_name, field, via
        )
    };
    Diagnostic::error(
        "E0338",
        what,
        "a computed field's formula may only reference OTHER fields — the dependency graph among computed fields must have no cycle".to_string(),
        format!(
            "break the cycle: make `{}` or `{}` a plain stored field, or rework the formula",
            field, via
        ),
        Some(span),
    )
}

/// Step 2: substitute every `Ident` in each computed field's expr that names
/// a sibling field (data or computed, self included — harmless on a field
/// already flagged E0338) with `self.<field>`.
fn rewrite_computed_field_bodies(s: &mut StructDef) {
    let names: HashSet<String> = s.fields.iter().map(|f| f.name.clone()).collect();
    for f in &mut s.fields {
        if let Some(expr) = &mut f.computed {
            rewrite_self_field_refs(expr, &names);
        }
    }
}

fn self_field(name: &str, span: Span) -> Expr {
    Expr::Field(
        Box::new(Expr::Ident(Syntax::KW_SELF.to_string(), span)),
        name.to_string(),
        span,
    )
}

/// Mirrors `Captures::expr_refs_name`'s coverage exactly (same Lambda/
/// if-block boundary) — see the module doc for why that boundary is safe.
fn rewrite_self_field_refs(expr: &mut Expr, names: &HashSet<String>) {
    match expr {
        Expr::Ident(n, span) => {
            if names.contains(n.as_str()) {
                *expr = self_field(n, *span);
            }
        }
        Expr::PtrFromAddr { addr, .. } => rewrite_self_field_refs(addr, names),
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _) => rewrite_self_field_refs(inner, names),
        Expr::Binary(_, l, r, _) => {
            rewrite_self_field_refs(l, names);
            rewrite_self_field_refs(r, names);
        }
        Expr::CompareChain { operands, .. } => {
            for e in operands {
                rewrite_self_field_refs(e, names);
            }
        }
        Expr::Call(c) => {
            for a in &mut c.args {
                rewrite_self_field_refs(&mut a.expr, names);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            rewrite_self_field_refs(callee, names);
            for a in args {
                rewrite_self_field_refs(&mut a.expr, names);
            }
        }
        Expr::Field(inner, _, _)
        | Expr::Tainted(inner, _)
        | Expr::Present(inner, _)
        | Expr::Try(inner, _, _) => rewrite_self_field_refs(inner, names),
        Expr::OptField { base, .. } => rewrite_self_field_refs(base, names),
        Expr::MethodCall {
            receiver, args, ..
        } => {
            rewrite_self_field_refs(receiver, names);
            for a in args {
                rewrite_self_field_refs(&mut a.expr, names);
            }
        }
        Expr::Index { base, index, .. } => {
            rewrite_self_field_refs(base, names);
            rewrite_self_field_refs(index, names);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            rewrite_self_field_refs(base, names);
            rewrite_self_field_refs(start, names);
            rewrite_self_field_refs(end, names);
        }
        Expr::ListLit(elems, _) => {
            for e in elems {
                rewrite_self_field_refs(e, names);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, e) in fields {
                rewrite_self_field_refs(e, names);
            }
        }
        Expr::MapLit(entries, _) => {
            for (k, v) in entries {
                rewrite_self_field_refs(k, names);
                rewrite_self_field_refs(v, names);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, e) in fields {
                rewrite_self_field_refs(e, names);
            }
        }
        Expr::EnumLit { args, .. } => {
            for a in args {
                match a {
                    EnumLitArg::Positional(e) => rewrite_self_field_refs(e, names),
                    EnumLitArg::Named { expr, .. } => rewrite_self_field_refs(expr, names),
                }
            }
        }
        Expr::Ok(inner, _) | Expr::Err(inner, _) => rewrite_self_field_refs(inner, names),
        Expr::OrFallback {
            value, fallback, ..
        } => {
            rewrite_self_field_refs(value, names);
            match fallback {
                OrFallback::Value(e) => rewrite_self_field_refs(e, names),
                OrFallback::Return(Some(e), _) => rewrite_self_field_refs(e, names),
                _ => {}
            }
        }
        Expr::PatternTest {
            subject, pattern, ..
        } => {
            rewrite_self_field_refs(subject, names);
            if let Pattern::Struct { fields, .. } = pattern {
                for field in fields {
                    if let StructPatField::Value { value, .. } = field {
                        rewrite_self_field_refs(value, names);
                    }
                }
            }
        }
        Expr::Str(parts, _) => {
            for p in parts {
                if let StrPart::Interp(e, _) = p {
                    rewrite_self_field_refs(e, names);
                }
            }
        }
        Expr::If {
            cond,
            then_value,
            else_value,
            ..
        } => {
            // Known v1 gap (see module doc): `then_body`/`else_body` are not
            // walked — a sibling-field reference in a `let` there surfaces as
            // an ordinary unknown-name sema error, not a miscompile.
            rewrite_self_field_refs(cond, names);
            rewrite_self_field_refs(then_value, names);
            rewrite_self_field_refs(else_value, names);
        }
        Expr::FanOut { callee, items, .. } => {
            rewrite_self_field_refs(callee, names);
            for e in items {
                rewrite_self_field_refs(e, names);
            }
        }
        Expr::Paren(inner, _) => rewrite_self_field_refs(inner, names),
        Expr::Spread(inner, _) => rewrite_self_field_refs(inner, names),
        // Leaves, and `Lambda` (a separate scope — not walked, matching
        // `expr_refs_name`'s own boundary).
        _ => {}
    }
}

/// Step 3: `fn <field>(self) -> T { return <rewritten expr>; }`, built the
/// same way `Registration::synthesize_delegation_method` builds an S62
/// forwarding method — a full `Func` with a placeholder self type (`S27`:
/// sema fills in the owner type from `owner_type` when it checks the body),
/// so it flows through the ordinary method-registration / body-check /
/// codegen pipeline with no special-casing anywhere else in sema.
fn synthesize_computed_field_getter(f: &Field) -> Func {
    let span = f.name_span;
    let body_expr = (**f.computed.as_ref().expect("computed field")).clone();

    let self_param = Param {
        convention: AccessConvention::Read,
        name: Syntax::KW_SELF.to_string(),
        name_span: span,
        ty: Type::Named(String::new()), // S27: sema fills in the actual type name
        ty_span: span,
        default: None,
        variadic: false,
        variadic_bound_list: None,
    };

    Func {
        is_pub: f.is_pub,
        is_package_pub: f.is_package_pub,
        external_type: None,
        name: f.name.clone(),
        name_span: span,
        type_params: vec![],
        params: vec![self_param],
        return_type: Some(f.ty.clone()),
        is_unsafe: false,
        is_pure: false,
        is_reactive: false,
        is_must_use: false,
        must_use_span: None,
        is_inline: false,
        is_inline_always: false,
        inline_span: None,
        is_sanitizer: false,
        declared_effects: None,
        effect_via: None,
        state_requires: None,
        state_transition: None,
        web_marker: None,
        pre: Vec::new(),
        post: Vec::new(),
        body: vec![Stmt::Return(Some(body_expr), span)],
    }
}
