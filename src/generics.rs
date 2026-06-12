//! M9 — generic types, traits, and built-in derive policy (S45/S28/S48/S55).

use crate::ast::{Type, TypeParam, TraitMethodSig};
use crate::diag::{Diagnostic, Span};
use crate::syntax;
use std::collections::{HashMap, HashSet};

/// Built-in trait names (prelude).
pub const PRINTABLE: &str = "Printable";
pub const EQUATABLE: &str = "Equatable";
pub const COMPARABLE: &str = "Comparable";
pub const SERIALIZE: &str = "Serialize";

pub const BUILTIN_TRAITS: &[&str] = &[PRINTABLE, EQUATABLE, COMPARABLE, SERIALIZE];

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
        Type::Named(n) => subst.get(n).cloned().unwrap_or_else(|| Type::Named(n.clone())),
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
        Type::Fn { params, ret } => Type::Fn {
            params: params.iter().map(|p| substitute_type(p, subst)).collect(),
            ret: ret.as_ref().map(|r| Box::new(substitute_type(r, subst))),
        },
        Type::TraitObject(t) => Type::TraitObject(t.clone()),
        other => other.clone(),
    }
}

/// Unify two types; extend `subst` with inferred type-parameter bindings.
pub fn unify_types(expected: &Type, found: &Type, subst: &mut HashMap<String, Type>) -> bool {
    let expected = substitute_type(expected, subst);
    let found = substitute_type(found, subst);
    match (&expected, &found) {
        (a, b) if a == b => true,
        (Type::Named(a), Type::Named(b)) if a == b => true,
        (Type::Named(param), concrete) | (concrete, Type::Named(param))
            if is_type_var_name(param) =>
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
                .all(|(x, y)| unify_types(x, y, subst))
        }
        (Type::List(e1), Type::List(e2)) => unify_types(e1, e2, subst),
        (Type::Option(e1), Type::Option(e2)) => unify_types(e1, e2, subst),
        (Type::Result { ok: o1, err: e1 }, Type::Result { ok: o2, err: e2 }) => {
            unify_types(o1, o2, subst) && unify_types(e1, e2, subst)
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
        Type::Fn { params, ret } => {
            params.iter().for_each(|p| collect_free(p, out));
            if let Some(r) = ret {
                collect_free(r, out);
            }
        }
        Type::TraitObject(_) => {}
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => {}
    }
}

/// Compare trait method signatures for impl checking.
pub fn sig_matches_trait(
    params: &[(crate::ast::AccessConvention, Type)],
    ret: &Option<Type>,
    is_view: bool,
    expected: &TraitMethodSig,
) -> bool {
    if params.len() != expected.params.len() {
        return false;
    }
    for ((_, pt), ep) in params.iter().zip(&expected.params) {
        if !types_equal_modulo_self(pt, &ep.ty) {
            return false;
        }
    }
    match (&ret, &expected.return_type) {
        (None, None) => !is_view && !expected.is_view_return,
        (Some(r), Some(er)) => types_equal_modulo_self(r, er),
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
                        _ if is_builtin_trait(b) => {
                            rust_trait_bound(b).unwrap_or("").to_string()
                        }
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
        format!(
            "`{type_name}` would need to implement `{trait_name}` before it can be used here"
        ),
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
            Type::Fn { params, ret } => {
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
        format!("{} uses the trait name directly, not `{keyword}`", syntax::LANG_NAME),
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
