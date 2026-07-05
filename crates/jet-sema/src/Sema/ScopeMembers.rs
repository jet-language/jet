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
//!   `.expect_fail` take none; `.skip` takes an optional reason string).
//! - E0618 — a member nested inside another member or a control block (they must
//!   stay flat, at the top level of the marker body).
//!
//! Everything here is compile-time only (I3): the checker recurses into member
//! bodies for ordinary type-checking, and codegen lowers the members into the
//! `jet test` harness. `#Test`/`#Bench` bodies are validated only under their
//! own modes (they are not compiled otherwise, mirroring the rest of sema).

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{ElseBranch, Expr, Item, Stmt, TraitImplBlock};

/// Validate every scope-member statement in the program. Runs in every mode
/// (not just `jet test`): a malformed member is a structural error that `jet
/// check` / `jet run` should surface too, even though the members only *run*
/// under `jet test`. The check needs no type information.
pub fn check(items: &[Item]) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
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
        diags.push(Diagnostic::error(
            "E0615",
            format!(
                "`.{}` only works inside a marker block that declares it",
                name
            ),
            "a leading-dot member statement resolves against the enclosing `#Marker`'s vocabulary — here there is no such block".to_string(),
            format!(
                "move it inside a `#{}(\"…\") {{ … }}` block, or write an ordinary statement",
                Syntax::KW_TEST
            ),
            Some(dot_span),
        ));
    });
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
                    "only some marker blocks declare a `.member {{ … }}` vocabulary; `#{}` isn't one of them",
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
        n if n == Syntax::SCOPE_TEST_SETUP || n == Syntax::SCOPE_TEST_EXPECT_FAIL => {
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
        Stmt::If(ifs) => {
            let mut v: Vec<&[Stmt]> = Vec::new();
            let mut cur = ifs;
            loop {
                v.push(cur.then_body.as_slice());
                match &cur.else_branch {
                    Some(ElseBranch::ElseIf(next)) => cur = next,
                    Some(ElseBranch::Else(body)) => {
                        v.push(body.as_slice());
                        break;
                    }
                    None => break,
                }
            }
            v
        }
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
        | Stmt::SuppressMustUse { body, .. }
        | Stmt::Region { body, .. }
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
