use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Sema::Diagnostics::{
    missing_arms_text, missing_pattern_coverage, pattern_binding_types, pattern_variant_name,
    suggest_field,
};
use crate::Sema::{Checker, LocalInfo};
use crate::Syntax;
use crate::AST::{BinOp, Expr, PatSlot, Pattern, Stmt, Type, VariantPayload};
use std::collections::{HashMap, HashSet};

fn note_pattern_ranges(pattern: &Pattern, ranges: &mut Vec<(i64, i64)>) {
    match pattern {
        Pattern::Range { lo, hi, .. } => ranges.push((*lo, *hi)),
        Pattern::Or(alts, _) => {
            for alt in alts {
                note_pattern_ranges(alt, ranges);
            }
        }
        _ => {}
    }
}

/// D-OPT-WRITE1 (#3974): whether a pattern subject is a `&place` write window.
pub(crate) fn is_write_window_subject(subject: &Expr) -> bool {
    matches!(
        subject.without_parens(),
        Expr::Place(_, crate::AST::PlaceAccess::Write, _)
    )
}

/// Names `pattern` binds as write windows under a `&place` subject. An
/// or-pattern's alternatives bind plain read-only values: one name cannot
/// alias two different payload slots.
pub(crate) fn collect_window_names(pattern: &Pattern, out: &mut HashSet<String>) {
    match pattern {
        Pattern::Ok {
            inner: Some(inner), ..
        }
        | Pattern::Err {
            inner: Some(inner), ..
        }
        | Pattern::Present {
            inner: Some(inner), ..
        } => collect_window_names(inner, out),
        Pattern::Ok { binding, .. }
        | Pattern::Err { binding, .. }
        | Pattern::Present { binding, .. } => {
            out.insert(binding.clone());
        }
        Pattern::Variant { bindings, .. } => {
            for slot in bindings {
                collect_window_slot_names(slot, out);
            }
        }
        Pattern::Struct { fields, .. } => {
            for field in fields {
                if let crate::AST::StructPatField::Bind { local, .. } = field {
                    out.insert(local.clone());
                }
            }
        }
        Pattern::Or(..)
        | Pattern::StrMatch { .. }
        | Pattern::BinMatch { .. }
        | Pattern::Absent(_)
        | Pattern::Range { .. } => {}
    }
}

fn collect_window_slot_names(slot: &PatSlot, out: &mut HashSet<String>) {
    match slot {
        PatSlot::Bind { name, .. } => {
            out.insert(name.clone());
        }
        PatSlot::Nested(inner) => collect_window_names(inner, out),
        PatSlot::Named { slot, .. } => collect_window_slot_names(slot, out),
        PatSlot::Wildcard | PatSlot::Range { .. } | PatSlot::Rest(_) => {}
    }
}

/// Names a condition's pattern tests bind through a `&place` write window.
/// `it_window`: the enclosing switch subject is one, so its `it` stands for it.
pub(crate) fn condition_window_names(cond: &Expr, it_window: bool) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_condition_window_names(cond, it_window, &mut out);
    out
}

fn collect_condition_window_names(cond: &Expr, it_window: bool, out: &mut HashSet<String>) {
    match cond {
        Expr::PatternTest {
            subject, pattern, ..
        } => {
            let window = is_write_window_subject(subject)
                || (it_window
                    && matches!(subject.without_parens(), Expr::Ident(name, _) if name == Syntax::KW_IT));
            if window {
                collect_window_names(pattern, out);
            }
        }
        Expr::Binary(BinOp::And, left, right, _) => {
            collect_condition_window_names(left, it_window, out);
            collect_condition_window_names(right, it_window, out);
        }
        Expr::Paren(inner, _) => collect_condition_window_names(inner, it_window, out),
        _ => {}
    }
}

/// L0303 (#3716): the authored `else` arm that follows a table's last pattern
/// arm, which ends at `after`. Returns the arm (from `else` through its body)
/// and the span deleting it removes, which also takes the line break and
/// indentation before it. `body_end` is the end of the arm's last statement
/// or value; a braced body extends to its matching `}`. Any unexpected source
/// shape answers `None`, so the lint stays quiet rather than guess an edit.
fn else_arm_extent(source: &str, after: usize, body_end: Option<usize>) -> Option<(Span, Span)> {
    let bytes = source.as_bytes();
    let skip_trivia = |mut at: usize| -> usize {
        loop {
            while bytes.get(at).is_some_and(|byte| byte.is_ascii_whitespace()) {
                at += 1;
            }
            if bytes.get(at) == Some(&b'/') && bytes.get(at + 1) == Some(&b'/') {
                while bytes.get(at).is_some_and(|byte| *byte != b'\n') {
                    at += 1;
                }
                continue;
            }
            return at;
        }
    };
    // A braced value arm ends at its value; its closing `}` comes first.
    let mut delete_start = after;
    let mut at = skip_trivia(after);
    if bytes.get(at) == Some(&b'}') {
        delete_start = at + 1;
        at = skip_trivia(at + 1);
    }
    let keyword = Syntax::KW_ELSE.as_bytes();
    if !bytes.get(at..)?.starts_with(keyword)
        || bytes
            .get(at + keyword.len())
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return None;
    }
    let arm_start = at;
    at = skip_trivia(at + keyword.len());
    let arrow = Syntax::OP_UNIFIED_ARROW.as_bytes();
    if !bytes.get(at..)?.starts_with(arrow) {
        return None;
    }
    at = skip_trivia(at + arrow.len());
    let arm_end = if bytes.get(at) == Some(&b'{') {
        let mut depth = 0usize;
        let mut index = at;
        loop {
            match *bytes.get(index)? {
                b'"' => {
                    index += 1;
                    while *bytes.get(index)? != b'"' {
                        index += if bytes[index] == b'\\' { 2 } else { 1 };
                    }
                }
                b'/' if bytes.get(index + 1) == Some(&b'/') => {
                    while bytes.get(index).is_some_and(|byte| *byte != b'\n') {
                        index += 1;
                    }
                    continue;
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break index + 1;
                    }
                }
                _ => {}
            }
            index += 1;
        }
    } else {
        body_end.filter(|end| *end > at)?
    };
    Some((Span::new(arm_start, arm_end), Span::new(delete_start, arm_end)))
}

/// The variant keys one arm covers: every alternative of an or-pattern,
/// recursing through nested or-patterns (mirrors Coverage.jet).
fn collect_covered_variant_names(pattern: &Pattern, names: &mut Vec<String>) {
    match pattern {
        Pattern::Or(alts, _) => {
            for alt in alts {
                collect_covered_variant_names(alt, names);
            }
        }
        other => names.extend(pattern_variant_name(other)),
    }
}

/// S31: head variants of the alternatives that test a payload with a nested
/// pattern. They only partly cover their head, so they never enter the
/// covered set; `nested_pattern_missing` proves their joint coverage.
fn collect_partial_variant_names(pattern: &Pattern, names: &mut Vec<String>) {
    match pattern {
        Pattern::Or(alts, _) => {
            for alt in alts {
                collect_partial_variant_names(alt, names);
            }
        }
        other if other.has_nested_pattern() => names.extend(cover_head(other).map(str::to_string)),
        _ => {}
    }
}

/// One column entry of the S31 nested-coverage matrix.
#[derive(Clone, Copy)]
enum CoverCell<'p> {
    /// A binding or `_`: matches every value.
    Any,
    /// A pattern that tests the value.
    Pat(&'p Pattern),
    /// A range slot or another test that names no constructor.
    Refutable,
}

fn slot_cover_cell(slot: &crate::AST::PatSlot) -> CoverCell<'_> {
    match slot {
        crate::AST::PatSlot::Bind { .. }
        | crate::AST::PatSlot::Wildcard
        | crate::AST::PatSlot::Rest(_) => CoverCell::Any,
        crate::AST::PatSlot::Range { .. } => CoverCell::Refutable,
        crate::AST::PatSlot::Nested(inner) => CoverCell::Pat(inner),
        crate::AST::PatSlot::Named { slot, .. } => slot_cover_cell(slot),
    }
}

/// D-PAT-NAMED-NEST1=A: what a case's payload fields are called.
enum PayloadFieldNames {
    /// Declared `Case(a: A, b: B)`: the field names in slot order.
    Named(Vec<String>),
    /// A positional payload of this many slots (`Case(T)`, `.Val(x)`, a unit case).
    Positional(usize),
    /// The case doesn't resolve on this subject; validation reports it.
    Unknown,
}

/// D-PAT-NAMED-NEST1=A: the written entries of a named payload in source
/// order, without their field names and `..`. Used where the case gives no
/// names to place them by (an error has been or will be reported).
fn strip_named_payload_slots(written: Vec<PatSlot>) -> Vec<PatSlot> {
    written
        .into_iter()
        .filter_map(|slot| match slot {
            PatSlot::Named { slot, .. } => Some(*slot),
            PatSlot::Rest(_) => None,
            slot => Some(slot),
        })
        .map(|mut slot| {
            if let PatSlot::Nested(inner) = &mut slot {
                strip_named_payload_pattern(inner);
            }
            slot
        })
        .collect()
}

/// `strip_named_payload_slots` over a whole nested pattern whose subject
/// type is unknown.
fn strip_named_payload_pattern(pattern: &mut Pattern) {
    match pattern {
        Pattern::Or(alts, _) => alts.iter_mut().for_each(strip_named_payload_pattern),
        Pattern::Variant { bindings, .. } => {
            let written = std::mem::take(bindings);
            *bindings = strip_named_payload_slots(written);
        }
        _ => {}
    }
}

/// The constructor a pattern tests, in the spelling of `pattern_constructors`.
fn cover_head(pattern: &Pattern) -> Option<&str> {
    match pattern {
        Pattern::Variant { variant, .. } => Some(variant),
        Pattern::Present { .. } => Some(Syntax::LIT_VALUE),
        Pattern::Absent(_) => Some(Syntax::LIT_NULL),
        Pattern::Ok { .. } => Some(Syntax::LIT_OK),
        Pattern::Err { .. } => Some(Syntax::LIT_ERR),
        _ => None,
    }
}

/// The payload cells a constructor pattern tests, padded to `arity`.
fn cover_args(pattern: &Pattern, arity: usize) -> Vec<CoverCell<'_>> {
    let cells: Vec<CoverCell<'_>> = match pattern {
        Pattern::Variant { bindings, .. } => bindings.iter().map(slot_cover_cell).collect(),
        Pattern::Present { inner, .. } | Pattern::Ok { inner, .. } | Pattern::Err { inner, .. } => {
            vec![inner.as_deref().map_or(CoverCell::Any, CoverCell::Pat)]
        }
        _ => Vec::new(),
    };
    cells
        .into_iter()
        .chain(std::iter::repeat(CoverCell::Any))
        .take(arity)
        .collect()
}

/// Split or-patterns in the first column into one row per alternative.
fn expand_cover_rows<'p>(rows: &[Vec<CoverCell<'p>>]) -> Vec<Vec<CoverCell<'p>>> {
    let mut out = Vec::with_capacity(rows.len());
    let mut pending: Vec<Vec<CoverCell<'p>>> = rows.iter().rev().cloned().collect();
    while let Some(row) = pending.pop() {
        let alternatives = match row.first() {
            Some(&CoverCell::Pat(Pattern::Or(alts, _))) => Some(alts),
            _ => None,
        };
        match alternatives {
            Some(alts) => {
                for alt in alts.iter().rev() {
                    let mut expanded = row.clone();
                    expanded[0] = CoverCell::Pat(alt);
                    pending.push(expanded);
                }
            }
            None => out.push(row),
        }
    }
    out
}

/// Rows that can match constructor `name`, with its `arity` payload cells in
/// place of the first column. A group head covers each leaf beneath it.
fn specialize_cover_rows<'p>(
    rows: &[Vec<CoverCell<'p>>],
    name: &str,
    arity: usize,
) -> Vec<Vec<CoverCell<'p>>> {
    rows.iter()
        .filter_map(|row| {
            let (first, rest) = row.split_first()?;
            let mut cells = match *first {
                CoverCell::Any => vec![CoverCell::Any; arity],
                CoverCell::Refutable => return None,
                CoverCell::Pat(pattern) => {
                    let head = cover_head(pattern)?;
                    if head == name {
                        cover_args(pattern, arity)
                    } else if jet_foundation::Facts::fact_covers(head, name) {
                        vec![CoverCell::Any; arity]
                    } else {
                        return None;
                    }
                }
            };
            cells.extend_from_slice(rest);
            Some(cells)
        })
        .collect()
}

/// A nested witness for constructor `name` with payload witnesses `args`.
fn cover_witness_text(name: &str, args: &[String]) -> String {
    if args.is_empty() {
        format!(".{name}")
    } else {
        format!(".{name}({})", args.join(", "))
    }
}

fn ranges_cover_interval(ranges: &[(i64, i64)], lo: i128, hi: i128) -> bool {
    if lo > hi {
        return false;
    }
    let mut sorted = ranges
        .iter()
        .map(|(range_lo, range_hi)| (i128::from(*range_lo), i128::from(*range_hi)))
        .collect::<Vec<_>>();
    sorted.sort_unstable_by_key(|(range_lo, _)| *range_lo);

    let mut next = lo;
    for (range_lo, range_hi) in sorted {
        if range_hi < next {
            continue;
        }
        if range_lo > next {
            return false;
        }
        next = next.max(range_hi.saturating_add(1));
        if next > hi {
            return true;
        }
    }
    false
}

/// The subject and pattern of a condition's leading pattern test, looking
/// through `&&` guards (`.Val(u) && u.active`).
fn leading_pattern_test(expr: &Expr) -> Option<(&Expr, &Pattern)> {
    match expr {
        Expr::PatternTest {
            subject, pattern, ..
        } => Some((subject.as_ref(), pattern)),
        Expr::Binary(BinOp::And, left, _, _) => leading_pattern_test(left),
        _ => None,
    }
}

fn leading_guard_pattern_subject(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::PatternTest { subject, .. } => Some(subject),
        Expr::Binary(BinOp::And, left, _, _) => leading_guard_pattern_subject(left),
        _ => None,
    }
}

/// D-FLOWTYPE1=A: `None` as a compared value (`x != None`), not a pattern head.
pub(crate) fn expr_is_absent_none(expr: &Expr) -> bool {
    match expr {
        Expr::Absent(_) => true,
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } if type_name.is_empty()
            && matches!(contextual_literal(variant), Some(ContextualLiteral::Null))
            && args.is_empty() =>
        {
            true
        }
        Expr::Paren(inner, _) | Expr::Copy(inner, _) => expr_is_absent_none(inner),
        _ => false,
    }
}

/// D-FLOWTYPE1=A: atomic `x == None` / `x == .None` pattern test subject.
pub(crate) fn atomic_absent_optional_subject(cond: &Expr) -> Option<(String, Span, Span)> {
    match cond {
        Expr::Binary(BinOp::Eq, left, right, span) if expr_is_absent_none(right) => {
            match left.as_ref() {
                Expr::Ident(name, name_span) => Some((name.clone(), *name_span, *span)),
                _ => None,
            }
        }
        Expr::PatternTest {
            subject,
            pattern,
            span,
        } => {
            let is_absent = match pattern {
                Pattern::Absent(_) => true,
                Pattern::Variant {
                    variant, bindings, ..
                } => {
                    matches!(contextual_literal(variant), Some(ContextualLiteral::Null))
                        && bindings.is_empty()
                }
                _ => false,
            };
            if !is_absent {
                return None;
            }
            match subject.as_ref() {
                Expr::Ident(name, name_span) => Some((name.clone(), *name_span, *span)),
                _ => None,
            }
        }
        Expr::Binary(BinOp::Eq, left, right, span) => {
            let subject = if expr_is_absent_none(right) {
                left.as_ref()
            } else if expr_is_absent_none(left) {
                right.as_ref()
            } else {
                return None;
            };
            match subject {
                Expr::Ident(name, name_span) => Some((name.clone(), *name_span, *span)),
                _ => None,
            }
        }
        Expr::Paren(inner, _) | Expr::Copy(inner, _) => atomic_absent_optional_subject(inner),
        _ => None,
    }
}

fn guard_subject_path(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Field(base, member, _) => Some(format!("{}.{}", guard_subject_path(base)?, member)),
        Expr::Copy(inner, _) | Expr::Paren(inner, _) => guard_subject_path(inner),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextualLiteral {
    Value,
    Null,
    Ok,
    Err,
}

pub(crate) fn contextual_literal(name: &str) -> Option<ContextualLiteral> {
    match name {
        name if name == Syntax::LIT_VALUE => Some(ContextualLiteral::Value),
        name if name == Syntax::LIT_NULL => Some(ContextualLiteral::Null),
        name if name == Syntax::LIT_OK => Some(ContextualLiteral::Ok),
        name if name == Syntax::LIT_ERR => Some(ContextualLiteral::Err),
        _ => None,
    }
}

pub(crate) fn pattern_consumes_result_carrier(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Ok { .. } | Pattern::Err { .. } => true,
        Pattern::Or(alts, _) => alts.iter().any(pattern_consumes_result_carrier),
        Pattern::Variant {
            variant, bindings, ..
        } => {
            bindings.len() == 1
                && matches!(
                    contextual_literal(variant),
                    Some(ContextualLiteral::Ok | ContextualLiteral::Err)
                )
        }
        _ => false,
    }
}
pub(crate) fn normalize_contextual_pattern(pattern: &mut Pattern, subject_ty: &Type) {
    if let Pattern::Or(alts, _) = pattern {
        for alt in alts {
            normalize_contextual_pattern(alt, subject_ty);
        }
        return;
    }
    let Pattern::Variant {
        variant,
        bindings,
        leading_dot,
        span,
    } = pattern
    else {
        return;
    };
    // S31: a nested payload pattern moves into `inner`; the carrier's own
    // binding is then the wildcard. The inner pattern is normalized against
    // the payload type in the same pass.
    let binding = |bindings: &mut Vec<crate::AST::PatSlot>, payload_ty: Option<&Type>| {
        let slot = bindings.pop()?;
        Some(match slot {
            crate::AST::PatSlot::Nested(mut inner) => {
                if let Some(payload_ty) = payload_ty {
                    normalize_contextual_pattern(&mut inner, payload_ty);
                }
                (Syntax::PAT_WILDCARD_SLOT.to_string(), inner.span(), Some(inner))
            }
            slot => (
                slot.as_bind()
                    .unwrap_or(Syntax::PAT_WILDCARD_SLOT)
                    .to_string(),
                slot.binding_span().unwrap_or(*span),
                None,
            ),
        })
    };
    let three_state_payload = match subject_ty {
        Type::Result { ok, .. } => match ok.as_ref() {
            Type::Option(payload) => Some(payload.as_ref()),
            _ => None,
        },
        _ => None,
    };
    let replacement = match (
        subject_ty,
        contextual_literal(variant),
        *leading_dot,
        bindings.len(),
    ) {
        // D-OUTCOME-SHAPE1=A: `T? E!` has three states. `.Val(x)` and
        // `.None` name its success states directly; they reach validation
        // and lowering as the carrier's `Ok` payload test.
        (Type::Result { .. }, Some(ContextualLiteral::Null), _, 0)
            if three_state_payload.is_some() =>
        {
            Some(three_state_success(Pattern::Absent(*span), *span))
        }
        (Type::Result { .. }, Some(ContextualLiteral::Value), _, 1)
            if three_state_payload.is_some() =>
        {
            let (binding, binding_span, inner) = binding(bindings, three_state_payload).unwrap();
            let present = Pattern::Present {
                binding,
                binding_span,
                inner,
                span: *span,
            };
            Some(three_state_success(present, *span))
        }
        (_, Some(ContextualLiteral::Null), false, 0)
        | (Type::Option(_), Some(ContextualLiteral::Null), true, 0) => Some(Pattern::Absent(*span)),
        (_, Some(ContextualLiteral::Value), false, 1)
        | (Type::Option(_), Some(ContextualLiteral::Value), true, 1) => {
            let payload_ty = match subject_ty {
                Type::Option(inner) => Some(inner.as_ref()),
                _ => None,
            };
            let (binding, binding_span, inner) = binding(bindings, payload_ty).unwrap();
            Some(Pattern::Present {
                binding,
                binding_span,
                inner,
                span: *span,
            })
        }
        (Type::Result { ok, .. }, Some(ContextualLiteral::Ok), _, 1) => {
            let (binding, binding_span, inner) = binding(bindings, Some(ok.as_ref())).unwrap();
            Some(Pattern::Ok {
                binding,
                binding_span,
                inner,
                span: *span,
            })
        }
        (Type::Result { err, .. }, Some(ContextualLiteral::Err), _, 1) => {
            let (binding, binding_span, inner) = binding(bindings, Some(err.as_ref())).unwrap();
            Some(Pattern::Err {
                binding,
                binding_span,
                inner,
                span: *span,
            })
        }
        _ => None,
    };
    if let Some(replacement) = replacement {
        *pattern = replacement;
    }
}

/// D-OUTCOME-SHAPE1=A: `T? E!` — a failure carrier around an optional.
pub(crate) fn is_three_state(ty: &Type) -> bool {
    matches!(ty, Type::Result { ok, .. } if matches!(ok.as_ref(), Type::Option(_)))
}

/// D-OUTCOME-SHAPE1=A: a `.Val(x)` / `.None` state of `T? E!` as the
/// carrier's success test with that optional state as its payload pattern.
fn three_state_success(state: Pattern, span: Span) -> Pattern {
    Pattern::Ok {
        binding: Syntax::PAT_WILDCARD_SLOT.to_string(),
        binding_span: span,
        inner: Some(Box::new(state)),
        span,
    }
}

/// D-OUTCOME-SHAPE1=A: true when `pattern` names only optional states
/// (`.Val(…)`, `.None`, or an or-pattern of them) — the payload the retired
/// nested `.Ok(…)` spelling wrapped.
pub(crate) fn names_optional_state(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Present { .. } | Pattern::Absent(_) => true,
        Pattern::Variant { variant, .. } => matches!(
            contextual_literal(variant),
            Some(ContextualLiteral::Value | ContextualLiteral::Null)
        ),
        Pattern::Or(alts, _) => alts.iter().all(names_optional_state),
        _ => false,
    }
}

impl<'a> Checker<'a> {
    fn check_switch_arm_body(
        &mut self,
        body: &mut Vec<Stmt>,
        new_scope: bool,
        span: Span,
        value_expected: Option<&Type>,
    ) {
        if let Some(expected) = value_expected {
            self.check_value_block(body, expected, new_scope, span);
        } else {
            self.check_block(body, new_scope);
        }
    }

    /// D-FLOWTYPE1=A: immutable local/param of type `T?` may refine to `T`.
    pub(crate) fn flow_narrowable_optional_inner(&self, name: &str) -> Option<Type> {
        // Keep asking the stable declaration plane: `lookup` intentionally
        // returns the payload overlay after a proven `.None` guard, but
        // later pattern checks still need to recognize the original Option
        // carrier.
        let info = self.flow.bindings.get(name)?;
        if info.mutable {
            return None;
        }
        match &info.ty {
            Type::Option(inner) => Some((**inner).clone()),
            _ => None,
        }
    }

    /// D-FLOWTYPE1=A: `Present` binding that refines the same stable Optional name.
    pub(crate) fn is_optional_flow_refine(&self, name: &str, binding_ty: &Type) -> bool {
        let Some(info) = self.flow.bindings.get(name) else {
            return false;
        };
        if info.mutable {
            return false;
        }
        matches!(&info.ty, Type::Option(inner) if inner.as_ref() == binding_ty)
    }

    /// D-FLOWTYPE1=A: refine a stable Optional name in the current scope (no E0118).
    pub(crate) fn declare_optional_flow_narrow(
        &mut self,
        name: &str,
        name_span: Span,
        inner: Type,
    ) {
        if name == "_" {
            return;
        }
        let depth = self.scope_depth();
        if self.flow.narrow.get_at(name, depth).is_some() {
            // A prior guard already installed this payload overlay at this
            // scope. Reusing it for an explicit `.Val(name)` pattern is a
            // refinement, not a duplicate declaration.
            return;
        }
        if self.flow.bindings.get_at(name, depth).is_some() {
            self.diags
                .push(crate::Sema::Registration::already_defined(name, name_span));
        }
        self.record_optional_flow_narrow(name, name_span, inner);

        }
    /// Carry a proven Optional complement on the path that continues at
    /// the current scope depth. Unlike a condition binding, this fact is
    /// not a child-scope declaration and therefore survives the guard's
    /// join until a later branch proves that it is no longer common.
    pub(crate) fn record_optional_flow_narrow(&mut self, name: &str, name_span: Span, inner: Type) {
        if name == "_" {
            return;
        }
        let depth = self.scope_depth();
        let sendable = self.sendability_for(name);
        self.flow.narrow.set_at(
            name,
            depth,
            LocalInfo {
                def_span: name_span,
                binding_sigil_span: None,
                ty: inner,
                mutable: false,
                param_conv: None,
                decl_loop_depth: self.loop_depth,
                interrupt_sendable: false,
                reactive_local: false,
                reactive_shared: false,
                single_use_span: None,
                invalid: false,
            },
        );
        self.flow.sendability.set_at(name, depth, sendable);
    }

    /// D-FLOWTYPE1=A: the stable Optional name a statement guard
    /// `if x == None -> …` (one arm, no `else`) tests, when no earlier guard
    /// has already proved its payload in this scope.
    pub(crate) fn optional_exit_guard_subject(&self, stmt: &Stmt) -> Option<String> {
        let Stmt::Switch {
            subject,
            arms,
            else_body: None,
            span,
        } = stmt
        else {
            return None;
        };
        if !crate::AST::is_subjectless_guard(subject, *span) {
            return None;
        }
        let [arm] = arms.as_slice() else {
            return None;
        };
        let (name, _, _) = atomic_absent_optional_subject(&arm.cond)?;
        if self.flow.narrow.get_at(&name, self.scope_depth()).is_some() {
            return None;
        }
        self.flow_narrowable_optional_inner(&name)?;
        Some(name)
    }

    /// D-FLOWTYPE1=A: once a checked `x == None` guard leaves on every path,
    /// the fallthrough holds the payload (`check_switch` records it). Rewrite
    /// the guard into the canonical refutable binding `x == .Val(x) ?? { arm }`
    /// so typed IR binds the proven payload for the statements that follow,
    /// just as the `else` form binds it through its Present test.
    pub(crate) fn rewrite_optional_exit_guard(&self, stmt: &mut Stmt, name: &str) {
        let Stmt::Switch { arms, span, .. } = stmt else {
            return;
        };
        let [arm] = arms.as_mut_slice() else {
            return;
        };
        let Some((subject, name_span, cond_span)) = atomic_absent_optional_subject(&arm.cond)
        else {
            return;
        };
        // The refutable binding requires a direct diverging miss route
        // (E0405); keep that proof intact if the block is checked again.
        if subject != name || !crate::Sema::Diagnostics::block_definitely_exits(&arm.body) {
            return;
        }
        let span = *span;
        let body = std::mem::take(&mut arm.body);
        *stmt = Stmt::Val(crate::AST::Binding {
            mutable: false,
            markers: Vec::new(),
            reactive_upgrade: false,
            meta: None,
            name: String::new(),
            name_span: span,
            sigil_span: None,
            pattern: Some(crate::AST::BindPattern::Refutable {
                pattern: Pattern::Present {
                    binding: subject.clone(),
                    binding_span: name_span,
                    inner: None,
                    span: cond_span,
                },
                fallback: crate::AST::OrFallback::Block {
                    body,
                    value: None,
                    span,
                },
                names: vec![crate::AST::BindName {
                    name: subject.clone(),
                    span: name_span,
                    rename: None,
                }],
                span,
                synthesized: true,
            }),
            ty: None,
            ty_span: None,
            init: Expr::Ident(subject, name_span),
            is_comptime: false,
            ct: None,
            uninit: false,
            arena_view: false,
            string_view: false,
            gc_promotion: None,
            gc_transferred: false,
        });
    }

    pub(crate) fn declare_condition_binding(
        &mut self,
        name: &str,
        span: Span,
        ty: Type,
    ) -> Option<(String, crate::Sema::FlowFacts::MoveOrigin)> {
        let restore = if self.is_optional_flow_refine(name, &ty) {
            let moved_at = self.flow.moved.remove(name);
            self.declare_optional_flow_narrow(name, span, ty);
            moved_at.map(|at| (name.to_string(), at))
        } else {
            self.declare(
                name,
                span,
                LocalInfo {
                    def_span: span,
                    binding_sigil_span: None,
                    ty,
                    mutable: false,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                    interrupt_sendable: false,
                    reactive_local: false,
                    reactive_shared: false,
                    single_use_span: None,
                    invalid: false,
                },
            );
            None
        };
        restore
    }

    /// D-OPT-WRITE1 (#3974): a pattern binding under a `&place` subject is a
    /// write window into the matched payload (Rust `if let Some(s) = &mut x`).
    /// It carries the write capability of a write parameter, so edits through
    /// it, and `&binding.field` passes onward, reach the owner's storage. MIR
    /// binds it as an alias of the subject place, never a copy.
    pub(crate) fn declare_pattern_write_window(&mut self, name: &str, span: Span, ty: Type) {
        self.declare(
            name,
            span,
            LocalInfo {
                def_span: span,
                binding_sigil_span: None,
                ty,
                mutable: true,
                param_conv: Some(crate::AST::AccessConvention::Write),
                decl_loop_depth: self.loop_depth,
                interrupt_sendable: false,
                reactive_local: false,
                reactive_shared: false,
                single_use_span: None,
                invalid: false,
            },
        );
    }

    /// Declare one pattern binding: a write window when `window`, otherwise
    /// an ordinary read-only condition binding.
    pub(crate) fn declare_pattern_binding(
        &mut self,
        name: &str,
        span: Span,
        ty: Type,
        window: bool,
    ) -> Option<(String, crate::Sema::FlowFacts::MoveOrigin)> {
        if window {
            self.declare_pattern_write_window(name, span, ty);
            return None;
        }
        self.declare_condition_binding(name, span, ty)
    }

    /// D-FLOWTYPE1=A: rewrite stable `x != None` into S31 `x == Val(x)` so TIR
    /// records a proven unwrap (`IfLet`) and codegen stays mechanical.
    pub(crate) fn rewrite_optional_flow_ne_none(&self, cond: &mut Expr) {
        match cond {
            Expr::Binary(BinOp::Ne, left, right, span) => {
                let Expr::Ident(name, name_span) = left.as_ref() else {
                    return;
                };
                if !expr_is_absent_none(right) {
                    return;
                }
                if self.flow_narrowable_optional_inner(name).is_none() {
                    return;
                }
                *cond = Expr::PatternTest {
                    subject: Box::new(Expr::Ident(name.clone(), *name_span)),
                    pattern: Pattern::Present {
                        binding: name.clone(),
                        binding_span: *name_span,
                        inner: None,
                        span: *span,
                    },
                    span: *span,
                };
            }
            Expr::Binary(BinOp::And, left, right, _) => {
                self.rewrite_optional_flow_ne_none(left);
                self.rewrite_optional_flow_ne_none(right);
            }
            Expr::Paren(inner, _) => self.rewrite_optional_flow_ne_none(inner),
            _ => {}
        }
    }

    pub(crate) fn check_condition_with_bindings(
        &mut self,
        cond: &mut Expr,
    ) -> HashMap<String, Type> {
        self.rewrite_optional_flow_ne_none(cond);
        match cond {
            Expr::PatternTest {
                subject,
                pattern,
                span,
            } => self.check_pattern_test(subject, pattern, *span),
            Expr::Binary(BinOp::Eq, l, r, span) => {
                let subj_name = match l.as_ref() {
                    Expr::Ident(n, _) => Some(n.clone()),
                    _ => None,
                };
                if let Some(lt) = self.infer(l) {
                    if let Some(pattern) =
                        self.eq_unit_variant_pattern(l, r, subj_name.as_deref(), &lt)
                    {
                        return self.validate_pattern(&lt, &pattern, *span);
                    }
                }
                self.require_bool(cond, "a condition");
                HashMap::new()
            }
            Expr::Binary(BinOp::And, l, r, span) => {
                let left_bindings = self.check_condition_with_bindings(l);
                self.push_scope();
                let mut restore_moved = Vec::new();
                let windows = condition_window_names(l, false);
                for (name, ty) in &left_bindings {
                    if let Some(restored) = self.declare_pattern_binding(
                        name,
                        l.span(),
                        ty.clone(),
                        windows.contains(name),
                    ) {
                        restore_moved.push(restored);
                    }
                }
                self.record_condition_view_bindings(l);
                let mut right_bindings = self.check_condition_with_bindings(r);
                self.pop_scope();
                for (name, at) in restore_moved {
                    self.flow.moved.set(&name, at);
                }
                // Subjectless guards are checked structurally here, so their
                // outer `&&` never reaches `infer_binary`. Keep the typed
                // normalized-Path containment guidance on this path too.
                if let Some(edit) = self.path_containment_string_prefix_edit(l, r, *span) {
                    self.diags
                        .push(Diagnostic::from_row("L0517", &[], Some(*span)).with_edit(edit));
                }
                left_bindings.into_iter().for_each(|(k, v)| {
                    right_bindings.entry(k).or_insert(v);
                });
                right_bindings
            }
            _ => {
                self.require_bool(cond, "a condition");
                HashMap::new()
            }
        }
    }

    /// D-FLOWTYPE1: facts for the path on which a single guard is false.
    /// The caller joins this path with the arm paths through `FlowFacts`, so
    /// the rule is about control flow, not about a particular statement
    /// spelling. Complex boolean complements stay unknown unless their
    /// fact can be proved without inventing an unsafe narrowing.
    pub(crate) fn complement_condition_bindings(
        &self,
        conditions: &[Expr],
    ) -> HashMap<String, Type> {
        let mut bindings = HashMap::new();
        for condition in conditions {
            let Some((name, _, _)) = atomic_absent_optional_subject(condition) else {
                continue;
            };
            let Some(inner) = self.flow_narrowable_optional_inner(&name) else {
                continue;
            };
            bindings.insert(name, inner);
        }
        bindings
    }

    /// S31: contextual normalization of a whole pattern tree. A nested
    /// payload pattern is normalized against its payload type, so
    /// `.Err(.Low)` reaches validation as `Err { inner: Variant }`.
    /// D-OUTCOME-SHAPE1=A: `.Val(x)` / `.None` on `T? E!` reach it as
    /// `Ok { inner: Present | Absent }`; the retired nested `.Ok(.Val(x))`
    /// spelling is refused first (E0392). D-PAT-NAMED-NEST1=A: named payload
    /// entries (`.Case{field: pat, ..}`) are mapped onto positional slots
    /// before either step, at every depth.
    pub(crate) fn normalize_pattern_tree(&mut self, pattern: &mut Pattern, subject_ty: &Type) {
        self.lift_flat_union_cases(pattern, subject_ty);
        self.resolve_named_payload_slots(pattern, subject_ty);
        self.report_retired_nested_outcome(pattern, subject_ty);
        self.normalize_checked_pattern_tree(pattern, subject_ty);
    }

    /// D-ERR-CASES1=A: a case name that exactly one member of a union
    /// declares may be matched flat (`.Err(.NotFound(w))` over
    /// `FindError | ParseError`). The raw tree is rewritten onto the member
    /// arm (`.FindError(.NotFound(w))`), so validation, coverage and every
    /// lowering tier only see the member form.
    fn lift_flat_union_cases(&self, pattern: &mut Pattern, subject_ty: &Type) {
        match pattern {
            Pattern::Or(alts, _) => {
                for alt in alts {
                    self.lift_flat_union_cases(alt, subject_ty);
                }
            }
            Pattern::Variant {
                variant,
                bindings,
                span,
                ..
            } => {
                if let Some(owner) = self.flat_union_case_owner(subject_ty, variant) {
                    let span = *span;
                    let mut flat = std::mem::replace(pattern, Pattern::Absent(span));
                    self.lift_flat_union_cases(&mut flat, &owner);
                    *pattern = Pattern::Variant {
                        variant: crate::AST::union_member_tag(&owner),
                        bindings: vec![PatSlot::Nested(Box::new(flat))],
                        leading_dot: true,
                        span,
                    };
                    return;
                }
                let payload = self.raw_pattern_payload_types(subject_ty, variant);
                for (index, slot) in bindings.iter_mut().enumerate() {
                    if let (PatSlot::Nested(inner), Some(ty)) = (slot, payload.get(index)) {
                        self.lift_flat_union_cases(inner, ty);
                    }
                }
            }
            _ => {}
        }
    }

    /// The one union member whose enum declares `variant`, when `variant` is
    /// not itself a member tag. `None` for an unknown or ambiguous case name,
    /// which then reaches the ordinary E0305 member check.
    pub(crate) fn flat_union_case_owner(&self, subject_ty: &Type, variant: &str) -> Option<Type> {
        let Type::Union(members) = subject_ty else {
            return None;
        };
        if members
            .iter()
            .any(|member| crate::AST::union_member_tag(member) == variant)
        {
            return None;
        }
        let mut owners = members.iter().filter(|member| match member {
            Type::Named(name) | Type::Apply { name, .. } => self
                .resolve_enum_variants_cloned(name)
                .is_some_and(|variants| variants.contains_key(variant)),
            _ => false,
        });
        match (owners.next(), owners.next()) {
            (Some(owner), None) => Some(owner.clone()),
            _ => None,
        }
    }

    /// D-PAT-NAMED-NEST1=A: rewrite every `.Case{field: pat, ..}` in a raw
    /// pattern tree onto the case's positional payload slots, so checking,
    /// exhaustiveness and lowering only ever see the positional form.
    fn resolve_named_payload_slots(&mut self, pattern: &mut Pattern, subject_ty: &Type) {
        match pattern {
            Pattern::Or(alts, _) => {
                for alt in alts {
                    self.resolve_named_payload_slots(alt, subject_ty);
                }
            }
            Pattern::Variant {
                variant,
                bindings,
                span,
                ..
            } => {
                if bindings
                    .iter()
                    .any(|slot| matches!(slot, PatSlot::Named { .. } | PatSlot::Rest(_)))
                {
                    let written = std::mem::take(bindings);
                    *bindings = self.positional_payload_slots(subject_ty, variant, written, *span);
                }
                let payload = self.raw_pattern_payload_types(subject_ty, variant);
                for (index, slot) in bindings.iter_mut().enumerate() {
                    if let PatSlot::Nested(inner) = slot {
                        match payload.get(index) {
                            Some(ty) => self.resolve_named_payload_slots(inner, ty),
                            None => strip_named_payload_pattern(inner),
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// D-PAT-NAMED-NEST1=A: place each `field: slot` entry at its field's
    /// position. A field left out needs the trailing `..` (E0326) and then
    /// matches anything; `..` after every field is redundant (E0327).
    fn positional_payload_slots(
        &mut self,
        subject_ty: &Type,
        variant: &str,
        written: Vec<PatSlot>,
        span: Span,
    ) -> Vec<PatSlot> {
        let fields = match self.payload_field_names(subject_ty, variant) {
            PayloadFieldNames::Named(fields) => fields,
            PayloadFieldNames::Positional(arity) => {
                let positional = strip_named_payload_slots(written);
                let fix = if arity == 0 {
                    format!("write `.{variant}` with no payload")
                } else {
                    let parts: Vec<String> = if positional.is_empty() {
                        vec![Syntax::PAT_WILDCARD_SLOT.to_string(); arity]
                    } else {
                        positional.iter().map(|slot| self.pat_slot_source(slot)).collect()
                    };
                    format!("match it by position: `.{variant}({})`", parts.join(", "))
                };
                self.diags.push(Diagnostic::error(
                    "E0303",
                    format!("case `{variant}` has no field names to match"),
                    "only a case declared with named fields can be matched with `{field: pattern}`; this case's payload is positional"
                        .to_string(),
                    fix,
                    Some(span),
                ));
                return positional;
            }
            // An unknown case is reported by validation; keep the written
            // entries in order so it still sees their bindings.
            PayloadFieldNames::Unknown => return strip_named_payload_slots(written),
        };
        let mut placed: Vec<Option<PatSlot>> = vec![None; fields.len()];
        let mut rest = None;
        for entry in written {
            match entry {
                PatSlot::Named {
                    field,
                    field_span,
                    slot,
                } => match fields.iter().position(|name| *name == field) {
                    Some(index) if placed[index].is_none() => placed[index] = Some(*slot),
                    Some(_) => self.diags.push(Diagnostic::error(
                        "E0303",
                        format!("field `{field}` appears more than once"),
                        "each payload field may be matched only once".to_string(),
                        "remove the duplicate field".to_string(),
                        Some(field_span),
                    )),
                    None => self.diags.push(Diagnostic::error(
                        "E0302",
                        format!("case `{variant}` has no field `{field}`"),
                        format!("`{variant}` declares the fields {}", fields.join(", ")),
                        suggest_field(&field, &fields)
                            .map(|name| format!("did you mean `{name}`?"))
                            .unwrap_or_else(|| "name one of the declared fields".to_string()),
                        Some(field_span),
                    )),
                },
                PatSlot::Rest(rest_span) => rest = Some(rest_span),
                PatSlot::Wildcard
                | PatSlot::Bind { .. }
                | PatSlot::Range { .. }
                | PatSlot::Nested(_) => {
                    unreachable!("the parser admits only `field: pattern` entries and `..` inside `.Case{{…}}`")
                }
            }
        }
        let missing: Vec<&str> = fields
            .iter()
            .zip(&placed)
            .filter(|(_, slot)| slot.is_none())
            .map(|(name, _)| name.as_str())
            .collect();
        match rest {
            None if !missing.is_empty() => self.diags.push(Diagnostic::error(
                "E0326",
                format!(
                    "this pattern leaves out fields of `{variant}`: {}",
                    missing.join(", ")
                ),
                "a pattern that doesn't name every field must end with `..` so the skipped fields are visible at a glance".to_string(),
                format!(
                    "add `, ..` before the closing `}}`, or name {}",
                    missing.join(", ")
                ),
                Some(span),
            )),
            Some(rest_span) if missing.is_empty() => self.diags.push(Diagnostic::error(
                "E0327",
                "this `..` is redundant".to_string(),
                format!("the pattern already names every field of `{variant}`"),
                "remove `..` or leave out at least one field".to_string(),
                Some(rest_span),
            )),
            _ => {}
        }
        placed
            .into_iter()
            .map(|slot| slot.unwrap_or(PatSlot::Wildcard))
            .collect()
    }

    /// Whether a case's payload fields carry names a pattern can use.
    fn payload_field_names(&self, subject_ty: &Type, variant: &str) -> PayloadFieldNames {
        match (subject_ty, contextual_literal(variant)) {
            (Type::Option(_) | Type::Result { .. }, Some(ContextualLiteral::Null)) => {
                PayloadFieldNames::Positional(0)
            }
            (Type::Option(_) | Type::Result { .. }, Some(_)) => PayloadFieldNames::Positional(1),
            (Type::Named(name) | Type::Apply { name, .. }, _) => match self
                .resolve_enum_variants_cloned(name)
                .and_then(|mut variants| variants.remove(variant))
            {
                Some((_, VariantPayload::Named(fields))) => {
                    PayloadFieldNames::Named(fields.into_iter().map(|field| field.name).collect())
                }
                Some((_, VariantPayload::Single(..))) => PayloadFieldNames::Positional(1),
                Some((_, VariantPayload::Unit)) => PayloadFieldNames::Positional(0),
                None => PayloadFieldNames::Unknown,
            },
            (Type::Union(members), _)
                if members
                    .iter()
                    .any(|member| crate::AST::union_member_tag(member) == variant) =>
            {
                PayloadFieldNames::Positional(1)
            }
            _ => PayloadFieldNames::Unknown,
        }
    }

    /// The source spelling of a positional payload slot, for fix text.
    fn pat_slot_source(&self, slot: &PatSlot) -> String {
        match slot {
            PatSlot::Bind { name, .. } => name.clone(),
            PatSlot::Range { lo, hi } => format!("{lo}..{hi}"),
            PatSlot::Nested(inner) => {
                let span = inner.span();
                self.source
                    .get(span.start..span.end)
                    .unwrap_or(Syntax::PAT_WILDCARD_SLOT)
                    .to_string()
            }
            PatSlot::Wildcard | PatSlot::Named { .. } | PatSlot::Rest(_) => {
                Syntax::PAT_WILDCARD_SLOT.to_string()
            }
        }
    }

    /// The payload types a raw (not yet normalized) variant pattern's slots
    /// test: contextual `.Val` / `.Ok` / `.Err` read the carrier's payload.
    fn raw_pattern_payload_types(&self, subject_ty: &Type, variant: &str) -> Vec<Type> {
        match (subject_ty, contextual_literal(variant)) {
            (Type::Result { ok, .. }, Some(ContextualLiteral::Ok)) => vec![(**ok).clone()],
            (Type::Result { ok, .. }, Some(ContextualLiteral::Value)) => match ok.as_ref() {
                Type::Option(payload) => vec![(**payload).clone()],
                _ => Vec::new(),
            },
            (Type::Result { err, .. }, Some(ContextualLiteral::Err)) => vec![(**err).clone()],
            (Type::Option(payload), Some(ContextualLiteral::Value)) => vec![(**payload).clone()],
            _ => self.pattern_payload_types(subject_ty, variant),
        }
    }

    fn normalize_checked_pattern_tree(&self, pattern: &mut Pattern, subject_ty: &Type) {
        normalize_contextual_pattern(pattern, subject_ty);
        match pattern {
            Pattern::Or(alts, _) => {
                for alt in alts {
                    self.normalize_checked_pattern_tree(alt, subject_ty);
                }
            }
            Pattern::Present {
                inner: Some(inner), ..
            } => {
                if let Type::Option(payload) = subject_ty {
                    self.normalize_checked_pattern_tree(inner, payload);
                }
            }
            Pattern::Ok {
                inner: Some(inner), ..
            } => {
                if let Type::Result { ok, .. } = subject_ty {
                    self.normalize_checked_pattern_tree(inner, ok);
                }
            }
            Pattern::Err {
                inner: Some(inner), ..
            } => {
                if let Type::Result { err, .. } = subject_ty {
                    self.normalize_checked_pattern_tree(inner, err);
                }
            }
            Pattern::Variant {
                variant, bindings, ..
            } => {
                if !bindings
                    .iter()
                    .any(|slot| matches!(slot, crate::AST::PatSlot::Nested(_)))
                {
                    return;
                }
                let payload = self.pattern_payload_types(subject_ty, variant);
                for (slot, ty) in bindings.iter_mut().zip(payload.iter()) {
                    if let crate::AST::PatSlot::Nested(inner) = slot {
                        self.normalize_checked_pattern_tree(inner, ty);
                    }
                }
            }
            _ => {}
        }
    }

    /// D-OUTCOME-SHAPE1=A: `T? E!` names its three states directly, so a
    /// source `.Ok(.Val(x))` / `.Ok(.None)` on such a value is the retired
    /// nested spelling. Walks the raw (not yet normalized) tree; a pattern
    /// that was already normalized reports nothing, so a re-check is silent.
    fn report_retired_nested_outcome(&mut self, pattern: &Pattern, subject_ty: &Type) {
        match pattern {
            Pattern::Or(alts, _) => {
                for alt in alts {
                    self.report_retired_nested_outcome(alt, subject_ty);
                }
            }
            Pattern::Variant {
                variant,
                bindings,
                leading_dot,
                span,
            } => {
                if !bindings
                    .iter()
                    .any(|slot| matches!(slot, crate::AST::PatSlot::Nested(_)))
                {
                    return;
                }
                if let (
                    Type::Result { ok, .. },
                    Some(ContextualLiteral::Ok),
                    true,
                    [crate::AST::PatSlot::Nested(state)],
                ) = (
                    subject_ty,
                    contextual_literal(variant),
                    *leading_dot,
                    bindings.as_slice(),
                ) {
                    if matches!(ok.as_ref(), Type::Option(_)) && names_optional_state(state) {
                        let diagnostic =
                            self.retired_nested_outcome_diagnostic(*span, state, subject_ty);
                        self.diags.push(diagnostic);
                        return;
                    }
                }
                let payload = self.raw_pattern_payload_types(subject_ty, variant);
                for (slot, ty) in bindings.iter().zip(payload.iter()) {
                    if let crate::AST::PatSlot::Nested(inner) = slot {
                        self.report_retired_nested_outcome(inner, ty);
                    }
                }
            }
            _ => {}
        }
    }

    fn retired_nested_outcome_diagnostic(
        &self,
        span: Span,
        state: &Pattern,
        subject_ty: &Type,
    ) -> Diagnostic {
        let written = self
            .source
            .get(span.start..span.end)
            .unwrap_or(".Ok(…)")
            .to_string();
        let state_span = state.span();
        let state_source = self
            .source
            .get(state_span.start..state_span.end)
            .map(str::to_string);
        let diagnostic = Diagnostic::error(
            "E0392",
            format!(
                "`{written}` wraps a state that `{}` names directly",
                subject_ty.show()
            ),
            "a `T? E!` value has three states, matched as `.Val(x)`, `.None` and `.Err(e)`; the nested `.Ok(…)` layer is retired (D-OUTCOME-SHAPE1=A)".to_string(),
            format!(
                "write `{}`",
                state_source.as_deref().unwrap_or(".Val(…)` or `.None")
            ),
            Some(span),
        );
        match state_source {
            Some(new_text) => diagnostic.with_edit(TextEdit { span, new_text }),
            None => diagnostic,
        }
    }

    /// The payload types a variant pattern's slots test, in slot order.
    pub(crate) fn pattern_payload_types(&self, subject_ty: &Type, variant: &str) -> Vec<Type> {
        match subject_ty {
            Type::Named(name) | Type::Apply { name, .. } => self
                .resolve_enum_variants_cloned(name)
                .and_then(|variants| {
                    variants
                        .get(variant)
                        .map(|(_, payload)| pattern_binding_types(payload))
                })
                .unwrap_or_default(),
            Type::Union(members) => members
                .iter()
                .find(|member| crate::AST::union_member_tag(member) == variant)
                .cloned()
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }

    /// S31: the closed constructor set a nested-coverage column splits on,
    /// each with its payload types. `None` for open types.
    fn pattern_constructors(&self, ty: &Type) -> Option<Vec<(String, Vec<Type>)>> {
        match ty {
            Type::Option(inner) => Some(vec![
                (Syntax::LIT_VALUE.to_string(), vec![(**inner).clone()]),
                (Syntax::LIT_NULL.to_string(), Vec::new()),
            ]),
            Type::Result { ok, err } => Some(vec![
                (Syntax::LIT_OK.to_string(), vec![(**ok).clone()]),
                (Syntax::LIT_ERR.to_string(), vec![(**err).clone()]),
            ]),
            Type::Named(name) | Type::Apply { name, .. } => {
                let variants = self.resolve_enum_variants_cloned(name)?;
                let order = match self.registry.enum_variant_order(name) {
                    Some(order) => order.to_vec(),
                    None => {
                        let mut names: Vec<String> = variants.keys().cloned().collect();
                        names.sort();
                        names
                    }
                };
                Some(
                    order
                        .into_iter()
                        .map(|variant| {
                            let payload = variants
                                .get(&variant)
                                .map(|(_, payload)| pattern_binding_types(payload))
                                .unwrap_or_default();
                            (variant, payload)
                        })
                        .collect(),
                )
            }
            // A union error domain is closed: each member is one constructor
            // whose single payload is the member value (`.Err(.BadNumStr(_))`).
            Type::Union(members) => Some(
                members
                    .iter()
                    .map(|member| (crate::AST::union_member_tag(member), vec![member.clone()]))
                    .collect(),
            ),
            _ => None,
        }
    }

    /// S31: nested coverage over the whole arm set. `None` when the subject
    /// has no closed constructor set; otherwise the missing arm witnesses in
    /// the flat spelling (`High`, `Ok(...)`) or nested (`Err(.High)`).
    fn nested_pattern_missing(&self, st: &Type, arms: &[Pattern]) -> Option<Option<Vec<String>>> {
        let ctors = self.pattern_constructors(st)?;
        let rows: Vec<Vec<CoverCell<'_>>> = arms.iter().map(|arm| vec![CoverCell::Pat(arm)]).collect();
        let rows = expand_cover_rows(&rows);
        let mut missing = Vec::new();
        for (name, payload) in &ctors {
            let specialized = specialize_cover_rows(&rows, name, payload.len());
            // D-OUTCOME-SHAPE1=A: a `T? E!` success names each optional
            // state directly (`Val(...)`, `None`), never `Ok(.None)`.
            if name == Syntax::LIT_OK && is_three_state(st) {
                let state_rows = expand_cover_rows(&specialized);
                for (state, state_payload) in self.pattern_constructors(&payload[0])? {
                    let state_specialized =
                        specialize_cover_rows(&state_rows, &state, state_payload.len());
                    let Some(witness) =
                        self.uncovered_witness(&state_payload, &state_specialized, 1)
                    else {
                        continue;
                    };
                    missing.push(if witness.is_empty() {
                        state
                    } else if witness.iter().all(|arg| arg == "_") {
                        format!("{state}(...)")
                    } else {
                        format!("{state}({})", witness.join(", "))
                    });
                }
                continue;
            }
            let Some(witness) = self.uncovered_witness(payload, &specialized, 0) else {
                continue;
            };
            missing.push(if witness.iter().all(|arg| arg == "_") {
                match st {
                    Type::Result { .. } => format!("{name}(...)"),
                    _ => name.clone(),
                }
            } else {
                format!("{name}({})", witness.join(", "))
            });
        }
        Some((!missing.is_empty()).then_some(missing))
    }

    /// S31: one value (one witness per column) that no row matches, or
    /// `None` when the rows are exhaustive over `types`.
    fn uncovered_witness<'p>(
        &self,
        types: &[Type],
        rows: &[Vec<CoverCell<'p>>],
        depth: usize,
    ) -> Option<Vec<String>> {
        if rows.is_empty() {
            return Some(vec!["_".to_string(); types.len()]);
        }
        let (first, rest) = types.split_first()?;
        let rows = expand_cover_rows(rows);
        let tests_constructor = rows
            .iter()
            .any(|row| matches!(row.first(), Some(CoverCell::Pat(_))));
        let ctors = if tests_constructor && depth < 64 {
            self.pattern_constructors(first)
        } else {
            None
        };
        if let Some(ctors) = ctors.filter(|ctors| !ctors.is_empty()) {
            for (name, payload) in &ctors {
                let specialized = specialize_cover_rows(&rows, name, payload.len());
                let mut sub_types = payload.clone();
                sub_types.extend_from_slice(rest);
                if let Some(witness) = self.uncovered_witness(&sub_types, &specialized, depth + 1) {
                    let (args, tail) = witness.split_at(payload.len());
                    let mut out = vec![cover_witness_text(name, args)];
                    out.extend_from_slice(tail);
                    return Some(out);
                }
            }
            return None;
        }
        let default: Vec<Vec<CoverCell<'p>>> = rows
            .iter()
            .filter(|row| matches!(row.first(), Some(CoverCell::Any)))
            .map(|row| row[1..].to_vec())
            .collect();
        let witness = self.uncovered_witness(rest, &default, depth + 1)?;
        let mut out = vec!["_".to_string()];
        out.extend(witness);
        Some(out)
    }

    /// One pattern arm's contribution to the covered set — shared by the
    /// statement table and the else-less value-dispatch chain (card #1440),
    /// so unreachable-arm policy (or-patterns, D-TAG1 subtree
    /// ancestors, E0365 vs L0301) lives in exactly one place.
    pub(crate) fn note_pattern_coverage(
        &mut self,
        pattern: &Pattern,
        st: &Type,
        covered: &mut HashSet<String>,
        covered_ranges: &mut Vec<(i64, i64)>,
        multi_head: bool,
    ) {
        note_pattern_ranges(pattern, covered_ranges);
        let pspan = pattern.span();
        // Or-patterns cover every alternative, including a nested or-pattern.
        // S31: a nested payload pattern only partly covers its head variant,
        // so it never enters `covered`, but an earlier full arm still makes
        // it unreachable.
        let mut covered_names = Vec::new();
        collect_covered_variant_names(pattern, &mut covered_names);
        let mut partial_names = Vec::new();
        collect_partial_variant_names(pattern, &mut partial_names);
        let heads = covered_names
            .into_iter()
            .map(|name| (name, true))
            .chain(partial_names.into_iter().map(|name| (name, false)));
        for (variant, full) in heads {
            // D-TAG1: an earlier group arm already covers every leaf in
            // its subtree, so `.Fire ->` makes a later `.Fire.Burn ->`
            // unreachable (ancestor-or-equal test on the dotted path).
            let already = covered
                .iter()
                .any(|c| jet_foundation::Facts::fact_covers(c, &variant));
            if already {
                let what = if multi_head {
                    format!(
                        "head `{}` is unreachable — an earlier head already handles it",
                        variant
                    )
                } else {
                    format!(
                        "arm `{}` is unreachable — that case is already handled",
                        variant
                    )
                };
                let why = if multi_head {
                    "multi-head declaration order selects the first matching head".to_string()
                } else {
                    "every earlier arm already covers this pattern".to_string()
                };
                let fix = if multi_head {
                    "remove this head or merge it with the earlier head".to_string()
                } else {
                    "remove this arm or merge it with the one above".to_string()
                };
                if matches!(st, Type::Union(_)) {
                    self.diags
                        .push(Diagnostic::error("E0365", what, why, fix, Some(pspan)));
                } else {
                    self.diags
                        .push(Diagnostic::lint("L0301", what, why, fix, Some(pspan)));
                }
            } else if full {
                covered.insert(variant);
            }
        }
    }

    /// The completion half of the shared policy: an else-less all-pattern
    /// table must cover the subject's whole type (E0307); open
    /// scalars and inline ranges without interval-aware coverage can
    /// never prove totality.
    pub(crate) fn check_pattern_coverage_complete(
        &mut self,
        st: &Type,
        covered: &HashSet<String>,
        covered_ranges: &[(i64, i64)],
        arms: &[Pattern],
        has_else: bool,
        span: Span,
        insert_at: Option<Span>,
        subj_name: Option<&str>,
    ) {
        if has_else {
            return;
        }
        let multi_head = subj_name == Some(Syntax::INTERNAL_MULTI_HEAD_SUBJECT);
        // Finite integer domains can prove an else-less range table when the
        // union of its arms covers the complete interval. Plain `Int` and
        // `Char` remain open to this checker and still require `else`.
        let finite_integer_interval = st
            .integer_range()
            .or_else(|| self.registry.integer_interval(st));
        if let Some((lo, hi)) = finite_integer_interval {
            if ranges_cover_interval(covered_ranges, lo, hi) {
                return;
            }
        }
        let distinct_integer = match st {
            Type::Named(name) => self
                .registry
                .distinct_base(name)
                .is_some_and(Type::is_integer),
            _ => false,
        };
        if matches!(st, Type::Char)
            || st.is_integer()
            || distinct_integer
            || finite_integer_interval.is_some()
        {
            let domain_note = if finite_integer_interval.is_some() {
                format!(
                    "`{}` has a finite integer interval, but the range arms do not cover every value",
                    st.show()
                )
            } else {
                format!(
                    "`{}` has infinitely many values; range arms only cover a subset",
                    st.show()
                )
            };
            self.diags.push(Diagnostic::error(
                "E0307",
                format!(
                    "this `{}` over `{}` has no `{}` arm — range arms can't cover every value",
                    Syntax::KW_IF,
                    st.show(),
                    Syntax::KW_ELSE,
                ),
                domain_note,
                format!(
                    "add `{} {} {{ … }}` to handle values not matched by any range",
                    Syntax::KW_ELSE,
                    Syntax::OP_UNIFIED_ARROW
                ),
                Some(span),
            ));
            return;
        }
        let missing = if is_three_state(st) || arms.iter().any(Pattern::has_nested_pattern) {
            self.nested_pattern_missing(st, arms)
                .unwrap_or_else(|| missing_pattern_coverage(st, covered, self.registry))
        } else {
            missing_pattern_coverage(st, covered, self.registry)
        };
        if let Some(missing) = missing {
            let mut diag = Diagnostic::error(
                "E0307",
                if multi_head {
                    format!(
                        "this multi-head function doesn't cover every case — missing: {}",
                        missing.join(", ")
                    )
                } else {
                    format!(
                        "this `{}` doesn't cover every case — missing: {}",
                        Syntax::KW_IF,
                        missing.join(", ")
                    )
                },
                if multi_head {
                    "each multi-head head covers one argument shape, so every variant must appear once"
                            .to_string()
                } else {
                    "every arm here is a pattern test, so each variant must appear once".to_string()
                },
                if multi_head {
                    format!("add a head for: {}", missing.join(", "))
                } else {
                    format!("add an arm for: {}", missing.join(", "))
                },
                Some(span),
            );
            // Attach a structured insert so LSP/CLI can add compilable arms.
            if !multi_head {
                if let Some(at) = insert_at {
                    diag.set_structured_edit(TextEdit {
                        span: at,
                        new_text: missing_arms_text(st, &missing, subj_name),
                    });
                }
            }
            self.diags.push(diag);
        }
    }

    /// L0303 (#3716): an `else` arm after pattern arms that already cover a
    /// closed set (enum, variant group, `T?`, `T E!`, union) can never run.
    /// Worse, a case added later would fall into it silently instead of
    /// raising E0307, so the lint offers the Safe edit that deletes the arm.
    /// Open sets (numbers, text, chars) never reach this check's verdict.
    fn lint_unreachable_else(
        &mut self,
        st: &Type,
        covered: &HashSet<String>,
        arms: &[Pattern],
        last_arm_end: usize,
        else_body_end: Option<usize>,
    ) {
        let closed = match st {
            Type::Named(name) => self.registry.enum_variant_order(name).is_some(),
            Type::Option(_) | Type::Result { .. } | Type::Union(_) => true,
            _ => false,
        };
        if !closed {
            return;
        }
        let missing = if is_three_state(st) || arms.iter().any(Pattern::has_nested_pattern) {
            self.nested_pattern_missing(st, arms)
                .unwrap_or_else(|| missing_pattern_coverage(st, covered, self.registry))
        } else {
            missing_pattern_coverage(st, covered, self.registry)
        };
        if missing.is_some() {
            return;
        }
        let Some((arm, delete)) = else_arm_extent(self.source, last_arm_end, else_body_end) else {
            return;
        };
        let shown = st.show();
        self.diags.push(
            Diagnostic::from_row("L0303", &[("type", shown.as_str())], Some(arm)).with_edit(
                TextEdit {
                    span: delete,
                    new_text: String::new(),
                },
            ),
        );
    }

    /// D-PARSESTR1 / D-BINPAT1: text and byte patterns are refutable even
    /// when they are written in a value-form dispatch chain. Keep this one
    /// diagnostic path shared with statement switches so `NoElse` never
    /// silently turns a refutable table into a total expression.
    fn report_refutable_pattern_without_else(
        &mut self,
        has_str_match_arm: bool,
        has_bin_match_arm: bool,
        span: Span,
    ) -> bool {
        if has_bin_match_arm && !has_str_match_arm {
            self.diags.push(Diagnostic::error(
                "E0148",
                format!(
                    "this `{}` matches bytes but has no `{}` arm",
                    Syntax::KW_IF,
                    Syntax::KW_ELSE
                ),
                "a binary pattern can always fail to match — the fixed bytes might differ, or the subject might be too short".to_string(),
                format!(
                    "add `{} {} {{ ... }}` to handle bytes that don't match",
                    Syntax::KW_ELSE,
                    Syntax::OP_UNIFIED_ARROW
                ),
                Some(span),
            ));
            return true;
        }
        if has_str_match_arm {
            self.diags.push(Diagnostic::error(
                "E0148",
                format!(
                    "this `{}` matches text but has no `{}` arm",
                    Syntax::KW_IF,
                    Syntax::KW_ELSE
                ),
                "a text pattern can always fail to match — the fixed text might differ, or a typed hole might not read as that type".to_string(),
                format!(
                    "add `{} {} {{ ... }}` to handle text that doesn't match",
                    Syntax::KW_ELSE,
                    Syntax::OP_UNIFIED_ARROW
                ),
                Some(span),
            ));
            return true;
        }
        false
    }

    /// D-OUTCOME-SHAPE1=A: a value-form pattern chain over one subject that
    /// handles the failure itself (an `.Ok`/`.Err` arm) keeps the `T? E!`
    /// carrier for its `.Val`/`.None` levels too, exactly as the statement
    /// table infers its subject once for every arm. Without an `.Err` arm the
    /// failure passes up and `.Val`/`.None` test the optional.
    pub(crate) fn note_carrier_chain_subject(&mut self, expr: &Expr) {
        let mut subject_start = None;
        let mut consumes_carrier = false;
        let mut cur = expr;
        while let Expr::If {
            cond, else_value, ..
        } = cur
        {
            let Some((subject, pattern)) = leading_pattern_test(cond) else {
                return;
            };
            let start = subject.span().start;
            if *subject_start.get_or_insert(start) != start {
                return;
            }
            consumes_carrier |= pattern_consumes_result_carrier(pattern);
            cur = else_value.as_ref();
        }
        if let (true, Some(start)) = (consumes_carrier, subject_start) {
            self.carrier_chain_subjects.insert(start);
        }
    }

    /// Card #1440: an else-less all-pattern value dispatch arrives from the
    /// parser as a nested `Expr::If` chain terminated by `Expr::NoElse`.
    /// Prove the pattern arms cover the subject's whole type with the same
    /// policy the statement table uses above. Runs once per chain (every
    /// level shares one span; the outermost caller wins the dedup insert).
    /// A table with a real `else` arm gets the L0303 check instead (#3716).
    pub(crate) fn check_noelse_dispatch_chain(&mut self, expr: &mut Expr) {
        let Expr::If { span, .. } = expr else { return };
        let span = *span;
        if !self.noelse_chains_checked.insert(span.start) {
            return;
        }
        // Pass 1: collect the subject and the raw arm patterns.
        let mut subject_clone: Option<Expr> = None;
        let mut raw: Vec<Pattern> = Vec::new();
        let mut same_subject = true;
        let mut every_level_pattern = true;
        let mut last_arm_end: Option<usize> = None;
        // The end of an authored `else` arm's body; `None` for `NoElse`.
        let mut else_end: Option<usize> = None;
        {
            let mut cur: &mut Expr = expr;
            loop {
                let Expr::If {
                    cond,
                    then_value,
                    else_body,
                    else_value,
                    ..
                } = cur
                else {
                    break;
                };
                if let Expr::PatternTest {
                    subject, pattern, ..
                } = cond.as_mut()
                {
                    if let Some(existing) = subject_clone.as_ref() {
                        same_subject &= existing.span() == subject.span();
                    }
                    if subject_clone.is_none() {
                        subject_clone = Some((**subject).clone());
                    }
                    raw.push(pattern.clone());
                    last_arm_end = Some(then_value.span().end);
                } else {
                    every_level_pattern = false;
                }
                match else_value.as_mut() {
                    Expr::If { span: level, .. } if *level == span => cur = else_value,
                    Expr::NoElse(_) if else_body.is_empty() => break,
                    value => {
                        let value_end = match value {
                            Expr::NoElse(_) => 0,
                            other => other.span().end,
                        };
                        let body_end = else_body.last().map_or(0, |stmt| stmt.span().end);
                        else_end = Some(value_end.max(body_end));
                        break;
                    }
                }
            }
        }
        let has_else = else_end.is_some();
        if has_else && (!same_subject || !every_level_pattern) {
            return;
        }
        let Some(mut subj) = subject_clone else {
            return;
        };
        // D-RESULT-DECON2=B: the parser's fixed two-arm handler already proves
        // exhaustive Result coverage. Its receiver is checked by the real
        // condition walk below; probing this clone would move it a second time
        // in sema. An arm that tests the payload (`.Err(.LookupError(_))`)
        // covers only part of its side, so that table takes the probe.
        if same_subject
            && raw.len() == 2
            && raw.first().is_some_and(|pattern| {
                matches!(pattern, Pattern::Ok { inner: None, .. })
                    || matches!(pattern, Pattern::Variant { variant, bindings, .. }
                        if matches!(bindings.as_slice(), [PatSlot::Bind { .. } | PatSlot::Wildcard])
                            && contextual_literal(variant) == Some(ContextualLiteral::Ok))
            })
            && raw.get(1).is_some_and(|pattern| {
                matches!(pattern, Pattern::Err { inner: None, .. })
                    || matches!(pattern, Pattern::Variant { variant, bindings, .. }
                        if matches!(bindings.as_slice(), [PatSlot::Bind { .. } | PatSlot::Wildcard])
                            && contextual_literal(variant) == Some(ContextualLiteral::Err))
            })
        {
            return;
        }
        // Result handlers are lowered to this ordinary exhaustive if chain.
        // Keep a direct fallible receiver as the carrier while the coverage
        // probe infers it; ordinary value positions still auto-propagate.
        let preserve_result_carrier = raw.iter().any(pattern_consumes_result_carrier);
        let subj_name = match &subj {
            Expr::Ident(n, _) => Some(n.clone()),
            _ => None,
        };
        // The subject is not the chain's result: the enclosing expectation
        // (`-> Light` for a tail/return dispatch over a `light: Light`
        // parameter) must not reach it, or the owning-copy rule wraps the
        // probed parameter in `Expr::Copy` and the arm probe below loses the
        // subject name, silently skipping E0307 (card #3587).
        let saved_expected = self.expected_type.take();
        // L0303 probes a table the ordinary per-level walk checks in full, so
        // the probe leaves no diagnostics or flow facts behind.
        let probe_start = has_else.then(|| (self.diags.len(), self.flow.clone()));
        if preserve_result_carrier {
            self.failure_auto_depth += 1;
        }
        let subj_ty = if preserve_result_carrier {
            self.infer_without_auto_propagation(&mut subj)
        } else {
            self.infer(&mut subj)
        };
        if preserve_result_carrier {
            self.failure_auto_depth -= 1;
        }
        self.expected_type = saved_expected;
        let Some(st) = subj_ty else {
            if let Some((len, flow)) = probe_start {
                self.diags.truncate(len);
                self.flow = flow;
            }
            return;
        };
        // Probe without retaining recovery diagnostics — the ordinary
        // per-level inference reports each arm's own errors once.
        let has_str_match_arm = raw
            .iter()
            .any(|pattern| matches!(pattern, Pattern::StrMatch { .. }));
        let has_bin_match_arm = raw
            .iter()
            .any(|pattern| matches!(pattern, Pattern::BinMatch { .. }));
        let diag_len = self.diags.len();
        let mut resolved: Vec<Pattern> = Vec::new();
        let mut all_pattern = !raw.is_empty();
        for mut p in raw {
            self.normalize_pattern_tree(&mut p, &st);
            // A subject that is not a name (a direct call, fallible or not)
            // is evaluated once for every level, so each arm tests the same
            // value and joins the coverage proof, as the statement table and
            // the Jet checker (span-keyed coverage) already do.
            if subj_name.is_none()
                && same_subject
                && !matches!(p, Pattern::StrMatch { .. } | Pattern::BinMatch { .. })
            {
                resolved.push(p);
                continue;
            }
            let pspan = p.span();
            let cond = Expr::PatternTest {
                subject: Box::new(subj.clone()),
                pattern: p,
                span: pspan,
            };
            match self.switch_arm_pattern(&cond, subj_name.as_deref(), &st) {
                Some(rp) => resolved.push(rp),
                None => all_pattern = false,
            }
        }
        self.diags.truncate(diag_len);
        if let Some((len, flow)) = probe_start {
            self.diags.truncate(len);
            self.flow = flow;
            if !all_pattern {
                return;
            }
            let mut covered = HashSet::new();
            let mut covered_ranges = Vec::new();
            for p in &resolved {
                self.note_pattern_coverage(p, &st, &mut covered, &mut covered_ranges, false);
            }
            self.diags.truncate(len);
            if let (Some(after), Some(body_end)) = (last_arm_end, else_end) {
                self.lint_unreachable_else(&st, &covered, &resolved, after, Some(body_end));
            }
            return;
        }
        if !all_pattern {
            self.report_refutable_pattern_without_else(has_str_match_arm, has_bin_match_arm, span);
            return;
        }
        let mut covered = HashSet::new();
        let mut covered_ranges = Vec::new();
        for p in &resolved {
            self.note_pattern_coverage(p, &st, &mut covered, &mut covered_ranges, false);
        }
        let insert_at = last_arm_end.map(|e| Span::new(e, e));
        self.check_pattern_coverage_complete(
            &st,
            &covered,
            &covered_ranges,
            &resolved,
            false,
            span,
            insert_at,
            subj_name.as_deref(),
        );
    }

    pub(crate) fn check_switch(
        &mut self,
        subject: &mut Expr,
        arms: &mut [crate::AST::SwitchArm],
        else_body: &mut Option<Vec<Stmt>>,
        span: Span,
        value_expected: Option<&Type>,
    ) {
        let subjectless_guard = crate::AST::is_subjectless_guard(subject, span);
        if subjectless_guard
            && arms
                .iter()
                .any(|arm| crate::AST::readiness_head(&arm.cond).is_some())
        {
            self.check_readiness_switch(arms, else_body, span);
            return;
        }
        let original_conditions = arms.iter().map(|arm| arm.cond.clone()).collect::<Vec<_>>();
        let mut reordered_optional_guard = false;
        // Normalize a single optional absence guard to the equivalent
        // Present test so TIR carries the same proven value that sema
        // checks. The branch swap preserves source evaluation order: only
        // the mutually exclusive bodies change places, while the guard is
        // evaluated once at the same point.
        if subjectless_guard && arms.len() == 1 && else_body.is_some() {
            self.rewrite_optional_flow_ne_none(&mut arms[0].cond);
            if let Some((name, name_span, cond_span)) =
                atomic_absent_optional_subject(&arms[0].cond)
            {
                if self.flow_narrowable_optional_inner(&name).is_some() {
                    let body = else_body
                        .as_mut()
                        .expect("else body checked as present above");
                    std::mem::swap(&mut arms[0].body, body);
                    arms[0].cond = Expr::PatternTest {
                        subject: Box::new(Expr::Ident(name.clone(), name_span)),
                        pattern: Pattern::Present {
                            binding: name,
                            binding_span: name_span,
                            inner: None,
                            span: cond_span,
                        },
                        span: cond_span,
                    };
                    reordered_optional_guard = true;
                }
            }
        } else {
            for arm in arms.iter_mut() {
                self.rewrite_optional_flow_ne_none(&mut arm.cond);
            }
        }
        // Result patterns consume the carrier itself. Keep a direct
        // fallible subject as `T !E`; ordinary value positions still use the
        // transparent automatic-propagation path.
        let preserve_result_carrier = arms.iter().any(|arm| {
            matches!(&arm.cond, Expr::PatternTest { pattern, .. }
                if pattern_consumes_result_carrier(pattern))
        });
        if preserve_result_carrier {
            self.failure_auto_depth += 1;
        }
        let subj_ty = if preserve_result_carrier {
            self.infer_without_auto_propagation(subject)
        } else {
            self.infer(subject)
        };
        if preserve_result_carrier {
            self.failure_auto_depth -= 1;
        }
        let subj_name = match &*subject {
            Expr::Ident(n, _) => Some(n.clone()),
            _ if !subjectless_guard && !matches!(&*subject, Expr::PatternTest { .. }) => {
                Some(Syntax::KW_IT.to_string())
            }
            _ if subj_ty.as_ref().is_some_and(|t| t.is_fallible()) => {
                Some(Syntax::KW_IT.to_string())
            }
            _ => None,
        };
        let it_scope = subj_name.as_deref() == Some(Syntax::KW_IT);
        if it_scope {
            self.push_scope();
            if let Some(st) = subj_ty.clone() {
                self.declare_in_scope(
                    Syntax::KW_IT,
                    LocalInfo {
                        def_span: span,
                        binding_sigil_span: None,
                        ty: st,
                        mutable: false,
                        param_conv: None,
                        decl_loop_depth: self.loop_depth,
                        interrupt_sendable: false,
                        reactive_local: false,
                        reactive_shared: false,
                        single_use_span: None,
                        invalid: false,
                    },
                );
            }
        }
        // A subjectless guard's arms test their own subjects; each such
        // pattern is normalized against its own subject type when the
        // condition is checked below, never against the guard placeholder.
        if let Some(st) = subj_ty.as_ref().filter(|_| !subjectless_guard) {
            for arm in arms.iter_mut() {
                if let Expr::PatternTest { pattern, .. } = &mut arm.cond {
                    self.normalize_pattern_tree(pattern, st);
                }
            }
        }
        let all_pattern = if let Some(st) = subj_ty.as_ref() {
            // Probe without retaining E0367 from bare-variant recovery —
            // those fire once when each arm is checked below.
            let diag_len = self.diags.len();
            let ok = !arms.is_empty()
                && arms.iter().all(|a| {
                    self.switch_arm_pattern(&a.cond, subj_name.as_deref(), st)
                        .is_some()
                });
            self.diags.truncate(diag_len);
            ok
        } else {
            false
        };
        let mut covered = HashSet::new();
        let mut covered_ranges = Vec::new();
        let mut arm_patterns = Vec::new();
        // D-FACT-FLOW1: one snapshot before the table, one store per arm,
        // and one shared join at the end. No plane keeps the last-walked arm.
        let before = self.flow.clone();
        let mut paths: Vec<crate::Sema::FlowFacts::FlowFacts> = Vec::new();
        for arm in arms.iter_mut() {
            self.flow = before.clone();
            if all_pattern {
                if let Some(ref st) = subj_ty {
                    let Some(pattern) =
                        self.switch_arm_pattern(&arm.cond, subj_name.as_deref(), st)
                    else {
                        continue;
                    };
                    let pspan = pattern.span();
                    self.note_pattern_coverage(
                        &pattern,
                        st,
                        &mut covered,
                        &mut covered_ranges,
                        subj_name.as_deref() == Some(Syntax::INTERNAL_MULTI_HEAD_SUBJECT),
                    );
                    arm_patterns.push(pattern.clone());
                    let bindings = self.validate_pattern(st, &pattern, pspan);
                    self.mark_pattern_subject_moved(subject, &bindings);
                    let mut windows = HashSet::new();
                    if is_write_window_subject(subject) {
                        collect_window_names(&pattern, &mut windows);
                    }
                    self.push_scope();
                    let mut restore_moved = Vec::new();
                    for (name, ty) in bindings {
                        let window = windows.contains(&name);
                        if let Some(restored) =
                            self.declare_pattern_binding(&name, pspan, ty, window)
                        {
                            restore_moved.push(restored);
                        }
                    }
                    self.record_pattern_view_bindings(subject, &pattern);
                    self.check_switch_arm_body(&mut arm.body, false, arm.span, value_expected);
                    self.pop_scope();
                    for (name, at) in restore_moved {
                        self.flow.moved.set(&name, at);
                    }
                    paths.push(self.flow.clone());
                    continue;
                }
            }
            let bindings = self.check_condition_with_bindings(&mut arm.cond);
            if bindings.is_empty() {
                self.check_switch_arm_body(&mut arm.body, true, arm.span, value_expected);
            } else {
                self.push_scope();
                let mut restore_moved = Vec::new();
                let windows = condition_window_names(&arm.cond, is_write_window_subject(subject));
                for (name, ty) in bindings {
                    let window = windows.contains(&name);
                    if let Some(restored) =
                        self.declare_pattern_binding(&name, arm.cond.span(), ty, window)
                    {
                        restore_moved.push(restored);
                    }
                }
                self.record_condition_view_bindings(&arm.cond);
                self.check_switch_arm_body(&mut arm.body, false, arm.span, value_expected);
                self.pop_scope();
                for (name, at) in restore_moved {
                    self.flow.moved.set(&name, at);
                }
            }
            paths.push(self.flow.clone());
        }
        self.flow = before.clone();
        if it_scope {
            self.pop_scope();
            for path in &mut paths {
                path.leave_scope();
            }
        }
        // The store as it stands outside the table, at the depth the merge
        // happens. The lint probes below may walk expressions, so the merge
        // reads this copy rather than whatever they touched.
        let outside_table = self.flow.clone();
        // True when some path can reach the code after the table without
        // running any arm. That path carries the pre-table facts into the
        // merge; a table that covers every case has no such path.
        let mut can_skip_every_arm = else_body.is_none();
        if all_pattern {
            if let Some(st) = subj_ty {
                let insert_at = if subj_name.as_deref() == Some(Syntax::INTERNAL_MULTI_HEAD_SUBJECT)
                {
                    None
                } else {
                    arms.last().map(|a| Span::new(a.span.end, a.span.end))
                };
                let reported = self.diags.len();
                self.check_pattern_coverage_complete(
                    &st,
                    &covered,
                    &covered_ranges,
                    &arm_patterns,
                    else_body.is_some(),
                    span,
                    insert_at,
                    subj_name.as_deref(),
                );
                can_skip_every_arm = else_body.is_none() && self.diags.len() > reported;
                if let (Some(else_stmts), Some(last)) = (else_body.as_ref(), arms.last()) {
                    if !subjectless_guard && subj_name.as_deref() != Some(Syntax::INTERNAL_MULTI_HEAD_SUBJECT) {
                        let body_end = else_stmts.last().map(|stmt| stmt.span().end);
                        self.lint_unreachable_else(&st, &covered, &arm_patterns, last.span.end, body_end);
                    }
                }
            }
        } else if else_body.is_none() && !subjectless_guard {
            // D-PARSESTR1: a str-match pattern arm is always refutable — the
            // literal text might not match, and a typed hole's read can fail
            // — so it gets its own E0148 instead of the generic E0003.
            let has_str_match_arm = arms.iter().any(|a| {
                matches!(
                    &a.cond,
                    Expr::PatternTest {
                        pattern: Pattern::StrMatch { .. },
                        ..
                    }
                )
            });
            // D-BINPAT1: a binary pattern arm is refutable the same way.
            let has_bin_match_arm = arms.iter().any(|a| {
                matches!(
                    &a.cond,
                    Expr::PatternTest {
                        pattern: Pattern::BinMatch { .. },
                        ..
                    }
                )
            });
            if !self.report_refutable_pattern_without_else(
                has_str_match_arm,
                has_bin_match_arm,
                span,
            ) {
                self.diags.push(Diagnostic::error(
                    "E0003",
                    format!(
                        "this `{}` needs an `{}` arm",
                        Syntax::KW_IF,
                        Syntax::KW_ELSE
                    ),
                    "mixed condition arms (or non-pattern arms) must always have a fallback (D-IF1)"
                        .to_string(),
                    format!(
                        "add `{} {} {{ ... }}` after the last arm",
                        Syntax::KW_ELSE,
                        Syntax::OP_UNIFIED_ARROW
                    ),
                    Some(span),
                ));
            }
        }
        if subjectless_guard && arms.len() > 1 {
            let subjects = arms
                .iter()
                .filter_map(|arm| leading_guard_pattern_subject(&arm.cond))
                .collect::<Vec<_>>();
            let subject_paths = subjects
                .iter()
                .filter_map(|subject| guard_subject_path(subject))
                .collect::<Vec<_>>();
            if subjects.len() == arms.len()
                && subject_paths.len() == arms.len()
                && subject_paths[1..]
                    .iter()
                    .all(|subject| subject == &subject_paths[0])
            {
                let mut first = subjects[0].clone();
                self.borrow_ctx = true;
                let enum_name = self.infer(&mut first).and_then(|ty| match ty {
                    Type::Named(name) if self.registry.enum_variants(&name).is_some() => Some(name),
                    _ => None,
                });
                if let Some(enum_name) = enum_name {
                    let first = &subject_paths[0];
                    self.diags.push(Diagnostic::lint(
                            "L0302",
                            format!("these arm heads all dispatch on `{enum_name}`"),
                            "naming the subject makes one closed enum's cases explicit and exhaustively checked"
                                .to_string(),
                            format!(
                                "write `if {first} == {{ ... }}` and put each variant pattern in an arm"
                            ),
                            Some(span),
                        ));
                }
            }
        }
        if let Some(body) = else_body {
            self.flow = outside_table.clone();
            let complement = if reordered_optional_guard {
                HashMap::new()
            } else {
                self.complement_condition_bindings(&original_conditions)
            };
            if !complement.is_empty() {
                self.push_scope();
                let mut restore_moved = Vec::new();
                let fact_span = original_conditions
                    .iter()
                    .find(|condition| atomic_absent_optional_subject(condition).is_some())
                    .map(|condition| condition.span())
                    .unwrap_or(span);
                for (name, ty) in complement {
                    if let Some(restored) = self.declare_condition_binding(&name, fact_span, ty) {
                        restore_moved.push(restored);
                    }
                }
                self.check_switch_arm_body(body, true, span, value_expected);
                self.pop_scope();
                for (name, at) in restore_moved {
                    self.flow.moved.set(&name, at);
                }
            } else {
                self.check_switch_arm_body(body, true, span, value_expected);
            }
            paths.push(self.flow.clone());
        } else if can_skip_every_arm {
            // Skipping every arm is itself a path through here.
            let all_arm_paths_exit = !paths.is_empty() && paths.iter().all(|path| !path.reachable);
            let mut fallthrough = outside_table.clone();
            // A statement guard may skip every arm, but a value-position
            // guard must provide a value on that reachable path as well.
            // Pattern tables already diagnose uncovered cases above.
            if subjectless_guard && fallthrough.reachable {
                if let Some(expected) = value_expected {
                    self.report_missing_block_value(expected, span);
                }
            }
            if all_arm_paths_exit {
                // A guard that exits on `.None` proves the payload on the
                // fallthrough path just like an explicit `else` complement.
                // Keep that proof in the flow overlay; pattern typing below
                // can still consult the stable Option binding when a later
                // `.Val(...)`/`.None` test needs the original carrier.
                let complement = self.complement_condition_bindings(&original_conditions);
                if !complement.is_empty() {
                    self.flow = fallthrough;
                    let fact_span = original_conditions
                        .iter()
                        .find(|condition| atomic_absent_optional_subject(condition).is_some())
                        .map(|condition| condition.span())
                        .unwrap_or(span);
                    for (name, ty) in complement {
                        self.record_optional_flow_narrow(&name, fact_span, ty);
                    }
                    fallthrough = self.flow.clone();
                }
            }
            paths.push(fallthrough);
        }
        // D-LIN1 / D-FACT-FLOW1: E0141 — a `#SingleUse` value consumed
        // on one arm and not another. `Moved::join` is a union (keeps
        // either arm's move), so the merged store alone would call this
        // value consumed and E0140 would never see the gap; check every
        // pre-merge path directly for exactly one side moving it, over
        // the bindings live at this scope before the table.
        let scope_depth = self.scope_depth();
        let mut divergent_single_use: Vec<(String, Span)> = outside_table
            .bindings
            .iter_at(scope_depth)
            .filter_map(|(name, info)| {
                let use_span = info.single_use_span?;
                if outside_table.moved.contains(name) {
                    return None;
                }
                let moved_paths = paths
                    .iter()
                    .filter(|path| path.reachable)
                    .map(|path| path.moved.contains(name))
                    .collect::<Vec<_>>();
                if moved_paths.iter().any(|moved| *moved) && moved_paths.iter().any(|moved| !*moved)
                {
                    Some((name.to_string(), use_span))
                } else {
                    None
                }
            })
            .collect();
        divergent_single_use.sort_by(|a, b| a.1.start.cmp(&b.1.start).then(a.0.cmp(&b.0)));
        for (name, use_span) in divergent_single_use {
            self.diags
                .push(crate::Sema::CheckerOwnership::e0141_unconsumed_branch(
                    &name, use_span,
                ));
        }
        self.flow = crate::Sema::FlowFacts::FlowFacts::merge_paths(&outside_table, &paths);
    }

    /// D-CONC-CHAN2=D: readiness tables are not Boolean guards. Each arm
    /// validates one plain `Receiver<T>` or one `Duration`, then checks its
    /// body in the scope containing the receive binding.
    fn check_readiness_switch(
        &mut self,
        arms: &mut [crate::AST::SwitchArm],
        else_body: &mut Option<Vec<Stmt>>,
        _span: Span,
    ) {
        enum ReadinessArm {
            Receive { binding: String, source: Expr },
            After { duration: Expr },
        }

        let before = self.flow.clone();
        let mut paths = Vec::with_capacity(arms.len() + usize::from(else_body.is_some()));
        let mut element_ty: Option<Type> = None;
        let mut has_receive = false;
        let mut has_after = false;

        for arm in arms.iter_mut() {
            self.flow = before.clone();
            let head = match crate::AST::readiness_head(&arm.cond) {
                Some(crate::AST::ReadinessHead::Receive { binding, source }) => {
                    ReadinessArm::Receive {
                        binding: binding.to_string(),
                        source: source.clone(),
                    }
                }
                Some(crate::AST::ReadinessHead::After { duration }) => ReadinessArm::After {
                    duration: duration.clone(),
                },
                None => {
                    self.diags.push(Diagnostic::error(
                            "E0112",
                            "a readiness table cannot mix channel arms with Boolean guards".to_string(),
                            "every arm in one readiness table waits on a Receiver<T> or a Duration".to_string(),
                            "write `value, receiver -> ...` or move the Boolean guard to a separate `if`".to_string(),
                            Some(arm.cond.span()),
                        ));
                    self.check_block(&mut arm.body, true);
                    paths.push(self.flow.clone());
                    continue;
                }
            };
            match head {
                ReadinessArm::Receive { binding, source } => {
                    has_receive = true;
                    // `readiness_head` borrows the compiler-private tuple carrier. Infer
                    // a private copy, then put the elaborated expression back into the
                    // carrier. In particular, `100ms` must become the checked Duration
                    // constructor before TIR coverage is decided; leaving the raw
                    // `UnitLit` behind makes every task containing a timer arm disappear
                    // from the interpreter program and makes AOT report an I2 ICE.
                    let mut source_expr = source;
                    let source_ty = self.infer(&mut source_expr);
                    let source_span = source_expr.span();
                    if let crate::AST::Expr::TupleLit(fields, ..) = &mut arm.cond {
                        if let Some((_, value)) = fields.first_mut() {
                            *value = source_expr;
                        }
                    }
                    let payload = match source_ty {
                        Some(Type::Apply { name, args })
                            if name == Syntax::TYPE_RECEIVER && args.len() == 1 =>
                        {
                            args[0].clone()
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "a readiness receive arm needs Receiver<T>, not {}",
                                    other.show()
                                ),
                                "a receive arm waits on one plain channel endpoint".to_string(),
                                "use the receiver returned by `channel<T>()`".to_string(),
                                Some(source_span),
                            ));
                            Type::Named("Unit".to_string())
                        }
                        None => Type::Named("Unit".to_string()),
                    };
                    if let Some(previous) = &element_ty {
                        if previous != &payload {
                            self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!(
                                        "readiness receive arms must share one element type, got {} and {}",
                                        previous.show(),
                                        payload.show()
                                    ),
                                    "one readiness table returns one receive payload type".to_string(),
                                    "use receivers with the same T, or split the table".to_string(),
                                    Some(source_span),
                                ));
                        }
                    } else {
                        element_ty = Some(payload.clone());
                    }
                    self.push_scope();
                    let mut restore_moved = Vec::new();
                    if let Some(restored) =
                        self.declare_condition_binding(&binding, arm.cond.span(), payload)
                    {
                        restore_moved.push(restored);
                    }
                    self.check_block(&mut arm.body, false);
                    self.pop_scope();
                    for (name, at) in restore_moved {
                        self.flow.moved.set(&name, at);
                    }
                }
                ReadinessArm::After { duration } => {
                    has_after = true;
                    let mut duration_expr = duration;
                    let duration_ty = self.infer(&mut duration_expr);
                    let duration_span = duration_expr.span();
                    if let crate::AST::Expr::TupleLit(fields, ..) = &mut arm.cond {
                        if let Some((_, value)) = fields.first_mut() {
                            *value = duration_expr;
                        }
                    }
                    if !matches!(duration_ty, Some(Type::Named(ref name)) if name == Syntax::DURATION_TYPE)
                    {
                        let shown = duration_ty
                            .as_ref()
                            .map(Type::show)
                            .unwrap_or_else(|| "this value".to_string());
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`after` needs a Duration, not {shown}"),
                            "a readiness timeout is one canonical Duration delta".to_string(),
                            "write `after 100ms` or pass a Duration binding".to_string(),
                            Some(duration_span),
                        ));
                    }
                    self.record_effect(crate::Sema::Effects::Effect::Time.name(), arm.cond.span());
                    self.check_block(&mut arm.body, false);
                }
            }
            paths.push(self.flow.clone());
        }

        if let Some(body) = else_body {
            self.flow = before.clone();
            self.check_block(body, true);
            paths.push(self.flow.clone());
        }
        if !has_receive && !has_after {
            self.diags.push(Diagnostic::error(
                "E0112",
                "a readiness table has no usable wait arm".to_string(),
                "a table must wait on a Receiver<T> or a Duration".to_string(),
                "add `value, receiver -> ...` or `after 100ms -> ...`".to_string(),
                Some(
                    arms.first()
                        .map(|arm| arm.cond.span())
                        .unwrap_or(Span::new(0, 0)),
                ),
            ));
        }
        self.flow = crate::Sema::FlowFacts::FlowFacts::merge_paths(&before, &paths);
    }
}
