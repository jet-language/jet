use crate::AST::{BinOp, ElseBranch, Expr, IfStmt, Pattern, Stmt, Type};
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
        } if type_name.is_empty() && variant == Syntax::LIT_NULL && args.is_empty() => true,
        Expr::Paren(inner, _) | Expr::Copy(inner, _) => expr_is_absent_none(inner),
        _ => false,
    }
}

/// D-FLOWTYPE1=A: atomic `x == None` / `x == .None` pattern test subject.
pub(crate) fn atomic_absent_optional_subject(cond: &Expr) -> Option<(String, Span, Span)> {
    match cond {
        Expr::PatternTest {
            subject,
            pattern: Pattern::Absent(_),
            span,
        } => match subject.as_ref() {
            Expr::Ident(name, name_span) => Some((name.clone(), *name_span, *span)),
            _ => None,
        },
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
    let replacement = match (subject_ty, variant.as_str(), bindings.len()) {
        (Type::Option(_), name, 0) if name == Syntax::LIT_NULL => {
            Some(Pattern::Absent(*span))
        }
        (Type::Option(_), name, 1) if name == Syntax::LIT_VALUE => {
            let (binding, binding_span) = binding().unwrap();
            Some(Pattern::Present {
                binding,
                binding_span,
                span: *span,
            })
        }
        (Type::Result { .. }, name, 1) if name == Syntax::LIT_OK => {
            let (binding, binding_span) = binding().unwrap();
            Some(Pattern::Ok {
                binding,
                binding_span,
                span: *span,
            })
        }
        (Type::Result { .. }, name, 1) if name == Syntax::LIT_ERR => {
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
            if self
                .scopes
                .last()
                .is_some_and(|scope| scope.contains_key(name))
            {
                self.diags
                    .push(crate::Sema::Registration::already_defined(name, name_span));
            }
            self.scopes.last_mut().unwrap().insert(
                name.to_string(),
                LocalInfo {
                    def_span: name_span,
                    ty: inner,
                    mutable: false,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                    sendable: true,
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
                let moved_at = self.moved.remove(name);
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

        /// D-FLOWTYPE1=A: `if x == None { A } else { B }` ≡ `if x == Val(x) { B } else { A }`.
        /// Only atomic None tests invert — compound conditions do not prove presence on the
        /// false path.
        pub(crate) fn invert_optional_none_else_narrow(&self, ifs: &mut IfStmt) {
            if ifs.else_branch.is_none() {
                return;
            }
            let Some((name, name_span, span)) = atomic_absent_optional_subject(&ifs.cond) else {
                return;
            };
            if self.flow_narrowable_optional_inner(&name).is_none() {
                return;
            }
            let then_body = std::mem::take(&mut ifs.then_body);
            let else_branch = ifs.else_branch.take();
            match else_branch {
                Some(ElseBranch::Else(else_body)) => {
                    ifs.then_body = else_body;
                    ifs.else_branch = Some(ElseBranch::Else(then_body));
                }
                Some(ElseBranch::ElseIf(next)) => {
                    ifs.then_body = vec![Stmt::If(*next)];
                    ifs.else_branch = Some(ElseBranch::Else(then_body));
                }
                None => return,
            }
            ifs.cond = Expr::PatternTest {
                subject: Box::new(Expr::Ident(name.clone(), name_span)),
                pattern: Pattern::Present {
                    binding: name,
                    binding_span: name_span,
                    span,
                },
                span,
            };
        }

        pub(crate) fn prepare_optional_flow_if(&self, ifs: &mut IfStmt) {
            self.invert_optional_none_else_narrow(ifs);
            self.rewrite_optional_flow_ne_none(&mut ifs.cond);
        }

        pub(crate) fn check_condition_with_bindings(
            &mut self,
            cond: &mut Expr,
        ) -> HashMap<String, Type> {
            // Idempotent with `prepare_optional_flow_if` for statement `if`.
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
                    let mut right_bindings = self.check_condition_with_bindings(r);
                    self.pop_scope();
                    for (name, at) in restore_moved {
                        self.moved.insert(name, at);
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
    
        pub(crate) fn check_switch(
            &mut self,
            subject: &mut Expr,
            arms: &mut [crate::AST::SwitchArm],
            else_body: &mut Option<Vec<Stmt>>,
            span: Span,
        ) {
            let subjectless_guard = crate::AST::is_subjectless_guard(subject, span);
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
            let all_pattern = subj_ty.is_some()
                && !arms.is_empty()
                && arms.iter().all(|a| {
                    self.switch_arm_pattern(&a.cond, subj_name.as_deref(), subj_ty.as_ref().unwrap())
                        .is_some()
                });
            let mut covered = HashSet::new();
            let move_before = self.moved.clone();
            let mut move_after = move_before.clone();
            for arm in arms.iter_mut() {
                self.moved = move_before.clone();
                if all_pattern {
                    if let Some(ref st) = subj_ty {
                        let Some(pattern) =
                            self.switch_arm_pattern(&arm.cond, subj_name.as_deref(), st)
                        else {
                            continue;
                        };
                        let pspan = pattern.span();
                        // D-PATO: or-patterns cover multiple variants; insert all of them.
                        let covered_names: Vec<String> = if let Pattern::Or(alts, _) = &pattern {
                            alts.iter().filter_map(pattern_variant_name).collect()
                        } else if let Some(v) = pattern_variant_name(&pattern) {
                            vec![v]
                        } else {
                            Vec::new()
                        };
                        for variant in covered_names {
                            // D-TAG1: an earlier group arm already covers every leaf in
                            // its subtree, so `.Fire ->` makes a later `.Fire.Burn ->`
                            // unreachable (ancestor-or-equal test on the dotted path).
                            let already = covered.contains(&variant)
                                || covered
                                    .iter()
                                    .any(|c| variant.starts_with(&format!("{c}.")));
                            if already {
                                let what = format!(
                                    "arm `{}` is unreachable — that case is already handled",
                                    variant
                                );
                                let why =
                                    "every earlier arm already covers this pattern".to_string();
                                let fix =
                                    "remove this arm or merge it with the one above".to_string();
                                if matches!(st, Type::Union(_)) {
                                    self.diags.push(Diagnostic::error(
                                        "E0365",
                                        what,
                                        why,
                                        fix,
                                        Some(pspan),
                                    ));
                                } else {
                                    self.diags.push(Diagnostic::lint(
                                        "L0301",
                                        what,
                                        why,
                                        fix,
                                        Some(pspan),
                                    ));
                                }
                            } else {
                                covered.insert(variant);
                            }
                        }
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
                        self.check_block(&mut arm.body, false);
                        self.pop_scope();
                        for (name, at) in restore_moved {
                            self.moved.insert(name, at);
                        }
                        for (k, v) in self.moved.drain() {
                            move_after.entry(k).or_insert(v);
                        }
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
                    self.check_block(&mut arm.body, false);
                    self.pop_scope();
                    for (name, at) in restore_moved {
                        self.moved.insert(name, at);
                    }
                }
                for (k, v) in self.moved.drain() {
                    move_after.entry(k).or_insert(v);
                }
            }
            if it_scope {
                self.pop_scope();
            }
            if all_pattern {
                if let Some(st) = subj_ty {
                    // D-PATR: Int/Char are open scalar types — range arms can never
                    // prove totality, so an `else` (or wildcard) is always required.
                    // `missing_pattern_coverage` returns None for Int/Char (infinite
                    // domain), so we detect this case separately.
                    let open_scalar_no_else =
                        matches!(st, Type::Int | Type::Char) && else_body.is_none();
                    if open_scalar_no_else {
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
                    } else if let Some(missing) = missing_pattern_coverage(&st, &covered, self.registry)
                    {
                        if else_body.is_none() {
                            let mut diag = Diagnostic::error(
                                "E0307",
                                format!(
                                    "this `{}` doesn't cover every case — missing: {}",
                                    Syntax::KW_IF,
                                    missing.join(", ")
                                ),
                                "every arm here is a pattern test, so each variant must appear once"
                                    .to_string(),
                                format!("add an arm for: {}", missing.join(", ")),
                                Some(span),
                            );
                            // Attach a structured insert so LSP/CLI can add compilable arms.
                            if let Some(last_arm) = arms.last() {
                                let new_text = missing_arms_text(&st, &missing, subj_name.as_deref());
                                diag.edit = Some(TextEdit {
                                    span: Span::new(last_arm.span.end, last_arm.span.end),
                                    new_text,
                                });
                            }
                            self.diags.push(diag);
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
                            format!("these guards all dispatch on `{enum_name}`"),
                            "subject dispatch makes one closed enum's cases explicit and exhaustively checked"
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
                self.moved = move_before.clone();
                self.check_block(body, true);
                for (k, v) in self.moved.drain() {
                    move_after.entry(k).or_insert(v);
                }
            }
            self.moved = move_after;
        }
    
}
