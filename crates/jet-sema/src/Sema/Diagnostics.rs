use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{self, is_type_var_name};
use crate::Syntax;
pub(crate) use crate::Syntax::edit_distance;
use crate::AST::{BinOp, Expr, Pattern, Stmt, Type, VariantPayload};
use std::collections::{HashMap, HashSet};

pub(crate) fn undeclared_value_tag(
    marker: &str,
    suggestion: Option<&str>,
    span: Span,
) -> Diagnostic {
    let fix = suggestion.map_or_else(
        || {
            format!(
                "declare it first with `tag {marker} {{ deny: [Effect] }}`, or check the spelling"
            )
        },
        |candidate| format!("did you mean `{candidate}`?"),
    );
    Diagnostic::error(
        "E0733",
        format!("there's no tag called `{marker}`"),
        "a value tag must name a declared `tag`".to_string(),
        fix,
        Some(span),
    )
}

pub(crate) fn compound_why(op: BinOp) -> String {
    match op {
        // D-FLOORDIV1=A: `/%` rounds down on whole numbers and on floats alike.
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::FloorDiv => {
            "`+ - * / /%` work on Int and Float".to_string()
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
        // D-SHAPE-CONVERT1: the destination type owns explicit conversion.
        (Type::Int, Type::IntN { signed, bits }) => format!(
            "widen it explicitly with `Int.from_{}{}(value)`",
            if *signed { "i" } else { "u" },
            bits
        ),
        (Type::String, _) => "put the value in text with interpolation: \"{x}\"".to_string(),
        (Type::Named(name), _) if name.ends_with(".Rng") => {
            format!("use {} here", Syntax::RNG_TYPE)
        }
        _ => format!("use {} here", want.show()),
    }
}

#[cfg(test)]
mod polish_tests {
    use super::*;

    #[test]
    fn rng_type_fix_uses_the_bare_type_name() {
        assert_eq!(
            type_fix_hint(&Type::Named("random.Rng".to_string()), &Type::Int),
            "use Rng here"
        );
    }
}

/// D-TYPEDTEXT1/D-FFI-SH1: plain `String` reaching checked typed text. `None`
/// when `want`/`got` isn't that shape — caller falls back to the generic
/// mismatch diagnostic.
pub(crate) fn typed_text_mismatch(want: &Type, got: &Type, span: Span) -> Option<Diagnostic> {
    let Type::Named(tn) = want else {
        return None;
    };
    if tn == Syntax::TYPE_REGEX && *got == Type::String {
        return Some(Diagnostic::error(
            "E0152",
            "a `String` is not a `Regex` pattern".to_string(),
            "regex patterns are typed literals, so Jet can validate them before the program runs"
                .to_string(),
            "write `.{\"...\"}` here, or call `re.compile(text)` for a pattern built at run time"
                .to_string(),
            Some(span),
        ));
    }
    let Some(typed_text_name) = Syntax::typed_text_name(tn) else {
        return None;
    };
    if *got != Type::String {
        return None;
    }
    Some(Diagnostic::error(
        "E0149",
        format!("a runtime `String` can't be used as `{}`", tn),
        if typed_text_name == Syntax::TYPE_SH {
            "a runtime string could change the executable or argument boundaries; only a checked literal may build `Sh`, where every `{value}` hole is exactly one argv item".to_string()
        } else {
            "interpolating untrusted text into a query or page is how injection happens; only a checked literal (its `{value}` holes become bound parameters or escaped insertions) may build one".to_string()
        },
        format!(
            "write `{tn}.{{\"...\"}}` with `{{value}}` holes, or use `{tn}.raw(\"…\")` if you have audited the text"
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
            "pass `{}{}` only once, or copy first with `{}{}`",
            Syntax::SIGIL_WRITE,
            name,
            Syntax::SIGIL_COPY,
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
            "drop the extra use of `{}`, or copy first with `{}{}`",
            name,
            Syntax::SIGIL_COPY,
            name
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
            Syntax::KW_NEXT,
            Syntax::KW_LOOP,
        ),
        "move this inside a loop, or remove it".to_string(),
        Some(span),
    )
}

/// D-ARROW-CONTROL1 (E0987): `break(name)` / `next(name)` targets a loop name that
/// is not in scope. The fix lists the names that *are* reachable here.
pub(crate) fn undefined_loop_label(name: &str, in_scope: &[String], span: Span) -> Diagnostic {
    let fix = if in_scope.is_empty() {
        "label an enclosing loop with `name :: loop { … }` first".to_string()
    } else {
        let labels = in_scope
            .iter()
            .map(|l| format!("`break({l})` or `next({l})`"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("use a label in scope: {labels}")
    };
    Diagnostic::error(
        "E0987",
        format!("no loop labeled `{name}` is in scope"),
        "a named `break`/`next` must name an enclosing `name :: loop` (D-LOOPLABEL3)"
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
        Stmt::Switch {
            subject,
            arms,
            else_body,
            span,
        } => {
            arms.iter().all(|a| block_definitely_returns(&a.body))
                && else_body
                    .as_ref()
                    .map(|b| block_definitely_returns(b))
                    .unwrap_or_else(|| !crate::AST::is_subjectless_guard(subject, *span))
        }
        _ => false,
    }
}

pub(crate) fn is_cloneable(ty: &Type, registry: &TypeRegistry) -> bool {
    let mut visiting = HashSet::new();
    is_cloneable_rec(ty, registry, &mut visiting)
}

/// `is_cloneable`'s recursion, with a `visiting` guard against self-referential
/// types (`enum Nat { Succ(n: Nat) }`). Every such type compiles to a `Box`
/// indirection (I3: sema already proves this elsewhere), so a cycle back to a
/// type still being visited is vacuously cloneable — the recursion doesn't need
/// to unwind further to know that. Without this guard the walk never terminates.
fn is_cloneable_rec(
    ty: &Type,
    registry: &TypeRegistry,
    visiting: &mut HashSet<String>,
) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
            is_cloneable_rec(inner, registry, visiting)
        }
        Type::Map { key, value, .. } => {
            is_cloneable_rec(key, registry, visiting)
                && is_cloneable_rec(value, registry, visiting)
        }
        Type::Result { ok, err } => {
            is_cloneable_rec(ok, registry, visiting)
                && is_cloneable_rec(err, registry, visiting)
        }
        Type::Fn { .. } => false,
        Type::Named(name) if builtin_resource_type(name) => false,
        Type::Named(name) if is_type_var_name(name) || core_type_known(name) => true,
        Type::Named(name) => {
            if !visiting.insert(name.clone()) {
                return true;
            }
            let result = registry.contains(name)
                && match registry.types.get(name) {
                    Some(TypeDef::Struct { fields, .. }) => fields
                        .iter()
                        .all(|(_, _, fty, _)| is_cloneable_rec(fty, registry, visiting)),
                    Some(TypeDef::Enum { variants, .. }) => {
                        variants.values().all(|(_, p)| match p {
                            VariantPayload::Unit => true,
                            VariantPayload::Single(t, _) => {
                                is_cloneable_rec(t, registry, visiting)
                            }
                            VariantPayload::Named(fs) => fs
                                .iter()
                                .all(|f| is_cloneable_rec(&f.ty, registry, visiting)),
                        })
                    }
                    // D-DIST1: distinct types wrap a scalar; they are always cloneable.
                    Some(TypeDef::Distinct { .. }) => true,
                    Some(TypeDef::Alias { target, .. }) => {
                        is_cloneable_rec(target, registry, visiting)
                    }
                    None => false,
                };
            visiting.remove(name);
            result
        }
        // A task handle owns one running child and its join slot. Copying the
        // handle would hand two owners the same join, so the runtime `JetTask`
        // implements no `Clone` and neither does the language type.
        Type::Apply { name, .. }
            if matches!(
                name.as_str(),
                "MutationPlan"
                    | "VaultWrite"
                    | "ViewMut"
                    | "CellReadGuard"
                    | "CellEditGuard"
                    | "Task"
                    | Syntax::TYPE_SHARED_GUARD
            ) =>
        {
            false
        }
        Type::Apply { name, .. } if name == "View" => true,
        Type::Apply { name, .. } if matches!(name.as_str(), "KeyRef" | "Rotation") => true,
        Type::Apply { args, .. } => args
            .iter()
            .all(|a| is_cloneable_rec(a, registry, visiting)),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| is_cloneable_rec(t, registry, visiting)),
        Type::TraitObject(_) => false,
        Type::FixedList { elem, .. } => is_cloneable_rec(elem, registry, visiting),
        Type::Tagged { inner, .. } => is_cloneable_rec(inner, registry, visiting),
        Type::Union(members) => members
            .iter()
            .all(|m| is_cloneable_rec(m, registry, visiting)),
        Type::Quantity { base, .. } => is_cloneable_rec(base, registry, visiting),
        Type::ComputeDim(_) => true,
    }
}

fn builtin_resource_type(name: &str) -> bool {
    matches!(
        name,
        "FileReader"
            | "FileWriter"
            | "FileLock"
            | "TcpStream"
            | "UnixStream"
            | "TLSStream"
            | "DBConnection"
            | "Arena"
            | "Bump"
            | "Pool"
            | "Fixed"
    )
}

/// D-MEM1/S7 (D-NOALLOC-SEM1=A): true when `ty` owns heap data — directly
/// (`String`/`[T]`/`[K,V]`/`Shared<T>`/a boxed trait object/a `[T#N]`, which
/// erases to `Vec<T>` at codegen) or transitively (a struct/enum/tuple/distinct/
/// alias with a heap-owning part). Backs `#Policy(no_alloc)` struct/enum-
/// literal and `copy` checks (E0921) — deliberately narrower than
/// `is_cloneable`, which asks a different question ("can Rust `.clone()` this",
/// true for nearly everything including heap types).
pub(crate) fn type_owns_heap(ty: &Type, registry: &TypeRegistry) -> bool {
    let mut visiting = HashSet::new();
    type_owns_heap_rec(ty, registry, &mut visiting)
}

/// `type_owns_heap`'s recursion, with the same self-referential-type cycle
/// guard as `is_cloneable_rec` (a cycle is already behind a `Box` indirection
/// by construction, so it's vacuously non-heap-owning here — the walk doesn't
/// need to unwind further to answer "does this type ITSELF own heap data").
fn type_owns_heap_rec(ty: &Type, registry: &TypeRegistry, visiting: &mut HashSet<String>) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::Char => false,
        Type::IntN { .. } | Type::Float32 => false,
        Type::String | Type::List(_) | Type::Shared(_) => true,
        Type::Map { .. } => true,
        Type::Option(inner) => type_owns_heap_rec(inner, registry, visiting),
        Type::Result { ok, err } => {
            type_owns_heap_rec(ok, registry, visiting)
                || type_owns_heap_rec(err, registry, visiting)
        }
        // A plain function value/pointer carries no heap-owned data at the
        // type level (a closure's captured environment isn't tracked here).
        Type::Fn { .. } => false,
        // A boxed dyn trait object is a heap allocation (Rust `Box<dyn …>`).
        Type::TraitObject(_) => true,
        Type::Named(name) if is_type_var_name(name) => false,
        Type::Named(name) if core_type_known(name) => false,
        Type::Named(name) => {
            if !visiting.insert(name.clone()) {
                return false;
            }
            let result = match registry.types.get(name) {
                Some(TypeDef::Struct { fields, .. }) => fields
                    .iter()
                    .any(|(_, _, fty, _)| type_owns_heap_rec(fty, registry, visiting)),
                Some(TypeDef::Enum { variants, .. }) => variants.values().any(|(_, p)| match p {
                    VariantPayload::Unit => false,
                    VariantPayload::Single(t, _) => type_owns_heap_rec(t, registry, visiting),
                    VariantPayload::Named(fs) => fs
                        .iter()
                        .any(|f| type_owns_heap_rec(&f.ty, registry, visiting)),
                }),
                // D-DIST1: a distinct type wraps a base type — heap-owning iff
                // the base is (e.g. `UserId :: distinct String`).
                Some(TypeDef::Distinct { base, .. }) => {
                    type_owns_heap_rec(base, registry, visiting)
                }
                Some(TypeDef::Alias { target, .. }) => {
                    type_owns_heap_rec(target, registry, visiting)
                }
                None => false,
            };
            visiting.remove(name);
            result
        }
        // D-POOLID-API1=A: `Pool<T>` is a generational arena (heap-owning
        // regardless of `T`); `Id<T>` is plain index+generation data (`Copy`,
        // never heap-owning). Any other generic application: best-effort —
        // recurse into its type args (approximates a generic field storing
        // one), OR'd with a direct field-registry lookup by name (no generic
        // substitution — a known gap for a user generic struct whose fields
        // are typed by a type PARAMETER rather than a concrete arg; see S7
        // report). Errs toward flagging (a false positive under `no_alloc` is
        // cheaper than a silent miss of an actual allocation).
        Type::Apply { name, args } if name == "Pool" => {
            let _ = args;
            true
        }
        Type::Apply { name, .. } if name == "Id" => false,
        Type::Apply { name, .. } if name == "KeyRef" => true,
        Type::Apply { name, args } => {
            args.iter()
                .any(|a| type_owns_heap_rec(a, registry, visiting))
                || matches!(
                    registry.types.get(name),
                    Some(TypeDef::Struct { fields, .. }) if fields.iter().any(|(_, _, fty, _)| type_owns_heap_rec(fty, registry, visiting))
                )
        }
        Type::Tuple(fields) => fields
            .iter()
            .any(|(_, t)| type_owns_heap_rec(t, registry, visiting)),
        // D-SG9/S76: `[T#N]` erases to `Vec<T>` at codegen (I3) — always a
        // heap allocation regardless of the element type.
        Type::FixedList { .. } => true,
        Type::Tagged { inner, .. } => type_owns_heap_rec(inner, registry, visiting),
        Type::Union(members) => members
            .iter()
            .any(|m| type_owns_heap_rec(m, registry, visiting)),
        Type::Quantity { base, .. } => type_owns_heap_rec(base, registry, visiting),
        Type::ComputeDim(_) => false,
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
        Pattern::Range { .. }
        | Pattern::Struct { .. }
        | Pattern::StrMatch { .. }
        | Pattern::BinMatch { .. } => None,
    }
}

/// Generate compilable switch arm source text for missing variants.
/// `subj_name` is the variable being switched on (e.g. `"c"` or `"it"` for fallible types).
pub(crate) fn missing_arms_text(
    subj_ty: &Type,
    missing: &[String],
    _subj_name: Option<&str>,
) -> String {
    let arms: Vec<String> = missing
        .iter()
        .map(|v| match subj_ty {
            // D-ENUMDOT1: leading-dot arm heads in `if subject == { … }`.
            Type::Named(_) | Type::Apply { .. } | Type::Union(_) => {
                format!("    .{} {} {{}}", v, crate::Syntax::OP_ARM_ARROW)
            }
            Type::Option(_) => {
                if v == crate::Syntax::LIT_VALUE {
                    format!(
                        "    .{}(inner) {} {{}}",
                        crate::Syntax::LIT_VALUE,
                        crate::Syntax::OP_ARM_ARROW
                    )
                } else {
                    format!(
                        "    .{} {} {{}}",
                        crate::Syntax::LIT_NULL,
                        crate::Syntax::OP_ARM_ARROW
                    )
                }
            }
            Type::Result { .. } => {
                if v.starts_with(crate::Syntax::LIT_OK) {
                    format!(
                        "    .{}(v) {} {{}}",
                        crate::Syntax::LIT_OK,
                        crate::Syntax::OP_ARM_ARROW
                    )
                } else {
                    format!(
                        "    .{}(e) {} {{}}",
                        crate::Syntax::LIT_ERR,
                        crate::Syntax::OP_ARM_ARROW
                    )
                }
            }
            _ => format!("    .{} {} {{}}", v, crate::Syntax::OP_ARM_ARROW),
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
            // D-TAG1: exhaustiveness is checked at the group level — a leaf is
            // covered when the leaf itself OR any ancestor group is covered.
            let leaf_covered = |leaf: &str| {
                if covered.contains(leaf) {
                    return true;
                }
                let mut prefix = String::new();
                for (i, seg) in leaf.split('.').enumerate() {
                    if i > 0 {
                        if covered.contains(&prefix) {
                            return true;
                        }
                        prefix.push('.');
                    }
                    prefix.push_str(seg);
                }
                false
            };
            let missing: Vec<_> = order.iter().filter(|v| !leaf_covered(v)).cloned().collect();
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
        // D-UNIONTYPE1=A: each member type name is an arm head.
        Type::Union(members) => {
            let missing: Vec<String> = members
                .iter()
                .map(crate::AST::union_member_tag)
                .filter(|tag| !covered.contains(tag))
                .collect();
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

fn secret_bearing_crypto_leaf(name: &str) -> bool {
    matches!(name, "Secret" | "SigningKey" | "X25519SecretKey" | "SharedSecret")
}

pub(crate) fn core_crypto_nominal(ty: Type) -> Type {
    match ty {
        Type::Named(name) if secret_bearing_crypto_leaf(&name) => Type::Tagged {
            marker: crate::AST::TagMarker::Internal(crate::AST::InternalTag::CoreCryptoNominal),
            inner: Box::new(Type::Named(name)),
        },
        Type::List(inner) => Type::List(Box::new(core_crypto_nominal(*inner))),
        Type::Shared(inner) => Type::Shared(Box::new(core_crypto_nominal(*inner))),
        Type::Map { key, key_span, value } => Type::Map {
            key: Box::new(core_crypto_nominal(*key)),
            key_span,
            value: Box::new(core_crypto_nominal(*value)),
        },
        Type::Option(inner) => Type::Option(Box::new(core_crypto_nominal(*inner))),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(core_crypto_nominal(*ok)),
            err: Box::new(core_crypto_nominal(*err)),
        },
        Type::Tuple(fields) => Type::Tuple(
            fields
                .into_iter()
                .map(|(name, ty)| (name, Box::new(core_crypto_nominal(*ty))))
                .collect(),
        ),
        Type::FixedList { elem, len, len_symbol } => Type::FixedList {
            elem: Box::new(core_crypto_nominal(*elem)),
            len,
            len_symbol,
        },
Type::Fn { params, ret, effect_bound, param_contract, return_view_provenance } => Type::Fn {
                    param_contract: param_contract.clone(),
            params: params.into_iter().map(core_crypto_nominal).collect(),
            ret: ret.map(|ty| Box::new(core_crypto_nominal(*ty))),
            effect_bound,
            return_view_provenance,
        },
        Type::Apply { name, args } => Type::Apply {
            name,
            args: args.into_iter().map(core_crypto_nominal).collect(),
        },
        // Already-provenanced leaves are idempotent. User flow tags remain
        // transparent wrappers while provenance is installed below them.
        Type::Tagged { marker, inner }
            if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::CoreCryptoNominal)) =>
        {
            Type::Tagged { marker, inner }
        }
        Type::Tagged { marker, inner } => Type::Tagged {
            marker,
            inner: Box::new(core_crypto_nominal(*inner)),
        },
        Type::Union(members) => crate::AST::canonicalize_union(
            members.into_iter().map(core_crypto_nominal).collect(),
        ),
        Type::Int => Type::Int,
        Type::Float => Type::Float,
        Type::Bool => Type::Bool,
        Type::String => Type::String,
        Type::Char => Type::Char,
        Type::Named(name) => Type::Named(name),
        Type::TraitObject(names) => Type::TraitObject(names),
        Type::IntN { signed, bits } => Type::IntN { signed, bits },
        Type::Float32 => Type::Float32,
        Type::Quantity { base, dimension } => Type::Quantity {
            base: Box::new(core_crypto_nominal(*base)),
            dimension,
        },
        Type::ComputeDim(value) => Type::ComputeDim(value),
    }
}

pub(crate) fn deterministic_clock_type(ty: Type) -> Type {
    Type::Tagged {
        marker: crate::AST::TagMarker::Internal(crate::AST::InternalTag::DeterministicClock),
        inner: Box::new(ty),
    }
}

pub(crate) fn system_clock_type(ty: Type) -> Type {
    Type::Tagged {
        marker: crate::AST::TagMarker::Internal(crate::AST::InternalTag::SystemClock),
        inner: Box::new(ty),
    }
}

pub(crate) fn expiring_secret_loan_type(ty: Type) -> Type {
    Type::Tagged {
        marker: crate::AST::TagMarker::Internal(crate::AST::InternalTag::ExpiringSecretLoan),
        inner: Box::new(ty),
    }
}

pub(crate) fn is_expiring_secret_member_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Tagged { marker, inner }
            if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::CoreCryptoNominal))
                && matches!(
                    inner.as_ref(),
                    Type::Named(name)
                        if matches!(
                            name.as_str(),
                            "Secret" | "SigningKey" | "X25519SecretKey"
                        )
                )
    )
}

pub(crate) fn expiring_secret_loan_matches(want: &Type, got: &Type) -> bool {
    matches!(
        got,
        Type::Tagged { marker, inner }
            if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::ExpiringSecretLoan))
                && inner.as_ref() == want
    )
}

pub(crate) fn contains_expiring_secret_loan(ty: &Type) -> bool {
    match ty {
        Type::Tagged { marker, .. }
            if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::ExpiringSecretLoan)) =>
        {
            true
        }
        Type::Tagged { inner, .. }
        | Type::Option(inner)
        | Type::List(inner)
        | Type::Shared(inner)
        | Type::FixedList { elem: inner, .. } => contains_expiring_secret_loan(inner),
        Type::Result { ok, err } | Type::Map { key: ok, value: err, .. } => {
            contains_expiring_secret_loan(ok) || contains_expiring_secret_loan(err)
        }
        Type::Fn { params, ret, .. } => {
            params.iter().any(contains_expiring_secret_loan)
                || ret.as_deref().is_some_and(contains_expiring_secret_loan)
        }
        Type::Tuple(fields) => fields
            .iter()
            .any(|(_, field)| contains_expiring_secret_loan(field)),
        Type::Apply { args, .. } => args.iter().any(contains_expiring_secret_loan),
        _ => false,
    }
}

pub(crate) fn is_deterministic_clock_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Tagged { marker, inner }
            if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::DeterministicClock))
                && matches!(inner.as_ref(), Type::Named(name) if name == crate::Syntax::CLOCK_TYPE)
    )
}

pub(crate) fn is_system_clock_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Tagged { marker, inner }
            if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::SystemClock))
                && matches!(inner.as_ref(), Type::Named(name) if name == crate::Syntax::CLOCK_TYPE)
    )
}

pub(crate) fn is_clock_type(ty: &Type) -> bool {
    match ty {
        Type::Named(name) => name == crate::Syntax::CLOCK_TYPE,
        Type::Tagged { inner, .. } => is_clock_type(inner),
        _ => false,
    }
}

pub(crate) fn is_secret_bearing_crypto_type(ty: &Type) -> bool {
    match ty {
        Type::Tagged { marker, inner }
            if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::CoreCryptoNominal)) =>
        {
            matches!(inner.as_ref(), Type::Named(name) if secret_bearing_crypto_leaf(name))
        }
        Type::Tagged { inner, .. } => is_secret_bearing_crypto_type(inner),
        _ => false,
    }
}

/// Some Core values are one-pass sources. Reading one consumes it, so it
/// cannot be shown from a read, and codegen has no `jet_show` to call. Sema
/// refuses showing it (I3).
pub(crate) fn is_one_pass_source(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Apply { name, .. }
            if name == Syntax::TYPE_ITER || name == Syntax::TYPE_STREAM
    ) || matches!(
        ty,
        Type::Named(name) if matches!(name.as_str(), "HTTPBody" | "HTTPBodyChunks")
    )
}

/// Name a real materializer when the one-pass type has one. `Stream<T>` and
/// `HTTPBodyChunks` are consumed with a loop instead.
pub(crate) fn one_pass_materializer(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Apply { name, .. } if name == Syntax::TYPE_ITER => Some(".to_list()"),
        Type::Named(n) if n == "HTTPBody" => Some(".text(limit)"),
        _ => None,
    }
}

/// Core types that carry a Prelude display but are not user items, so they
/// never reach the auto-derive sets. Every "can this be shown" predicate must
/// agree on this list. Splitting it is what let interpolation accept a value
/// that print rejected.
pub(crate) fn is_core_shown_type(name: &str) -> bool {
    matches!(name, "ServiceUpgradeReceipt")
}

pub(crate) fn is_printable(
    ty: &Type,
    registry: &TypeRegistry,
    trait_reg: &crate::Traits::TraitRegistry,
) -> bool {
    if is_secret_bearing_crypto_type(ty) || is_one_pass_source(ty) {
        return false;
    }
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::Option(inner) => is_printable(inner, registry, trait_reg),
        Type::Result { ok, err } => {
            is_printable(ok, registry, trait_reg) && is_printable(err, registry, trait_reg)
        }
        Type::List(inner) => is_printable(inner, registry, trait_reg),
        Type::Map { value, .. } => is_printable(value, registry, trait_reg),
        Type::Named(n) => {
            registry.is_unit_type(n)
                || trait_reg.implements_trait(n, Generics::PRINTABLE)
                || is_core_shown_type(n)
        }
        Type::Quantity { .. } => true,
        Type::Apply { name, .. } if name == "KeyRef" => true,
        Type::Apply { name, args } => {
            (name == "View"
                && matches!(args.as_slice(), [Type::Named(inner)] if inner == "str"))
                || (trait_reg.implements_trait(name, Generics::PRINTABLE)
                    && args
                        .iter()
                        .all(|a| is_printable(a, registry, trait_reg)))
        }
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| is_printable(t, registry, trait_reg)),
        Type::TraitObject(_) | Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => is_printable(elem, registry, trait_reg),
        Type::Tagged { inner, .. } => is_printable(inner, registry, trait_reg),
        Type::Union(members) => members
            .iter()
            .all(|m| is_printable(m, registry, trait_reg)),
        // Same as the retired `\0compute.dimension.N` string encoding: it
        // never matched any of the `Type::Named` arms above, so this stayed
        // non-printable on its own (only ever seen as a `Type::Apply` arg).
        Type::ComputeDim(_) => false,
    }
}

/// D-DISPLAYDBG1: bare `{value}` requires `Display` (explicit impl or builtin scalar).
pub(crate) fn is_displayable(
    ty: &Type,
    type_reg: &TypeRegistry,
    trait_reg: &crate::Traits::TraitRegistry,
) -> bool {
    if is_secret_bearing_crypto_type(ty) || is_one_pass_source(ty) {
        return false;
    }
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::Option(inner) => is_displayable(inner, type_reg, trait_reg),
        Type::Result { ok, err } => {
            is_displayable(ok, type_reg, trait_reg)
                && is_displayable(err, type_reg, trait_reg)
        }
        Type::List(inner) => is_displayable(inner, type_reg, trait_reg),
        Type::Map { value, .. } => is_displayable(value, type_reg, trait_reg),
        Type::Named(n) => {
            type_reg.is_unit_type(n)
                || trait_reg.implements_trait(n, Generics::DISPLAY)
                || is_core_shown_type(n)
                || matches!(
                    n.as_str(),
                    Syntax::TYPE_INT
                        | Syntax::TYPE_FLOAT
                        | Syntax::TYPE_BOOL
                        | Syntax::TYPE_STRING
                        | Syntax::TYPE_CHAR
                )
        }
        Type::Quantity { .. } => true,
        Type::Apply { name, .. } if name == "KeyRef" => true,
        Type::Apply { name, args } => {
            (name == "View"
                && matches!(args.as_slice(), [Type::Named(inner)] if inner == "str"))
                || trait_reg.implements_trait(name, Generics::DISPLAY)
                || args.iter().all(|a| is_displayable(a, type_reg, trait_reg))
        }
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| is_displayable(t, type_reg, trait_reg)),
        // D-ANY-JAI1 (c7jaiany): the one blessed trait-object shape that IS
        // displayable — `Renderable` means "has `JetDisplay`" by construction
        // (see `TraitRegistry::implements_trait`), so a `Renderable` trait-object
        // value (the loop element of a trait-bounded variadic) is interpolatable.
        // Every other bare trait-object stays non-displayable.
        // D-ANY-JAI1: displayable if ANY bound trait is `Renderable` — a
        // multi-trait-bounded variadic loop element (`...[Renderable, Debug]`)
        // is still interpolatable via its `Renderable` bound.
        Type::TraitObject(t) => t.iter().any(|n| n == Generics::RENDERABLE),
        Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => is_displayable(elem, type_reg, trait_reg),
        Type::Tagged { inner, .. } => is_displayable(inner, type_reg, trait_reg),
        Type::Union(members) => members
            .iter()
            .all(|m| is_displayable(m, type_reg, trait_reg)),
        // Same as `is_printable`'s note: preserves the retired string
        // encoding's behavior (never matched, so non-displayable on its own).
        Type::ComputeDim(_) => false,
    }
}

/// D-CAPBUNDLE1: an operation used on a nominal `distinct` type whose
/// capability bundles don't grant it. `operation` names the thing that was
/// attempted ("string interpolation", …), `needed_bundle` is the `#Bundle`
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

/// D-PREPOST1: a `#Pre`/`#Post` contract condition used an effect — contract
/// clauses are checked at every call, so they must stay pure (same checker
/// as `#Pure fn`, E3401). `clause_kw` is `"Pre"`/`"Post"`; `span` is the
/// impure call site inside the condition (from `Purity::check_pure_expr`).
pub(crate) fn e0139(clause_kw: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E0139",
        format!("a `#{clause_kw}` condition can't do I/O"),
        "a contract is checked at every call and must be a pure claim about values".to_string(),
        "move the effect out; keep only a pure test".to_string(),
        span,
    )
}

/// D-ATTR4=A: `{value#Debug}` uses auto-derived or explicit `Debug`.
pub(crate) fn is_debuggable(
    ty: &Type,
    type_reg: &TypeRegistry,
    trait_reg: &crate::Traits::TraitRegistry,
) -> bool {
    if is_secret_bearing_crypto_type(ty) {
        return false;
    }
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
            type_reg.is_unit_type(n)
                || trait_reg.implements_trait(n, Generics::DEBUG)
                || is_core_shown_type(n)
        }
        Type::Apply { name, .. } if name == "KeyRef" => true,
        Type::Quantity { .. } => true,
        Type::Apply { name, args } => {
            trait_reg.implements_trait(name, Generics::DEBUG)
                && args
                    .iter()
                    .all(|a| is_debuggable(a, type_reg, trait_reg))
        }
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| is_debuggable(t, type_reg, trait_reg)),
        Type::TraitObject(_) | Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => is_debuggable(elem, type_reg, trait_reg),
        Type::Tagged { inner, .. } => is_debuggable(inner, type_reg, trait_reg),
        Type::Union(members) => members
            .iter()
            .all(|m| is_debuggable(m, type_reg, trait_reg)),
        // Same as `is_printable`'s note: preserves the retired string
        // encoding's behavior (never matched, so non-debuggable on its own).
        Type::ComputeDim(_) => false,
    }
}

pub(crate) fn is_equatable(
    ty: &Type,
    registry: &TypeRegistry,
    trait_reg: &crate::Traits::TraitRegistry,
) -> bool {
    if is_secret_bearing_crypto_type(ty) {
        return false;
    }
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::Option(inner) | Type::List(inner) => is_equatable(inner, registry, trait_reg),
        Type::Result { ok, err } => {
            is_equatable(ok, registry, trait_reg) && is_equatable(err, registry, trait_reg)
        }
        Type::Named(name) if name == "U8" => true,
        Type::Named(name)
            if matches!(
                name.as_str(),
                "EncodingFormat"
                    | "EncodingErrorKind"
                    | "EncodingCause"
                    | "EncodingError"
                    | "EncodingLimits"
                    | "DataError"
                    | "DataErrorKind"
                    | "DataLimits"
            ) =>
        {
            true
        }
        Type::Named(name) => trait_reg.implements_trait(name, Generics::EQUATABLE),
        Type::Apply { name, .. } if name == "KeyRef" => true,
        Type::Apply { name, .. } if name == "Id" => {
            trait_reg.implements_trait(name, Generics::EQUATABLE)
        }
        Type::Apply { name, .. }
            if matches!(name.as_str(), "MutationPlan" | "VaultWrite" | "Rotation") =>
        {
            false
        }
        Type::Apply { name, args } => {
            trait_reg.implements_trait(name, Generics::EQUATABLE)
                && args
                    .iter()
                    .all(|arg| is_equatable(arg, registry, trait_reg))
        }
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, field)| is_equatable(field, registry, trait_reg)),
        Type::TraitObject(_) | Type::Map { .. } | Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => is_equatable(elem, registry, trait_reg),
        Type::Tagged { inner, .. } => is_equatable(inner, registry, trait_reg),
        Type::Union(members) => members
            .iter()
            .all(|member| is_equatable(member, registry, trait_reg)),
        // Same as the prior `\0Quantity` encoding: falls through the generic
        // `Type::Apply` trait-lookup path, which never registered an
        // `Equatable` impl for the marker name, so this stayed non-equatable.
        Type::Quantity { .. } => false,
        // Same note applies to the retired `\0compute.dimension.N` encoding.
        Type::ComputeDim(_) => false,
    }
}

pub(crate) fn types_comparable(ty: &Type, registry: &TypeRegistry) -> bool {
    if is_secret_bearing_crypto_type(ty) {
        return false;
    }
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::Option(inner) => types_comparable(inner, registry),
        Type::Result { ok, err } => {
            types_comparable(ok, registry) && types_comparable(err, registry)
        }
        Type::List(inner) => types_comparable(inner, registry),
        Type::Named(name) if name == "U8" => true,
        // D-ENCSTREAM-SURFACE1=A: shared encoding value types compare by value.
        Type::Named(name)
            if matches!(
                name.as_str(),
                "EncodingFormat"
                    | "EncodingErrorKind"
                    | "EncodingCause"
                    | "EncodingError"
                    | "EncodingLimits"
                    | "DataError"
                    | "DataErrorKind"
                    | "DataLimits"
            ) =>
        {
            true
        }
        Type::Named(name) => registry.contains(name) && incomparable_field(ty, registry).is_none(),
        Type::Apply { name, .. } if name == "KeyRef" => true,
        Type::Apply { name, .. } if matches!(name.as_str(), "MutationPlan" | "VaultWrite" | "Rotation") => false,
        Type::Apply { args, .. } => args.iter().all(|a| types_comparable(a, registry)),
        Type::Tuple(fields) => fields.iter().all(|(_, t)| types_comparable(t, registry)),
        Type::TraitObject(_) | Type::Map { .. } | Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => types_comparable(elem, registry),
        Type::Tagged { inner, .. } => types_comparable(inner, registry),
        Type::Union(members) => members.iter().all(|m| types_comparable(m, registry)),
        // Same as the prior `\0Quantity` encoding: falls through the generic
        // `Type::Apply` trait-lookup path, which never registered an
        // `Equatable` impl for the marker name, so this stayed non-comparable.
        Type::Quantity { .. } => false,
        // Same note applies to the retired `\0compute.dimension.N` encoding.
        Type::ComputeDim(_) => false,
    }
}

pub(crate) fn incomparable_field(ty: &Type, registry: &TypeRegistry) -> Option<String> {
    match ty {
        Type::Named(name) => match registry.types.get(name) {
            Some(TypeDef::Struct { fields, .. }) => fields.iter().find_map(|(fname, _, fty, _)| {
                if !types_comparable(fty, registry) {
                    Some(fname.clone())
                } else {
                    None
                }
            }),
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
            "collect changes into a second list, or loop over indices: `loop i, 0..{}.len()-1 {{ }}`",
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

/// Types every execution tier copies implicitly (no move on read).
pub(crate) fn type_is_copy(ty: &Type) -> bool {
    ty.is_scalar()
        || matches!(ty, Type::Char)
        || is_u8_ty(ty)
        || matches!(ty, Type::Named(name) if name == crate::Syntax::TYPE_RANGE)
}

pub(crate) fn is_task_type(ty: &Type) -> bool {
    matches!(ty, Type::Apply { name, .. } if name == "Task")
}

/// True when `ty` holds a task handle anywhere inside it.
///
/// A `Task<T>` owns one running child and its join slot, so no `Clone` is
/// emitted for it. Any container carrying one is equally uncopyable: a
/// `[[Task<Int>]]` element cannot be `.cloned()` either. Sema and TIR lowering
/// must answer this identically — sema decides whether the loop consumes its
/// collection, lowering decides whether to iterate by value, and a disagreement
/// is either a rustc rejection the user sees as an ICE (I2) or a silent move
/// that was never recorded.
pub fn type_holds_task_handle(ty: &Type) -> bool {
    match ty {
        // A ViewMut element is iterated through the mut-view path, not by
        // consuming the outer list — do not treat it as a task-handle payload.
        Type::Apply { name, .. } if name == "ViewMut" => false,
        Type::Apply { name, args } => {
            name == "Task" || args.iter().any(type_holds_task_handle)
        }
        Type::List(inner) | Type::FixedList { elem: inner, .. } => type_holds_task_handle(inner),
        Type::Tuple(fields) => fields.iter().any(|(_, ty)| type_holds_task_handle(ty)),
        Type::Option(inner) | Type::Shared(inner) => type_holds_task_handle(inner),
        Type::Result { ok, err } => type_holds_task_handle(ok) || type_holds_task_handle(err),
        Type::Map { key, value, .. } => {
            type_holds_task_handle(key) || type_holds_task_handle(value)
        }
        _ => false,
    }
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
        SendProblemKind::ClosureNeedsTake => {
            if let (Some(root), false) = (problem.root.as_deref(), problem.path.is_empty()) {
                format!(
                    "`{}` contains `{}`, whose captures cannot cross this boundary",
                    root,
                    problem.path.join(".")
                )
            } else {
                "the closure holds state that cannot cross this boundary".to_string()
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
        SendProblemKind::ThreadConfined(name) => format!(
            "`{}` owns thread-local state and must stay on the thread that created it",
            name
        ),
        SendProblemKind::ViewBorrow => "a view is a borrow, not an owned value".to_string(),
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
    if let Some(numeric) = crate::AST::numeric_type_from_name(name) {
        return Some(numeric);
    }
    match name {
        Syntax::TYPE_BOOL => Some(Type::Bool),
        Syntax::TYPE_STRING => Some(Type::String),
        Syntax::TYPE_CHAR => Some(Type::Char),
        Syntax::DURATION_TYPE => Some(Type::Named(Syntax::DURATION_TYPE.to_string())),
        Syntax::CLOCK_TYPE => Some(Type::Named(Syntax::CLOCK_TYPE.to_string())),
        Syntax::TYPE_CONDITION => Some(Type::Named(Syntax::TYPE_CONDITION.to_string())),
        "Cell" => Some(Type::Named("Cell".to_string())),
        _ => None,
    }
}

/// D-FIELDPOL1: writing to a computed field (`s.field = v`, `s.field++`) — a
/// computed field is never stored, so there's nothing to write.
pub(crate) fn computed_field_not_settable(field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0339",
        format!("`{}` is a computed field — it can't be assigned", field),
        format!(
            "`{}` is declared `{} => …` — its value always comes from that formula, recomputed on every read",
            field, field
        ),
        format!("change the fields `{}`'s formula reads, not `{}` itself", field, field),
        Some(span),
    )
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

/// D-SHAPE-INTERNAL1=A: crossing a module boundary through a public `_name`
/// is legal but deliberately never a supported-API promise. There is no allow
/// marker for this lint, so every resolved use remains visible.
pub(crate) fn soft_public_use(name: &str, span: Span) -> Diagnostic {
    Diagnostic::lint(
        "L0601",
        format!("`{name}` is a soft-public API"),
        "a leading underscore allows outside use but carries no compatibility promise across minor releases"
            .to_string(),
        "use a public name without a leading underscore when callers need a stable API"
            .to_string(),
        Some(span),
    )
}

#[cfg(test)]
mod tests {
    use super::{core_crypto_nominal, is_secret_bearing_crypto_type};
    use crate::AST::{InternalTag, TagMarker, Type};

    fn count_core_crypto_markers(ty: &Type) -> usize {
        match ty {
            Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
                count_core_crypto_markers(inner)
            }
            Type::Map { key, value, .. } => {
                count_core_crypto_markers(key) + count_core_crypto_markers(value)
            }
            Type::Result { ok, err } => {
                count_core_crypto_markers(ok) + count_core_crypto_markers(err)
            }
            Type::Fn { params, ret, .. } => {
                params.iter().map(count_core_crypto_markers).sum::<usize>()
                    + ret.as_deref().map(count_core_crypto_markers).unwrap_or(0)
            }
            Type::Apply { args, .. } => args.iter().map(count_core_crypto_markers).sum(),
            Type::Tuple(fields) => fields
                .iter()
                .map(|(_, ty)| count_core_crypto_markers(ty))
                .sum(),
            Type::FixedList { elem, .. } => count_core_crypto_markers(elem),
            Type::Quantity { base, .. } => count_core_crypto_markers(base),
            Type::Tagged { marker, inner } => {
                usize::from(matches!(
                    marker,
                    TagMarker::Internal(InternalTag::CoreCryptoNominal)
                )) + count_core_crypto_markers(inner)
            }
            Type::Union(members) => members.iter().map(count_core_crypto_markers).sum(),
            Type::Int
            | Type::Float
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::Named(_)
            | Type::TraitObject(_)
            | Type::IntN { .. }
            | Type::ComputeDim(_)
            | Type::Float32 => 0,
        }
    }

    #[test]
    fn crypto_nominal_provenance_is_recursive_and_never_inferred_from_a_leaf_name() {
        for leaf in ["Secret", "SigningKey", "X25519SecretKey", "SharedSecret"] {
            let local_generic = Type::Named(leaf.to_string());
            assert!(!is_secret_bearing_crypto_type(&local_generic));
            assert!(is_secret_bearing_crypto_type(&core_crypto_nominal(local_generic)));
        }

        let wrapped = Type::Fn {
            params: vec![
                Type::Shared(Box::new(Type::Named("Secret".to_string()))),
                Type::FixedList {
                    elem: Box::new(Type::Named("SigningKey".to_string())),
                    len: 1,
                    len_symbol: None,
                },
                Type::Tagged {
                    marker: TagMarker::User("Audit".to_string()),
                    inner: Box::new(Type::Named("X25519SecretKey".to_string())),
                },
                Type::Apply {
                    name: "Holder".to_string(),
                    args: vec![Type::Named("SharedSecret".to_string())],
                },
            ],
            ret: Some(Box::new(Type::Tuple(vec![
                (
                    "list".to_string(),
                    Box::new(Type::List(Box::new(Type::Named("Secret".to_string())))),
                ),
                (
                    "maybe".to_string(),
                    Box::new(Type::Option(Box::new(Type::Named("SigningKey".to_string())))),
                ),
                (
                    "result".to_string(),
                    Box::new(Type::Result {
                        ok: Box::new(Type::Named("X25519SecretKey".to_string())),
                        err: Box::new(Type::Named("SharedSecret".to_string())),
                    }),
                ),
                (
                    "map".to_string(),
                    Box::new(Type::Map {
                        key: Box::new(Type::String),
                        key_span: None,
                        value: Box::new(Type::Named("Secret".to_string())),
                    }),
                ),
            ]))),
            effect_bound: None,
            param_contract: None,
            return_view_provenance: None,
        };

        let resolved = core_crypto_nominal(wrapped);
        assert_eq!(count_core_crypto_markers(&resolved), 9);
        let (flow_inner_is_secret, flow_tag_is_secret) = match &resolved {
            Type::Fn { params, .. } => match params.get(2) {
                Some(tag @ Type::Tagged { inner, .. }) => (
                    is_secret_bearing_crypto_type(inner),
                    is_secret_bearing_crypto_type(tag),
                ),
                _ => (false, false),
            },
            _ => (false, false),
        };
        assert!(flow_inner_is_secret, "flow tag must preserve inner provenance");
        assert!(flow_tag_is_secret, "flow tag must remain secret-bearing");
    }
}
