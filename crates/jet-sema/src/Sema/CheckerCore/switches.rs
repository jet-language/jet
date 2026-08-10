use crate::AST::{BinOp, Expr, Pattern, Stmt, Type};
use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Sema::Diagnostics::{missing_arms_text, missing_pattern_coverage, pattern_variant_name};
use crate::Sema::{Checker, LocalInfo};
use crate::Syntax;
use std::collections::{HashMap, HashSet};

fn leading_guard_pattern_subject(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::PatternTest { subject, .. } => Some(subject),
        Expr::Binary(BinOp::And, left, _, _) => leading_guard_pattern_subject(left),
        _ => None,
    }
}

/// D-FLOWTYPE1=A: `None` as a compared value (`x != None`), not a pattern head.
fn expr_is_absent_none(expr: &Expr) -> bool {
    match expr {
        Expr::Absent(_) => true,
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } if type_name.is_empty()
            && matches!(contextual_literal(variant), Some(ContextualLiteral::Null))
            && args.is_empty() => true,
        Expr::Paren(inner, _) | Expr::Copy(inner, _) => expr_is_absent_none(inner),
        _ => false,
    }
}

/// D-FLOWTYPE1=A: atomic `x == None` / `x == .None` pattern test subject.
pub(crate) fn atomic_absent_optional_subject(cond: &Expr) -> Option<(String, Span, Span)> {
    match cond {
        Expr::PatternTest {
            subject,
            pattern,
            span,
        } => {
            let is_absent = match pattern {
                Pattern::Absent(_) => true,
                Pattern::Variant {
                    variant, bindings, ..
                } => matches!(contextual_literal(variant), Some(ContextualLiteral::Null))
                    && bindings.is_empty(),
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
        Expr::Field(base, member, _) => {
            Some(format!("{}.{}", guard_subject_path(base)?, member))
        }
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
    let binding = || {
        bindings
            .first()
            .map(|slot| {
                (
                    slot.as_bind()
                        .unwrap_or(Syntax::PAT_WILDCARD_SLOT)
                        .to_string(),
                    slot.binding_span().unwrap_or(*span),
                )
            })
    };
    let replacement = match (
        subject_ty,
        contextual_literal(variant),
        *leading_dot,
        bindings.len(),
    ) {
        (_, Some(ContextualLiteral::Null), false, 0)
        | (Type::Option(_), Some(ContextualLiteral::Null), true, 0) => {
            Some(Pattern::Absent(*span))
        }
        (_, Some(ContextualLiteral::Value), false, 1)
        | (Type::Option(_), Some(ContextualLiteral::Value), true, 1) => {
            let (binding, binding_span) = binding().unwrap();
            Some(Pattern::Present {
                binding,
                binding_span,
                span: *span,
            })
        }
        (Type::Result { .. }, Some(ContextualLiteral::Ok), _, 1) => {
            let (binding, binding_span) = binding().unwrap();
            Some(Pattern::Ok {
                binding,
                binding_span,
                span: *span,
            })
        }
        (Type::Result { .. }, Some(ContextualLiteral::Err), _, 1) => {
            let (binding, binding_span) = binding().unwrap();
            Some(Pattern::Err {
                binding,
                binding_span,
                span: *span,
            })
        }
        _ => None,
    };
    if let Some(replacement) = replacement {
        *pattern = replacement;
    }
}

impl<'a> Checker<'a> {
        /// D-FLOWTYPE1=A: immutable local/param of type `T?` may refine to `T`.
        pub(crate) fn flow_narrowable_optional_inner(&self, name: &str) -> Option<Type> {
            let info = self.lookup(name)?;
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
            let Some(info) = self.lookup(name) else {
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
            if self.flow.bindings.get_at(name, depth).is_some()
                || self.flow.narrow.get_at(name, depth).is_some()
            {
                self.diags
                    .push(crate::Sema::Registration::already_defined(name, name_span));
            }
            self.flow.narrow.set_at(
                name,
                depth,
                LocalInfo {
                    def_span: name_span,
                    ty: inner,
                    mutable: false,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                    sendable: true,
                    interrupt_sendable: false,
                    reactive_local: false,
                    reactive_shared: false,
                    task_lint_span: None,
                    single_use_span: None,
                    constant_value: None,
                },
            );
        }

        pub(crate) fn declare_condition_binding(
            &mut self,
            name: &str,
            span: Span,
            ty: Type,
        ) -> Option<(String, Span)> {
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
                        ty,
                        mutable: false,
                        param_conv: None,
                        decl_loop_depth: self.loop_depth,
                        sendable: true,
                        interrupt_sendable: false,
                        reactive_local: false,
                        reactive_shared: false,
                        task_lint_span: None,
                        single_use_span: None,
                        constant_value: None,
                    },
                );
                None
            };
            restore
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
                Expr::Binary(BinOp::And, l, r, _) => {
                    let left_bindings = self.check_condition_with_bindings(l);
                    self.push_scope();
                    let mut restore_moved = Vec::new();
                    for (name, ty) in &left_bindings {
                        if let Some(restored) =
                            self.declare_condition_binding(name, l.span(), ty.clone())
                        {
                            restore_moved.push(restored);
                        }
                    }
                    self.record_condition_view_bindings(l);
                    let mut right_bindings = self.check_condition_with_bindings(r);
                    self.pop_scope();
                    for (name, at) in restore_moved {
                        self.flow.moved.set(&name, at);
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
    
        /// One pattern arm's contribution to the covered set — shared by the
        /// statement table and the else-less value-dispatch chain (card #1440),
        /// so unreachable-arm policy (D-PATO or-patterns, D-TAG1 subtree
        /// ancestors, E0365 vs L0301) lives in exactly one place.
        pub(crate) fn note_pattern_coverage(
            &mut self,
            pattern: &Pattern,
            st: &Type,
            covered: &mut HashSet<String>,
            multi_head: bool,
        ) {
            let pspan = pattern.span();
            // D-PATO: or-patterns cover multiple variants; insert all of them.
            let covered_names: Vec<String> = if let Pattern::Or(alts, _) = pattern {
                alts.iter().filter_map(pattern_variant_name).collect()
            } else if let Some(v) = pattern_variant_name(pattern) {
                vec![v]
            } else {
                Vec::new()
            };
            for variant in covered_names {
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
                        self.diags.push(Diagnostic::error("E0365", what, why, fix, Some(pspan)));
                    } else {
                        self.diags.push(Diagnostic::lint("L0301", what, why, fix, Some(pspan)));
                    }
                } else {
                    covered.insert(variant);
                }
            }
        }

        /// The completion half of the shared policy: an else-less all-pattern
        /// table must cover the subject's whole type (E0307); D-PATR open
        /// scalars can never prove totality.
        pub(crate) fn check_pattern_coverage_complete(
            &mut self,
            st: &Type,
            covered: &HashSet<String>,
            has_else: bool,
            span: Span,
            insert_at: Option<Span>,
            subj_name: Option<&str>,
        ) {
            if has_else {
                return;
            }
            let multi_head = subj_name == Some(Syntax::INTERNAL_MULTI_HEAD_SUBJECT);
            // D-PATR: Int/Char are open scalar types — range arms can never
            // prove totality, so an `else` (or wildcard) is always required.
            // `missing_pattern_coverage` returns None for Int/Char (infinite
            // domain), so we detect this case separately.
            if matches!(st, Type::Int | Type::Char) {
                self.diags.push(Diagnostic::error(
                    "E0307",
                    format!(
                        "this `{}` over `{}` has no `{}` arm — range arms can't cover every value",
                        Syntax::KW_IF,
                        st.show(),
                        Syntax::KW_ELSE,
                    ),
                    format!(
                        "`{}` has infinitely many values; range arms only cover a subset (D-PATR)",
                        st.show()
                    ),
                    format!(
                        "add `{} {} {{ … }}` to handle values not matched by any range",
                        Syntax::KW_ELSE,
                        Syntax::OP_ARM_ARROW
                    ),
                    Some(span),
                ));
                return;
            }
            if let Some(missing) = missing_pattern_coverage(st, covered, self.registry) {
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
                        "every arm here is a pattern test, so each variant must appear once"
                            .to_string()
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
                        diag.edit = Some(TextEdit {
                            span: at,
                            new_text: missing_arms_text(st, &missing, subj_name),
                        });
                    }
                }
                self.diags.push(diag);
            }
        }

        /// Card #1440: an else-less all-pattern value dispatch arrives from the
        /// parser as a nested `Expr::If` chain terminated by `Expr::NoElse`.
        /// Prove the pattern arms cover the subject's whole type with the same
        /// policy the statement table uses above. Runs once per chain (every
        /// level shares one span; the outermost caller wins the dedup insert).
        pub(crate) fn check_noelse_dispatch_chain(&mut self, expr: &mut Expr) {
            let Expr::If { span, .. } = expr else { return };
            let span = *span;
            if !self.noelse_chains_checked.insert(span.start) {
                return;
            }
            // Pass 1: collect the subject and the raw arm patterns.
            let mut subject_clone: Option<Expr> = None;
            let mut raw: Vec<Pattern> = Vec::new();
            let mut last_arm_end: Option<usize> = None;
            {
                let mut cur: &mut Expr = expr;
                loop {
                    let Expr::If {
                        cond,
                        then_value,
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
                        if subject_clone.is_none() {
                            subject_clone = Some((**subject).clone());
                        }
                        raw.push(pattern.clone());
                        last_arm_end = Some(then_value.span().end);
                    }
                    match else_value.as_mut() {
                        Expr::If { .. } => cur = else_value,
                        _ => break,
                    }
                }
            }
            let Some(mut subj) = subject_clone else { return };
            let Some(st) = self.infer(&mut subj) else { return };
            let subj_name = match &subj {
                Expr::Ident(n, _) => Some(n.clone()),
                _ => None,
            };
            // Probe without retaining recovery diagnostics — the ordinary
            // per-level inference reports each arm's own errors once.
            let diag_len = self.diags.len();
            let mut resolved: Vec<Pattern> = Vec::new();
            let mut all_pattern = !raw.is_empty();
            for mut p in raw {
                normalize_contextual_pattern(&mut p, &st);
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
            if !all_pattern {
                return;
            }
            let mut covered = HashSet::new();
            for p in &resolved {
                self.note_pattern_coverage(p, &st, &mut covered, false);
            }
            let insert_at = last_arm_end.map(|e| Span::new(e, e));
            self.check_pattern_coverage_complete(
                &st,
                &covered,
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
        ) {
            let subjectless_guard = crate::AST::is_subjectless_guard(subject, span);
            // D-FLOWTYPE1=A: statement `if x == None { … } else { … }` uses
            // the same canonical Present-pattern fact as value `if`. Swap the
            // two branches before checking so the proven unwrap is retained in
            // the AST/TIR path; a sema-only narrowed scope would leave codegen
            // with the original Optional value.
            if subjectless_guard && arms.len() == 1 {
                self.rewrite_optional_flow_ne_none(&mut arms[0].cond);
                if let Some((name, name_span, cond_span)) =
                    atomic_absent_optional_subject(&arms[0].cond)
                {
                    if else_body.is_some()
                        && self.flow_narrowable_optional_inner(&name).is_some()
                    {
                        let original_else = else_body.take().expect("checked above");
                        let original_then = std::mem::replace(&mut arms[0].body, Vec::new());
                        arms[0].body = original_else;
                        *else_body = Some(original_then);
                        arms[0].cond = Expr::PatternTest {
                            subject: Box::new(Expr::Ident(name.clone(), name_span)),
                            pattern: Pattern::Present {
                                binding: name,
                                binding_span: name_span,
                                span: cond_span,
                            },
                            span: cond_span,
                        };
                    }
                }
            } else {
                for arm in arms.iter_mut() {
                    self.rewrite_optional_flow_ne_none(&mut arm.cond);
                }
            }
            let subj_ty = self.infer(subject);
            let subj_name = match &*subject {
                Expr::Ident(n, _) => Some(n.clone()),
                _ if subj_ty.as_ref().is_some_and(|t| t.is_fallible()) => {
                    Some(Syntax::KW_IT.to_string())
                }
                _ => None,
            };
            let it_scope = subj_name.as_deref() == Some(Syntax::KW_IT);
            if it_scope {
                self.push_scope();
                if let Some(st) = subj_ty.clone() {
                    self.declare(
                        Syntax::KW_IT,
                        span,
                        LocalInfo {
                            def_span: span,
                            ty: st,
                            mutable: false,
                            param_conv: None,
                            decl_loop_depth: self.loop_depth,
                            sendable: true,
                            interrupt_sendable: false,
                            reactive_local: false,
                            reactive_shared: false,
                            task_lint_span: None,
                            single_use_span: None,
                            constant_value: None,
                        },
                    );
                }
            }
            if let Some(st) = &subj_ty {
                for arm in arms.iter_mut() {
                    if let Expr::PatternTest { pattern, .. } = &mut arm.cond {
                        normalize_contextual_pattern(pattern, st);
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
                            subj_name.as_deref() == Some(Syntax::INTERNAL_MULTI_HEAD_SUBJECT),
                        );
                        let bindings = self.validate_pattern(st, &pattern, pspan);
                        self.mark_pattern_subject_moved(subject, &bindings);
                        self.push_scope();
                        let mut restore_moved = Vec::new();
                        for (name, ty) in bindings {
                            if let Some(restored) =
                                self.declare_condition_binding(&name, pspan, ty)
                            {
                                restore_moved.push(restored);
                            }
                        }
                        self.record_pattern_view_bindings(subject, &pattern);
                        self.check_block(&mut arm.body, false);
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
                    self.check_block(&mut arm.body, true);
                } else {
                    self.push_scope();
                    let mut restore_moved = Vec::new();
                    for (name, ty) in bindings {
                        if let Some(restored) =
                            self.declare_condition_binding(&name, arm.cond.span(), ty)
                        {
                            restore_moved.push(restored);
                        }
                    }
                    self.record_condition_view_bindings(&arm.cond);
                    self.check_block(&mut arm.body, false);
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
                    let insert_at = if subj_name.as_deref()
                        == Some(Syntax::INTERNAL_MULTI_HEAD_SUBJECT)
                    {
                        None
                    } else {
                        arms.last().map(|a| Span::new(a.span.end, a.span.end))
                    };
                    let reported = self.diags.len();
                    self.check_pattern_coverage_complete(
                        &st,
                        &covered,
                        else_body.is_some(),
                        span,
                        insert_at,
                        subj_name.as_deref(),
                    );
                    can_skip_every_arm = else_body.is_none() && self.diags.len() > reported;
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
                            Syntax::OP_ARM_ARROW
                        ),
                        Some(span),
                    ));
                } else if has_str_match_arm {
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
                            Syntax::OP_ARM_ARROW
                        ),
                        Some(span),
                    ));
                } else {
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
                            Syntax::OP_ARM_ARROW
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
                        Type::Named(name) if self.registry.enum_variants(&name).is_some() => {
                            Some(name)
                        }
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
            let else_narrow = else_body.as_ref().and_then(|_| {
                arms.iter().find_map(|arm| {
                    let (name, name_span, cond_span) =
                        atomic_absent_optional_subject(&arm.cond)?;
                    let inner = self.flow_narrowable_optional_inner(&name)?;
                    Some((name, name_span, cond_span, inner))
                })
            });
            if let Some(body) = else_body {
                self.flow = outside_table.clone();
                if let Some((name, _name_span, cond_span, inner)) = else_narrow {
                    self.push_scope();
                    let restore_moved = self.declare_condition_binding(&name, cond_span, inner);
                    self.check_block(body, true);
                    self.pop_scope();
                    if let Some((name, at)) = restore_moved {
                        self.flow.moved.set(&name, at);
                    }
                } else {
                    self.check_block(body, true);
                }
                paths.push(self.flow.clone());
            } else if can_skip_every_arm {
                // Skipping every arm is itself a path through here.
                paths.push(outside_table.clone());
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
                        .map(|path| path.moved.contains(name))
                        .collect::<Vec<_>>();
                    if moved_paths.iter().any(|moved| *moved)
                        && moved_paths.iter().any(|moved| !*moved)
                    {
                        Some((name.to_string(), use_span))
                    } else {
                        None
                    }
                })
                .collect();
            divergent_single_use.sort_by(|a, b| a.1.start.cmp(&b.1.start).then(a.0.cmp(&b.0)));
            for (name, use_span) in divergent_single_use {
                self.diags.push(
                    crate::Sema::CheckerOwnership::e0141_unconsumed_branch(&name, use_span),
                );
            }
            self.flow = crate::Sema::FlowFacts::FlowFacts::merge_paths(&outside_table, &paths);
        }
    
}
