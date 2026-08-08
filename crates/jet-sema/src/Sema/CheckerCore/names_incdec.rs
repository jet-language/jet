use crate::AST::{Expr, IncDecOp, LValue, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Diagnostics::{computed_field_not_settable, edit_distance, expr_root_ident, is_task_type};
use crate::Sema::{Checker, LocalInfo};
use crate::Syntax;
impl<'a> Checker<'a> {
        /// Declare one name bound by a destructuring pattern (S74).
        pub(crate) fn declare_bound(&mut self, name: &str, span: Span, ty: Type, mutable: bool) {
            let sendable = self.sendability_problem(&ty, true).is_none();
            let task_lint_span = if is_task_type(&ty) { Some(span) } else { None };
            let single_use_span = if self.type_is_single_use(&ty) {
                Some(span)
            } else {
                None
            };
            self.declare(
                name,
                span,
                LocalInfo {
                    def_span: span,
                    ty,
                    mutable,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                    sendable,
                    reactive_local: false,
                    reactive_shared: false,
                    task_lint_span,
                    single_use_span,
                    constant_value: None,
                },
            );
        }
    
        // --- expressions ------------------------------------------------------
    
        pub(crate) fn require_bool(&mut self, e: &mut Expr, what: &str) {
            if let Some(t) = self.infer(e) {
                if t != Type::Bool {
                    self.diags.push(Diagnostic::error(
                        "E0110",
                        format!(
                            "{} must be {}, but this is {}",
                            what,
                            Type::Bool.show(),
                            t.show()
                        ),
                        "the program needs a clear yes or no here".to_string(),
                        "compare the value to something, e.g. `x > 0` or `name == \"ok\"`".to_string(),
                        Some(e.span()),
                    ));
                }
            }
        }
    
        pub(crate) fn unknown_name(&mut self, name: &str, span: Span) {
            let mut fix = format!(
                "declare it first: `{} {} ...`",
                name,
                Syntax::SIGIL_BIND_IMMUT
            );
            if let Some(module) = unique_core_module_for_alias(name) {
                fix = format!("add `use {module} as {name}`");
            }
            let mut best: Option<(String, usize)> = None;
            let candidates: Vec<String> = self
                .visible_names()
                .into_iter()
                .chain(self.consts.keys().cloned())
                .collect();
            for cand in candidates {
                let d = edit_distance(name, &cand);
                if d <= 2 && best.as_ref().map_or(true, |(_, bd)| d < *bd) {
                    best = Some((cand, d));
                }
            }
            if let Some((cand, _)) = best {
                fix = format!("did you mean `{}`?", cand);
            }
            self.diags.push(Diagnostic::error(
                "E0107",
                format!("nothing named `{}` exists here", name),
                "a name must be declared before it's used".to_string(),
                fix,
                Some(span),
            ));
        }
    
        /// D-INCR1: type-check `++`/`--`. Prefix returns the updated value; postfix
        /// returns the value before the update. Operand must be a mutable integer
        /// lvalue (same LHS policy as S17; indexed slots are rejected like `+=`).
        pub(crate) fn check_incdec(
            &mut self,
            _op: IncDecOp,
            operand: &mut Expr,
            _postfix: bool,
            span: Span,
        ) -> Option<Type> {
            let lvalue = match operand {
                Expr::Ident(name, name_span) => LValue::Local {
                    name: name.clone(),
                    name_span: *name_span,
                },
                Expr::Index { span: idx_span, .. } => {
                    self.diags.push(Diagnostic::error(
                        "E0163",
                        "increment and decrement can't target an indexed slot".to_string(),
                        "write the full update: `map[key] = map[key] + 1`".to_string(),
                        "use `+= 1` on a name, or assign through `=` with the whole right-hand side"
                            .to_string(),
                        Some(*idx_span),
                    ));
                    return None;
                }
                Expr::Field(base, field, field_span) => LValue::Field {
                    base: base.clone(),
                    field: field.clone(),
                    span: *field_span,
                },
                other => {
                    self.diags.push(Diagnostic::error(
                        "E0160",
                        "this value can't be incremented or decremented".to_string(),
                        "only a mutable name or field like `count` or `self.hits` accepts `++`/`--`"
                            .to_string(),
                        format!(
                            "use a `{}` binding and write `name += 1` / `name -= 1`",
                            Syntax::SIGIL_BIND_MUT
                        ),
                        Some(other.span()),
                    ));
                    return None;
                }
            };
    
            let ty = match &lvalue {
                LValue::Local { name, name_span } => {
                    let name_span = *name_span;
                    if let Some(info) = self.flow.uninit.get(name) {
                        let _ = info;
                        self.diags.push(Diagnostic::error(
                            "E0420",
                            format!("`{}` may be read before it is given a value", name),
                            format!(
                                "`{}` was declared with `Type.{{ uninit }}` and has no value yet — `++`/`--` read it first",
                                name
                            ),
                            format!("give `{}` a value with `{} = …` before updating it", name, name),
                            Some(name_span),
                        ));
                    }
                    let Some(info) = self.lookup(name).cloned() else {
                        if self.consts.contains_key(name.as_str()) {
                            self.diags.push(Diagnostic::error(
                                "E0111",
                                format!("`{}` is a const and can never change", name),
                                "a const is fixed for the whole program".to_string(),
                                format!(
                                    "use a `{}` binding if it needs to change",
                                    Syntax::SIGIL_BIND_MUT
                                ),
                                Some(name_span),
                            ));
                        } else {
                            self.unknown_name(name, name_span);
                        }
                        return None;
                    };
                    if !info.mutable {
                        let what = if info.param_conv.is_some() {
                            format!("the parameter `{}` can't be changed here", name)
                        } else {
                            format!(
                                "`{}` was made with `{}`, so it can't change",
                                name,
                                Syntax::SIGIL_BIND_IMMUT
                            )
                        };
                        let fix = if info.param_conv.is_some() {
                            format!(
                                "mark the parameter `{}: {}{}` if the function should change it",
                                name,
                                Syntax::SIGIL_WRITE,
                                info.ty.name()
                            )
                        } else {
                            format!(
                                "declare it with `{} {} ...` instead",
                                name,
                                Syntax::SIGIL_BIND_MUT
                            )
                        };
                        self.diags.push(Diagnostic::error(
                            "E0161",
                            what,
                            format!(
                                "only `{}` bindings (and `{}` parameters) can use `++`/`--`",
                                Syntax::SIGIL_BIND_MUT,
                                Syntax::SIGIL_WRITE
                            ),
                            fix,
                            Some(name_span),
                        ));
                        return None;
                    }
                    if !info.ty.is_integer() {
                        self.diags.push(Diagnostic::error(
                            "E0162",
                            format!("`++`/`--` is not defined for {}", info.ty.show()),
                            "increment and decrement work on integer types only".to_string(),
                            if info.ty.is_float() {
                                format!("use `{} += 1.0` / `{} -= 1.0` instead", name, name)
                            } else {
                                format!(
                                    "use `{} += 1` / `{} -= 1` on an integer binding",
                                    name, name
                                )
                            },
                            Some(span),
                        ));
                        return None;
                    }
                    info.ty.clone()
                }
                LValue::Field {
                    base,
                    field,
                    span: field_span,
                } => {
                    self.borrow_ctx = true;
                    let mut base_expr = base.as_ref().clone();
                    let base_ty = self.infer(&mut base_expr)?;
                    if let Some(root) = expr_root_ident(base) {
                        let root = root.to_string();
                        if let Some(info) = self.lookup(&root) {
                            if !info.mutable {
                                let is_self = root == Syntax::KW_SELF;
                                let what = if is_self {
                                    format!(
                                        "cannot edit `{}` — `{}` has read access only; write access (`&`) is required",
                                        field,
                                        Syntax::KW_SELF
                                    )
                                } else {
                                    format!(
                                        "cannot edit `{}` — `{}` does not have write access (`&`)",
                                        field, root
                                    )
                                };
                                let fix = if is_self {
                                    format!(
                                        "write the receiver as `{}{}` to grant write access",
                                        Syntax::SIGIL_WRITE,
                                        Syntax::KW_SELF
                                    )
                                } else {
                                    format!("declare `{} {} ...`", root, Syntax::SIGIL_BIND_MUT)
                                };
                                self.diags.push(Diagnostic::error(
                                    "E0161",
                                    what,
                                    "increment and decrement edit the field in place".to_string(),
                                    fix,
                                    Some(*field_span),
                                ));
                                return None;
                            }
                        }
                    }
                    // D-FIELDPOL1: `s.computed_field++` — a computed field is never
                    // stored, so `++`/`--` has nothing to edit in place.
                    if self.field_is_computed(&base_ty, field) {
                        self.diags
                            .push(computed_field_not_settable(field, *field_span));
                        return None;
                    }
                    let field_ty = self.field_type(&base_ty, field, *field_span)?;
                    if !field_ty.is_integer() {
                        self.diags.push(Diagnostic::error(
                            "E0162",
                            format!(
                                "`++`/`--` is not defined for field `{}` ({})",
                                field,
                                field_ty.show()
                            ),
                            "increment and decrement work on integer fields only".to_string(),
                            format!("use `self.{field} += 1` / `self.{field} -= 1` instead"),
                            Some(span),
                        ));
                        return None;
                    }
                    field_ty
                }
                LValue::Index { .. } => unreachable!("indexed inc/dec rejected above"),
            };
            Some(ty)
        }
}

const CORE_DIAGNOSTIC_ALIASES: &[(&str, &str)] = &[
    ("fs", "core.files"),
    ("ar", "core.archive"),
    ("gz", "core.compress.gzip"),
    ("re", "core.regex"),
];

fn unique_core_module_for_alias(alias: &str) -> Option<&'static str> {
    for &(known_alias, module) in CORE_DIAGNOSTIC_ALIASES {
        if known_alias == alias && Syntax::KNOWN_CORE_MODULES.contains(&module) {
            return Some(module);
        }
    }

    let mut matches = Syntax::KNOWN_CORE_MODULES
        .iter()
        .copied()
        .filter(|module| module.rsplit('.').next().unwrap_or(module) == alias);
    let module = matches.next()?;
    matches.next().is_none().then_some(module)
}
