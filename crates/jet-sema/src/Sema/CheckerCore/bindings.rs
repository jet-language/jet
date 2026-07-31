use crate::AST::{AccessConvention, BindPattern, Binding, CallArg, Expr, MetaAttr, MetaField, StrPart, Type};
use crate::Diagnostics::{Diagnostic, Severity};
use crate::Sema::Diagnostics::{edit_distance, is_task_type, type_fix_hint};
use crate::Sema::{Checker, LocalInfo};
use crate::Syntax;
use super::helpers::is_pod_uninit_type;

fn direct_fixed_constructor(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::MethodCall { receiver, method, .. }
            if matches!(&**receiver, Expr::Field(_, name, _) if name == "Fixed")
                && matches!(method.as_str(), "new" | "over")
    )
}

fn contains_taskgroup(ty: &Type) -> bool {
    match ty {
        Type::Named(name) => name == Syntax::TYPE_TASKGROUP,
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::FixedList { elem: inner, .. }
        | Type::Tagged { inner, .. } => contains_taskgroup(inner),
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            contains_taskgroup(key) || contains_taskgroup(value)
        }
        // Function signatures have their own direct TaskGroup parameter/return
        // checks; do not duplicate those diagnostics at the binding site.
        Type::Fn { .. } => false,
        Type::Apply { args, .. } | Type::Union(args) => args.iter().any(contains_taskgroup),
        Type::Tuple(fields) => fields.iter().any(|(_, field)| contains_taskgroup(field)),
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::TraitObject(_)
        | Type::IntN { .. }
        | Type::Float32 => false,
    }
}
pub(crate) fn check_meta_attr_fields(meta: &MetaAttr) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut seen_category = false;
    let mut seen_tunable = false;
    let mut seen_maturity = false;
    for field in &meta.fields {
        match field {
            MetaField::Category { value, span } => {
                if seen_category {
                    diags.push(Diagnostic::error(
                        "E0346",
                        "`#Meta` repeats `category`".to_string(),
                        "`category` has one value; writing it twice would make tooling choose between two labels".to_string(),
                        "keep one `category: \"...\"` field".to_string(),
                        Some(*span),
                    ));
                }
                seen_category = true;
                match value {
                    Expr::Str(parts, _) => match parts.as_slice() {
                        [StrPart::Lit(s)] if s.is_empty() => diags.push(Diagnostic::error(
                            "E0348",
                            "`#Meta` category cannot be empty".to_string(),
                            "Canvas groups use this text as a visible label".to_string(),
                            "write a non-empty category, e.g. `category: \"Movement\"`".to_string(),
                            Some(value.span()),
                        )),
                        [StrPart::Lit(_)] => {}
                        _ => diags.push(Diagnostic::error(
                            "E0347",
                            "`#Meta` category needs plain quoted text".to_string(),
                            "`category` is compile-time tooling data, not a runtime string".to_string(),
                            "write a string literal, e.g. `category: \"Movement\"`".to_string(),
                            Some(value.span()),
                        )),
                    },
                    _ => diags.push(Diagnostic::error(
                        "E0347",
                        "`#Meta` category needs plain quoted text".to_string(),
                        "`category` is compile-time tooling data, not a runtime value".to_string(),
                        "write a string literal, e.g. `category: \"Movement\"`".to_string(),
                        Some(value.span()),
                    )),
                }
            }
            MetaField::Tunable { span } => {
                if seen_tunable {
                    diags.push(Diagnostic::error(
                        "E0346",
                        "`#Meta` repeats `tunable`".to_string(),
                        "`tunable` is a flag; writing it twice does not add meaning".to_string(),
                        "keep one `tunable` field".to_string(),
                        Some(*span),
                    ));
                }
                seen_tunable = true;
            }
            MetaField::Maturity { value, span } => {
                if seen_maturity {
                    diags.push(Diagnostic::error(
                        "E0346",
                        "`#Meta` repeats `maturity`".to_string(),
                        "a declaration has one maturity value".to_string(),
                        "keep one `maturity: .Experimental`, `.Tested`, or `.Hardened` field".to_string(),
                        Some(*span),
                    ));
                }
                seen_maturity = true;
                let valid = matches!(value,
                    Expr::EnumLit { type_name, variant, args, .. }
                        if type_name.is_empty()
                            && args.is_empty()
                            && matches!(variant.as_str(),
                                Syntax::ATTR_EXPERIMENTAL
                                    | Syntax::ATTR_TESTED
                                    | Syntax::ATTR_HARDENED));
                if !valid {
                    diags.push(Diagnostic::error(
                        "E0352",
                        "`#Meta` maturity needs a known maturity value".to_string(),
                        "maturity metadata is a closed documentation scale".to_string(),
                        "write `maturity: .Experimental`, `.Tested`, or `.Hardened`".to_string(),
                        Some(value.span()),
                    ));
                }
            }
            MetaField::Unknown { name, span, .. } => {
                let fix = if edit_distance(name, Syntax::META_FIELD_CATEGORY) <= 2 {
                    format!("did you mean `{}`?", Syntax::META_FIELD_CATEGORY)
                } else if edit_distance(name, Syntax::META_FIELD_TUNABLE) <= 2 {
                    format!("did you mean `{}`?", Syntax::META_FIELD_TUNABLE)
                } else if edit_distance(name, Syntax::META_FIELD_MATURITY) <= 2 {
                    format!("did you mean `{}`?", Syntax::META_FIELD_MATURITY)
                } else {
                    format!(
                        "use `{}`, `{}`, or `{}`",
                        Syntax::META_FIELD_CATEGORY,
                        Syntax::META_FIELD_TUNABLE,
                        Syntax::META_FIELD_MATURITY
                    )
                };
                diags.push(Diagnostic::error(
                    "E0345",
                    format!("`#Meta` does not have a `{}` field", name),
                    "`#Meta` fields are owner-ratified tooling metadata".to_string(),
                    fix,
                    Some(*span),
                ));
            }
        }
    }
    diags
}

impl<'a> Checker<'a> {
        pub(crate) fn check_meta_attr(&mut self, meta: &mut MetaAttr) {
            if let Some(mut marker) = self.take_rule_fact(Syntax::ATTR_META, meta.span) {
                let Some(arguments) = self.validate_rule_signature(&mut marker) else {
                    return;
                };
                let mut normalized = Vec::with_capacity(meta.fields.len());
                for (source_index, mut field) in meta.fields.drain(..).enumerate() {
                    let parameter = arguments
                        .bindings
                        .iter()
                        .find(|binding| binding.source_index == source_index)
                        .and_then(|binding| binding.parameter_index);
                    match (
                        parameter,
                        arguments.constant_for_source(source_index),
                        &mut field,
                    ) {
                        (
                            Some(0),
                            Some(crate::Comptime::CtValue::Str(value)),
                            MetaField::Category { value: expression, .. },
                        ) => {
                            let span = expression.span();
                            *expression = Expr::Str(
                                vec![StrPart::Lit(value.clone())],
                                span,
                            );
                            normalized.push(field);
                        }
                        (
                            Some(1),
                            Some(crate::Comptime::CtValue::Bool(true)),
                            _,
                        ) => normalized.push(MetaField::Tunable { span: marker.args[source_index].span() }),
                        (
                            Some(1),
                            Some(crate::Comptime::CtValue::Bool(false)),
                            _,
                        ) => {}
                        _ => normalized.push(field),
                    }
                }
                meta.fields = normalized;
            }
            self.diags.extend(check_meta_attr_fields(meta));
        }

        /// D-UNINIT-SENTINEL2: `name := Type.{ uninit }` — gate on `use core.mem`,
        /// restrict to plain-data types (E0423), declare the binding, and record
        /// it as not-yet-written so the dataflow can prove write-before-read
        /// (E0420). Reuses D-UNINIT1's engine unchanged; only the surface syntax
        /// moved (SENTINEL1 annotated RHS → SENTINEL2 typed-literal body).
        pub(crate) fn check_uninit_binding(&mut self, b: &mut Binding) {
            let has_mem = self
                .core_imports
                .values()
                .any(|m| m == Syntax::CORE_MEM_MODULE);
            if !has_mem {
                self.diags.push(Diagnostic::error(
                    "E0424",
                    format!("`{}` needs the low-level memory tier", Syntax::KW_UNINIT),
                    format!(
                        "`{}` skips the automatic zero-fill — an expert-tier operation",
                        Syntax::KW_UNINIT
                    ),
                    format!(
                        "add `use {}` at the top of this file to opt in",
                        Syntax::CORE_MEM_MODULE
                    ),
                    Some(b.name_span),
                ));
            }
            let (ty, ty_span) = match (&b.ty, b.ty_span) {
                (Some(t), Some(s)) => (self.resolve_type(t.clone()), s),
                _ => return,
            };
            if let Some(slot) = b.ty.as_mut() {
                *slot = ty.clone();
            }
            self.check_declared_type(&ty, ty_span);
            if !is_pod_uninit_type(&ty) {
                self.diags.push(Diagnostic::error(
                    "E0423",
                    format!("`{}` needs a plain-data type", Syntax::KW_UNINIT),
                    format!(
                        "`{}` may own heap memory or need cleanup, so leaving it uninitialized is unsafe",
                        ty.show()
                    ),
                    "use plain data — a number, `Bool`, `Char`, `U8`, or a fixed array of those (e.g. `[4096]U8`)".to_string(),
                    Some(ty_span),
                ));
            }
            let state = match &ty {
                Type::FixedList { len, .. } => super::super::UninitState::fixed(*len),
                _ => super::super::UninitState::scalar(),
            };
            self.declare(
                &b.name,
                b.name_span,
                LocalInfo {
                    def_span: b.name_span,
                    ty,
                    mutable: true,
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
            self.uninit.insert(b.name.clone(), state);
        }
    
        /// A write-convention argument is not a definite-initialization proof:
        /// Jet has no callee contract that guarantees one scalar, or every fixed
        /// slot, was written. Keep the state and reject the unproved handoff.
        pub(crate) fn clear_uninit_mut_args(&mut self, args: &[CallArg]) {
            if self.uninit.is_empty() {
                return;
            }
            for arg in args {
                if arg.convention == AccessConvention::Write {
                    if let Expr::Ident(n, span) = &arg.expr {
                        if self.uninit.contains_key(n) {
                            self.diags.push(Diagnostic::error(
                                "E0420",
                                format!("`{n}` may be read before it is given a value"),
                                format!(
                                    "passing `{n}` with write access does not prove that the callee initializes it"
                                ),
                                format!(
                                    "write every value in `{n}` here before passing it to another function"
                                ),
                                Some(*span),
                            ));
                        }
                    }
                }
            }
        }
    
        pub(crate) fn check_binding(&mut self, b: &mut Binding) {
            if let Some(meta) = &mut b.meta {
                self.check_meta_attr(meta);
            }
            // D-DETACH1: record the binding name so report_unsendable can flag view-capturing tasks.
            let prev_binding_name = self.current_binding_name.take();
            self.current_binding_name = Some(b.name.clone());
            if b.pattern.is_some() {
                self.check_destructuring_binding(b);
                self.current_binding_name = prev_binding_name;
                return;
            }
            if b.uninit {
                self.check_uninit_binding(b);
                self.current_binding_name = prev_binding_name;
                return;
            }
            let mut annot_valid = true;
            let saved_expected = self.expected_type.clone();
            if let (Some(ty), Some(span)) = (&mut b.ty, b.ty_span) {
                let t = self.resolve_type(ty.clone());
                *ty = t.clone();
                self.expected_type = Some(t.clone());
                let before = self.diags.len();
                self.check_declared_type(&t, span);
                if self.diags.len() > before {
                    annot_valid = false;
                }
            }
            if let Expr::Ident(n, nspan) = &mut b.init {
                if let Some(info) = self.lookup(n) {
                    if !info.ty.is_scalar()
                        && matches!(
                            info.param_conv,
                            Some(AccessConvention::Read) | Some(AccessConvention::Write)
                        )
                    {
                        self.diags.push(Diagnostic::error(
                            "E0120",
                            format!("`{}` was not moved here, so it cannot be taken (`^`)", n),
                            "this function has read access only and does not own the value"
                                .to_string(),
                            format!(
                                "copy it instead: `{} {} {}{}`",
                                b.name,
                                if b.mutable {
                                    Syntax::SIGIL_BIND_MUT
                                } else {
                                    Syntax::SIGIL_BIND_IMMUT
                                },
                                Syntax::SIGIL_COPY,
                                n
                            ),
                            Some(*nspan),
                        ));
                    }
                }
            }
            let saved_esc = self.lambda_escapes;
            let saved_bind = self.lambda_binding.clone();
            if matches!(&b.init, Expr::Lambda(_)) {
                self.lambda_escapes = true;
                self.lambda_binding = Some(b.name.clone());
            }
            // D-CTMARKER1=C: `$name` in a comptime binding RHS is valid; set
            // `in_comptime` before `infer()` so E2712 is suppressed during type
            // inference of the RHS (the evaluator runs after, independently).
            if b.is_comptime {
                self.in_comptime = true;
            }
            // D-SHAPE-PLACE1=A: a bare maximal place bound locally is a checked
            // read window, never an implicit move/copy. Decide this before
            // `infer`'s owning-position rewrite (#642).
            if !b.mutable
                && !matches!(b.init, Expr::Copy(..) | Expr::Place(..))
                && self.place_from_expr(&b.init).is_some()
            {
                let span = b.init.span();
                let inner = std::mem::replace(&mut b.init, Expr::Absent(span));
                b.init = Expr::Place(
                    Box::new(inner),
                    crate::AST::PlaceAccess::Read,
                    span,
                );
            }
            let saved_fixed_constructor = self.allow_fixed_constructor;
            self.allow_fixed_constructor = direct_fixed_constructor(&b.init);
            if let Expr::CallValue { callee, .. } = &mut b.init {
                if let Expr::Lambda(lambda) = callee.as_mut() {
                    if lambda.meta.result_loop || lambda.meta.collecting_loop {
                        lambda.meta.loop_label = Some((b.name.clone(), b.name_span));
                    }
                }
            }
            let diagnostics_before_init = self.diags.len();
            let mut it = self.infer(&mut b.init);
            let init_has_error = self.diags[diagnostics_before_init..]
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error);
            self.allow_fixed_constructor = saved_fixed_constructor;
            if let (
                Some(Type::Result { ok, .. }),
                Expr::MethodCall {
                    receiver,
                    method,
                    args,
                    ..
                },
            ) = (&it, &b.init)
            {
                if let Type::Named(type_name) = ok.as_ref() {
                    if self.registry.distinct_range(type_name).is_some()
                        && matches!(receiver.as_ref(), Expr::Ident(name, _) if name == type_name)
                        && Syntax::numeric_conversion_source(method).is_some()
                        && args.len() == 1
                    {
                        self.diags.push(Diagnostic::error(
                            "E0136",
                            format!("making a `{type_name}` from a runtime value can fail"),
                            "only a literal is checked at compile time; a runtime number needs the fallible form so a bad value is handled".to_string(),
                            format!("write `{type_name}.{method}(raw)?` and handle the failure"),
                            Some(args[0].expr.span()),
                        ));
                        it = Some(Type::Named(type_name.clone()));
                    }
                }
            }
            if b.is_comptime {
                self.in_comptime = false;
            }
            self.lambda_escapes = saved_esc;
            self.lambda_binding = saved_bind;
            self.expected_type = saved_expected;
            self.reject_borrowed_param_subplace(
                &b.init,
                it.as_ref(),
                "be stored in a binding",
            );
            if it
                .as_ref()
                .is_some_and(crate::Sema::Diagnostics::contains_expiring_secret_loan)
            {
                self.diags.push(Diagnostic::error(
                    "E0201",
                    "an ExpiringSecret loan cannot be stored".to_string(),
                    "the callback parameter is a temporary read-only loan that ends when `.with` returns".to_string(),
                    "use the loan inside the callback and store only a non-secret result".to_string(),
                    Some(b.init.span()),
                ));
            }
    
            // E2502 (E2-M7): a line stream — `FileReader.lines()` / `StdinHandle
            // .lines()` — is a loop-source-only value. It may only be consumed
            // directly by `loop line; handle.lines()`; binding it to a name lets it
            // escape loop position, where there is no meaningful lowering. (Codegen
            // previously emitted a placeholder that rustc rejected — an I2 hole. This
            // moves the guarantee into sema, c109/I3.)
            if let Some(Type::Named(n)) = &it {
                if n == "FileLines" || n == "StdinLines" || n == "ProcessLines" {
                    self.diags.push(Diagnostic::error(
                        "E2502",
                        "a line stream can only be used directly in a loop".to_string(),
                        "`.lines()` hands back a lazy line reader meant to be iterated in place; storing it in a name would let it leave the loop, where it has no use".to_string(),
                        format!(
                            "iterate it directly: `loop {} in handle.lines() {{ … }}`",
                            if b.name.is_empty() { "line" } else { b.name.as_str() }
                        ),
                        Some(b.init.span()),
                    ));
                }
            }
    
            if let Expr::Lambda(lam) = &b.init {
                if lam.meta.escapes {
                    for name in &lam.meta.mut_captures {
                        self.lambda_mut_borrow_stack
                            .last_mut()
                            .unwrap()
                            .insert(name.clone());
                    }
                    for name in &lam.meta.moved_captures {
                        self.mark_moved(name.clone(), lam.span);
                    }
                }
            }
    
            // `a :: b` moves `b` when the type isn't a scalar (M2 model:
            // assignment moves). Borrowed parameters can't be moved at all.
            if let Expr::Ident(n, nspan) = &b.init {
                if let Some(info) = self.lookup(n) {
                    if !info.ty.is_scalar() {
                        if info.param_conv.is_none() {
                            self.mark_moved(n.clone(), *nspan);
                        }
                    }
                }
            }
    
            let final_ty = match (&b.ty, it) {
                (Some(_), Some(actual)) if !annot_valid => actual,
                (Some(annot), Some(actual)) => {
                    let annot = self.resolve_type(annot.clone());
                    let mut actual = self.resolve_type(actual.clone());
                    let preserve_clock_provenance = crate::Sema::Diagnostics::is_clock_type(&annot)
                        && (crate::Sema::Diagnostics::is_deterministic_clock_type(&actual)
                            || crate::Sema::Diagnostics::is_system_clock_type(&actual));
                    if annot != actual
                        && self.implicitly_convert_unit(&mut b.init, &annot, &actual)
                    {
                        actual = annot.clone();
                    }
                    // D-SG9: a fixed-width literal is range-checked and re-typed in
                    // `infer` (E1003), so it arrives matching `annot`. A non-literal
                    // width mismatch falls to E0108 below — no implicit narrowing or
                    // widening between integer widths.
                    if annot != actual {
                        // D-DIST1/D-DIST3 (E0128): distinct-type coercion is never implicit.
                        let distinct_name = if let Type::Named(n) = &annot {
                            if self.registry.is_distinct(n) {
                                Some(n.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        if let Some(dt) = distinct_name {
                            self.diags.push(Diagnostic::error(
                                "E0128",
                                format!("a `{}` can't be used where a `{}` is expected", actual.name(), dt),
                                format!("`{}` and `{}` are different types — even though `{}` is built on `{}`, one is never accepted in place of the other", dt, actual.name(), dt, self.registry.distinct_base(&dt).map(|t| t.name()).unwrap_or_default()),
                                format!("construct a `{}`: `{}({})`", dt, dt, "expr"),
                                Some(b.init.span()),
                            ));
                        } else if let Some(diag) = crate::Sema::Diagnostics::typed_text_mismatch(
                            &annot,
                            &actual,
                            b.init.span(),
                        ) {
                            self.diags.push(diag);
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0108",
                                format!(
                                    "`{}` says it holds {}, but the value is {}",
                                    b.name,
                                    annot.show(),
                                    actual.show()
                                ),
                                "the type written after `:` must match the value".to_string(),
                                type_fix_hint(&annot, &actual),
                                Some(b.init.span()),
                            ));
                        }
                    }
                    if preserve_clock_provenance {
                        actual
                    } else {
                        annot
                    }
                }
                (Some(annot), None) => self.resolve_type(annot.clone()),
                (None, Some(actual)) => actual,
                (None, None) => Type::Int, // an error was already reported
            };
            if contains_taskgroup(&final_ty)
                && !matches!(&final_ty, Type::Named(name) if name == Syntax::TYPE_TASKGROUP)
            {
                self.diags.push(Diagnostic::error(
                    "E1110",
                    "`TaskGroup` cannot be stored inside another value".to_string(),
                    "a taskgroup is call-stack-only spawn authority; aliases and aggregates could outlive its owner"
                        .to_string(),
                    "pass `group: TaskGroup` directly to a named helper and use it there"
                        .to_string(),
                    Some(b.name_span),
                ));
            }
            if b.ty.is_none() {
                b.ty = Some(final_ty.clone());
            }
            self.report_lending_view_escape(&b.init, "be stored in a binding");
            // D-DECIMAL1: default-on float-money lint for money-like binding names.
            if final_ty.is_float() && crate::Numeric::is_money_like_name(&b.name) {
                self.diags.push(Diagnostic::lint(
                    "L0504",
                    format!("binding `{}` looks like money but has type `Float`", b.name),
                    "floating-point money loses cents on common values like `0.1 + 0.2`".to_string(),
                    "use `Decimal` for exact money: `price: Decimal := Decimal(\"9.99\")`".to_string(),
                    Some(b.name_span),
                ));
            }
            if b.is_comptime
                && !crate::Comptime::check_build_time_io(&b.init, self.ct_base_dir, &mut self.diags)
            {
                // D-CTIO1: the path law already reported against the call.
            } else if b.is_comptime {
                let globals = self.current_ct_globals();
                // D-CTCORE1: pass core_imports so the interpreter can evaluate
                // whitelisted pure Core calls (e.g. `math.sqrt(x)`).
                // D-CTEFFECT1: pass impure context so bindings inside #Impure blocks
                // start with the gate already open.
                match crate::Comptime::evaluate_owned_with_imports_opts_collecting(
                    &b.init,
                    self.ct_funcs,
                    self.ct_externs,
                    self.ct_base_dir,
                    &globals,
                    self.core_imports,
                    self.allow_impure && self.ct_impure_depth > 0,
                    self.ct_impure_depth,
                ) {
                    Ok((v, inputs)) => {
                        b.ct = Some(v.clone());
                        self.ct_scopes.last_mut().unwrap().insert(b.name.clone(), v);
                        // D-CTEFFECT1 Tier-1: accumulate embed inputs for .jet/lock.
                        self.ct_embed_inputs.extend(inputs);
                    }
                    Err(d) => self.diags.push(d),
                }
            } else if !b.mutable {
                // D-VERDICT-1308-1: an ordinary immutable binding is an
                // implicit folding opportunity. Failure is silent; only
                // explicit `#Known` demands a compile-time answer.
                let globals = self.current_ct_globals();
                if let Ok((v, _)) =
                    crate::Comptime::evaluate_owned_with_imports_opts_collecting(
                        &b.init,
                        self.ct_funcs,
                        self.ct_externs,
                        self.ct_base_dir,
                        &globals,
                        self.core_imports,
                        false,
                        0,
                    )
                {
                    b.ct = Some(v.clone());
                    self.ct_scopes.last_mut().unwrap().insert(b.name.clone(), v);
                }
            }
            if b.name == "_" {
                if self.type_is_single_use(&final_ty) {
                    self.diags.push(Diagnostic::error(
                        "E0140",
                        "a `#SingleUse` value cannot be discarded".to_string(),
                        "this value carries one job that must be completed exactly once"
                            .to_string(),
                        "bind it to a name, then move it exactly once to the operation that completes its job"
                            .to_string(),
                        Some(b.name_span),
                    ));
                } else if is_task_type(&final_ty) && !self.in_taskgroup_spawn {
                    self.diags.push(Diagnostic::lint(
                        "L1101",
                        "a spawned task is discarded without `.join()`".to_string(),
                        "the program may end before this task finishes".to_string(),
                        "bind the task and call `.join()`, or chain `.detach()` for fire-and-forget"
                            .to_string(),
                        Some(b.name_span),
                    ));
                }
                self.current_binding_name = prev_binding_name;
                return;
            }
            let binding_sendable = if let Expr::Lambda(lam) = &b.init {
                self.lambda_value_sendable(lam, &final_ty)
            } else {
                self.sendability_problem(&final_ty, true).is_none()
            };
            let task_lint_span = if is_task_type(&final_ty) && !self.in_taskgroup_spawn {
                Some(b.name_span)
            } else {
                None
            };
            // D-LIN1: a binding that owns a `#SingleUse` value carries the duty to
            // consume it exactly once. The duty transfers on `y :: x` (the move marks
            // `x` consumed via `note_move_if_direct_ident`; `y` now owns it).
            let single_use_span = if self.type_is_single_use(&final_ty) {
                Some(b.name_span)
            } else {
                None
            };
            // D-ALLOC2: `x :: arena.alloc(v)` makes `x` a scope-bound view into
            // `arena`. Record it so E0631 (escape) / E0632 (use-after-reset) can
            // fire, and flag the binding for codegen (it lowers to a `&mut T`, read
            // through a deref). E0631: a binding whose *initializer is itself a view
            // name* (`y :: x`) would move the view to a new — possibly
            // longer-lived — binding; reject it (views are non-reassignable
            // non-escaping locals, I8).
            if let Some(arena) = self.arena_alloc_source(&b.init) {
                b.arena_view = true;
                self.record_arena_view(&b.name, arena, b.name_span);
            } else if let Expr::Ident(src, src_span) = &b.init {
                if self.is_arena_view(src) || self.is_fixed_backing_view(src) {
                    self.report_view_escape(src, "be stored in another binding", *src_span);
                }
            }
            if matches!(&final_ty, Type::Named(name) if name == "Fixed") {
                if let Some(owner) = self.fixed_backing_source(&b.init) {
                    self.record_fixed_backing(&b.name, owner, b.name_span);
                }
            }
            let direct_mutable_view =
                matches!(&final_ty, Type::Apply { name, .. } if name == "ViewMut");
            let transferred_view_root = direct_mutable_view
                .then(|| self.mutable_view_aggregate_root(&b.init))
                .flatten();
            // D-SHAPE-PLACE1: `x :: list[a..b]` makes `x` a scope-bound window
            // into `list`. E2305 fires the same way E0631 does for arena views —
            // rebinding a live view to a second name is itself an escape (views
            // are non-reassignable non-escaping locals, I8), not re-tracked.
            for (output_path, place, kind, access) in self.view_call_sources(&b.init) {
                let access = if direct_mutable_view && output_path.is_empty() {
                    crate::Sema::ViewAccess::Write
                } else {
                    access
                };
                self.record_list_view(
                    &b.name,
                    output_path,
                    place,
                    kind,
                    access,
                    b.name_span,
                    &final_ty,
                    transferred_view_root.as_deref(),
                );
            }
            self.finish_mutable_view_aggregate_transfer(
                transferred_view_root.as_deref(),
                b.init.span(),
            );
            if let Expr::Ident(src, src_span) = &b.init {
                let _ = src_span;
                self.transfer_named_view(&b.name, src, b.name_span);
            }
            // D-MEM1 stage S5: `x :: s.trim()` / `x :: s.after(sep)` / `x ::
            // s.before(sep)` makes `x` a scope-bound string view into `s` — the
            // same E2305 reasoning as `View<T>`, reported as E2307 since `String`
            // has no distinct view type for codegen to key off (`b.string_view`
            // flags the binding itself instead). Immutable (`::`) only, matching
            // "views are non-reassignable non-escaping locals" (I8) — a `:=`
            // binding can be reassigned to an ordinary owned `String` later, and
            // codegen's `&str` place has nowhere to put that; fall back to the
            // ordinary eager/owned lowering for `:=` (unchanged pre-S5 behavior).
            if !b.mutable {
                if let Some(owner) = self.string_view_call_source(&b.init) {
                    b.string_view = true;
                    self.record_string_view(&b.name, owner, b.name_span);
                }
            }
            // No dedicated "rebound to another binding" check here — the general
            // E2307 check on `Expr::Ident` reads (this binding's init was already
            // inferred above) already caught `y :: d` for a live view `d`; a
            // second check here would double-report the same span.
            let concrete_unit_value = (!b.mutable)
                .then(|| self.concrete_unit_value(&b.init))
                .flatten();
            let constant_value = (!b.mutable
                && !init_has_error
                && !matches!(&final_ty, Type::Named(name) if name == "Fixed"))
                .then(|| self.evaluate_constant(&b.init))
                .flatten();
            self.declare(
                &b.name,
                b.name_span,
                LocalInfo {
                    def_span: b.name_span,
                    ty: final_ty.clone(),
                    mutable: b.mutable && !b.is_comptime,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                    sendable: binding_sendable,
                    reactive_local: b.reactive_local,
                    reactive_shared: b.reactive_shared,
                    task_lint_span,
                    single_use_span,
                    constant_value,
                },
            );
            if b.reactive_shared && crate::Sema::CheckerInfer::is_reactive_handle_ty(&final_ty) {
                self.note_reactive_upgrade(&b.name, &final_ty, "#Shared pin");
                b.reactive_upgrade = true;
            }
            if let Some(value) = concrete_unit_value {
                self.concrete_unit_values
                    .last_mut()
                    .expect("binding scope exists")
                    .insert(b.name.clone(), value);
            }
            self.current_binding_name = prev_binding_name;
        }
    
        /// S74: a `val`/`var` binding that destructures a struct (`Point { x, y }`)
        /// or a list (`[a, b]`). Each bound name is declared separately; move and
        /// mutability follow the per-name M2 rules. Struct destructuring is
        /// irrefutable (you may bind any subset of fields); list destructuring is
        /// guarded by a runtime length check in codegen, and a literal of the wrong
        /// length is caught here (E0315).
        pub(crate) fn check_destructuring_binding(&mut self, b: &mut Binding) {
            let inferred = self.infer(&mut b.init);
            let pattern = b
                .pattern
                .clone()
                .expect("destructuring binding has a pattern");
            let Some(it) = inferred else {
                // The initializer itself didn't type-check; declare error
                // placeholders so the bound names don't cascade into E0107.
                for n in pattern.names() {
                    self.declare_bound(n.local_name(), n.span, Type::Int, b.mutable);
                }
                return;
            };
            let it = self.resolve_type(it);
            match &pattern {
                BindPattern::Struct {
                    type_name,
                    type_span,
                    fields,
                    rest,
                    span: pat_span,
                } => {
                    let actual = match &it {
                        Type::Named(n) => Some(n.clone()),
                        Type::Apply { name, .. } => Some(name.clone()),
                        _ => None,
                    };
                    let is_struct = actual.as_deref().is_some_and(|n| {
                        self.struct_owner_module(n, None)
                            .and_then(|m| self.struct_fields_of(m, n))
                            .is_some()
                    });
                    if !is_struct {
                        self.diags.push(Diagnostic::error(
                            "E0313",
                            format!(
                                "`{} {{ … }}` can only destructure a `{}` value, but this is {}",
                                type_name,
                                type_name,
                                it.show()
                            ),
                            "destructuring with `{ }` pulls fields out of a struct value".to_string(),
                            format!(
                                "destructure a `{}`, or bind the whole value with a name",
                                type_name
                            ),
                            Some(*type_span),
                        ));
                        for n in pattern.names() {
                            self.declare_bound(n.local_name(), n.span, Type::Int, b.mutable);
                        }
                        return;
                    }
                    let actual = actual.unwrap();
                    if actual != *type_name {
                        self.diags.push(Diagnostic::error(
                            "E0313",
                            format!("this value is a `{}`, not a `{}`", actual, type_name),
                            "the type named before `{ }` must match the value you destructure"
                                .to_string(),
                            format!("write `{} {{ … }}` to match the value", actual),
                            Some(*type_span),
                        ));
                        for n in pattern.names() {
                            self.declare_bound(n.local_name(), n.span, Type::Int, b.mutable);
                        }
                        return;
                    }
                    for f in fields {
                        // `field_type` resolves the field's type and reports E0302
                        // with a suggestion if the field name is unknown.
                        let fty = self.field_type(&it, &f.name, f.span).unwrap_or(Type::Int);
                        self.declare_bound(f.local_name(), f.span, fty, b.mutable);
                    }
                    // D-DESTRUCT1: `..` is mandatory whenever the pattern doesn't
                    // name every field, and redundant when it already does.
                    let total_fields = self
                        .struct_owner_module(&actual, None)
                        .and_then(|m| self.struct_fields_of(m, &actual))
                        .map(|fs| fs.len());
                    if let Some(total) = total_fields {
                        let partial = fields.len() < total;
                        if partial && rest.is_none() {
                            self.diags.push(Diagnostic::error(
                                "E0326",
                                format!("this pattern leaves out fields of `{}`", type_name),
                                "a destructure that doesn't name every field must end with `..` so the skipped fields are visible at a glance".to_string(),
                                "add `, ..` before the closing `}`, or name the remaining fields".to_string(),
                                Some(*pat_span),
                            ));
                        } else if !partial {
                            if let Some(rest_span) = rest {
                                self.diags.push(Diagnostic::error(
                                    "E0327",
                                    format!("`..` is redundant — this pattern already names every field of `{}`", type_name),
                                    "a trailing `..` only makes sense when some fields are left unnamed".to_string(),
                                    "remove the `..`".to_string(),
                                    Some(*rest_span),
                                ));
                            }
                        }
                    }
                }
                BindPattern::List { elems, span } => {
                    let (elem_ty, fixed_len) = match &it {
                        Type::List(inner) => ((**inner).clone(), None),
                        // S76: [T#N] can be destructured; E0963 if count doesn't match.
                        Type::FixedList { elem, len, .. } => ((**elem).clone(), Some(*len)),
                        _ => {
                            self.diags.push(Diagnostic::error(
                                "E0313",
                                format!(
                                    "`[ … ]` can only destructure a list, but this is {}",
                                    it.show()
                                ),
                                "destructuring with `[ ]` pulls elements out of a list value"
                                    .to_string(),
                                "destructure a list, or bind the whole value with a name".to_string(),
                                Some(*span),
                            ));
                            for n in pattern.names() {
                                self.declare_bound(n.local_name(), n.span, Type::Int, b.mutable);
                            }
                            return;
                        }
                    };
                    // E0963: destructure count must match the fixed-size length.
                    if let Some(fixed) = fixed_len {
                        if elems.len() as u64 != fixed {
                            self.diags.push(Diagnostic::error(
                                "E0963",
                                format!(
                                    "destructuring with {} name{}, but this fixed-size list has {} element{}",
                                    elems.len(),
                                    if elems.len() == 1 { "" } else { "s" },
                                    fixed,
                                    if fixed == 1 { "" } else { "s" }
                                ),
                                "a fixed-size list `[T#N]` has a known length — the pattern must match exactly".to_string(),
                                format!(
                                    "use {} name{} in the pattern",
                                    fixed,
                                    if fixed == 1 { "" } else { "s" }
                                ),
                                Some(*span),
                            ));
                        }
                    }
                    // A list literal has a known length: a mismatch is a compile
                    // error rather than a runtime length failure.
                    if let Expr::ListLit(items, _) = &b.init {
                        if items.len() != elems.len() {
                            self.diags.push(Diagnostic::error(
                                "E0315",
                                format!(
                                    "this pattern binds {} item{}, but the list has {}",
                                    elems.len(),
                                    if elems.len() == 1 { "" } else { "s" },
                                    items.len()
                                ),
                                "a list pattern must name exactly as many items as the list holds"
                                    .to_string(),
                                format!(
                                    "name {} item{} to match the list",
                                    items.len(),
                                    if items.len() == 1 { "" } else { "s" }
                                ),
                                Some(*span),
                            ));
                        }
                    }
                    for e in elems {
                        self.declare_bound(&e.name, e.span, elem_ty.clone(), b.mutable);
                    }
                }
                BindPattern::Tuple { elems, span } => {
                    let Type::Tuple(fields) = &it else {
                        self.diags.push(Diagnostic::error(
                            "E0313",
                            format!(
                                "`( … )` can only destructure a tuple, but this is {}",
                                it.show()
                            ),
                            "destructuring with `( )` pulls named members out of a tuple value"
                                .to_string(),
                            "destructure a tuple, or bind the whole value with a name".to_string(),
                            Some(*span),
                        ));
                        for n in pattern.names() {
                            self.declare_bound(n.local_name(), n.span, Type::Int, b.mutable);
                        }
                        return;
                    };
                    if elems.len() != fields.len() {
                        self.diags.push(Diagnostic::error(
                            "E0315",
                            format!(
                                "this pattern binds {} member{}, but the tuple has {}",
                                elems.len(),
                                if elems.len() == 1 { "" } else { "s" },
                                fields.len()
                            ),
                            "a tuple pattern must name exactly as many members as the tuple holds"
                                .to_string(),
                            format!(
                                "name {} member{} to match the tuple",
                                fields.len(),
                                if fields.len() == 1 { "" } else { "s" }
                            ),
                            Some(*span),
                        ));
                    } else if let Expr::TupleLit(items, _, _) = &b.init {
                        if items.len() != elems.len() {
                            self.diags.push(Diagnostic::error(
                                "E0315",
                                format!(
                                    "this pattern binds {} member{}, but the tuple literal has {}",
                                    elems.len(),
                                    if elems.len() == 1 { "" } else { "s" },
                                    items.len()
                                ),
                                "a tuple pattern must name exactly as many members as the literal holds"
                                    .to_string(),
                                format!(
                                    "name {} member{} to match the tuple",
                                    items.len(),
                                    if items.len() == 1 { "" } else { "s" }
                                ),
                                Some(*span),
                            ));
                        }
                    }
                    for (e, (_, fty)) in elems.iter().zip(fields.iter()) {
                        self.declare_bound(&e.name, e.span, (**fty).clone(), b.mutable);
                    }
                }
            }
            // Move the initializer when it's an owned, non-scalar local (M2): the
            // whole value is consumed to produce the bound parts.
            if let Expr::Ident(n, nspan) = &b.init {
                if let Some(info) = self.lookup(n) {
                    if !info.ty.is_scalar() && info.param_conv.is_none() {
                        self.mark_moved(n.clone(), *nspan);
                    }
                }
            }
        }
    
}
