//! M9 — generic types, traits, and built-in derive policy (S45/S28/S48/S55).

use crate::AST::{TraitMethodSig, Type, TypeParam};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use std::collections::{HashMap, HashSet};

/// Built-in trait names (prelude).
pub const PRINTABLE: &str = "Printable";
pub const EQUATABLE: &str = "Equatable";
pub const COMPARABLE: &str = "Comparable";
pub const SERIALIZE: &str = "Serialize";
/// D-SERDE4 (= B, owner-modified): the serde derive traits. `#[Codable]` derives
/// both; `#[Encode]`/`#[Decode]` derive one. They lower to `user_Encode`/`user_Decode`.
pub const ENCODE: &str = "Encode";
pub const DECODE: &str = "Decode";

pub const BUILTIN_TRAITS: &[&str] =
    &[PRINTABLE, EQUATABLE, COMPARABLE, SERIALIZE, ENCODE, DECODE];

pub fn is_builtin_trait(name: &str) -> bool {
    BUILTIN_TRAITS.contains(&name)
}

/// Rust trait bound for codegen.
pub fn rust_trait_bound(trait_name: &str) -> Option<&'static str> {
    match trait_name {
        PRINTABLE => Some("JetShow"),
        EQUATABLE => Some("PartialEq"),
        COMPARABLE => Some("PartialOrd"),
        SERIALIZE => Some("user_Serialize"),
        ENCODE => Some("user_Encode"),
        DECODE => Some("user_Decode"),
        _ => None,
    }
}

/// User trait → Rust trait name.
pub fn user_trait_rust(name: &str) -> String {
    format!("user_{name}")
}

/// Substitute type parameters in `ty` using `subst`.
pub fn substitute_type(ty: &Type, subst: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Named(n) => subst
            .get(n)
            .cloned()
            .unwrap_or_else(|| Type::Named(n.clone())),
        Type::Apply { name, args } => Type::Apply {
            name: name.clone(),
            args: args.iter().map(|a| substitute_type(a, subst)).collect(),
        },
        Type::List(inner) => Type::List(Box::new(substitute_type(inner, subst))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(substitute_type(key, subst)),
            value: Box::new(substitute_type(value, subst)),
        },
        Type::Shared(inner) => Type::Shared(Box::new(substitute_type(inner, subst))),
        Type::Option(inner) => Type::Option(Box::new(substitute_type(inner, subst))),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(substitute_type(ok, subst)),
            err: Box::new(substitute_type(err, subst)),
        },
        Type::Fn { params, ret, effect_bound } => Type::Fn {
            params: params.iter().map(|p| substitute_type(p, subst)).collect(),
            ret: ret.as_ref().map(|r| Box::new(substitute_type(r, subst))),
            // D-EFF2: the callback effect bound is a plain annotation, not a
            // generic-substitutable type — carry it through unchanged.
            effect_bound: effect_bound.clone(),
        },
        Type::Tuple(fields) => Type::Tuple(
            fields
                .iter()
                .map(|(n, t)| (n.clone(), Box::new(substitute_type(t, subst))))
                .collect(),
        ),
        Type::TraitObject(t) => Type::TraitObject(t.clone()),
        other => other.clone(),
    }
}

/// Unify two types; extend `subst` with inferred type-parameter bindings.
///
/// `type_params` is the declared parameter name set for the current generic context
/// (e.g. `{"Kind", "Val"}`). A `Type::Named(n)` where `n` is in `type_params` acts as
/// a unification variable — c148: multi-char params like `Kind` must work like `T`.
pub fn unify_types(
    expected: &Type,
    found: &Type,
    subst: &mut HashMap<String, Type>,
    type_params: &HashSet<String>,
) -> bool {
    let expected = substitute_type(expected, subst);
    let found = substitute_type(found, subst);
    match (&expected, &found) {
        (a, b) if a == b => true,
        (Type::Named(a), Type::Named(b)) if a == b => true,
        (Type::Named(param), concrete) | (concrete, Type::Named(param))
            if is_type_var_name(param) || type_params.contains(param.as_str()) =>
        {
            if let Some(existing) = subst.get(param) {
                existing == concrete
            } else {
                subst.insert(param.clone(), concrete.clone());
                true
            }
        }
        (Type::Apply { name: n1, args: a1 }, Type::Apply { name: n2, args: a2 })
            if n1 == n2 && a1.len() == a2.len() =>
        {
            a1.iter()
                .zip(a2.iter())
                .all(|(x, y)| unify_types(x, y, subst, type_params))
        }
        (Type::List(e1), Type::List(e2)) => unify_types(e1, e2, subst, type_params),
        (Type::Option(e1), Type::Option(e2)) => unify_types(e1, e2, subst, type_params),
        (Type::Result { ok: o1, err: e1 }, Type::Result { ok: o2, err: e2 }) => {
            unify_types(o1, o2, subst, type_params)
                && unify_types(e1, e2, subst, type_params)
        }
        (Type::TraitObject(t1), Type::TraitObject(t2)) if t1 == t2 => true,
        _ => false,
    }
}

pub fn is_type_var_name(name: &str) -> bool {
    name.len() == 1 && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// Collect free type-parameter names referenced in `ty`.
pub fn free_type_params(ty: &Type) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_free(ty, &mut out);
    out
}

fn collect_free(ty: &Type, out: &mut HashSet<String>) {
    match ty {
        Type::IntN { .. } | Type::Float32 => {}
        Type::Named(n) if is_type_var_name(n) => {
            out.insert(n.clone());
        }
        Type::Named(_) => {}
        Type::Apply { args, .. } => args.iter().for_each(|a| collect_free(a, out)),
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => collect_free(inner, out),
        Type::Map { key, value } => {
            collect_free(key, out);
            collect_free(value, out);
        }
        Type::Result { ok, err } => {
            collect_free(ok, out);
            collect_free(err, out);
        }
        Type::Fn { params, ret, .. } => {
            params.iter().for_each(|p| collect_free(p, out));
            if let Some(r) = ret {
                collect_free(r, out);
            }
        }
        Type::TraitObject(_) => {}
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => {}
        Type::Tuple(fields) => fields.iter().for_each(|(_, t)| collect_free(t, out)),
        Type::FixedList { elem, .. } => collect_free(elem, out),
        Type::Tagged { inner, .. } => collect_free(inner, out),
    }
}

/// Compare trait method signatures for impl checking.
pub fn sig_matches_trait(
    params: &[(crate::AST::AccessConvention, Type)],
    ret: &Option<Type>,
    is_view: bool,
    expected: &TraitMethodSig,
    assoc: &HashMap<String, Type>,
) -> bool {
    if params.len() != expected.params.len() {
        return false;
    }
    // D-LIB2: the trait signature may name an associated type (`Item`); resolve it
    // to the impl's `type Item = Concrete` binding before comparing, so a concrete
    // impl method matches the abstract trait method.
    for ((_, pt), ep) in params.iter().zip(&expected.params) {
        let exp_ty = substitute_type(&ep.ty, assoc);
        if !types_equal_modulo_self(pt, &exp_ty) {
            return false;
        }
    }
    match (&ret, &expected.return_type) {
        (None, None) => !is_view && !expected.is_view_return,
        (Some(r), Some(er)) => types_equal_modulo_self(r, &substitute_type(er, assoc)),
        _ => false,
    }
}

fn types_equal_modulo_self(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Named(sa), Type::Named(sb)) if sa.is_empty() || sb.is_empty() => true,
        _ => a == b,
    }
}

/// Format type params for Rust generics: `<T: PartialOrd>`.
pub fn rust_type_param_list(
    params: &[TypeParam],
    extra_bounds: &HashMap<String, Vec<String>>,
) -> String {
    if params.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = params
        .iter()
        .map(|p| {
            let mut bounds: Vec<String> = p
                .bounds
                .iter()
                .filter_map(|b| {
                    if is_builtin_trait(b) {
                        rust_trait_bound(b).map(str::to_string)
                    } else {
                        Some(user_trait_rust(b))
                    }
                })
                .collect();
            if let Some(ex) = extra_bounds.get(&p.name) {
                for b in ex {
                    let rb = match b.as_str() {
                        "Clone" | "JetShow" => b.clone(),
                        _ if is_builtin_trait(b) => rust_trait_bound(b).unwrap_or("").to_string(),
                        _ => user_trait_rust(b),
                    };
                    if !rb.is_empty() && !bounds.contains(&rb) {
                        bounds.push(rb);
                    }
                }
            }
            if bounds.is_empty() {
                p.name.clone()
            } else {
                format!("{}: {}", p.name, bounds.join(" + "))
            }
        })
        .collect();
    format!("<{}>", parts.join(", "))
}

pub fn e0904(span: Span, param: &str) -> Diagnostic {
    Diagnostic::error(
        "E0904",
        format!("can't figure out what `{param}` should be here"),
        "generic calls need enough context to pick a concrete type".to_string(),
        "add a type annotation on a binding, like `val p: Pair<Int> = …`".to_string(),
        Some(span),
    )
}

pub fn e0905(type_name: &str, trait_name: &str, span: Span, needs_derive: bool) -> Diagnostic {
    let fix = if needs_derive && (trait_name == COMPARABLE || trait_name == SERIALIZE) {
        format!(
            "add `derive {trait_name};` inside the `{type_name}` body, or write a different approach"
        )
    } else if trait_name == COMPARABLE {
        format!("add `derive Comparable;` inside `{type_name}`, or use `sort_by` with a key")
    } else {
        format!("write `impl {type_name}: {trait_name} {{ … }}` with every required method")
    };
    Diagnostic::error(
        "E0905",
        format!("`{type_name}` isn't `{trait_name}`"),
        format!("`{type_name}` would need to implement `{trait_name}` before it can be used here"),
        fix,
        Some(span),
    )
}

pub fn e0906(trait_name: &str, missing: &[String], span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0906",
        format!(
            "`impl …: {trait_name}` is missing {}",
            missing
                .iter()
                .map(|m| format!("`{m}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        format!("every `{trait_name}` method must appear in the impl block"),
        "add the missing method signatures and bodies".to_string(),
        Some(span),
    )
}

pub fn e0907(trait_name: &str, method: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0907",
        format!("`{method}` doesn't match `{trait_name}`"),
        "impl methods must match the trait signature exactly".to_string(),
        format!("check the parameter and return types for `{method}` against the trait"),
        Some(span),
    )
}

pub fn e0908(type_name: &str, trait_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0908",
        format!("`{type_name}` already implements `{trait_name}`"),
        "each type may implement a trait only once".to_string(),
        "remove the duplicate `impl` block".to_string(),
        Some(span),
    )
}

/// D-LIB2: an `impl …: Trait` left one of the trait's associated types unbound.
pub fn e0913(trait_name: &str, missing: &[String], span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0913",
        format!(
            "`impl …: {trait_name}` doesn't set {}",
            missing
                .iter()
                .map(|m| format!("`type {m}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        format!("`{trait_name}` declares an associated type each impl must define"),
        format!(
            "add {} inside the impl block",
            missing
                .iter()
                .map(|m| format!("`type {m} = …`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some(span),
    )
}

/// D-QUAL2: a method appears in a `tag` body. A tag is a marker that erases at
/// runtime; only a `trait` carries methods and dispatches.
pub fn e0732(tag_name: &str, method: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0732",
        format!("the tag `{tag_name}` declares a method `{method}`, but tags have no methods"),
        "a `tag` is a marker that erases at runtime; only a `trait` carries methods and dispatches"
            .to_string(),
        format!(
            "make `{tag_name}` a `trait` if `{method}` should dispatch, or remove the method to keep `{tag_name}` a marker tag"
        ),
        Some(span),
    )
}

/// D-QUAL2: a `tag` is used where dispatch/methods are expected (e.g. `derive`d,
/// or named as a trait bound). A tag has no methods to attach or dispatch.
pub fn e0731(tag_name: &str, context: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0731",
        format!("`{tag_name}` is a tag, but {context} needs a trait"),
        "a `tag` is a marker that erases at runtime and carries no methods; dispatch and method attachment need a `trait`"
            .to_string(),
        format!("declare `{tag_name}` as a `trait` with the method(s) it should provide"),
        Some(span),
    )
}

pub fn e0901(method: &str, bound: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0901",
        format!("calling `{method}` needs `{bound}`"),
        "methods on a type parameter require that bound".to_string(),
        format!("add `<T: {bound}>` to the generic parameter list"),
        Some(span),
    )
}

pub fn e0902(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0902",
        "this `impl` can't live here".to_string(),
        "at least one of the trait or the type must be defined in this program (orphan rule)"
            .to_string(),
        "define the trait or type in this file, or move the impl".to_string(),
        Some(span),
    )
}

pub fn e0903(trait_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0903",
        format!("hand-written `{trait_name}` isn't available yet"),
        format!(
            "built-in `{trait_name}` uses `derive {trait_name};` for now — custom impls arrive later"
        ),
        format!("add `derive {trait_name};` inside the type body instead"),
        Some(span),
    )
}

pub const MAX_GENERIC_DEPTH: usize = 64;

/// Returns a human-readable chain when nested `Type<…>` exceeds [`MAX_GENERIC_DEPTH`].
pub fn generic_depth_exceeded(ty: &Type) -> Option<String> {
    let mut stack: Vec<(&Type, Vec<String>)> = vec![(ty, Vec::new())];
    let mut deepest = 0usize;
    let mut deepest_chain = String::new();
    while let Some((ty, chain)) = stack.pop() {
        match ty {
            Type::Apply { name, args } => {
                let mut next = chain.clone();
                next.push(name.clone());
                let depth = next.len();
                if depth > deepest {
                    deepest = depth;
                    deepest_chain = next.join(" → ");
                }
                if depth > MAX_GENERIC_DEPTH {
                    return Some(deepest_chain);
                }
                for arg in args {
                    stack.push((arg, next.clone()));
                }
            }
            Type::List(inner) | Type::Option(inner) | Type::Shared(inner) => {
                let mut next = chain.clone();
                next.push(match ty {
                    Type::List(_) => "List".to_string(),
                    Type::Option(_) => "T?".to_string(),
                    _ => "Shared".to_string(),
                });
                let depth = next.len();
                if depth > deepest {
                    deepest = depth;
                    deepest_chain = next.join(" → ");
                }
                if depth > MAX_GENERIC_DEPTH {
                    return Some(deepest_chain);
                }
                stack.push((inner, next));
            }
            Type::Map { key, value } => {
                stack.push((key, chain.clone()));
                stack.push((value, chain));
            }
            Type::Result { ok, err } => {
                stack.push((ok, chain.clone()));
                stack.push((err, chain));
            }
            Type::Fn { params, ret, .. } => {
                for p in params {
                    stack.push((p, chain.clone()));
                }
                if let Some(r) = ret {
                    stack.push((r, chain));
                }
            }
            _ => {}
        }
    }
    if deepest > MAX_GENERIC_DEPTH {
        Some(deepest_chain)
    } else {
        None
    }
}

pub fn e0036(keyword: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0036",
        format!(
            "{} uses the trait name directly, not `{keyword}`",
            Syntax::LANG_NAME
        ),
        "a trait in type position means dynamic dispatch — Jet handles the details for you"
            .to_string(),
        "write the trait name, like `Shape` or `List<Shape>`".to_string(),
        Some(span),
    )
}

pub fn e0909(chain: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0909",
        "generic instantiation goes too deep".to_string(),
        "nested generic types hit a depth limit to prevent runaway monomorphization".to_string(),
        format!("simplify the types involved: {chain}"),
        Some(span),
    )
}

/// Extra Rust `Clone` bounds for every type parameter (list indexing, derived struct copies).
pub fn rust_extra_clone_bounds(params: &[TypeParam]) -> HashMap<String, Vec<String>> {
    params
        .iter()
        .map(|p| (p.name.clone(), vec!["Clone".to_string()]))
        .collect()
}

/// Extra Rust `JetShow` bounds for generic `JetShow` impls.
pub fn rust_extra_jetshow_bounds(params: &[TypeParam]) -> HashMap<String, Vec<String>> {
    params
        .iter()
        .map(|p| (p.name.clone(), vec!["JetShow".to_string()]))
        .collect()
}

/// Collect every type-parameter name (drawn from `param_names`) that the type
/// `ty` mentions anywhere in its structure. A type parameter `T` appears as
/// `Type::Named("T")`; nested positions (`[T]`, `Map<String, T>`, `Box<T>`, …)
/// count too.
pub fn collect_type_param_mentions(ty: &Type, param_names: &HashSet<&str>, out: &mut HashSet<String>) {
    match ty {
        Type::Named(n) => {
            if param_names.contains(n.as_str()) {
                out.insert(n.clone());
            }
        }
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
            collect_type_param_mentions(inner, param_names, out)
        }
        Type::FixedList { elem, .. } => collect_type_param_mentions(elem, param_names, out),
        Type::Map { key, value } => {
            collect_type_param_mentions(key, param_names, out);
            collect_type_param_mentions(value, param_names, out);
        }
        Type::Result { ok, err } => {
            collect_type_param_mentions(ok, param_names, out);
            collect_type_param_mentions(err, param_names, out);
        }
        Type::Apply { args, .. } => {
            for a in args {
                collect_type_param_mentions(a, param_names, out);
            }
        }
        Type::Tuple(fields) => {
            for (_, t) in fields {
                collect_type_param_mentions(t, param_names, out);
            }
        }
        Type::Fn { params, ret, .. } => {
            for p in params {
                collect_type_param_mentions(p, param_names, out);
            }
            if let Some(r) = ret {
                collect_type_param_mentions(r, param_names, out);
            }
        }
        _ => {}
    }
}

/// D-SERDE9/D-SERDE10: extra Rust serde bounds for a generic `#[Codable]`/
/// `#[Encode]`/`#[Decode]` impl. The compiler injects `T: user_Encode` /
/// `T: user_Decode` — never spelled by the user — for *exactly* the type params
/// that reach the wire (D-SERDE10: those mentioned by some non-skipped field
/// type in `wire_types`). A phantom/skip-only param gets no serde bound, so e.g.
/// `Id<Kind>` serializes regardless of `Kind`.
///
/// `bound` is the serde trait name (`Encode`/`Decode`); it flows through
/// `rust_type_param_list`'s builtin-trait mapping to `user_Encode`/`user_Decode`.
pub fn rust_extra_serde_bounds(
    params: &[TypeParam],
    wire_types: &[&Type],
    bound: &str,
) -> HashMap<String, Vec<String>> {
    let names: HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
    let mut reaching: HashSet<String> = HashSet::new();
    for ty in wire_types {
        collect_type_param_mentions(ty, &names, &mut reaching);
    }
    reaching
        .into_iter()
        .map(|n| (n, vec![bound.to_string()]))
        .collect()
}

pub fn type_param_rust_list(params: &[TypeParam]) -> String {
    if params.is_empty() {
        String::new()
    } else {
        format!(
            "<{}>",
            params
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

pub fn format_type_params(params: &[TypeParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let inner: Vec<String> = params
        .iter()
        .map(|p| {
            if p.bounds.is_empty() {
                p.name.clone()
            } else {
                format!("{}: {}", p.name, p.bounds.join(" + "))
            }
        })
        .collect();
    format!("<{}>", inner.join(", "))
}

pub fn split_qualified(name: &str) -> (Option<&str>, &str) {
    match name.rsplit_once('.') {
        Some((mod_name, ty)) => (Some(mod_name), ty),
        None => (None, name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_depth_limit_detects_long_chains() {
        let mut ty = Type::Int;
        for _ in 0..65 {
            ty = Type::List(Box::new(ty));
        }
        assert!(generic_depth_exceeded(&ty).is_some());
    }
}
