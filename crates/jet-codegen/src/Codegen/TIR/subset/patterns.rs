use super::*;

/// c109 Phase 15: an arm head that `emit_switch_arm_cond` would emit as a PLAIN
/// expression (`_ => emit_expr(cond)`) — NOT a variant/Eq-to-variant pattern (which it
/// routes through `emit_pattern_matches`) and NOT an arm-head range (shape B). This is
/// the comparison/Bool arm of the general mixed switch (shape D).
pub(crate) fn arm_is_plain_cond(cx: &Cx, cond: &Expr, subject: &Expr) -> bool {
    // A variant or Eq-to-variant arm → `emit_pattern_matches` (excluded here).
    if arm_variant_pattern(cx, cond, subject).is_some() {
        return false;
    }
    // An arm-head range → shape B / `emit_pattern_matches` Range (excluded here).
    if arm_head_range(cx, cond, subject).is_some() {
        return false;
    }
    // Any other pattern test (`ok`/`err`/`value`/`null`/`present`/wildcard) → not a
    // plain comparison; exclude (those are shape C or unsupported).
    if matches!(cond, Expr::PatternTest { .. }) {
        return false;
    }
    true
}

/// D-PARSESTR1: a str-match arm head over the switch subject — an
/// interpolation literal in pattern position (`subject == "prefix-{id:Int}"`).
pub(crate) fn arm_str_match_pattern(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<Pattern> {
    match cond {
        Expr::PatternTest {
            subject: s,
            pattern: pattern @ Pattern::StrMatch { .. },
            ..
        } if pattern_subjects_match(cx, s, subject) => Some(pattern.clone()),
        _ => None,
    }
}

/// D-DESTRUCT1: a struct-shaped arm head over the switch subject.
pub(crate) fn arm_struct_pattern(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<Pattern> {
    match cond {
        Expr::PatternTest {
            subject: s,
            pattern: Pattern::Struct { .. },
            ..
        } if pattern_subjects_match(cx, s, subject) => {
            if let Expr::PatternTest { pattern, .. } = cond {
                Some(pattern.clone())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Value checks inside a struct arm head are ordinary expressions and must be lowerable.
pub(crate) fn struct_pattern_values_in_subset(
    pattern: &Pattern,
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    match pattern {
        Pattern::Struct { fields, .. } => fields.iter().all(|f| match f {
            StructPatField::Value { value, .. } => expr_in_subset(value, cx, locals),
            StructPatField::Bind { .. } => true,
        }),
        _ => true,
    }
}

/// c109 Phase 8: an arm head that is a fallible/optional pattern test over the
/// subject — `subject == ok(b)` / `err(b)` / `value(b)` / `null`. Returns the
/// `Pattern::{Ok,Err,Present,Absent}`, else `None` (a variant/range/comparison arm).
pub(crate) fn arm_fallible_pattern(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<Pattern> {
    match cond {
        Expr::PatternTest {
            subject: s,
            pattern,
            ..
        } if pattern_subjects_match(cx, s, subject) => match pattern {
            Pattern::Ok { .. }
            | Pattern::Err { .. }
            | Pattern::Present { .. }
            | Pattern::Absent(_) => Some(pattern.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The single name an `ok(b)`/`err(b)`/`value(b)` pattern binds (`null` binds none).
pub(crate) fn fallible_pattern_binding(pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Ok { binding, .. }
        | Pattern::Err { binding, .. }
        | Pattern::Present { binding, .. } => Some(binding.clone()),
        _ => None,
    }
}

/// Mirror codegen's `switch_arm_pattern_owned` (Statement.rs): an arm whose head
/// is a variant pattern over `subject`. Returns the `Pattern` (Variant or Or of
/// variants), or `None` for ranges / comparison / Bool arms. The arm head is a
/// `PatternTest` (`c == Active(id)`) or a bare-value `Binary(Eq, subject, Ident)`
/// that names a known variant. Range patterns at arm head deliberately return
/// `None` (they go through the mixed-switch path, shape B).
pub(crate) fn arm_variant_pattern(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<Pattern> {
    match cond {
        Expr::PatternTest {
            subject: s,
            pattern,
            ..
        } if pattern_subjects_match(cx, s, subject) => {
            if matches!(pattern, Pattern::Range { .. }) {
                return None;
            }
            // The subset covers only variant / or-of-variant patterns (no
            // optional/`ok`/`err` patterns — those are Phase 8).
            if pattern_is_variant_or_orvariant(pattern) {
                Some(pattern.clone())
            } else {
                None
            }
        }
        Expr::Binary(BinOp::Eq, lhs, rhs, _) if pattern_subjects_match(cx, lhs, subject) => {
            if let Expr::Ident(variant, rhs_span) = rhs.as_ref() {
                if cx.variant_owner.contains_key(variant) {
                    return Some(Pattern::Variant {
                        variant: variant.clone(),
                        bindings: Vec::new(),
                        span: *rhs_span,
                    });
                }
            }
            None
        }
        _ => None,
    }
}

/// True for a `Variant` pattern or an `Or` whose every alternative is a `Variant`.
/// Excludes optional/result patterns (Present/Absent/Ok/Err) — out of Phase 4.
pub(crate) fn pattern_is_variant_or_orvariant(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Variant { bindings, .. } => bindings
            .iter()
            // Only plain name-binds, wildcards, and ranges in payload slots are
            // covered (those are the slot kinds the TIR reproduces).
            .all(|s| {
                matches!(
                    s,
                    PatSlot::Bind(_) | PatSlot::Wildcard | PatSlot::Range { .. }
                )
            }),
        Pattern::Or(alts, _) => {
            !alts.is_empty() && alts.iter().all(pattern_is_variant_or_orvariant)
        }
        _ => false,
    }
}

/// The owning enum of a variant (or or-of-variant) pattern, via `cx.variant_owner`.
pub(crate) fn variant_pattern_enum(cx: &Cx, pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Variant { variant, .. } => cx.variant_owner.get(variant).cloned(),
        Pattern::Or(alts, _) => alts.iter().find_map(|a| variant_pattern_enum(cx, a)),
        _ => None,
    }
}

/// An arm-head range pattern (`lo..hi -> …`), as `(lo, hi)`. Mirrors the parser's
/// arm-head range lowering: a `PatternTest` whose pattern is `Pattern::Range`.
pub(crate) fn arm_head_range(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<(i64, i64)> {
    match cond {
        Expr::PatternTest {
            subject: s,
            pattern: Pattern::Range { lo, hi, .. },
            ..
        } if pattern_subjects_match(cx, s, subject) => Some((*lo, *hi)),
        _ => None,
    }
}

/// Mirror codegen's `pattern_subjects_match` (Statement.rs): an arm subject names
/// the same ident as the switch subject, is the implicit `it`, or (B1) reads
/// identically in source — a NON-IDENT subject (`h.val`, `pick()`, `xs[0]`) compared
/// spanlessly via its source slice, matching the AST so a non-ident pattern switch
/// routes through the SAME `lower_enum_match` / `lower_fallible_match` the AST's
/// `emit_pattern_match_switch` uses.
pub(crate) fn pattern_subjects_match(cx: &Cx, a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(na, _), Expr::Ident(nb, _)) => na == nb,
        (Expr::Ident(n, _), _) if n == Syntax::KW_IT => true,
        _ => {
            let sa = cx.src.get(a.span().start..a.span().end);
            let sb = cx.src.get(b.span().start..b.span().end);
            matches!((sa, sb), (Some(x), Some(y)) if x == y)
        }
    }
}

/// Record the names a variant (or or-of-variant) pattern binds, so an arm body's
/// classification sees them as locals. Wildcard/Range slots bind nothing; an Or
/// pattern binds its first alt's names (all alts bind the same names — E0317).
pub(crate) fn add_pattern_binding_names(pattern: &Pattern, locals: &mut HashSet<String>) {
    match pattern {
        Pattern::Variant { bindings, .. } => {
            for slot in bindings {
                if let PatSlot::Bind(name) = slot {
                    locals.insert(name.clone());
                }
            }
        }
        Pattern::Or(alts, _) => {
            if let Some(first) = alts.first() {
                add_pattern_binding_names(first, locals);
            }
        }
        _ => {}
    }
}

/// Record names bound by a struct-pattern arm head.
pub(crate) fn add_struct_pattern_binding_names(pattern: &Pattern, locals: &mut HashSet<String>) {
    if let Pattern::Struct { fields, .. } = pattern {
        for field in fields {
            if let StructPatField::Bind { local, .. } = field {
                locals.insert(local.clone());
            }
        }
    }
}

/// D-PARSESTR1: record names bound by a str-match arm head's holes.
pub(crate) fn add_str_match_pattern_binding_names(pattern: &Pattern, locals: &mut HashSet<String>) {
    if let Pattern::StrMatch { parts, .. } = pattern {
        for part in parts {
            if let crate::AST::StrMatchPart::Hole { name, .. } = part {
                locals.insert(name.clone());
            }
        }
    }
}
