use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{self, is_type_var_name};
use crate::Syntax;
use crate::AST::{BinOp, ElseBranch, Expr, IfStmt, Pattern, Stmt, Type, VariantPayload};
use std::collections::{HashMap, HashSet};

pub(crate) fn compound_why(op: BinOp) -> String {
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
            "`+ - * /` work on Int and Float".to_string()
        }
        _ => format!("`{}` is a whole-number operation (Int only)", op.spell()),
    }
}

/// `T?` passed where plain `T` is expected (E0310).
pub(crate) fn option_used_where_plain_expected(want: &Type, got: &Type) -> bool {
    matches!(got, Type::Option(inner) if want.unwrap_option().is_none() && **inner == *want)
}

pub(crate) fn is_default_error(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if n == Syntax::TYPE_ERROR)
}

pub(crate) fn type_fix_hint(want: &Type, got: &Type) -> String {
    match (want, got) {
        (Type::Float, Type::Int) => "write the number with a decimal part, like `2.0`".to_string(),
        (Type::Int, Type::Float) => "drop the decimal part, like `2`".to_string(),
        (Type::String, _) => "put the value in text with interpolation: \"{x}\"".to_string(),
        _ => format!("use {} here", want.show()),
    }
}

/// D-TYPEDTEXT1=D: a plain `String` reaching a `Sql`/`Html` position. `None`
/// when `want`/`got` isn't that shape — caller falls back to the generic
/// mismatch diagnostic.
pub(crate) fn typed_text_mismatch(want: &Type, got: &Type, span: Span) -> Option<Diagnostic> {
    let Type::Named(tn) = want else {
        return None;
    };
    if (tn != "Sql" && tn != "Html") || *got != Type::String {
        return None;
    }
    Some(Diagnostic::error(
        "E0149",
        format!("a runtime `String` can't be used as `{}`", tn),
        "interpolating untrusted text into a query or page is how injection happens; only a checked literal (its `{value}` holes become bound parameters or escaped insertions) may build one".to_string(),
        format!(
            "write it as a literal with `{{value}}` holes, or use `{}.raw(\"…\")` if you have audited the text",
            tn
        ),
        Some(span),
    ))
}

pub(crate) fn aliasing_while_mut(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0204",
        format!(
            "`{}` is being changed in this call, so it can't be used again here",
            name
        ),
        "while something is being changed, nobody else may be looking at it".to_string(),
        format!(
            "pass `{}{}` only once, or copy first with `{}.clone()`",
            Syntax::SIGIL_MUTATE,
            name,
            name
        ),
        Some(span),
    )
}

pub(crate) fn aliasing_mut_after_read(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0204",
        format!(
            "`{}` is already shared in this call, so it can't be changed here too",
            name
        ),
        "while something is being looked at, nobody else may be changing it".to_string(),
        format!(
            "drop the extra use of `{}`, or copy first with `{} .clone()`",
            name, name
        ),
        Some(span),
    )
}

pub(crate) fn loop_control_outside(kw: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0115",
        format!("`{}` only works inside a loop", kw),
        format!(
            "`{}` and `{}` steer the nearest `{}` loop",
            Syntax::KW_BREAK,
            Syntax::KW_CONTINUE,
            Syntax::KW_LOOP,
        ),
        "move this inside a loop, or remove it".to_string(),
        Some(span),
    )
}

/// D-LOOPLABEL2 (E0987): `break name@` / `continue name@` names a loop label that is
/// not in scope. The fix lists the labels that *are* reachable here.
pub(crate) fn undefined_loop_label(name: &str, in_scope: &[String], span: Span) -> Diagnostic {
    let fix = if in_scope.is_empty() {
        "label an enclosing loop with `name@ loop { … }` first".to_string()
    } else {
        let labels = in_scope
            .iter()
            .map(|l| format!("`{l}@`"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("use a label in scope: {labels}")
    };
    Diagnostic::error(
        "E0987",
        format!("no loop labeled `{name}@` is in scope"),
        "a labeled `break`/`continue` must name an enclosing `name@ loop` (D-LOOPLABEL2)"
            .to_string(),
        fix,
        Some(span),
    )
}

/// Does this block definitely hit a `return` on every path?
pub(crate) fn block_definitely_returns(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_definitely_returns)
}

pub(crate) fn stmt_definitely_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_, _) => true,
        // D-TOOL2 (E2-M11): `todo` is diverging — a bare `todo;` satisfies
        // the "every path must return" check just like `return`.
        Stmt::Expr(Expr::Todo { .. }) => true,
        Stmt::If(ifs) => if_definitely_returns(ifs),
        Stmt::Switch {
            arms, else_body, ..
        } => {
            arms.iter().all(|a| block_definitely_returns(&a.body))
                && else_body
                    .as_ref()
                    .map(|b| block_definitely_returns(b))
                    .unwrap_or(true)
        }
        _ => false,
    }
}

pub(crate) fn if_definitely_returns(ifs: &IfStmt) -> bool {
    if !block_definitely_returns(&ifs.then_body) {
        return false;
    }
    match &ifs.else_branch {
        Some(ElseBranch::Else(b)) => block_definitely_returns(b),
        Some(ElseBranch::ElseIf(next)) => if_definitely_returns(next),
        None => false,
    }
}

pub(crate) fn is_cloneable(
    ty: &Type,
    registry: &TypeRegistry,
    structs: &HashMap<String, Vec<(Option<String>, Type)>>,
) -> bool {
    let mut visiting = HashSet::new();
    is_cloneable_rec(ty, registry, structs, &mut visiting)
}

/// `is_cloneable`'s recursion, with a `visiting` guard against self-referential
/// types (`enum Nat { Succ(n: Nat) }`). Every such type compiles to a `Box`
/// indirection (I3: sema already proves this elsewhere), so a cycle back to a
/// type still being visited is vacuously cloneable — the recursion doesn't need
/// to unwind further to know that. Without this guard the walk never terminates.
fn is_cloneable_rec(
    ty: &Type,
    registry: &TypeRegistry,
    structs: &HashMap<String, Vec<(Option<String>, Type)>>,
    visiting: &mut HashSet<String>,
) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
            is_cloneable_rec(inner, registry, structs, visiting)
        }
        Type::Map { key, value } => {
            is_cloneable_rec(key, registry, structs, visiting)
                && is_cloneable_rec(value, registry, structs, visiting)
        }
        Type::Result { ok, err } => {
            is_cloneable_rec(ok, registry, structs, visiting)
                && is_cloneable_rec(err, registry, structs, visiting)
        }
        Type::Fn { .. } => false,
        Type::Named(name) if is_type_var_name(name) || core_type_known(name) => true,
        Type::Named(name) => {
            if !visiting.insert(name.clone()) {
                return true;
            }
            let result = registry.contains(name)
                && match registry.types.get(name) {
                    Some(TypeDef::Struct { fields, .. }) => {
                        fields.iter().all(|(_, _, fty, is_ref, _)| {
                            !*is_ref && is_cloneable_rec(fty, registry, structs, visiting)
                        })
                    }
                    Some(TypeDef::Enum { variants, .. }) => {
                        variants.values().all(|(_, p)| match p {
                            VariantPayload::Unit => true,
                            VariantPayload::Single(t, _) => {
                                is_cloneable_rec(t, registry, structs, visiting)
                            }
                            VariantPayload::Named(fs) => fs
                                .iter()
                                .all(|f| is_cloneable_rec(&f.ty, registry, structs, visiting)),
                        })
                    }
                    // D-DIST1: distinct types wrap a scalar; they are always cloneable.
                    Some(TypeDef::Distinct { .. }) => true,
                    Some(TypeDef::Alias { target, .. }) => {
                        is_cloneable_rec(target, registry, structs, visiting)
                    }
                    None => false,
                };
            visiting.remove(name);
            result
        }
        Type::Apply { args, .. } => args
            .iter()
            .all(|a| is_cloneable_rec(a, registry, structs, visiting)),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| is_cloneable_rec(t, registry, structs, visiting)),
        Type::TraitObject(_) => false,
        Type::FixedList { elem, .. } => is_cloneable_rec(elem, registry, structs, visiting),
        Type::Tagged { inner, .. } => is_cloneable_rec(inner, registry, structs, visiting),
    }
}

pub(crate) fn expr_is_same_ident(a: &Expr, name: &str) -> bool {
    matches!(a, Expr::Ident(n, _) if n == name)
}

pub(crate) fn pattern_variant_name(pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            // D-PATR: a Variant with Range slots only partially covers the variant
            // (it matches a subset of payloads), so we don't mark it as fully covered.
            let has_range = bindings
                .iter()
                .any(|s| matches!(s, crate::AST::PatSlot::Range { .. }));
            if has_range {
                None
            } else {
                Some(variant.clone())
            }
        }
        Pattern::Present { .. } => Some(Syntax::LIT_VALUE.to_string()),
        Pattern::Absent(_) => Some(Syntax::LIT_NULL.to_string()),
        Pattern::Ok { .. } => Some(Syntax::LIT_OK.to_string()),
        Pattern::Err { .. } => Some(Syntax::LIT_ERR.to_string()),
        // D-PATO: use the first alternative's name as the canonical coverage key.
        // The check_switch loop also inserts the remaining alt names separately.
        Pattern::Or(alts, _) => alts.first().and_then(pattern_variant_name),
        // D-PATR/D-DESTRUCT1/D-PARSESTR1: range, struct, and str-match patterns
        // don't cover a single variant name.
        Pattern::Range { .. } | Pattern::Struct { .. } | Pattern::StrMatch { .. } => None,
    }
}

/// Generate compilable switch arm source text for missing variants.
/// `subj_name` is the variable being switched on (e.g. `"c"` or `"it"` for fallible types).
pub(crate) fn missing_arms_text(
    subj_ty: &Type,
    missing: &[String],
    subj_name: Option<&str>,
) -> String {
    let subj = subj_name.unwrap_or("it");
    let arms: Vec<String> = missing
        .iter()
        .map(|v| match subj_ty {
            // Named enum: `(subject == VariantName) -> {};`
            Type::Named(_) => {
                format!(
                    "    ({} == {}) {} {{}};",
                    subj,
                    v,
                    crate::Syntax::OP_ARM_ARROW
                )
            }
            // Option: `value(inner) -> {};`  or  `null -> {};`
            Type::Option(_) => {
                if v == crate::Syntax::LIT_VALUE {
                    format!(
                        "    ({} is {}(inner)) {} {{}};",
                        subj,
                        crate::Syntax::LIT_VALUE,
                        crate::Syntax::OP_ARM_ARROW
                    )
                } else {
                    format!(
                        "    ({} == {}) {} {{}};",
                        subj,
                        crate::Syntax::LIT_NULL,
                        crate::Syntax::OP_ARM_ARROW
                    )
                }
            }
            // Result: `ok(v) -> {};` or `err(e) -> {};`
            Type::Result { .. } => {
                if v.starts_with(crate::Syntax::LIT_OK) {
                    format!(
                        "    ({} is {}(v)) {} {{}};",
                        subj,
                        crate::Syntax::LIT_OK,
                        crate::Syntax::OP_ARM_ARROW
                    )
                } else {
                    format!(
                        "    ({} is {}(e)) {} {{}};",
                        subj,
                        crate::Syntax::LIT_ERR,
                        crate::Syntax::OP_ARM_ARROW
                    )
                }
            }
            _ => format!(
                "    ({} == {}) {} {{}};",
                subj,
                v,
                crate::Syntax::OP_ARM_ARROW
            ),
        })
        .collect();
    format!("\n{}", arms.join("\n"))
}

pub(crate) fn missing_pattern_coverage(
    subject_ty: &Type,
    covered: &HashSet<String>,
    registry: &TypeRegistry,
) -> Option<Vec<String>> {
    match subject_ty {
        Type::Named(name) => {
            let order = registry.enum_variant_order(name)?;
            let missing: Vec<_> = order
                .iter()
                .filter(|v| !covered.contains(*v))
                .cloned()
                .collect();
            if missing.is_empty() {
                None
            } else {
                Some(missing)
            }
        }
        Type::Option(_) => {
            let mut missing = Vec::new();
            if !covered.contains(Syntax::LIT_VALUE) {
                missing.push(Syntax::LIT_VALUE.to_string());
            }
            if !covered.contains(Syntax::LIT_NULL) {
                missing.push(Syntax::LIT_NULL.to_string());
            }
            if missing.is_empty() {
                None
            } else {
                Some(missing)
            }
        }
        Type::Result { .. } => {
            let mut missing = Vec::new();
            if !covered.contains(Syntax::LIT_OK) {
                missing.push(format!("{}(...)", Syntax::LIT_OK));
            }
            if !covered.contains(Syntax::LIT_ERR) {
                missing.push(format!("{}(...)", Syntax::LIT_ERR));
            }
            if missing.is_empty() {
                None
            } else {
                Some(missing)
            }
        }
        _ => None,
    }
}

/// `T ? E` passed where plain `T` is expected (E0401).
pub(crate) fn result_used_where_plain_expected(want: &Type, got: &Type) -> bool {
    matches!(got, Type::Result { ok, .. } if want.unwrap_result().is_none() && **ok == *want)
}

pub(crate) fn pattern_binding_types(payload: &VariantPayload) -> Vec<Type> {
    match payload {
        VariantPayload::Unit => Vec::new(),
        VariantPayload::Single(t, _) => vec![t.clone()],
        VariantPayload::Named(fs) => fs.iter().map(|f| f.ty.clone()).collect(),
    }
}

pub(crate) fn suggest_field(name: &str, candidates: &[String]) -> Option<String> {
    let mut best: Option<(String, usize)> = None;
    for cand in candidates {
        let d = edit_distance(name, cand);
        if d <= 2 && best.as_ref().map_or(true, |(_, bd)| d < *bd) {
            best = Some((cand.clone(), d));
        }
    }
    best.map(|(s, _)| s)
}

pub(crate) fn is_printable(ty: &Type, registry: &TypeRegistry) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::Option(inner) => is_printable(inner, registry),
        Type::Result { ok, err } => is_printable(ok, registry) && is_printable(err, registry),
        Type::List(inner) => is_printable(inner, registry),
        Type::Map { value, .. } => is_printable(value, registry),
        Type::Named(n) => registry.contains(n) || core_type_known(n),
        Type::Apply { args, .. } => args.iter().all(|a| is_printable(a, registry)),
        Type::Tuple(fields) => fields.iter().all(|(_, t)| is_printable(t, registry)),
        Type::TraitObject(_) | Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => is_printable(elem, registry),
        Type::Tagged { inner, .. } => is_printable(inner, registry),
    }
}

/// D-DISPLAYDBG1: bare `{value}` requires `Display` (explicit impl or builtin scalar).
pub(crate) fn is_displayable(ty: &Type, trait_reg: &crate::Traits::TraitRegistry) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::Option(inner) => is_displayable(inner, trait_reg),
        Type::Result { ok, err } => is_displayable(ok, trait_reg) && is_displayable(err, trait_reg),
        Type::List(inner) => is_displayable(inner, trait_reg),
        Type::Map { value, .. } => is_displayable(value, trait_reg),
        Type::Named(n) => {
            trait_reg.implements_trait(n, Generics::DISPLAY)
                || matches!(
                    n.as_str(),
                    Syntax::TYPE_INT
                        | Syntax::TYPE_FLOAT
                        | Syntax::TYPE_BOOL
                        | Syntax::TYPE_STRING
                        | Syntax::TYPE_CHAR
                )
        }
        Type::Apply { name, args } => {
            trait_reg.implements_trait(name, Generics::DISPLAY)
                || args.iter().all(|a| is_displayable(a, trait_reg))
        }
        Type::Tuple(fields) => fields.iter().all(|(_, t)| is_displayable(t, trait_reg)),
        // D-ANY-JAI1 (c7jaiany): the one blessed trait-object shape that IS
        // displayable — `Renderable` means "has `JetDisplay`" by construction
        // (see `TraitRegistry::implements_trait`), so a `Renderable` trait-object
        // value (the loop element of a trait-bounded variadic) is interpolatable.
        // Every other bare trait-object stays non-displayable.
        Type::TraitObject(t) => t == Generics::RENDERABLE,
        Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => is_displayable(elem, trait_reg),
        Type::Tagged { inner, .. } => is_displayable(inner, trait_reg),
    }
}

/// D-CAPBUNDLE1: an operation used on a nominal `distinct` type whose
/// capability bundles don't grant it. `operation` names the thing that was
/// attempted ("string interpolation", …), `needed_bundle` is the `@Bundle`
/// spelling that would grant it, and `granted` lists the bundles already
/// present on the type (empty when the type is still fully inert).
pub(crate) fn e0138(
    type_name: &str,
    operation: &str,
    needed_bundle: &str,
    granted: Vec<&'static str>,
    span: Span,
) -> Diagnostic {
    let has = if granted.is_empty() {
        "no capability bundles".to_string()
    } else {
        format!("only {}", granted.join(", "))
    };
    Diagnostic::error(
        "E0138",
        format!("`{type_name}` doesn't support {operation}"),
        format!(
            "a nominal type only gets the operations its capability bundles grant; `{type_name}` has {has}"
        ),
        format!(
            "add `{needed_bundle}` before the declaration, or convert to the base type first"
        ),
        Some(span),
    )
}

/// D-PREPOST1: a `@Pre`/`@Post` contract condition used an effect — contract
/// clauses are checked at every call, so they must stay pure (same checker
/// as `#Pure fn`, E3401). `clause_kw` is `"Pre"`/`"Post"`; `span` is the
/// impure call site inside the condition (from `Purity::check_pure_expr`).
pub(crate) fn e0139(clause_kw: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E0139",
        format!("a `@{clause_kw}` condition can't do I/O"),
        "a contract is checked at every call and must be a pure claim about values".to_string(),
        "move the effect out; keep only a pure test".to_string(),
        span,
    )
}

/// D-DISPLAYDBG1: `{value@Debug}` uses auto-derived or explicit `Debug`.
pub(crate) fn is_debuggable(
    ty: &Type,
    type_reg: &TypeRegistry,
    trait_reg: &crate::Traits::TraitRegistry,
) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::Option(inner) => is_debuggable(inner, type_reg, trait_reg),
        Type::Result { ok, err } => {
            is_debuggable(ok, type_reg, trait_reg) && is_debuggable(err, type_reg, trait_reg)
        }
        Type::List(inner) => is_debuggable(inner, type_reg, trait_reg),
        Type::Map { value, .. } => is_debuggable(value, type_reg, trait_reg),
        Type::Named(n) => {
            type_reg.contains(n)
                || core_type_known(n)
                || trait_reg.implements_trait(n, Generics::DEBUG)
        }
        Type::Apply { args, .. } => args.iter().all(|a| is_debuggable(a, type_reg, trait_reg)),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| is_debuggable(t, type_reg, trait_reg)),
        Type::TraitObject(_) | Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => is_debuggable(elem, type_reg, trait_reg),
        Type::Tagged { inner, .. } => is_debuggable(inner, type_reg, trait_reg),
    }
}

pub(crate) fn types_comparable(ty: &Type, registry: &TypeRegistry) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::Option(inner) => types_comparable(inner, registry),
        Type::Result { ok, err } => {
            types_comparable(ok, registry) && types_comparable(err, registry)
        }
        Type::List(inner) => types_comparable(inner, registry),
        Type::Named(name) if name == "U8" => true,
        Type::Named(name) => registry.contains(name) && incomparable_field(ty, registry).is_none(),
        Type::Apply { args, .. } => args.iter().all(|a| types_comparable(a, registry)),
        Type::Tuple(fields) => fields.iter().all(|(_, t)| types_comparable(t, registry)),
        Type::TraitObject(_) | Type::Map { .. } | Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => types_comparable(elem, registry),
        Type::Tagged { inner, .. } => types_comparable(inner, registry),
    }
}

pub(crate) fn incomparable_field(ty: &Type, registry: &TypeRegistry) -> Option<String> {
    match ty {
        Type::Named(name) => match registry.types.get(name) {
            Some(TypeDef::Struct { fields, .. }) => {
                fields.iter().find_map(|(fname, _, fty, is_ref, _)| {
                    if *is_ref || !types_comparable(fty, registry) {
                        Some(fname.clone())
                    } else {
                        None
                    }
                })
            }
            Some(TypeDef::Enum { variants, .. }) => {
                variants.values().find_map(|(_, payload)| match payload {
                    VariantPayload::Unit => None,
                    VariantPayload::Single(t, _) if !types_comparable(t, registry) => {
                        Some("payload".to_string())
                    }
                    VariantPayload::Named(fs) => fs.iter().find_map(|f| {
                        if types_comparable(&f.ty, registry) {
                            None
                        } else {
                            Some(f.name.clone())
                        }
                    }),
                    _ => None,
                })
            }
            // D-DIST1: distinct types wrap a comparable base; they are always comparable.
            Some(TypeDef::Distinct { .. }) => None,
            Some(TypeDef::Alias { .. }) => None,
            None => Some("?".to_string()),
        },
        Type::Option(inner) => incomparable_field(inner, registry),
        Type::Result { ok, err } => {
            incomparable_field(ok, registry).or_else(|| incomparable_field(err, registry))
        }
        _ => Some("?".to_string()),
    }
}

pub(crate) fn collection_changed_in_loop(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0507",
        format!(
            "while the loop is reading `{}`, nothing may change it",
            name
        ),
        "a `loop` borrows the whole collection until the body finishes".to_string(),
        format!(
            "collect changes into a second list, or loop over indices: `loop i in 0..{}.len()-1 {{ }}`",
            name
        ),
        Some(span),
    )
}

pub(crate) fn collection_root_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(n, _) => Some(n.clone()),
        Expr::MethodCall {
            receiver, method, ..
        } if method == "chars" => collection_root_name(receiver),
        _ => None,
    }
}

/// Walk `a.b[i].c` down to the root name (`a`, possibly `self`).
pub(crate) fn expr_root_ident(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(n, _) => Some(n),
        Expr::Field(inner, _, _) => expr_root_ident(inner),
        Expr::Index { base, .. } | Expr::Slice { base, .. } => expr_root_ident(base),
        _ => None,
    }
}

/// Types the generated Rust copies implicitly (no move on read).
pub(crate) fn type_is_copy(ty: &Type) -> bool {
    ty.is_scalar() || matches!(ty, Type::Char) || is_u8_ty(ty)
}

pub(crate) fn is_task_type(ty: &Type) -> bool {
    matches!(ty, Type::Apply { name, .. } if name == "Task")
}

pub(crate) fn prepend_send_path(
    root: &str,
    field: &str,
    mut problem: SendabilityProblem,
) -> SendabilityProblem {
    problem.root = Some(root.to_string());
    problem.path.insert(0, field.to_string());
    problem
}

pub(crate) fn describe_sendability_problem(problem: &SendabilityProblem) -> String {
    match &problem.kind {
        SendProblemKind::RefField => {
            let root = problem.root.as_deref().unwrap_or("this value");
            match problem.path.as_slice() {
                [] => format!("`{}` holds a `ref` field", root),
                [field] => format!("`{}` contains `{}`, which is a `ref` field", root, field),
                [first, ..] => format!(
                    "`{}` contains `{}`, which holds a `ref` field at `{}`",
                    root,
                    first,
                    problem.path.join(".")
                ),
            }
        }
        SendProblemKind::ClosureNeedsTake => {
            if let (Some(root), false) = (problem.root.as_deref(), problem.path.is_empty()) {
                format!(
                    "`{}` contains `{}`, which is a closure that was not handed over with `take`",
                    root,
                    problem.path.join(".")
                )
            } else {
                "a closure may hold outside state, so it must be handed over with `take` before it crosses this boundary".to_string()
            }
        }
        SendProblemKind::ClosureCaptures => {
            "the closure holds captures that are not sendable".to_string()
        }
        SendProblemKind::TraitValue(name) => {
            format!(
                "`{}` is a trait value, so the compiler cannot prove which concrete value crosses this boundary",
                name
            )
        }
        SendProblemKind::ViewBorrow => "`&` results are shared views, not owned values".to_string(),
    }
}

/// True when `e` is a struct-field *value* read (not enum-literal sugar like
/// `Color.Red`, not `.clone`, not an import-alias path).
pub(crate) fn field_read_to_clone(
    e: &Expr,
    registry: &TypeRegistry,
    imports: &HashMap<String, usize>,
) -> bool {
    match e {
        Expr::Field(inner, member, _) => {
            if member == "clone" {
                return false;
            }
            match inner.as_ref() {
                Expr::Ident(n, _) => {
                    registry.enum_variants(n).is_none() && !imports.contains_key(n)
                }
                _ => true,
            }
        }
        _ => false,
    }
}

pub(crate) fn builtin_type_from_ident(name: &str) -> Option<Type> {
    match name {
        Syntax::TYPE_INT => Some(Type::Int),
        Syntax::TYPE_FLOAT => Some(Type::Float),
        Syntax::TYPE_BOOL => Some(Type::Bool),
        Syntax::TYPE_STRING => Some(Type::String),
        Syntax::TYPE_CHAR => Some(Type::Char),
        _ => None,
    }
}

pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}

pub(crate) fn private_item(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0605",
        format!("`{}` exists but is private to its file", name),
        "only names marked `pub` can be used from another file (S18)".to_string(),
        format!(
            "add `pub` before `{}`, or don't reach across files here",
            name
        ),
        Some(span),
    )
}
