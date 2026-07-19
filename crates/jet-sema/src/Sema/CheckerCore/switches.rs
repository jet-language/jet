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
        pub(crate) fn check_condition_with_bindings(
            &mut self,
            cond: &mut Expr,
        ) -> HashMap<String, Type> {
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
                    for (name, ty) in &left_bindings {
                        self.declare(
                            name,
                            l.span(),
                            LocalInfo {
                                def_span: l.span(),
                                ty: ty.clone(),
                                mutable: false,
                                param_conv: None,
                                decl_loop_depth: self.loop_depth,
                                sendable: true,
                                task_lint_span: None,
                                single_use_span: None,
                            },
                        );
                    }
                    let mut right_bindings = self.check_condition_with_bindings(r);
                    self.pop_scope();
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
                            task_lint_span: None,
                            single_use_span: None,
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
                                self.diags.push(Diagnostic::lint(
                                    "L0301",
                                    format!(
                                        "arm `{}` is unreachable — that case is already handled",
                                        variant
                                    ),
                                    "every earlier arm already covers this pattern".to_string(),
                                    "remove this arm or merge it with the one above".to_string(),
                                    Some(pspan),
                                ));
                            } else {
                                covered.insert(variant);
                            }
                        }
                        let bindings = self.validate_pattern(st, &pattern, pspan);
                        self.mark_pattern_subject_moved(subject, &bindings);
                        self.push_scope();
                        for (name, ty) in bindings {
                            self.declare(
                                &name,
                                pspan,
                                LocalInfo {
                                    def_span: pspan,
                                    ty,
                                    mutable: false,
                                    param_conv: None,
                                    decl_loop_depth: self.loop_depth,
                                    sendable: true,
                                    task_lint_span: None,
                                    single_use_span: None,
                                },
                            );
                        }
                        self.check_block(&mut arm.body, false);
                        self.pop_scope();
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
                    for (name, ty) in bindings {
                        self.declare(
                            &name,
                            arm.cond.span(),
                            LocalInfo {
                                def_span: arm.cond.span(),
                                ty,
                                mutable: false,
                                param_conv: None,
                                decl_loop_depth: self.loop_depth,
                                sendable: true,
                                task_lint_span: None,
                                single_use_span: None,
                            },
                        );
                    }
                    self.check_block(&mut arm.body, false);
                    self.pop_scope();
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
