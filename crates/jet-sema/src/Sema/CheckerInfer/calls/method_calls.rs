use crate::Collections;
use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Generics::e0901;
use crate::Sema::Checker;
use crate::Sema::{json_ty, result_ty};
use crate::Sema::CheckerCoreLib::{
    data_nominal_leaf,
    alloc_method_return, app_method_return, args_spec_method_return, binary_reader_method_return,
    civil_time_method_contract, civil_time_method_return, core_generic_struct_field,
    data_renamed_to_dyn, dyn_method_return, db_value_method_return, decode_error_ty,
    devserver_method_return, email_method_return, encoding_handle_method_return,
    file_handle_method_return, http_nominal_leaf, http_type_method_return,
    is_allocator_type, is_db_value_type_name, is_geometry_type, is_json_type_name, is_layout_axis_type,
    is_layout_type, is_math_type,
    is_reflect_type_name, is_simd_lane_type, job_queue_method_return,
    layout_method_arg_ty, layout_method_return, loadable_method_return, math_method_arg_ty,
    math_method_return,
    math_scalar_ty, math_static_arg_ty, math_static_return, net_method_return,
    parsed_args_method_return, path_method_return, process_child_method_return,
    process_spec_method_return, process_stdin_method_return, process_stream_method_return,
    reflect_method_return, regex_method_return, require_exact_labels, require_net_method_labels,
    simd_reduce_markers, terminal_session_method_return,
    text_cursor_method_return, u8_ty, ui_backend_method_return, unit_ty,
    web_method_return, wrong_core_arity, geometry_is_delta, geometry_is_point, geometry_method_return,
    geometry_space, geometry_static_arg_types, geometry_static_return_with_owner,
    geometry_transform_diagnostic,
};
use crate::Sema::CheckerInfer::{exact_integer_literal, IntegerInterval};
use crate::Sema::Diagnostics::{
    builtin_type_from_ident, expr_root_ident, is_debuggable, is_printable,
    suggest_method_for_receiver, type_is_copy, MethodSuggestion,
};


use crate::Sema::Effects::Effect;
use crate::Syntax;
use crate::AST::{
    AccessConvention, Call, CallArg, CallArgFlags, CtValue, EnumLitArg, Expr, FuncSig, LambdaBody,
    StrPart, Type,
};

fn http_route_find_func_in_items<'a>(
    items: &'a [crate::AST::Item],
    name: &str,
) -> Option<&'a crate::AST::Func> {
    for item in items {
        match item {
            crate::AST::Item::Func(function) if function.name == name => return Some(function),
            crate::AST::Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    if let Some(function) = http_route_find_func_in_items(body, name) {
                        return Some(function);
                    }
                }
            }
            _ => {}
        }
    }
    None
}
use std::collections::{HashMap, HashSet};


#[derive(Debug, Clone)]
struct RootCallTarget {
    module_idx: Option<usize>,
    alias: Option<String>,
    core_module: Option<String>,
    name: String,
}


impl<'a> Checker<'a> {
    fn record_static_time_effect(&mut self, type_name: &str, method: &str, span: Span) {
        let api = match (type_name, method) {
            (crate::Syntax::CLOCK_TYPE, "system") => Some("Clock.system"),
            (crate::Syntax::CLOCK_TYPE, "now") => Some("Clock.now"),
            ("Date", "today") => Some("Date.today"),
            _ => None,
        };
        let Some(api) = api else { return };
        self.record_effect(Effect::Time.name(), span);
        if self.in_pure && self.det_suppress == 0 {
            self.diags.push(crate::Sema::e3403(api, Some(span)));
        }
    }


    fn method_suggestion(
        &self,
        method: &str,
        receiver_ty: &Type,
        type_name: Option<&str>,
    ) -> Option<MethodSuggestion> {
        let mut candidates = Collections::builtin_method_names(receiver_ty);
        let receiver_type_name = match receiver_ty {
            Type::Named(name) | Type::Apply { name, .. } => Some(name.as_str()),
            _ => None,
        };
        if let Some(type_name) = type_name.or(receiver_type_name) {
            candidates.extend(
                self.method_names_for_type_name(type_name)
                    .into_iter()
                    .filter(|candidate| {
                        self.resolve_method_sig(type_name, candidate)
                            .is_some_and(|(_, signature)| !signature.is_static)
                    }),
            );
        }
        candidates.sort_unstable();
        candidates.dedup();
        let receiver_family = Self::receiver_method_label(receiver_ty);
        suggest_method_for_receiver(method, Some(&receiver_family), &candidates)
    }
    /// Return the arity of a reserved cursor method when its call has reached
    /// the generic fallback. The Core return tables key on `(method, arity)`,
    /// so a wrong-arity call would otherwise look like an unknown method and
    /// suggest an illegal `impl Cursor`/`impl Reader`.
    fn protected_cursor_method_arity(type_name: &str, method: &str) -> Option<usize> {
        (0..=8).find(|arity| match type_name {
            "Reader" => binary_reader_method_return(type_name, method, *arity).is_some(),
            "Cursor" => text_cursor_method_return(type_name, method, *arity).is_some(),
            _ => false,
        })
    }


    fn receiver_method_label(receiver_ty: &Type) -> String {
        match receiver_ty {
            Type::Tagged { inner, .. } => Self::receiver_method_label(inner),
            Type::List(_) | Type::FixedList { .. } => "List".to_string(),
            Type::Map { .. } => "Map".to_string(),
            Type::String => "String".to_string(),
            Type::Named(name) => name.clone(),
            Type::Apply { name, .. } => name.clone(),
            _ => receiver_ty.name(),
        }
    }

    fn missing_receiver_method_why(receiver_ty: &Type, method: &str) -> String {
        format!(
            "this `{}` value has no instance method named `{method}`",
            Self::receiver_method_label(receiver_ty)
        )
    }

    fn builtin_method_arity_diagnostic(
        receiver_ty: &Type,
        method: &str,
        actual: usize,
        span: Span,
    ) -> Option<Diagnostic> {
        // Read the same arity-keyed table as successful method resolution.
        let mut first = None;
        let mut last = 0;
        for arity in 0..=8 {
            if Collections::builtin_method_return(receiver_ty, method, arity, false).is_some() {
                if arity == actual {
                    return None;
                }
                first.get_or_insert(arity);
                last = arity;
            }
        }
        let first = first?;
        let expected = if first == last {
            format!("{first} argument{}", if first == 1 { "" } else { "s" })
        } else {
            format!("{first} to {last} arguments")
        };
        let label = Self::receiver_method_label(receiver_ty);
        Some(Diagnostic::error(
            "E0104",
            format!("`{label}.{method}` expects {expected}, got {actual}"),
            format!("`{label}` has a `{method}` method, but no form of it takes {actual} arguments"),
            format!("Call `.{method}(…)` with {expected}."),
            Some(span),
        ))
    }

    fn missing_instance_method_diagnostic(
        &self,
        method: &str,
        receiver_ty: &Type,
        type_name: Option<&str>,
        why: String,
        span: Span,
    ) -> Diagnostic {
        let suggestion = self.method_suggestion(method, receiver_ty, type_name);
        let fix = suggestion.as_ref().map_or_else(
            || format!("check the spelling of `{method}`"),
            |suggestion| suggestion.fix.clone(),
        );
        let mut diagnostic = Diagnostic::error(
            "E0311",
            format!("`{method}` isn't a method on this value"),
            why,
            fix,
            Some(span),
        );
        if let Some(suggestion) = suggestion {
            diagnostic = diagnostic.with_edit(TextEdit {
                span,
                new_text: suggestion.name,
            });
        }
        diagnostic
    }

    /// D-FAIL-CARRIER1=A: a fact an error type carries on its report.
    ///
    /// An error type opts into a middle state by carrying it on its report:
    /// the surviving payload under `partial`, the notes it collected under
    /// `notes`. An error type that declined gets the ordinary "no field"
    /// report, so nothing new is taught.
    fn carrier_report_fact(
        &mut self,
        err: &Type,
        field: &str,
        why: &str,
        span: Span,
    ) -> Option<Type> {
        let name = match err {
            Type::Named(name) => name.as_str(),
            Type::Apply { name, .. } => name.as_str(),
            _ => return None,
        };
        let (import_ns, leaf) = Self::split_type_name(name);
        let owner = self.struct_owner_module(leaf, import_ns)?;
        let fields = self.struct_fields_of(owner, leaf)?;
        if let Some((_, _, ty)) = fields.iter().find(|(known, ..)| known == field) {
            return Some(ty.clone());
        }
        self.diags.push(Diagnostic::error(
            "E0302",
            format!("`{}` has no field `{}`", name, field),
            why.to_string(),
            format!("add a `{}` field to `{}`", field, name),
            Some(span),
        ));
        None
    }

    fn plot_column_value_supported(ty: &Type) -> bool {
        match ty.without_user_tags() {
            Type::Int | Type::IntN { .. } | Type::Float | Type::Float32 | Type::Bool | Type::String => {
                true
            }
            Type::InlineRange { base, .. } => Self::plot_column_value_supported(base),
            _ => false,
        }
    }

    // D-DX-PLOT1=A: contextual leading-dot plot selectors remain the original
    // `EnumLit` so TIR can distinguish them from ordinary values. Sema validates
    // the selector against the receiver's row schema and records normal field
    // reference/visibility facts through `field_type`.
    fn check_plot_column_selector(&mut self, row: &Type, arg: &mut CallArg) -> bool {
        let Some((field, selector_span, has_payload)) = (match arg.expr.without_parens() {
            Expr::EnumLit {
                type_name,
                variant,
                args,
                leading_dot: true,
                span,
                ..
            } if type_name.is_empty() => Some((variant.clone(), *span, !args.is_empty())),
            _ => None,
        }) else {
            return false;
        };

        if has_payload {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("plot selector `.{field}` cannot carry payload arguments"),
                "a leading-dot plot selector names one row field and does not construct an enum variant"
                    .to_string(),
                format!("write `.{field}` without parentheses"),
                Some(selector_span),
            ));
            return true;
        }

        if arg.convention == AccessConvention::Move {
            self.diags.push(Diagnostic::error(
                "E0203",
                "a value was passed with the move marker `^` to a parameter that does not consume"
                    .to_string(),
                "plot selectors are compiler-owned field projections; the plot builder reads their row field"
                    .to_string(),
                "remove the move marker `^` here".to_string(),
                Some(arg.span),
            ));
        }

        let mut row_type = row;
        while let Type::Tagged { inner, .. } = row_type {
            row_type = inner.as_ref();
        }
        let type_name = match row_type {
            Type::Named(name) | Type::Apply { name, .. } => name,
            _ => {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "plot selector `.{field}` needs a declared row schema, but the plot row is `{}`",
                        row_type.show()
                    ),
                    "contextual plot selectors are checked against named row fields before lowering"
                        .to_string(),
                    "use a declared row struct, or pass an explicit JetDataPlotColumn value"
                        .to_string(),
                    Some(selector_span),
                ));
                return true;
            }
        };
        if self.struct_fields_for_type_name(type_name).is_none() {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!(
                    "plot selector `.{field}` needs a declared row schema, but `{type_name}` has no fields"
                ),
                "contextual plot selectors are checked against named row fields before lowering"
                    .to_string(),
                "use a declared row struct, or pass an explicit JetDataPlotColumn value"
                    .to_string(),
                Some(selector_span),
            ));
            return true;
        }

        // This is the same checked field path used by ordinary `row.field`
        // reads: it enforces foreign-field visibility, records the exact
        // owner/field reference, and substitutes generic row parameters.
        if let Some(field_ty) = self.field_type(row_type, &field, selector_span) {
            if !Self::plot_column_value_supported(&field_ty) {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "plot selector `.{field}` needs a scalar field, but `{}` is not plot-compatible",
                        field_ty.show()
                    ),
                    "plot columns carry integer, floating-point, boolean, and string values"
                        .to_string(),
                    "select a scalar field or pass an explicit JetDataPlotColumn value".to_string(),
                    Some(selector_span),
                ));
            }
        }
        true
    }

    // D-DX-FORM1=A: a leading-dot field selector is the stable form
    // validation API. The typed-form runtime checks the field name against
    // its derived schema; sema lowers the selector to the ABI string.
    fn check_web_form_field_selector(&mut self, arg: &mut CallArg) -> bool {
        let Some((field, selector_span, has_payload)) = (match arg.expr.without_parens() {
            Expr::EnumLit {
                type_name,
                variant,
                args,
                leading_dot: true,
                span,
                ..
            } if type_name.is_empty() => Some((variant.clone(), *span, !args.is_empty())),
            _ => None,
        }) else {
            return false;
        };

        if has_payload {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("form field selector `.{field}` cannot carry payload arguments"),
                "a leading-dot form selector names one field and does not construct an enum variant"
                    .to_string(),
                format!("write `.{field}` without parentheses"),
                Some(selector_span),
            ));
            return true;
        }
        if arg.convention == AccessConvention::Move {
            self.diags.push(Diagnostic::error(
                "E0203",
                "a value was passed with the move marker `^` to a parameter that does not consume"
                    .to_string(),
                "form field selectors are compiler-owned names; validation reads the field"
                    .to_string(),
                "remove the move marker `^` here".to_string(),
                Some(arg.span),
            ));
        }
        arg.expr = Expr::Str(vec![StrPart::Lit(field)], selector_span);
        true
    }

    fn root_param_accepts(
        sig: &FuncSig,
        fn_params: &[crate::AST::TypeParam],
        receiver_ty: &Type,
    ) -> bool {
        let Some((convention, param_ty)) = sig.params.first() else {
            return false;
        };
        if *convention != AccessConvention::Read {
            return false;
        }
        param_ty == receiver_ty
            || receiver_ty.numeric_widening_to(param_ty).is_some()
            || matches!(
                param_ty,
                Type::Named(name) if fn_params.iter().any(|param| param.name == *name)
            )
    }

    fn root_call_candidates(&self, method: &str, receiver_ty: &Type) -> Vec<RootCallTarget> {
        let mut candidates = Vec::new();
        if let Some(sig) = self.funcs.get(method) {
            let fn_params = self
                .trait_reg
                .fn_params
                .get(method)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if sig.root_param && Self::root_param_accepts(sig, fn_params, receiver_ty) {
                candidates.push(RootCallTarget {
                    module_idx: None,
                    alias: None,
                    core_module: None,
                    name: method.to_string(),
                });
            }
        }

        // D-CALLDUAL1=E: ambient `print` is the one prelude free function that
        // earns the receiver-first spelling without a user declaration. An
        // explicit `core.term` import also makes the spelling available in a
        // `#NoPrelude` file. A user declaration named `print` shadows the
        // ambient prelude, matching ordinary direct-call resolution.
        if method == Syntax::BUILTIN_PRINT
            && !self.funcs.contains_key(method)
            && (!self.no_prelude
                || self
                    .core_imports
                    .values()
                    .any(|module| module == "core.term"))
            && (is_printable(receiver_ty, self.registry, self.trait_reg)
                || self.is_unit_type(receiver_ty))
        {
            candidates.push(RootCallTarget {
                module_idx: None,
                alias: None,
                core_module: Some("core.term".to_string()),
                name: method.to_string(),
            });
        }

        let Some(modules) = self.modules else {
            return candidates;
        };
        let mut imports: Vec<(String, usize)> = self
            .imports
            .iter()
            .map(|(alias, index)| (alias.clone(), *index))
            .collect();
        imports.sort_by(|left, right| left.cmp(right));
        let mut seen_modules = HashSet::new();
        for (alias, module_idx) in imports {
            if !seen_modules.insert(module_idx) {
                continue;
            }
            let target = &modules[module_idx];
            let Some(sig) = target.funcs.get(method) else {
                continue;
            };
            let visible = self
                .name_ledger
                .visible(self.module_idx, module_idx, method);
            if !visible || !sig.root_param {
                continue;
            }
            let fn_params = target
                .trait_reg
                .fn_params
                .get(method)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if Self::root_param_accepts(sig, fn_params, receiver_ty) {
                candidates.push(RootCallTarget {
                    module_idx: Some(module_idx),
                    alias: Some(alias),
                    core_module: None,
                    name: method.to_string(),
                });
            }
        }
        candidates
    }

    fn select_root_call(
        &mut self,
        method: &str,
        receiver_ty: &Type,
        span: Span,
    ) -> Option<Result<RootCallTarget, ()>> {
        let candidates = self.root_call_candidates(method, receiver_ty);
        if candidates.is_empty() {
            return None;
        }
        let receiver_name = match receiver_ty {
            Type::Named(name) | Type::Apply { name, .. } => Some(name.clone()),
            Type::Option(inner) => match inner.as_ref() {
                Type::Named(name) | Type::Apply { name, .. } => Some(name.clone()),
                _ => None,
            },
            _ => None,
        };
        if let Some(type_name) = receiver_name {
            if self.resolve_method_sig(&type_name, method).is_some() {
                self.diags.push(Diagnostic::error(
                    "E0105",
                    format!("dot call `.{method}()` conflicts with a method on `{type_name}`"),
                    "a `#Root` function cannot silently compete with a real instance method"
                        .to_string(),
                    format!("rename the `#Root` function or call it as `{method}(value, …)`"),
                    Some(span),
                ));
                return Some(Err(()));
            }
        }
        if candidates.len() > 1 {
            let names = candidates
                .iter()
                .map(|candidate| {
                    candidate.core_module.as_deref().map_or_else(
                        || {
                            candidate.alias.as_deref().map_or_else(
                                || candidate.name.clone(),
                                |alias| format!("{alias}.{}", candidate.name),
                            )
                        },
                        |module| format!("{module}.{}", candidate.name),
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            self.diags.push(Diagnostic::error(
                "E0105",
                format!("dot call `.{method}()` is ambiguous"),
                format!("more than one imported `#Root` function accepts this receiver: {names}"),
                format!("call one function explicitly: `{method}(value, …)`"),
                Some(span),
            ));
            return Some(Err(()));
        }
        Some(Ok(candidates.into_iter().next().expect("root candidate")))
    }

    fn infer_root_call(
        &mut self,
        target: RootCallTarget,
        receiver: &mut Box<Expr>,
        method_span: Span,
        type_args: &[Type],
        args: &mut Vec<crate::AST::CallArg>,
        recv_type_out: &mut Option<String>,
        resolved_ret_out: &mut Option<Type>,
    ) -> Option<Type> {
        let receiver_expr =
            std::mem::replace(receiver, Box::new(Expr::Ident(String::new(), method_span)));
        let mut call_args = Vec::with_capacity(args.len() + 1);
        call_args.push(CallArg {
            convention: AccessConvention::Read,
            expr: *receiver_expr,
            span: method_span,
            flags: CallArgFlags::default(),
            label: None,
            spread: false,
        });
        call_args.append(args);

        let result = if let Some(core_module) = target.core_module.as_deref() {
            let ret = self.infer_core_call(
                core_module,
                &target.name,
                None,
                method_span,
                method_span,
                type_args,
                &mut call_args,
                resolved_ret_out,
            );
            if resolved_ret_out.is_none() {
                *resolved_ret_out = ret.clone();
            }
            ret
        } else if let Some(module_idx) = target.module_idx {
            self.infer_import_call(
                target.alias.as_deref().unwrap_or_default(),
                module_idx,
                &target.name,
                method_span,
                method_span,
                type_args,
                &mut call_args,
                resolved_ret_out,
            )
        } else {
            let mut call = Call {
                name: target.name.clone(),
                name_span: method_span,
                type_args: type_args.to_vec(),
                args: call_args,
                range_checked: false,
                resolved_ret: None,
                widen_approx: false,
            };
            let result = self.check_call(&mut call, true).flatten();
            *resolved_ret_out = call.resolved_ret;
            call_args = call.args;
            result
        };
        let receiver_arg = call_args
            .first()
            .expect("root call always has a receiver")
            .expr
            .clone();
        *receiver = Box::new(receiver_arg);
        *args = call_args.into_iter().skip(1).collect();
        *recv_type_out = Some(if let Some(core_module) = target.core_module {
            format!("{}{core_module}", Syntax::INTERNAL_ROOT_CALL_CORE_PREFIX)
        } else {
            target.alias.map_or_else(
                || Syntax::INTERNAL_ROOT_CALL_LOCAL.to_string(),
                |alias| format!("{}{alias}", Syntax::INTERNAL_ROOT_CALL_IMPORT_PREFIX),
            )
        });
        result
    }
}

fn is_http_route_registration(type_name: &str, method: &str) -> bool {
    match type_name {
        "HTTPRouter" => matches!(method, "get" | "post" | "put" | "delete"),
        "HTTPMux" => matches!(
            method,
            "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
        ),
        "App" => matches!(method, "route" | "page" | "layout"),
        _ => false,
    }
}


fn http_route_json_type(body: &LambdaBody, request_params: &HashSet<String>) -> Option<Type> {
    let mut found = None;
    let mut visit = |expr: &Expr| {
        if found.is_some() {
            return;
        }
        let Expr::MethodCall {
            receiver,
            method,
            type_args,
            recv_type,
            ..
        } = expr
        else {
            return;
        };
        if method == "json"
            && type_args.len() == 1
            && (recv_type.as_deref() == Some("HTTPRequest")
                || matches!(receiver.as_ref(), Expr::Ident(name, _) if request_params.contains(name)))
        {
            found = Some(type_args[0].clone());
        }
    };
    match body {
        LambdaBody::Expr(expr) => expr.for_each_expr(&mut visit),
        LambdaBody::Block(stmts) => {
            for stmt in stmts {
                stmt.for_each_expr(&mut visit);
            }
        }
    }
    found
}

fn http_route_find_struct_in_items<'a>(
    items: &'a [crate::AST::Item],
    name: &str,
) -> Option<&'a crate::AST::StructDef> {
    for item in items {
        match item {
            crate::AST::Item::Struct(def) if def.name == name => return Some(def),
            crate::AST::Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    if let Some(def) = http_route_find_struct_in_items(body, name) {
                        return Some(def);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

impl<'a> Checker<'a> {
    pub(crate) fn http_route_declared_verb(declaration: &crate::AST::MarkerDecl) -> Option<&'static str> {
        match declaration.name.as_str() {
            "Get" => Some("get"), "Post" => Some("post"), "Put" => Some("put"),
            "Delete" => Some("delete"), "Patch" => Some("patch"), "Head" => Some("head"),
            "Options" => Some("options"), _ => None,
        }
    }

    fn http_route_marker_verb(&self, marker: &crate::AST::Marker) -> Option<&'static str> {
        if marker.negated { return None; }
        let (declaring_module, span) = marker.declaration?;
        let module = self.modules?.get(declaring_module)?;
        let source = jet_foundation::CoreModuleExports::core_source_module("core.http.server")?;
        if module.module_alias != source.alias { return None; }
        let declaration = crate::Sema::MarkerModules::declaration_by_span(&module.items, span)?;
        Self::http_route_declared_verb(declaration)
    }

    /// D-HTTP-ROUTE-EXPOSE1=A: compile-time reflection over the one explicitly
    /// mounted value, expanded into the same calls that value routes check.
    pub(crate) fn expand_http_mount(&mut self, expression: &mut Expr) -> bool {
        let Expr::MethodCall { receiver, method, method_span, args, .. } = expression else {
            return false;
        };
        if method != "mount" || args.len() != 1 {
            return false;
        }
        let diagnostic_start = self.diags.len();
        let (mux_type, value_type) = self.with_erased_scope(|checker| {
            (checker.infer(receiver), checker.infer(&mut args[0].expr))
        });
        self.diags.truncate(diagnostic_start);
        if mux_type != Some(Type::Named("HTTPMux".to_string())) {
            return false;
        }
        let span = *method_span;
        let Some(Type::Named(name) | Type::Apply { name, .. }) = value_type else {
            self.diags.push(Diagnostic::error("E2805",
                "mux.mount needs an ordinary struct".to_string(),
                "only the explicitly mounted struct's tagged methods contribute routes".to_string(),
                "pass a struct value with #server.Get or another core.http.server verb marker".to_string(), Some(span)));
            return true;
        };
        let (namespace, leaf) = self.struct_type_name_parts(&name);
        let definition = self.struct_owner_module(leaf, namespace).and_then(|owner| {
            let items = if owner == self.module_idx { self.items }
                else { self.modules?.get(owner)?.items.as_slice() };
            http_route_find_struct_in_items(items, leaf).map(|definition| (definition.clone(), owner))
        });
        let Some((definition, owner)) = definition else {
            self.diags.push(Diagnostic::error("E2805",
                format!("mux.mount needs an ordinary struct, not `{name}`"),
                "only the explicitly mounted struct's tagged methods contribute routes".to_string(),
                "pass a struct value with #server.Get or another core.http.server verb marker".to_string(), Some(span)));
            return true;
        };
        let source_owner = self.canonical_nominal_name(owner, &definition.name);
        let mux_name = format!("\0jet.http.mount.mux.{}", span.start);
        let value_name = format!("\0jet.http.mount.value.{}", span.start);
        if let Expr::Ident(original, _) = receiver.as_ref() {
            let original = self.http_mux_aliases.get(original).unwrap_or(original).clone();
            self.http_mux_aliases.insert(mux_name.clone(), original);
        }
        let binding = |name: String, init: Expr| crate::AST::Stmt::Val(crate::AST::Binding {
            mutable: false, markers: Vec::new(), reactive_upgrade: false,
            meta: None, name, name_span: span, sigil_span: None, pattern: None,
            ty: None, ty_span: None, init, is_comptime: false, ct: None,
            uninit: false, arena_view: false, string_view: false,
            gc_promotion: None, gc_transferred: false,
        });
        let argument = |expr: Expr, argument_span: Span| CallArg {
            convention: AccessConvention::Read, expr, span: argument_span,
            flags: CallArgFlags::default(), label: None, spread: false,
        };
        let method_call = |receiver: Expr, method: String, args: Vec<CallArg>, method_span: Span| Expr::MethodCall {
            receiver: Box::new(receiver), method, method_span,
            owner_type_args: Vec::new(), type_args: Vec::new(), args,
            recv_type: None, resolved_ret: None, operator_rhs: None, checked_widen: false,
            checked_callee: None,
        };
        let mut body = vec![binding(mux_name.clone(), (**receiver).clone()),
            binding(value_name.clone(), args[0].expr.clone())];
        for function in &definition.methods {
            for marker in &function.markers {
                let Some(verb) = self.http_route_marker_verb(marker) else { continue; };
                if function.params.first().is_none_or(|parameter| parameter.name != "self" || parameter.convention != AccessConvention::Read)
                    || !function.type_params.is_empty() {
                    self.diags.push(Diagnostic::error("E2805",
                        format!("HTTP route method `{}` needs read-only self and a concrete signature", function.name),
                        "mounted handlers retain shared state and are called on request workers".to_string(),
                        format!("write `fn {}(self, ...)` without method type parameters; use Shared<T> for mutable shared state", function.name), Some(marker.span)));
                    continue;
                }
                let [crate::AST::MarkerCallArg::Expr(path)] = marker.args.as_slice() else { continue; };
                let params = function.params.iter().skip(1).map(|parameter| crate::AST::LambdaParam {
                    name: parameter.name.clone(), name_span: parameter.name_span,
                    ty: Some(parameter.ty.clone()), ty_span: None,
                }).collect();
                let invocation_args = function.params.iter().skip(1).map(|parameter| {
                    let mut arg = argument(Expr::Ident(parameter.name.clone(), parameter.name_span), parameter.name_span);
                    arg.convention = parameter.convention;
                    arg
                }).collect();
                let invocation = method_call(Expr::Ident(value_name.clone(), span), function.name.clone(), invocation_args, function.name_span);
                let handler = Expr::Lambda(Box::new(crate::AST::Lambda {
                    take_names: Vec::new(), params,
                    effects: None, body: LambdaBody::Expr(Box::new(invocation)),
                    span: marker.span, meta: crate::AST::LambdaMeta {
                        http_route_source: Some(format!("{source_owner}::{}", function.name)),
                        ..crate::AST::LambdaMeta::default()
                    },
                }));
                body.push(crate::AST::Stmt::Expr(method_call(Expr::Ident(mux_name.clone(), span),
                    verb.to_string(), vec![argument(path.clone(), marker.span), argument(handler, marker.span)], marker.span)));
            }
        }
        *expression = Expr::CallValue {
            callee: Box::new(Expr::Lambda(Box::new(crate::AST::Lambda {
                take_names: Vec::new(), params: Vec::new(),
                effects: None, body: LambdaBody::Block(body),
                span, meta: crate::AST::LambdaMeta::default(),
            }))),
            args: Vec::new(), span,
        };
        true
    }
}

impl<'a> Checker<'a> {
    fn http_route_handler_shape(
        &self,
        expr: &Expr,
    ) -> Option<(Vec<(String, Type)>, LambdaBody)> {
        match expr {
            Expr::Paren(inner, _) => self.http_route_handler_shape(inner),
            Expr::Lambda(lambda) => Some((
                lambda
                    .params
                    .iter()
                    .map(|param| Some((param.name.clone(), param.ty.clone()?)))
                    .collect::<Option<Vec<_>>>()?,
                if let Some(source) = &lambda.meta.http_route_source {
                    let (owner, method) = source.rsplit_once("::")?;
                    let (namespace, leaf) = self.struct_type_name_parts(owner);
                    let owner = self.struct_owner_module(leaf, namespace)?;
                    let items = if owner == self.module_idx { self.items }
                        else { self.modules?.get(owner)?.items.as_slice() };
                    let definition = http_route_find_struct_in_items(items, leaf)?;
                    let function = definition.methods.iter().find(|function| function.name == method)?;
                    LambdaBody::Block(function.body.clone())
                } else {
                    match &lambda.body {
                        LambdaBody::Expr(expr) => LambdaBody::Expr(expr.clone()),
                        LambdaBody::Block(body) => LambdaBody::Block(body.clone()),
                    }
                },
            )),
            Expr::Ident(name, _) => {
                let function = http_route_find_func_in_items(self.items, name).or_else(|| {
                    let (original, owner) = self.unqualified_file.get(name)
                        .map(|(original, owner)| (original.as_str(), *owner))
                        .or_else(|| self.namespace_function_owner(name).map(|owner| (name.as_str(), owner)))?;
                    http_route_find_func_in_items(&self.modules?.get(owner)?.items, original)
                })?;
                Some((
                    function
                        .params
                        .iter()
                        .map(|param| (param.name.clone(), param.ty.clone()))
                        .collect(),
                    LambdaBody::Block(function.body.clone()),
                ))
            }
            _ => None,
        }
    }

    fn http_route_struct_fields(&self, name: &str) -> Option<HashSet<String>> {
        let definition = http_route_find_struct_in_items(self.items, name).or_else(|| {
            let (namespace, leaf) = Self::split_type_name(name);
            let owner = self.struct_owner_module(leaf, namespace)?;
            http_route_find_struct_in_items(&self.modules?.get(owner)?.items, leaf)
        })?;
        Some(
            definition
                .fields
                .iter()
                .filter(|field| field.is_pub && field.computed.is_none())
                .map(|field| field.name.clone())
                .collect(),
        )
    }

    /// HTTP route callbacks are retained and invoked on request workers. Keep
    /// the boundary fact active for the full argument check so every capture
    /// path (including route-specific inference) uses the existing Send
    /// crossing prover before codegen sees the lambda.
    fn expect_http_handler_arg(
        &mut self,
        method: &str,
        index: usize,
        handler_ty: &Type,
        arg: &mut crate::AST::CallArg,
    ) {
        let saved_http_depth = self.http_handler_depth;
        let saved_escapes = self.lambda_escapes;
        self.http_handler_depth += 1;
        self.lambda_escapes = true;
        self.expect_core_arg(method, index, handler_ty, arg);
        self.http_handler_depth = saved_http_depth;
        self.lambda_escapes = saved_escapes;
    }

    fn check_http_route_registration(
        &mut self,
        receiver: &Expr,
        method: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) {
        if let (Some(root), Some(CtValue::Str(pattern))) =
            (expr_root_ident(receiver), args.first().and_then(|arg| self.evaluate_constant(&arg.expr)))
        {
            let root = self.http_mux_aliases.get(root).map_or(root, String::as_str);
            if !self.http_routes.insert((root.to_string(), method.to_string(), pattern.clone())) {
                self.diags.push(Diagnostic::error("E2804",
                    format!("Two routes both match `{} {pattern}`.", method.to_uppercase()),
                    "a duplicate route makes dispatch ambiguous; mounted and value routes share one ordered list".to_string(),
                    "remove one registration or use a distinct method and path".to_string(), Some(span)));
            }
        }
        self.check_http_router_contract(method, args, span);
    }

    fn check_http_router_contract(
        &mut self,
        method: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) {
        if args.len() != 2 {
            self.diags
                .push(wrong_core_arity(method, 2, args.len(), span));
            return;
        }
        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
        let Some((handler_params, body)) = self.http_route_handler_shape(&args[1].expr) else {
            return;
        };
        let expected_handler = http_handler_type(
            handler_params
                .iter()
                .map(|(_, ty)| ty.clone())
                .collect(),
        );
        self.expect_http_handler_arg(method, 1, &expected_handler, &mut args[1]);
        let Some(CtValue::Str(pattern)) = self.evaluate_constant(&args[0].expr) else {
            for (name, ty) in &handler_params {
                if matches!(ty, Type::Named(name) if name == "HTTPRequest") || self.is_decodable(ty) {
                    continue;
                }
                self.diags.push(Diagnostic::error("E2805",
                    format!("dynamic HTTP route input `{name}` cannot decode text into handler type `{}`", ty.show()),
                    "route inputs use the same checked Decode capability as request decoding".to_string(),
                    "use a decodable input type or implement Decode".to_string(), Some(args[1].expr.span())));
            }
            return;
        };
        if Syntax::validate_http_route_pattern(&pattern).is_err() {
            return;
        }
        let mut request_params = HashSet::new();
        let mut typed = Vec::new();
        for (name, ty) in &handler_params {
            if matches!(ty, Type::Named(request) if request == "HTTPRequest") {
                request_params.insert(name.clone());
            } else {
                typed.push((name.clone(), ty.clone()));
            }
        }
        if request_params.len() > 1 {
            self.diags.push(Diagnostic::error(
                "E2805",
                format!("HTTP route `{pattern}` has more than one HTTPRequest input"),
                "one handler request value owns the transport envelope and all decoded bindings"
                    .to_string(),
                "keep one `HTTPRequest` input and remove the duplicate".to_string(),
                Some(args[1].expr.span()),
            ));
        }
        let path_names = pattern
            .split('/')
            .skip(1)
            .filter_map(|segment| {
                segment
                    .strip_prefix(':')
                    .or_else(|| segment.strip_prefix('*'))
            })
            .map(str::to_string)
            .collect::<Vec<_>>();
        let mut seen = HashSet::new();
        for (name, _) in &typed {
            if !seen.insert(name.clone()) {
                self.diags.push(Diagnostic::error(
                    "E2805",
                    format!("HTTP route `{pattern}` has ambiguous handler input `{name}`"),
                    "one handler field can bind only one transport source".to_string(),
                    "rename or remove the duplicate handler input".to_string(),
                    Some(args[1].expr.span()),
                ));
            }
        }
        for name in &path_names {
            let matches = typed.iter().filter(|(candidate, _)| candidate == name).count();
            if matches != 1 {
                self.diags.push(Diagnostic::error(
                    "E2805",
                    format!(
                        "HTTP route `{pattern}` path field `{name}` does not have exactly one typed handler input"
                    ),
                    "path placeholders are bound by name before request decoding".to_string(),
                    format!("declare one `{name}: T` handler input"),
                    Some(args[0].expr.span()),
                ));
            }
            for (_, ty) in typed.iter().filter(|(candidate, _)| candidate == name) {
                if !self.is_decodable(ty) {
                    self.diags.push(Diagnostic::error(
                        "E2805",
                        format!("HTTP route `{pattern}` path field `{name}` cannot decode text into handler type `{}`", ty.show()),
                        "path parameters use the same checked Decode capability as request decoding".to_string(),
                        format!("give `{}` a `Decode` implementation, or use a decodable parameter type", ty.show()),
                        Some(args[0].expr.span()),
                    ));
                }
            }
        }
        let path_set = path_names.iter().cloned().collect::<HashSet<_>>();
        let query_names = typed
            .iter()
            .filter(|(name, _)| !path_set.contains(name))
            .map(|(name, _)| name.clone())
            .collect::<HashSet<_>>();
        if !matches!(method, "get" | "head") && !query_names.is_empty() {
            let name = query_names.iter().next().cloned().unwrap_or_default();
            self.diags.push(Diagnostic::error(
                "E2805",
                format!("HTTP route `{pattern}` has unbound non-GET handler input `{name}`"),
                "GET remainder is query data; non-GET payloads must use an explicit body model"
                    .to_string(),
                "remove the input or decode it with `req.json<T>()`".to_string(),
                Some(args[1].expr.span()),
            ));
        }
        if let Some(body_ty) = http_route_json_type(&body, &request_params) {
            if let Some(name) = match &body_ty {
                Type::Named(name) | Type::Apply { name, .. } => Some(name.as_str()),
                Type::Option(inner) => match inner.as_ref() {
                    Type::Named(name) | Type::Apply { name, .. } => Some(name.as_str()),
                    _ => None,
                },
                _ => None,
            } {
                if let Some(fields) = self.http_route_struct_fields(name) {
                    if let Some(overlap) = fields
                        .iter()
                        .find(|field| path_set.contains(*field) || query_names.contains(*field))
                    {
                        self.diags.push(Diagnostic::error(
                            "E2805",
                            format!("HTTP route `{pattern}` binds body field `{overlap}` twice"),
                            "path, query, and body fields are distinct transport sources"
                                .to_string(),
                            format!("rename `{overlap}` in the body or remove the duplicate binding"),
                            Some(args[1].expr.span()),
                        ));
                    }
                }
            }
        }
    }
}
fn http_middleware_type() -> Type {
    Type::Fn {
        params: vec![Type::Named("HTTPHandler".to_string())],
        ret: Some(Box::new(Type::Named("HTTPHandler".to_string()))),
        effect_bound: None,
        return_view_provenance: None,
        param_contract: None,
        call_metadata: None,
    }
}


fn http_handler_type(params: Vec<Type>) -> Type {
    Type::Fn {
        params,
        ret: Some(Box::new(Type::Result {
            ok: Box::new(Type::Named("HTTPResponse".to_string())),
            err: Box::new(Type::Named("HTTPError".to_string())),
        })),
        effect_bound: None,
        return_view_provenance: None,
        param_contract: None,
        call_metadata: None,
    }
}

/// D-TYPE2-SPELL1 / card #1549: recover the parser's ordinary call-shaped
/// receiver for `Int(lo..hi).from_int(...)`. The call is a type-head
/// descriptor, not a runtime constructor; sema records its resolved range on
/// the inner call for TIR to consume.
fn inline_range_descriptor(expr: &Expr) -> Option<(i64, i64)> {
    let Expr::Call(call) = expr else {
        return None;
    };
    if call.name != crate::Syntax::TYPE_INT || call.args.len() != 1 {
        return None;
    }
    let Expr::Range {
        start,
        end,
        exclusive: false,
        ..
    } = &call.args[0].expr
    else {
        return None;
    };
    let Expr::Int(lo, ..) = start.as_ref() else {
        return None;
    };
    let Expr::Int(hi, ..) = end.as_ref() else {
        return None;
    };
    Some((*lo, *hi))
}

fn inline_range_literal(expr: &Expr) -> Option<(crate::Numeric::CtBigInt, Span)> {
    let Expr::Int(value, span, _, raw) = expr else {
        return None;
    };
    Some((exact_integer_literal(*value, raw.as_deref()), *span))
}

impl<'a> Checker<'a> {
    /// D-ALLOC-D: `mem.Fixed.new(size:)` / `mem.Fixed.over(&bytes)` validation.
    /// Shared by the `core.mem` constructor-sentinel route and the
    /// `Fixed`-typed receiver route, so neither skips the E0103 checks.
    fn check_fixed_constructor(
        &mut self,
        method: &str,
        args: &mut Vec<crate::AST::CallArg>,
        span: Span,
    ) {
        if !self.allow_fixed_constructor {
            self.diags.push(Diagnostic::error(
                "E0103",
                format!("`Fixed.{method}` must directly initialize a lexical binding"),
                "the Fixed handle borrows inline storage whose lifetime and drop order are fixed at its declaration".to_string(),
                format!("write `fixed :: mem.Fixed.{method}(…)`"),
                Some(span),
            ));
        }
        if args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0103",
                format!("`Fixed.{method}` takes exactly one argument"),
                if method == "new" {
                    "the inline backing size must be a positive compile-time integer".to_string()
                } else {
                    "the backing must be one mutable fixed-size byte array".to_string()
                },
                if method == "new" {
                    "write `fixed :: mem.Fixed.new(size: 4096)`".to_string()
                } else {
                    "write `fixed :: mem.Fixed.over(&bytes)` with the write-access marker `&`".to_string()
                },
                Some(span),
            ));
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return;
        }
        let inferred = self.infer(&mut args[0].expr);
        if method == "new" {
            let direct_size = match &args[0].expr {
                Expr::Int(value, _, _, _) => Some(*value),
                Expr::ComptimeName {
                    value: Some(CtValue::Int(value)),
                    ..
                } => Some(*value),
                Expr::Ident(name, _) => match self.current_ct_globals().get(name) {
                    Some(CtValue::Int(value)) => Some(*value),
                    _ => None,
                },
                _ => None,
            };
            if !self.defer_ct_evaluation {
                let size = direct_size.or_else(|| {
                    let evaluated = {
                        let globals = self.current_ct_globals();
                        crate::Comptime::evaluate_owned_with_imports_opts_collecting(
                            &args[0].expr,
                            self.ct_checked_funcs_for_evaluation(),
                            self.ct_externs,
                            self.ct_base_dir,
                            globals.as_ref(),
                            self.core_imports,
                            self.gates,
                            0,
                            None,
                        )
                    };
                    match evaluated {
                        Ok((CtValue::Int(value), inputs)) => {
                            self.ct_embed_inputs.extend(inputs);
                            Some(value)
                        }
                        _ => None,
                    }
                });
                if let Some(size) = size.filter(|size| *size > 0) {
                    let arg_span = args[0].expr.span();
                    args[0].expr = Expr::Int(size, arg_span, None, None);
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        "`Fixed.new` needs a positive compile-time byte size".to_string(),
                        "runtime-sized storage cannot become an inline fixed array in the current stack frame".to_string(),
                        "use a positive literal or comptime integer, e.g. `mem.Fixed.new(size: 4096)`".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
            }
        } else {
            let fixed_bytes = matches!(
                inferred,
                Some(Type::FixedList { elem, len, .. })
                    if len.literal_value().is_some_and(|len| len > 0)
                        && matches!(*elem, Type::IntN { signed: false, bits: 8 })
            );
            let direct_mutable_buffer = args[0].convention == AccessConvention::Write
                && matches!(&args[0].expr, Expr::Ident(..));
            if !fixed_bytes || !direct_mutable_buffer {
                self.diags.push(Diagnostic::error(
                    "E0103",
                    "`Fixed.over` needs one mutable fixed-size byte buffer".to_string(),
                    "the allocator exclusively borrows that exact inline buffer until the Fixed handle is closed".to_string(),
                    "bind `[Byte#N]` storage, then write `fixed :: mem.Fixed.over(&storage)` with the write-access marker `&`".to_string(),
                    Some(args[0].expr.span()),
                ));
            }
        }
    }

    fn check_http_route_constant(
        &mut self,
        type_name: &str,
        method: &str,
        args: &[crate::AST::CallArg],
    ) {
        if !is_http_route_registration(type_name, method) {
            return;
        }
        let Some(arg) = args.first() else {
            return;
        };
        let result = if let Expr::Str(parts, _) = &arg.expr {
            if parts.iter().any(|part| matches!(part, StrPart::Interp(..))) {
                let mut source = String::new();
                for part in parts {
                    match part {
                        StrPart::Lit(text) => source.push_str(text),
                        StrPart::Interp(expr, _) => match expr.as_ref() {
                            Expr::Ident(name, _) => source.push_str(&format!("{{{name}}}")),
                            _ => source.push_str("{…}"),
                        },
                    }
                }
                Err((
                    source,
                    "brace interpolation is not allowed in route patterns; it is indistinguishable from the retired `{name}` parameter spelling".to_string(),
                ))
            } else {
                match self.evaluate_constant(&arg.expr) {
                    Some(CtValue::Str(pattern)) => Syntax::validate_http_route_pattern(&pattern)
                        .map_err(|reason| (pattern, reason)),
                    _ => return,
                }
            }
        } else {
            match self.evaluate_constant(&arg.expr) {
                Some(CtValue::Str(pattern)) => Syntax::validate_http_route_pattern(&pattern)
                    .map_err(|reason| (pattern, reason)),
                _ => return,
            }
        };
        if let Err((pattern, reason)) = result {
            self.diags.push(Diagnostic::error(
                "E2805",
                format!("invalid HTTP route `{pattern}`: {reason}"),
                "route patterns use one canonical grammar; ambiguous escapes, traversal, duplicate names, and retired markers would make routing and audit metadata disagree"
                    .to_string(),
                "use `:name` for one segment or final `*name` for a catch-all; percent-encode a literal leading `:` or `*`, and never encode `/`"
                    .to_string(),
                Some(arg.span),
            ));
        }
    }


    /// D-COPY-WORD1=A: `receiver.copy()` parses as an ordinary method call;
    /// sema decides what it means by name resolution. A receiver that names
    /// a declared type (`Recipe.copy()`, `List<Int>.copy()`,
    /// `alias.Type.copy()`) keeps associated-function lookup; every other
    /// receiver is a value and gets the built-in copy. The Jet half mirrors
    /// this in `sema_expr_copy_receiver_names_type`.
    pub(crate) fn copy_receiver_names_type(&self, receiver: &Expr, owner_type_args: &[Type]) -> bool {
        if !owner_type_args.is_empty() {
            return true;
        }
        let names_value = |name: &str| {
            self.binder_ref_types.contains_key(name)
                || self.lookup(name).is_some()
                || self.consts.contains_key(name)
                || self.funcs.contains_key(name)
        };
        match receiver {
            Expr::Ident(name, _) => !names_value(name) && self.registry.contains(name),
            Expr::Field(base, _, _) => matches!(
                base.as_ref(),
                Expr::Ident(alias, _)
                    if !names_value(alias)
                        && (self.imports.contains_key(alias) || self.core_imports.contains_key(alias))
            ),
            _ => false,
        }
    }

    pub(crate) fn infer_method_call(
        &mut self,
        receiver: &mut Box<Expr>,
        method: &str,
        span: Span,
        owner_type_args: &mut Vec<Type>,
        type_args: &mut Vec<Type>,
        args: &mut Vec<crate::AST::CallArg>,
        recv_type_out: &mut Option<String>,
        resolved_ret_out: &mut Option<Type>,
        operator_rhs_out: &mut Option<Type>,
        checked_callee_out: &mut Option<String>,
    ) -> Option<Type> {
        *checked_callee_out = None;
        if let Some(path) = checker_hardware_path(receiver, &self.core_imports) {
            let is_board = self
                .core_imports
                .get(path.first().map(String::as_str).unwrap_or_default())
                .is_some_and(|module| module.starts_with("board."));
            if is_board && matches!(method, "read" | "write" | "set" | "clear") {
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                let ret = if method == "read" {
                    Type::Int
                } else {
                    Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
        }
        if method == "start"
            && matches!(receiver.as_ref(), Expr::Ident(name, _) if name == "dma")
            && args.len() >= 2
        {
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            let ret = Type::Named("__JetDmaTransfer".to_string());
            *recv_type_out = Some("__JetDma".to_string());
            *resolved_ret_out = Some(ret.clone());
            return Some(ret);
        }
        if method == "wait"
            && matches!(receiver.as_ref(), Expr::Ident(name, _) if self
                .lookup(name)
                .is_some_and(|info| matches!(&info.ty, Type::Named(ty) if ty == "__JetDmaTransfer")))
        {
            let ret = Type::Named("__JetDmaBuffer".to_string());
            *resolved_ret_out = Some(ret.clone());
            return Some(ret);
        }
        // D-VALIDATE1: the first pass elaborates `Validate.over(value)` into
        // the hidden builder literal. Chained calls infer their receiver again;
        // keep that compiler-owned node idempotent instead of trying to resolve
        // the erased `over` method on the materialized builder.
        if recv_type_out.as_deref() == Some(Syntax::INTERNAL_VALIDATE_OVER)
            && method == "over"
            && args.is_empty()
            && matches!(
                receiver.as_ref(),
                Expr::StructLit { type_name, .. }
                    if type_name == Syntax::TYPE_VALIDATE_BUILDER
            )
        {
            return resolved_ret_out.clone();
        }
        // D-MEMO1=A: `name.cache()` is a read-only projection of one
        // memoized function's store. It is resolved from the registered
        // function signature before ordinary receiver/method lookup.
        if method == Syntax::METHOD_MEMO_CACHE && args.is_empty() {
            if let Expr::Ident(name, _) = receiver.as_ref() {
                if self
                    .funcs
                    .get(name)
                    .is_some_and(|signature| signature.memo_bound.is_some())
                {
                    let stats = Type::Named(Syntax::TYPE_MEMO_STATS.to_string());
                    *resolved_ret_out = Some(stats.clone());
                    return Some(stats);
                }
            }
        }
        if matches!(receiver.as_ref(), Expr::Ident(name, _) if name == Syntax::INTERNAL_TASK_RECEIVER)
        {
            return self.infer_task_surface_method(
                receiver,
                method,
                span,
                args,
                recv_type_out,
                resolved_ret_out,
            );
        }
        // D-ALLOC2: allocator methods operate through the runtime's audited
        // interior-mutable storage. `alloc` may coexist with existing views;
        // `reset` invalidates them. Do not run the ordinary owner-read check
        // for either operation, or a valid live view produces E0220 before the
        // allocator-specific transition can run.
        let allocator_view_preserving_receiver = matches!(method, "alloc" | "try_alloc" | "reset")
            && matches!(receiver.as_ref(), Expr::Ident(name, _) if self
                .lookup(name)
                .is_some_and(|info| is_allocator_type(&info.ty)));
        // D-MOD2: an imported file can expose an inline module, so
        // `alias.module.item(...)` must resolve the module member before the
        // ordinary receiver walk tries to treat `alias.module` as a value.
        if let Expr::Field(base, inline_module, _) = receiver.as_ref() {
            if let Expr::Ident(alias, alias_span) = base.as_ref() {
                if let Some(&module_idx) = self.imports.get(alias).filter(|_| self.lookup(alias).is_none()) {
                    let is_inline_module = self.modules.is_some_and(|modules| {
                        modules[module_idx].code_modules.contains_key(inline_module)
                    });
                    if is_inline_module {
                        return self.infer_import_call(
                            alias,
                            module_idx,
                            &format!("{inline_module}.{method}"),
                            *alias_span,
                            span,
                            type_args,
                            args,
                            resolved_ret_out,
                        );
                    }
                }
            }
        }
        // An imported type is a namespace owner, not a runtime field receiver.
        // Use the ordinary declaration-owned associated-method checker so
        // visibility, generics and the checked callee remain unchanged.
        if let Expr::Field(base, leaf, _) = receiver.as_ref() {
            if let Expr::Ident(alias, alias_span) = base.as_ref() {
                if self.lookup(alias).is_none() && self.imports.contains_key(alias) {
                    if let Some(owner) = self.struct_owner_module(leaf, Some(alias)) {
                        if self.modules.is_some_and(|modules| {
                            modules[owner].registry.method(leaf, method).is_some()
                        }) {
                            let type_name = self.canonical_nominal_name(owner, leaf);
                            if !self.name_ledger.visible(self.module_idx, owner, leaf) {
                                self.diags.push(crate::Sema::Diagnostics::private_item(leaf, span));
                            }
                            self.record_import_alias_reference(alias, *alias_span);
                            let ret = self.check_static_method(
                                &type_name, method, span, owner_type_args, type_args, args,
                                checked_callee_out,
                            );
                            *resolved_ret_out = ret.clone();
                            return ret;
                        }
                    }
                }
            }
        }
        // D-FOUND-RECEIPT1: `receipt` is one compiler-owned ambient handle.
        // The payload is checked against its declaration-owned #Receipt/#Codable
        // fact; no source constructor or `core.receipt` module exists.
        if method == Syntax::METHOD_RECEIPT_ATTACH
            && matches!(receiver.as_ref(), Expr::Ident(name, _) if name == "receipt")
            && self.lookup("receipt").is_none()
        {
            let ret = Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string());
            *recv_type_out = Some(Syntax::INTERNAL_RECEIPT_HANDLE.to_string());
            *resolved_ret_out = Some(ret.clone());
            if args.len() != 1 {
                self.diags.push(Diagnostic::error(
                    "E0103",
                    "`receipt.attach` takes exactly one argument".to_string(),
                    "one typed value becomes one named receipt section".to_string(),
                    "write `receipt.attach(value)`".to_string(),
                    Some(span),
                ));
                return Some(ret);
            }
            let payload_ty = self.infer(&mut args[0].expr);
            let type_name = payload_ty.as_ref().and_then(|ty| match ty {
                Type::Named(name) | Type::Apply { name, .. } => Some(name.as_str()),
                _ => None,
            });
            let Some(type_name) = type_name else {
                self.diags.push(Diagnostic::error(
                    "E2411",
                    "receipt sections require a named `#Receipt` type".to_string(),
                    "the section schema and wire encoder are declaration-owned".to_string(),
                    "declare a `#Codable` record with `#Receipt(\"name\")`".to_string(),
                    Some(args[0].expr.span()),
                ));
                return Some(ret);
            };
            if self.registry.receipt_section(type_name).is_none() {
                self.diags.push(Diagnostic::error(
                    "E2411",
                    format!("type `{type_name}` is not a `#Receipt` section"),
                    "receipt attachment accepts only declaration-owned typed sections".to_string(),
                    "add `#Receipt(\"name\")` and `#Codable` to the record".to_string(),
                    Some(args[0].expr.span()),
                ));
            } else if let Some(payload_ty) = payload_ty.as_ref() {
                self.check_encodable(payload_ty, args[0].expr.span());
            }
            return Some(ret);
        }
        self.check_call_receiver_evaluation(receiver, span);
        // D-SHAPE-PLACE1=A: `.view(a..b)` is retired. Keep the parser's
        // range-shaped recovery long enough to point at the old spelling,
        // but never admit it to the type system.
        if method == Syntax::METHOD_VIEW {
            self.infer(receiver);
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            self.diags.push(Diagnostic::error(
                "E0214",
                "`.view(a..b)` is now a bare range place".to_string(),
                "place access follows the sigil reading rule".to_string(),
                "replace `value.view(a..b)` with `value[a..b]`".to_string(),
                Some(span),
            ));
            return None;
        }
        // D-TYPEDTEXT1=D: `SQL.raw("…")` / `HTML.raw("…")` — the sole audited
        // escape from a runtime `String` into a typed-text position. `SQL`/
        // `HTML` here name the type, not a value (checked via `lookup` so a
        // shadowing local of that name still resolves normally below).
        if method == "raw" {
            let (type_name, shadowed) = match receiver.as_ref() {
                Expr::Ident(name, _) => (Some(name.clone()), self.lookup(name).is_some()),
                Expr::Field(base, member, _) if matches!(base.as_ref(), Expr::Ident(..)) => {
                    let Expr::Ident(alias, _) = base.as_ref() else {
                        unreachable!("checked qualified type receiver")
                    };
                    (
                        Some(format!("{alias}.{member}")),
                        self.lookup(alias).is_some(),
                    )
                }
                _ => (None, false),
            };
            if let (Some(type_name), false) = (type_name, shadowed) {
                let is_builtin =
                    Syntax::typed_head_kind(&type_name).is_some_and(|kind| kind.is_typed_text());
                let checked = self.checked_text_type_name(&type_name);
                let is_checked = checked.is_some();
                if is_builtin || is_checked {
                    let result = checked
                        .map(|(canonical, _)| Type::Named(canonical))
                        .unwrap_or_else(|| Type::Named(type_name.clone()));
                    *resolved_ret_out = Some(result.clone());
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E0103",
                            format!("`{}.raw()` takes exactly one argument", type_name),
                            "`.raw()` wraps one already-audited `String` as the escape hatch"
                                .to_string(),
                            format!("write `{}.raw(text)`", type_name),
                            Some(span),
                        ));
                        return Some(result);
                    }
                    let arg_ty = self.infer(&mut args[0].expr);
                    if let Some(t) = arg_ty {
                        self.check_type_assignable(&Type::String, &t, args[0].expr.span());
                    }
                    if is_checked {
                        if !self.in_unsafe {
                            self.diags
                                .push(Diagnostic::from_row("E0387", &[], Some(span)));
                        }
                    }
                    return Some(result);
                }
            }
        }
        // D-CAP2 (D-MEM1/S4): `.clone()` is not user-typable Jet syntax — `clone`
        // falls through to the ordinary "no such method" path below like any
        // other unrecognized name (I8: `copy x` is the one copy spelling).
        self.lint_allocation_hints(method, span);
        // D-DIST3 (ratified 2026-06-20): `.raw()` unwraps a distinct type.
        if method == crate::Syntax::METHOD_DISTINCT_RAW {
            self.borrow_ctx = true;
            let recv_ty = self.infer(receiver)?;
            if let Type::Named(ref n) = recv_ty {
                if let Some(base) = self.distinct_base_for_reader(n) {
                    if !crate::Sema::knowledge_gate_allows(
                        crate::Sema::KnowledgePlane::Range,
                        crate::Sema::KnowledgeGate::RawProjection,
                    ) {
                        return None;
                    }
                    if !args.is_empty() {
                        self.diags.push(Diagnostic::error(
                            "E0103",
                            format!(
                                "`.{}()` takes no arguments",
                                crate::Syntax::METHOD_DISTINCT_RAW
                            ),
                            "`.raw()` simply unwraps the base value — no arguments needed"
                                .to_string(),
                            "write `.raw()` with no arguments".to_string(),
                            Some(span),
                        ));
                    }
                    *recv_type_out = Some(n.clone());
                    *resolved_ret_out = Some(base.clone());
                    return Some(base);
                }
            }
            self.diags.push(Diagnostic::error(
                "E0311",
                format!(
                    "`.{}()` is only valid on a distinct type value",
                    crate::Syntax::METHOD_DISTINCT_RAW
                ),
                "`.raw()` unwraps a distinct type to its base representation".to_string(),
                format!(
                    "only call `.raw()` on a value whose type was declared with `{} distinct`",
                    crate::Syntax::SIGIL_BIND_IMMUT
                ),
                Some(span),
            ));
            return None;
        }
        // D-TOOL4 (E2-M11): `expect(x).snapshot()` — the special snapshot
        // assertion. Recognized by checking the receiver type.
        if method == Syntax::BUILTIN_SNAPSHOT
            && self.core_module_path_from_receiver(receiver).is_none()
        {
            let recv_ty = self.infer(receiver);
            if recv_ty
                .as_ref()
                .map(|t| t == &Type::Named("__JetExpect__".to_string()))
                .unwrap_or(false)
            {
                // Valid: snapshot assertion — void, no return type.
                return None;
            }
            // Not from expect() — error.
            self.diags.push(Diagnostic::error(
                "E2901",
                format!(
                    "`.{}()` is only valid on the result of `{}(…)`",
                    Syntax::BUILTIN_SNAPSHOT,
                    Syntax::BUILTIN_EXPECT
                ),
                "snapshot testing: call `expect(value).snapshot()` in a test block".to_string(),
                format!("e.g. `{}(my_result).snapshot()`", Syntax::BUILTIN_EXPECT),
                Some(span),
            ));
            return None;
        }
        if let Expr::Ident(root, _) = &**receiver {
            if root == "File" && method == Syntax::FOREIGN_OPEN {
                self.diags.push(Diagnostic::error(
                    "E0038",
                    "`File.open` is not the M10 file API".to_string(),
                    "M10 uses whole-file helpers in `core.files`, not a `File.open` handle type"
                        .to_string(),
                    "import `core.files as fs` and call `fs.read(path)` or `fs.write(path, text)` \
                         (or `fs.open(path)` for a streaming handle)"
                        .to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
        }
        // D-PROTO1/D-PROTO2: `Payment.Client.client()` — static call on a dotted
        // protocol handle type (PascalCase.PascalCase).
        if let Expr::Field(base, leaf, _) = &**receiver {
            if let Expr::Ident(prefix, _) = &**base {
                if prefix
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_uppercase())
                {
                    let full = format!("{prefix}.{leaf}");
                    if self.registry.method(&full, method).is_some() {
                        return self.check_static_method(&full,
                        method,
                        span,
                        owner_type_args,
                        type_args,
                        args, checked_callee_out);
                    }
                }
            }
        }
        // D-ENC1: nested-namespace access — `encoding.json.to_string(x)` where `encoding`
        // is a library alias (`use core.encoding`) and `json` a registered submodule. The
        // method-call receiver is `Field(Ident(alias), leaf)`; resolve to the submodule
        // `<ns>.<leaf>` as a core call. Guarded by `is_known_core_module`, so it fires only
        // for real submodules (e.g. `core.encoding.json`), never plain field access.
        if let Expr::Field(base, leaf_storage, _) = &mut **receiver {
            if let Expr::Ident(alias_storage, alias_span) = &mut **base {
                let alias = &*alias_storage;
                let leaf = &*leaf_storage;
                if let Some(ns) = self.core_imports.get(alias).filter(|_| self.lookup(alias).is_none()).cloned() {
                    self.record_import_alias_reference(alias, *alias_span);
                    if ns == "core.net.tls" && leaf == "ClientConfig" && method == "default" {
                        if !args.is_empty() {
                            self.diags.push(wrong_core_arity(
                                "ClientConfig.default",
                                0,
                                args.len(),
                                span,
                            ));
                            for arg in args.iter_mut() {
                                self.infer(&mut arg.expr);
                            }
                        }
                        *recv_type_out = Some("TLSClientConfigType".to_string());
                        return Some(Type::Named("TLSClientConfig".to_string()));
                    }
                    if ns == "core.http.client" && leaf == "Client" && method == "new" {
                        if !args.is_empty() {
                            self.diags
                                .push(wrong_core_arity("Client.new", 0, args.len(), span));
                            for arg in args.iter_mut() {
                                self.infer(&mut arg.expr);
                            }
                        }
                        *recv_type_out = Some("HTTPClientType".to_string());
                        return Some(Type::Named("HTTPClient".to_string()));
                    }
                    if ns == "core.net.tls" && leaf == "RootCertificates" && method == "from_pem" {
                        if args.len() != 1 {
                            self.diags.push(wrong_core_arity(
                                "RootCertificates.from_pem",
                                1,
                                args.len(),
                                span,
                            ));
                        }
                        if let Some(arg) = args.first_mut() {
                            self.expect_core_arg(
                                "RootCertificates.from_pem",
                                0,
                                &Type::List(Box::new(u8_ty())),
                                arg,
                            );
                        }
                        *recv_type_out = Some("TLSRootCertificatesType".to_string());
                        return Some(result_ty(
                            Type::Named("TLSRootCertificates".to_string()),
                            Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
                        ));
                    }
                    if ns == "core.net.tls" && leaf == "ClientIdentity" && method == "from_pem" {
                        if args.len() != 2 {
                            self.diags.push(wrong_core_arity(
                                "ClientIdentity.from_pem",
                                2,
                                args.len(),
                                span,
                            ));
                        }
                        crate::Sema::CheckerCoreLib::require_exact_labels(
                            "ClientIdentity.from_pem",
                            args,
                            &[(0, "cert_chain"), (1, "private_key")],
                            span,
                            &mut self.diags,
                        );
                        for (index, arg) in args.iter_mut().enumerate() {
                            self.expect_core_arg(
                                "ClientIdentity.from_pem",
                                index,
                                &Type::List(Box::new(u8_ty())),
                                arg,
                            );
                        }
                        *recv_type_out = Some("TLSClientIdentityType".to_string());
                        return Some(result_ty(
                            Type::Named("TLSClientIdentity".to_string()),
                            Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
                        ));
                    }
                    if ns == "core.crypto.vault"
                        && leaf == "KeyUnlock"
                        && matches!(method, "Recipient" | "Passphrase")
                    {
                        if args.len() != 1 {
                            self.diags.push(wrong_core_arity(
                                &format!("KeyUnlock.{method}"),
                                1,
                                args.len(),
                                span,
                            ));
                        }
                        let expected = crate::Sema::Diagnostics::core_crypto_nominal(
                            if method == "Recipient" {
                                Type::Named("X25519SecretKey".to_string())
                            } else {
                                Type::Named("Secret".to_string())
                            },
                        );
                        if let Some(arg) = args.first_mut() {
                            self.expect_core_arg(&format!("KeyUnlock.{method}"), 0, &expected, arg);
                        }
                        // The constructor mints the source-owned `KeyUnlock`
                        // record that `prepare_import_wrapped` declares, so the
                        // value carries that canonical identity on every call path.
                        let unlock = self
                            .core_source_owner_module(&ns, "KeyUnlock")
                            .map(|owner| {
                                Type::Named(self.canonical_nominal_name(owner, "KeyUnlock"))
                            })
                            .unwrap_or_else(|| Type::Named("KeyUnlock".to_string()));
                        **receiver = Expr::Ident("KeyUnlock".to_string(), span);
                        *resolved_ret_out = Some(unlock.clone());
                        return Some(unlock);
                    }
                    // `core.game` constructors mint runtime handles (GameScene,
                    // GameReplay, GameBackend) that `game.run` and codegen's
                    // `game_static_type` expect with the module-qualified
                    // receiver intact, so claim them before the generic Core
                    // type-item route rewrites the receiver to the source struct.
                    if ns == "core.game" {
                        match (leaf.as_str(), method) {
                            ("Scene", "new") => {
                                if args.len() != 1 {
                                    self.diags.push(wrong_core_arity(
                                        "Scene.new",
                                        1,
                                        args.len(),
                                        span,
                                    ));
                                }
                                if let Some(arg) = args.get_mut(0) {
                                    self.expect_core_arg("Scene.new", 0, &Type::String, arg);
                                }
                                *recv_type_out = Some("GameSceneType".to_string());
                                return Some(Type::Named("GameScene".to_string()));
                            }
                            ("Replay", "record") => {
                                if args.len() != 1 {
                                    self.diags.push(wrong_core_arity(
                                        "Replay.record",
                                        1,
                                        args.len(),
                                        span,
                                    ));
                                }
                                if let Some(arg) = args.get_mut(0) {
                                    self.expect_core_arg("Replay.record", 0, &Type::String, arg);
                                    if let Expr::Str(parts, literal_span) = &arg.expr {
                                        if let [StrPart::Lit(path)] = parts.as_slice() {
                                            if crate::Syntax::artifact_kind(path)
                                                != Some(crate::Syntax::ArtifactKind::GameReplay)
                                            {
                                                let actual = crate::Syntax::artifact_kind(path);
                                                let why = match actual {
                                                        Some(crate::Syntax::ArtifactKind::ProofReplay) => "that suffix identifies a proof replay, not a game input replay".to_string(),
                                                        Some(kind) => format!("that suffix identifies a {kind:?} artifact, not a game input replay"),
                                                        None => format!("game input replay paths end in `{}`", crate::Syntax::ARTIFACT_EXT_GAME_REPLAY),
                                                    };
                                                self.diags.push(Diagnostic::error(
                                                        "E0103",
                                                        format!("`Replay.record` needs a `{}` game replay path", crate::Syntax::ARTIFACT_EXT_GAME_REPLAY),
                                                        why,
                                                        format!("rename the path to end in `{}`", crate::Syntax::ARTIFACT_EXT_GAME_REPLAY),
                                                        Some(*literal_span),
                                                    ));
                                            }
                                        }
                                    }
                                }
                                *recv_type_out = Some("GameReplayType".to_string());
                                return Some(Type::Named("GameReplay".to_string()));
                            }
                            ("Backend", "headless") => {
                                if !args.is_empty() {
                                    self.diags.push(wrong_core_arity(
                                        "Backend.headless",
                                        0,
                                        args.len(),
                                        span,
                                    ));
                                    for a in args.iter_mut() {
                                        self.infer(&mut a.expr);
                                    }
                                }
                                *recv_type_out = Some("GameBackendType".to_string());
                                return Some(Type::Named("GameBackend".to_string()));
                            }
                            _ => {}
                        }
                    }
                    if crate::Sema::CheckerCoreLib::core_module_type_item(&ns, leaf) {
                        let type_name = if let Some(owner) =
                            self.core_source_enum_owner_module(&ns, leaf)
                        {
                            self.canonical_nominal_name(owner, leaf)
                        } else if matches!(
                            ns.as_str(),
                            "core.http" | "core.http.client" | "core.http.server"
                        ) {
                            match leaf.as_str() {
                                "Body" | "Headers" => leaf.clone(),
                                "Method" | "Status" | "Version" | "HeaderName" | "HeaderValue"
                                | "Request" | "Response" | "Handler" | "Error" | "Proxy" => {
                                    format!("HTTP{leaf}")
                                }
                                "HTTPError" => "HTTPError".to_string(),
                                _ => leaf.clone(),
                            }
                        } else {
                            leaf.clone()
                        };
                        // Preserve the source spellings for enum constructors before
                        // replacing the receiver, without cloning either AST string.
                        let alias = std::mem::take(alias_storage);
                        let leaf = std::mem::take(leaf_storage);
                        **receiver = Expr::Ident(type_name.clone(), span);
                        if type_name == "Fixed" && matches!(method, "new" | "over") {
                            *recv_type_out = Some(type_name.clone());
                            *resolved_ret_out = Some(Type::Named(type_name.clone()));
                            self.check_fixed_constructor(method, args, span);
                            return Some(Type::Named(type_name));
                        }
                        // `core.mem` allocator fields are constructor sentinels, not
                        // ordinary static types. Preserve the checked return fact on
                        // the call node before the generic static-method fallback.
                        if matches!(type_name.as_str(), "Arena" | "Bump" | "Pool" | "Fixed") {
                            if let Some(ret) = alloc_method_return(
                                &type_name,
                                method,
                                args,
                                span,
                                &mut self.diags,
                            ) {
                                for arg in args.iter_mut() {
                                    self.infer(&mut arg.expr);
                                }
                                *recv_type_out = Some(type_name.clone());
                                *resolved_ret_out = ret.clone();
                                return ret;
                            }
                        }
                        if matches!(type_name.as_str(), "SigningKey" | "X25519SecretKey")
                            && method == "generate"
                        {
                            self.diags.push(Diagnostic::error(
                                    "E1004",
                                    format!("`{type_name}.generate` was retired"),
                                    "constructors that draw entropy use `new_random` (D-SHAPE-CTORVERB1)".to_string(),
                                    format!("use `{type_name}.new_random()`"),
                                    Some(span),
                                ));
                            for arg in args.iter_mut() {
                                self.infer(&mut arg.expr);
                            }
                            let ret = result_ty(
                                Type::Named(type_name.clone()),
                                Type::Named("CryptoError".to_string()),
                            );
                            *recv_type_out = Some(type_name.clone());
                            *resolved_ret_out = Some(ret.clone());
                            return Some(ret);
                        }
                        if ns == "core.crypto.vault"
                            && type_name == "ExpiringSecret"
                            && method == "new"
                        {
                            if args.len() != 3 {
                                self.diags.push(wrong_core_arity(
                                    "ExpiringSecret.new",
                                    3,
                                    args.len(),
                                    span,
                                ));
                                for arg in args.iter_mut() {
                                    self.infer(&mut arg.expr);
                                }
                                return None;
                            }
                            let value_ty = self.infer(&mut args[0].expr).unwrap_or(Type::Int);
                            let allowed =
                                crate::Sema::Diagnostics::is_expiring_secret_member_type(&value_ty);
                            if !allowed {
                                self.diags.push(Diagnostic::error(
                                        "E0112",
                                        format!(
                                            "`ExpiringSecret.new` cannot own {}",
                                            value_ty.show()
                                        ),
                                        "expiring secrets accept only the closed, move-only secret family with audited zeroizing Drop behavior".to_string(),
                                        "convert text or bytes to `crypto.Secret`, or use `crypto.SigningKey` / `crypto.X25519SecretKey`".to_string(),
                                        Some(args[0].expr.span()),
                                    ));
                            }
                            self.check_take_arg_ownership(
                                "ExpiringSecret.new",
                                0,
                                &value_ty,
                                &mut args[0],
                            );
                            self.expect_core_arg(
                                "ExpiringSecret.new",
                                1,
                                &Type::Named(crate::Syntax::DURATION_TYPE.to_string()),
                                &mut args[1],
                            );
                            self.borrow_ctx = true;
                            let clock_ty = self.infer(&mut args[2].expr).unwrap_or_else(|| {
                                Type::Named(crate::Syntax::CLOCK_TYPE.to_string())
                            });
                            if !crate::Sema::Diagnostics::is_clock_type(&clock_ty) {
                                self.diags.push(Diagnostic::error(
                                        "E0112",
                                        format!(
                                            "`ExpiringSecret.new` wants `Clock` for argument 3, but this is {}",
                                            clock_ty.show()
                                        ),
                                        "secret expiry must observe the explicitly injected clock"
                                            .to_string(),
                                        "use `Clock.new(...)` for deterministic code or `Clock.system()` for production"
                                            .to_string(),
                                        Some(args[2].expr.span()),
                                    ));
                            }
                            let owner = self.core_source_owner_module(&ns, &type_name)?;
                            let source_name = self.canonical_nominal_name(owner, &type_name);
                            let ret = self.check_static_method(
                                &source_name, method, span, owner_type_args, type_args, args, checked_callee_out,
                            )?;
                            // Ordinary static lowering uses the checked source identity,
                            // never the retired native ExpiringSecretNew carrier.
                            *receiver = Box::new(Expr::Ident(source_name, receiver.span()));
                            let ret =
                                if crate::Sema::Diagnostics::is_deterministic_clock_type(&clock_ty)
                                {
                                    crate::Sema::Diagnostics::deterministic_clock_type(ret)
                                } else if crate::Sema::Diagnostics::is_system_clock_type(&clock_ty)
                                {
                                    crate::Sema::Diagnostics::system_clock_type(ret)
                                } else {
                                    ret
                                };
                            *recv_type_out = None;
                            *resolved_ret_out = Some(ret.clone());
                            return Some(ret);
                        }
                        let ty = Type::Named(type_name.clone());
                        let http_error = || Type::Named("HTTPError".to_string());
                        let http_result = |ok: Type| Type::Result {
                            ok: Box::new(ok),
                            err: Box::new(http_error()),
                        };
                        let http_static = match (type_name.as_str(), method, args.len()) {
                            ("HTTPMethod", "custom", 1) => {
                                self.expect_core_arg(
                                    "Method.custom",
                                    0,
                                    &Type::String,
                                    &mut args[0],
                                );
                                Some(http_result(ty.clone()))
                            }
                            (
                                "HTTPMethod",
                                "get" | "head" | "post" | "put" | "delete" | "connect" | "options"
                                | "trace" | "patch",
                                0,
                            ) => Some(ty.clone()),
                            ("HTTPStatus", "new", 1) => {
                                self.expect_core_arg("Status.new", 0, &Type::Int, &mut args[0]);
                                Some(http_result(ty.clone()))
                            }
                            ("HTTPVersion", "http_1_0" | "http_1_1" | "http_2", 0) => {
                                Some(ty.clone())
                            }
                            ("HTTPHeaderName", "new", 1) | ("HTTPHeaderValue", "new", 1) => {
                                self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                                Some(http_result(ty.clone()))
                            }
                            ("Headers", "new", 0) | ("Body", "empty", 0) => {
                                Some(ty.clone())
                            }
                            ("Body", "bytes", 1) => {
                                self.expect_core_arg(
                                    "Body.bytes",
                                    0,
                                    &Type::List(Box::new(u8_ty())),
                                    &mut args[0],
                                );
                                Some(ty.clone())
                            }
                            ("Body", "text", 1 | 2) => {
                                self.expect_core_arg("Body.text", 0, &Type::String, &mut args[0]);
                                if args.len() == 2 {
                                    self.expect_core_arg(
                                        "Body.text",
                                        1,
                                        &Type::Named("MIME".to_string()),
                                        &mut args[1],
                                    );
                                }
                                Some(ty.clone())
                            }
                            ("Body", "form" | "multipart", 1) => {
                                self.expect_core_arg(
                                    method,
                                    0,
                                    &Type::Map {
                                        key: Box::new(Type::String),
                                        key_span: None,
                                        value: Box::new(Type::String),
                                    },
                                    &mut args[0],
                                );
                                Some(ty.clone())
                            }
                            ("Body", "reader", 1 | 2) => {
                                self.expect_core_arg_moving(
                                    "Body.reader",
                                    0,
                                    &Type::Named("FileReader".to_string()),
                                    &mut args[0],
                                );
                                args[0].convention = AccessConvention::Move;
                                if args.len() == 2 {
                                    self.expect_core_arg(
                                        "Body.reader",
                                        1,
                                        &Type::Int,
                                        &mut args[1],
                                    );
                                }
                                Some(http_result(ty.clone()))
                            }
                            _ => None,
                        };
                        if let Some(ret) = http_static {
                            *resolved_ret_out = Some(ret.clone());
                            return Some(ret);
                        }
                        if let Some(ret) =
                            Collections::builtin_method_return(&ty, method, args.len(), true)
                        {
                            self.record_static_time_effect(&type_name, method, span);
                            let ret =
                                self.finish_builtin_method(receiver, method, &ty, args, span, ret);
                            let ret = if type_name == crate::Syntax::CLOCK_TYPE {
                                if method == "system" {
                                    ret.map(crate::Sema::Diagnostics::system_clock_type)
                                } else if method == "new" {
                                    ret.map(crate::Sema::Diagnostics::deterministic_clock_type)
                                } else {
                                    ret
                                }
                            } else if ns.as_str() == "core.crypto" {
                                ret.map(crate::Sema::Diagnostics::core_crypto_nominal)
                            } else {
                                ret
                            };
                            *resolved_ret_out = ret.clone();
                            return ret;
                        }
                        if is_json_type_name(&type_name) {
                            if let Some(ret) = self.check_core_json_lit(method, args, span) {
                                **receiver = Expr::Ident(type_name.clone(), span);
                                *resolved_ret_out = Some(ret.clone());
                                return Some(ret);
                            }
                        }
                        // Module-qualified payload constructors such as
                        // `encoding.DataEvent.Key("name")` rewrite to a type
                        // ident above, then parse as method calls. Resolve them
                        // as enum literals before the static-method fallback.
                        {
                            let has_variant = self
                                .resolve_enum_variants_cloned(&type_name)
                                .map(|v| v.contains_key(method))
                                .unwrap_or(false);
                            if has_variant {
                                let enum_name = if jet_foundation::CoreModuleExports::core_source_modules()
                                    .iter().any(|source| source.module == ns.as_str())
                                {
                                    let requested = format!("{alias}.{leaf}");
                                    // Keep unresolved source spelling for the
                                    // ordinary missing-enum diagnostic; never
                                    // substitute a bare carrier for its owner.
                                    self.import_qualified_nominal(&requested).unwrap_or(requested)
                                } else {
                                    // Genuine native-only enums retain their
                                    // existing checked intrinsic type identity.
                                    type_name.clone()
                                };
                                let mut enum_args: Vec<EnumLitArg> = args.iter_mut()
                                    .map(super::super::enum_literal_call_arg)
                                    .collect();
                                let ty = self.check_enum_lit(
                                    &enum_name,
                                    method,
                                    &mut enum_args,
                                    span,
                                    Some(span),
                                );
                                for (arg, enum_arg) in args.iter_mut().zip(enum_args) {
                                    if let EnumLitArg::Positional(expr) = enum_arg {
                                        arg.expr = expr;
                                    }
                                }
                                // Leave recv_type unset: this is a checked
                                // enum constructor, not a value receiver.
                                **receiver = Expr::Ident(enum_name, span);
                                return Some(ty);
                            }
                        }
                        return self.check_static_method(&type_name,
                        method,
                        span,
                        owner_type_args,
                        type_args,
                        args, checked_callee_out);
                    }
                    if ns == "core.encoding" && leaf == "EncodingLimits" && method == "safe" {
                        return self.check_static_method("EncodingLimits",
                        method,
                        span,
                        owner_type_args,
                        type_args,
                        args, checked_callee_out);
                    }
                    if ns == "core.encoding.cbor" && leaf == "CBOROptions" && method == "safe" {
                        return self.check_static_method("CBOROptions",
                        method,
                        span,
                        owner_type_args,
                        type_args,
                        args, checked_callee_out);
                    }
                    if ns == "core.encoding.xml"
                        && (leaf == "XMLLimits" || leaf == "XMLParseOptions")
                        && method == "safe"
                    {
                        return self.check_static_method(leaf,
                        method,
                        span,
                        owner_type_args,
                        type_args,
                        args, checked_callee_out);
                    }
                    if ns == "core.email" && leaf == "Limits" && method == "safe" {
                        return self.check_static_method("Limits",
                        method,
                        span,
                        owner_type_args,
                        type_args,
                        args, checked_callee_out);
                    }
                    if ns == "core.data" && leaf == "DataLimits" && method == "safe" {
                        return self.check_static_method("DataLimits",
                        method,
                        span,
                        owner_type_args,
                        type_args,
                        args, checked_callee_out);
                    }
                    if let Some(owner) = self.core_source_owner_module(&ns, leaf) {
                        if self.modules.is_some_and(|states|
                            states[owner].registry.method(leaf, method).is_some())
                            && jet_foundation::CoreModuleExports::core_modules().iter()
                                .find(|declaration| declaration.module == ns)
                                .is_some_and(|declaration|
                                    declaration.type_exports.iter().any(|(name, _)| *name == leaf))
                        {
                            let source_name = self.canonical_nominal_name(owner, leaf);
                            let ret = self.check_static_method(
                                &source_name, method, span, owner_type_args, type_args, args, checked_callee_out,
                            );
                            **receiver = Expr::Ident(source_name, span);
                            *resolved_ret_out = ret.clone();
                            return ret;
                        }
                    }
                }
            }
        }
        // D-VERDICT-1867-1: an inline module may publicly re-export a
        // foreign namespace. Keep the namespace hop structural here;
        // the actual library function still resolves through the same
        // imported-module checker as a direct `use`.
        if let Expr::Field(base, leaf, _) = &**receiver {
            if let Expr::Ident(inline_alias, inline_alias_span) = &**base {
                if let Some(&mod_idx) = self
                    .inline_reexport_foreign
                    .get(&(inline_alias.clone(), leaf.clone()))
                    .filter(|_| self.lookup(inline_alias).is_none())
                {
                    return self.infer_import_call(
                        inline_alias,
                        mod_idx,
                        method,
                        *inline_alias_span,
                        span,
                        type_args,
                        args,
                        resolved_ret_out,
                    );
                }
            }
        }
        // Direct grouped imports bind a type name to its owning Core module.
        // Resolve constructors on that alias before the module-call fallback;
        // otherwise `use core.encoding.[Dyn]` is misread as
        // `core.encoding.Bool`.
        if let Expr::Ident(alias, alias_span) = &**receiver {
            if let (None, Some(module), Some(item)) = (
                self.lookup(alias),
                self.core_imports.get(alias).cloned(),
                self.core_item_imports.get(alias).cloned(),
            ) {
                if is_json_type_name(&item) {
                    if let Some(ret) = self.check_core_json_lit(method, args, span) {
                        self.record_import_alias_reference(alias, *alias_span);
                        **receiver = Expr::Ident(item, span);
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                }
                let enum_identity = if let Some(owner) =
                    self.core_source_enum_owner_module(&module, &item)
                {
                    self.modules.is_some_and(|states|
                        states[owner].registry.enum_variants(&item)
                            .is_some_and(|variants| variants.contains_key(method)))
                        .then(|| self.canonical_nominal_name(owner, &item))
                } else {
                    crate::Sema::CheckerCoreLib::core_native_enum_variant_names(
                        &module, &item,
                    ).filter(|variants| variants.contains(&method)).map(|_| item.clone())
                };
                if let Some(identity) = enum_identity {
                    self.record_import_alias_reference(alias, *alias_span);
                    let mut enum_args: Vec<EnumLitArg> = args.iter_mut()
                        .map(super::super::enum_literal_call_arg).collect();
                    let ty = self.check_enum_lit(
                        &identity,
                        method,
                        &mut enum_args,
                        span,
                        Some(span),
                    );
                    for (arg, enum_arg) in args.iter_mut().zip(enum_args) {
                        if let EnumLitArg::Positional(expr) = enum_arg {
                            arg.expr = expr;
                        }
                    }
                    **receiver = Expr::Ident(identity, span);
                    *resolved_ret_out = Some(ty.clone());
                    return Some(ty);
                }
                if module == "core.encoding" && item == "EncodingLimits" && method == "safe" {
                    let ret = self.check_static_method(&item,
                    method,
                    span,
                    owner_type_args,
                    type_args,
                    args, checked_callee_out);
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
                // Source-owned Core records retain their ordinary associated
                // methods; an imported type alias is not a module receiver.
                if let Some(owner) = self.core_source_owner_module(&module, &item) {
                    if self.modules.is_some_and(|states|
                        states[owner].registry.method(&item, method).is_some())
                    {
                        let source_name = self.canonical_nominal_name(owner, &item);
                        self.record_import_alias_reference(alias, *alias_span);
                        let ret = self.check_static_method(
                            &source_name, method, span, owner_type_args, type_args, args, checked_callee_out,
                        );
                        **receiver = Expr::Ident(source_name, span);
                        *resolved_ret_out = ret.clone();
                        return ret;
                    }
                }
                if module == "core.crypto"
                    && item == "Secret"
                    && matches!(method, "from_text" | "from_bytes")
                {
                    self.record_import_alias_reference(alias, *alias_span);
                    **receiver = Expr::Ident(item.clone(), span);
                    let ret = self.check_static_method(&item,
                    method,
                    span,
                    owner_type_args,
                    type_args,
                    args, checked_callee_out);
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
                let _ = alias_span;
            }
        }
        if let Some((module, alias, alias_span)) = self.core_module_path_from_receiver(receiver) {
            self.inferred_call_type_args = None;
            let ret = self.infer_core_call(
                &module,
                method,
                Some(&alias),
                alias_span,
                span,
                type_args,
                args,
                resolved_ret_out,
            );
            self.record_inferred_call_type_args(type_args);
            // A source-owned call returns its effective carrier for expression
            // inference and publishes its plain declared return separately.
            if resolved_ret_out.is_none() {
                *resolved_ret_out = ret.clone();
            }
            return ret;
        }
        if let Expr::Ident(alias, alias_span) = &**receiver {
            if let Some(module) = self.core_imports.get(alias).filter(|_| self.lookup(alias).is_none()).cloned() {
                self.inferred_call_type_args = None;
                let ret = self.infer_core_call(
                    &module,
                    method,
                    Some(alias),
                    *alias_span,
                    span,
                    type_args,
                    args,
                    resolved_ret_out,
                );
                self.record_inferred_call_type_args(type_args);
                if resolved_ret_out.is_none() {
                    *resolved_ret_out = ret.clone();
                }
                return ret;
            }
            if let Some(&mod_idx) = self.imports.get(alias).filter(|_| self.lookup(alias).is_none()) {
                self.inferred_call_type_args = None;
                let ret = self.infer_import_call(
                    alias,
                    mod_idx,
                    method,
                    *alias_span,
                    span,
                    type_args,
                    args,
                    resolved_ret_out,
                );
                self.record_inferred_call_type_args(type_args);
                return ret;
            }
            // D-MOD2: inline code module call — `math.double(x)` where `math` is an
            // inline `module math { … }` in this file. Resolve via mangled name.
            if let Some(canonical) = self.code_modules.get(alias.as_str()).filter(|_| self.lookup(alias).is_none()) {
                let mangled = jet_foundation::Names::member_name(canonical, method);
                self.inferred_call_type_args = None;
                let ret = self.infer_code_module_call(
                    alias,
                    &mangled,
                    *alias_span,
                    span,
                    type_args,
                    args,
                    resolved_ret_out,
                );
                self.record_inferred_call_type_args(type_args);
                return ret;
            }
        }
        // D-TYPE2-SPELL1: an inline range uses the destination-owned
        // conversion seam. Runtime inputs stay fallible; a literal is
        // discharged here after the same interval proof used by literal
        // parameter/return checking.
        if self.lookup(crate::Syntax::TYPE_INT).is_none()
            && method == crate::Syntax::conversion_method_for_source("Int")
            && inline_range_descriptor(receiver).is_some()
        {
            let (lo, hi) =
                inline_range_descriptor(receiver).expect("inline range descriptor checked above");
            if lo > hi {
                self.diags.push(Diagnostic::error(
                    "E0137",
                    format!("this range is empty — {lo} is after {hi}"),
                    "a range's low bound must not be greater than its high bound".to_string(),
                    format!("write `{hi}..{lo}` (swap the bounds), or fix the values"),
                    Some(span),
                ));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
            if args.len() != 1 {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!(
                        "`Int({lo}..{hi}).{method}` takes one value, got {}",
                        args.len()
                    ),
                    "a range conversion wraps exactly one integer value".to_string(),
                    format!("write `Int({lo}..{hi}).{method}(value)`"),
                    Some(span),
                ));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
            let source = Type::Int;
            let old = self.expected_type.replace(source.clone());
            let got = self.infer(&mut args[0].expr);
            self.expected_type = old;
            if let Some(got) = got.as_ref() {
                self.check_type_assignable(&source, got, args[0].expr.span());
            }
            let target = Type::InlineRange {
                base: Box::new(Type::Int),
                lo,
                hi,
            };
            // The inner call is sema-approved descriptor metadata. TIR
            // reads it to recognize the static receiver without redoing
            // parser or type inference work in an engine.
            if let Expr::Call(call) = receiver.as_mut() {
                call.resolved_ret = Some(target.clone());
            }
            if let Some((value, literal_span)) = inline_range_literal(&args[0].expr) {
                if !IntegerInterval::new(i128::from(lo), i128::from(hi)).contains_exact(&value) {
                    self.diags.push(
                        crate::Sema::Diagnostics::inline_range_literal_out_of_bounds(
                            &value,
                            lo,
                            hi,
                            literal_span,
                        ),
                    );
                }
                *resolved_ret_out = Some(target.clone());
                return Some(target);
            }
            // D-FAILURE-FOUNDATION1: ordinary calls auto-propagate their
            // Result carrier, which would hide the binding-level range rule
            // below by rewriting this method call to `Try`. Keep the source
            // diagnostic at the conversion seam while the binding is active;
            // an explicit `Try` has nonzero `failure_auto_depth` and remains
            // legal.
            if self.failure_auto_depth == 0 && self.current_binding_name.is_some() {
                self.diags
                    .push(crate::Sema::Diagnostics::inline_range_runtime_conversion(
                        lo,
                        hi,
                        method,
                        args[0].expr.span(),
                    ));
            }
            let ret = Type::Result {
                ok: Box::new(target),
                err: Box::new(Type::String),
            };
            *resolved_ret_out = Some(ret.clone());
            return Some(ret);
        }
        // D-FOUND-PLATFORM1=A: a leading `.cmd("key")` is contextual syntax,
        // not a dynamic host shortcut call. The argument expected by `button`
        // supplies the UiShortcut type; turn the parser's empty receiver into
        // the canonical static owner before TIR lowering.
        if method == "cmd"
            && matches!(
                receiver.as_ref(),
                Expr::Ident(name, _) if name.is_empty() || name == "UIShortcut"
            )
        {
            let receiver_span = receiver.span();
            **receiver = Expr::Ident("UIShortcut".to_string(), receiver_span);
            let shortcut_ty = Type::Named("UIShortcut".to_string());
            *resolved_ret_out = Some(shortcut_ty.clone());
            if args.len() != 1 {
                self.diags
                    .push(wrong_core_arity("UIShortcut.cmd", 1, args.len(), span));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return Some(shortcut_ty);
            }
            let literal = match args[0].expr.without_parens() {
                Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
                    crate::AST::StrPart::Lit(value) => Some(value.clone()),
                    _ => None,
                },
                _ => None,
            };
            self.expect_core_arg(
                "UIShortcut.cmd",
                0,
                &Type::String,
                &mut args[0],
            );
            let valid = args[0].label.is_none()
                && literal.as_deref().is_some_and(|value| !value.trim().is_empty());
            if !valid {
                let shortcut = literal.as_deref().unwrap_or("<expression>");
                let reason = if args[0].label.is_some() {
                    "the key must be positional"
                } else if literal.is_none() {
                    "the key must be one plain string literal"
                } else {
                    "the key must not be empty"
                };
                self.diags.push(Diagnostic::from_row(
                    "E2938",
                    &[("shortcut", shortcut), ("reason", reason)],
                    Some(args[0].span),
                ));
            }
            return Some(shortcut_ty);
        }
        if let Expr::Ident(type_name, type_span) = &**receiver {
            let display_type_name = self.display_type_name(type_name, None);
            // D-VALIDATE1: the use-site escape keeps the same `check` rule
            // vocabulary as an in-body validator. `over` is a compiler-owned
            // static head; it materializes the ordinary generated builder so
            // every later `check`/`finish` call remains normal Jet code.
            if type_name == Syntax::TYPE_VALIDATE
                && method == "over"
                && self.lookup(type_name).is_none()
            {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("Validate.over", 1, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                let value_ty = self.infer(&mut args[0].expr)?;
                let value_span = args[0].span;
                let value =
                    std::mem::replace(&mut args[0].expr, Expr::Int(0, value_span, None, None));
                let builder_ty = Type::Apply {
                    name: Syntax::TYPE_VALIDATE_BUILDER.to_string(),
                    args: vec![value_ty],
                };
                **receiver = Expr::StructLit {
                    type_name: Syntax::TYPE_VALIDATE_BUILDER.to_string(),
                    type_args: match &builder_ty {
                        Type::Apply { args, .. } => args.clone(),
                        _ => Vec::new(),
                    },
                    import_ns: None,
                    as_trait: None,
                    fields: vec![
                        ("value".to_string(), span, Expr::Copy(Box::new(value), span)),
                        ("errors".to_string(), span, Expr::ListLit(Vec::new(), span)),
                    ],
                    inferred: false,
                    span,
                };
                args.clear();
                *recv_type_out = Some(Syntax::INTERNAL_VALIDATE_OVER.to_string());
                *resolved_ret_out = Some(builder_ty.clone());
                return Some(builder_ty);
            }
            // D-SERDE13=B: `Data.Text(x)` etc. — the retired spelling of the value
            // tree. Point at `Dyn` (no alias, I8) before generic resolution.
            if type_name == "Data" {
                self.diags.push(data_renamed_to_dyn(*type_span));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(json_ty());
            }
            // D-VALIDATE-DECODE1=B: one shared transform frames every
            // error in a child Result while preserving its success type.
            // Generated codecs use this for the field/index boundary;
            // it is an error-list helper, not another decode API.
            if type_name == "FieldError" && method == "under" {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("FieldError.under", 2, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                self.expect_core_arg("FieldError.under", 0, &Type::String, &mut args[0]);
                let result_ty = self.infer(&mut args[1].expr)?;
                if !matches!(&result_ty, Type::Result { err, .. } if **err == decode_error_ty()) {
                    self.diags.push(Diagnostic::error(
                            "E0905",
                            "`FieldError.under` expects a result with `[FieldError]` errors".to_string(),
                            "the framing helper preserves the child result and prefixes every accumulated decode failure".to_string(),
                            "pass a typed decode/accessor result, such as `tree.field(\"name\")?.decode<T>()`".to_string(),
                            Some(args[1].expr.span()),
                        ));
                }
                *resolved_ret_out = Some(result_ty.clone());
                return Some(result_ty);
            }
            if self.lookup(type_name).is_none()
                && matches!(type_name.as_str(), "SigningKey" | "X25519SecretKey")
                && method == "generate"
            {
                self.diags.push(Diagnostic::error(
                    "E1004",
                    format!("`{type_name}.generate` was retired"),
                    "constructors that draw entropy use `new_random` (D-SHAPE-CTORVERB1)"
                        .to_string(),
                    format!("use `{type_name}.new_random()`"),
                    Some(span),
                ));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return Some(result_ty(
                    Type::Named(type_name.clone()),
                    Type::Named("CryptoError".to_string()),
                ));
            }
            // D-ENC-DYN1=A+: `Dyn`/`JSON`/`TOML`/`YAML`/`CSV` name the one dynamic
            // value; they are reserved core type names (a user type may not redefine them).
            // TIR lowers both constructors from `resolved_ret`; the checked
            // dynamic-value type is carried on the call node here, never
            // re-derived from the receiver's spelling.
            if is_json_type_name(type_name) {
                if let Some(ret) = self.check_core_json_lit(method, args, span) {
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
            }
            // D-DBDRIVER1: `DBValue.Int(n)` / `.Float(f)` / `.Text(s)` / `.Bool(b)` —
            // the tagged SQL parameter/column value construction (same mechanism as
            // `Data`/`JSON` above). `DBValue.Null` (no args) is a `Field`, not a
            // `MethodCall` — handled in `infer_field` alongside `Data.Null`.
            if type_name == Syntax::TYPE_DB_VALUE {
                if let Some(ret) = self.check_core_dbvalue_lit(method, args, span) {
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
            }
            // D-ATOMIC-WIDTH1=A: generic Atomic constructors are static
            // Prelude calls, not user associated methods. Keep the source
            // generic argument on the checked result so every backend emits
            // the same one-word carrier.
            if type_name == Syntax::TYPE_ATOMIC
                && self.lookup(type_name).is_none()
                && owner_type_args.len() == 1
            {
                let inner = owner_type_args[0].clone();
                if jet_foundation::Layout::atomic_scalar_type(&inner) {
                    let (expected, ret) = match method {
                        "new" => (
                            inner.clone(),
                            Type::Apply {
                                name: Syntax::TYPE_ATOMIC.to_string(),
                                args: vec![inner.clone()],
                            },
                        ),
                        "try_new" => (
                            inner.clone(),
                            Type::Result {
                                ok: Box::new(Type::Apply {
                                    name: Syntax::TYPE_ATOMIC.to_string(),
                                    args: vec![inner.clone()],
                                }),
                                err: Box::new(Type::Named(
                                    Syntax::TYPE_ALLOC_ERROR.to_string(),
                                )),
                            },
                        ),
                        _ => {
                            // This is not a constructor; let normal static
                            // method resolution produce its canonical error.
                            return None;
                        }
                    };
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(
                            &format!("Atomic.{method}"),
                            1,
                            args.len(),
                            span,
                        ));
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                    } else {
                        self.expect_core_arg(
                            &format!("Atomic.{method}"),
                            0,
                            &expected,
                            &mut args[0],
                        );
                    }
                    *recv_type_out = Some(Syntax::TYPE_ATOMIC.to_string());
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
            }
            {
                let has_variant = self
                    .resolve_enum_variants_cloned(type_name)
                    .map(|v| v.contains_key(method))
                    .unwrap_or(false);
                if has_variant {
                    let mut enum_args: Vec<EnumLitArg> = args.iter_mut()
                        .map(super::super::enum_literal_call_arg)
                        .collect();
                    let ty =
                        self.check_enum_lit(type_name, method, &mut enum_args, span, Some(span));
                    for (a, ea) in args.iter_mut().zip(enum_args) {
                        if let EnumLitArg::Positional(e) = ea {
                            a.expr = e;
                        }
                    }
                    return Some(ty);
                }
            }
            let distinct_numeric_conversion = self.registry.is_distinct(type_name)
                && self.registry.distinct_base(type_name).is_some_and(|base| {
                    base.is_numeric() && Syntax::numeric_conversion_source(method).is_some()
                });
            if ((type_name == "EncodingLimits"
                || type_name == "CBOROptions"
                || type_name == "XMLLimits"
                || type_name == "XMLParseOptions"
                || type_name == "Limits"
                || type_name == "DataLimits")
                && method == "safe")
                || (!distinct_numeric_conversion
                    && self.resolve_method_sig(type_name, method).is_some())
                || (matches!(method, "from" | "raw" | "encode_hole")
                    && self.is_checked_text_type_name(type_name))
            {
                self.record_static_time_effect(type_name, method, span);
                let ret = self.check_static_method(type_name,
                method,
                span,
                owner_type_args,
                type_args,
                args, checked_callee_out);
                let ret = if type_name == crate::Syntax::CLOCK_TYPE && method == "system" {
                    ret.map(crate::Sema::Diagnostics::system_clock_type)
                } else {
                    ret
                };
                *resolved_ret_out = ret.clone();
                return ret;
            }
            // D-FIDELITY-API1=A: `core.perf.Perf` static API. `use core.perf as perf`
            // remains accepted as the existing module-alias path.
            if type_name == "Perf" && !self.registry.contains("Perf") {
                let ret = self.infer_core_call(
                    "core.perf",
                    method,
                    None,
                    receiver.span(),
                    span,
                    type_args,
                    args,
                    resolved_ret_out,
                );
                if resolved_ret_out.is_none() {
                    *resolved_ret_out = ret.clone();
                }
                return ret;
            }
            // A value binding wins over an ambient built-in type spelling.
            // Keep this aligned with TIR's static-call shadow check so sema
            // never accepts code that lowering must reject.
            if self.lookup(type_name).is_none() {
                if let Some(mut ty) = builtin_type_from_ident(type_name) {
                    // Generic collection statics retain their written owner
                    // argument. A contextual success type refines only an
                    // omitted argument; the bare builtin default is last.
                    if let Type::List(inner) = &mut ty {
                        if let Some(element) = owner_type_args.first() {
                            **inner = self.resolve_type(element.clone());
                        } else {
                            let expected = match &self.expected_type {
                                Some(Type::Result { ok, .. }) => Some(ok.as_ref()),
                                other => other.as_ref(),
                            };
                            if let Some(Type::List(element)) = expected {
                                **inner = (**element).clone();
                            }
                        }
                    }
                    if let Some(ret) =
                        Collections::builtin_method_return(&ty, method, args.len(), true)
                    {
                        self.record_static_time_effect(type_name, method, span);
                        let ret =
                            self.finish_builtin_method(receiver, method, &ty, args, span, ret);
                        let ret = if type_name == crate::Syntax::CLOCK_TYPE {
                            if method == "system" {
                                ret.map(crate::Sema::Diagnostics::system_clock_type)
                            } else if method == "new" {
                                ret.map(crate::Sema::Diagnostics::deterministic_clock_type)
                            } else {
                                ret
                            }
                        } else {
                            ret
                        };
                        // Keep the checked static return fact on the method-call
                        // node. TIR constructors consume this fact; re-deriving it
                        // from the receiver would weaken shadowing and fallibility.
                        *resolved_ret_out = ret.clone();
                        return ret;
                    }
                }
            }
            // D-SHAPE-CONVERT1=A: every numeric-backed distinct type,
            // including #UnitFamily members, gets the same destination-owned
            // conversion as its base (`UserId.from_int`, `Meter.from_float`).
            if self.registry.is_distinct(type_name) {
                if let Some(base) = self.registry.distinct_base(type_name).cloned() {
                    let base_method = Syntax::conversion_method_for_source(&base.name());
                    if !base.is_numeric() && method == base_method {
                        if args.len() != 1 {
                            self.diags.push(Diagnostic::error(
                                "E0104",
                                format!(
                                    "`{display_type_name}.{method}` takes one value, got {}",
                                    args.len()
                                ),
                                "a distinct conversion wraps exactly one value of its base type"
                                    .to_string(),
                                format!("write `{display_type_name}.{method}(value)`"),
                                Some(span),
                            ));
                            for arg in args.iter_mut() {
                                self.infer(&mut arg.expr);
                            }
                            return None;
                        }
                        let old = self.expected_type.replace(base.clone());
                        let got = self.infer(&mut args[0].expr);
                        self.expected_type = old;
                        if got.as_ref().is_some_and(|got| got != &base) {
                            self.diags.push(Diagnostic::error(
                                    "E0108",
                                    format!("argument to `{display_type_name}.{method}` should be {}, not {}", base.name(), got.as_ref().unwrap().name()),
                                    "the source name fixes the conversion input type".to_string(),
                                    format!("pass a {} value", base.name()),
                                    Some(args[0].expr.span()),
                                ));
                        }
                        let ret = Type::Named(type_name.clone());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    if let Some(source) = base
                        .is_numeric()
                        .then_some(method)
                        .and_then(Syntax::numeric_conversion_source)
                        .and_then(crate::AST::numeric_type_from_name)
                    {
                        if args.len() != 1 {
                            self.diags.push(Diagnostic::error(
                                "E0104",
                                format!(
                                    "`{display_type_name}.{method}` takes one value, got {}",
                                    args.len()
                                ),
                                "a distinct conversion wraps exactly one value of its base type"
                                    .to_string(),
                                format!("write `{display_type_name}.{method}(value)`"),
                                Some(span),
                            ));
                            for arg in args.iter_mut() {
                                self.infer(&mut arg.expr);
                            }
                            return None;
                        }
                        let old = self.expected_type.replace(source.clone());
                        let got = self.infer(&mut args[0].expr);
                        self.expected_type = old;
                        if got.as_ref().is_some_and(|got| got != &source) {
                            self.diags.push(Diagnostic::error(
                                    "E0108",
                                    format!("argument to `{display_type_name}.{method}` should be {}, not {}", source.name(), got.as_ref().unwrap().name()),
                                    "the source name fixes the conversion input type".to_string(),
                                    format!("pass a {} value", source.name()),
                                    Some(args[0].expr.span()),
                                ));
                        }
                        let target = Type::Named(type_name.clone());
                        let base_conversion =
                            Collections::numeric_conversion_return(&base, method, 1)
                                .flatten()
                                .expect("numeric distinct conversion has a numeric base");
                        let converted_literal = match (&args[0].expr, &source, &base) {
                            (Expr::Int(n, literal_span, _, raw), source, base)
                                if source.is_integer() && base.is_integer() =>
                            {
                                let value = exact_integer_literal(*n, raw.as_deref());
                                let fits_base = base
                                    .integer_range()
                                    .map(IntegerInterval::from_bounds)
                                    .is_none_or(|interval| interval.contains_exact(&value));
                                fits_base.then_some((value, *literal_span))
                            }
                            (Expr::Float(n, literal_span, _, _), source, base)
                                if source.is_float() && base.is_integer() && n.is_finite() =>
                            {
                                let truncated = n.trunc();
                                let (lo, upper_exclusive) = match base {
                                    Type::Int => (i64::MIN as f64, -(i64::MIN as f64)),
                                    Type::IntN { .. } => {
                                        let interval = IntegerInterval::from_bounds(
                                            base.integer_range().expect(
                                                "fixed integer widths carry an interval fact",
                                            ),
                                        );
                                        (interval.lo as f64, interval.hi as f64 + 1.0)
                                    }
                                    _ => unreachable!(),
                                };
                                (truncated >= lo && truncated < upper_exclusive).then_some((
                                    crate::Numeric::CtBigInt::from_int(truncated as i64),
                                    *literal_span,
                                ))
                            }
                            _ => None,
                        };
                        let literal_in_range = self.registry.distinct_range(type_name).and_then(|(lo, hi)| {
                                converted_literal.map(|(value, literal_span)| {
                                    let interval = IntegerInterval::new(i128::from(lo), i128::from(hi));
                                    if !interval.contains_exact(&value) {
                                        let shown = value.to_string_rep();
                                        self.diags.push(Diagnostic::error(
                                            "E0135",
                                            format!("`{shown}` is outside `{display_type_name}`'s range {lo}..{hi}"),
                                            format!("a range type only holds values inside its bounds; `{shown}` can never be a `{display_type_name}`"),
                                            format!("use a value in `{lo}..{hi}`, or widen the type's range"),
                                            Some(literal_span),
                                        ));
                                    }
                                })
                            }).is_some();
                        let ret = if matches!(base_conversion, Type::Result { .. })
                            || (!literal_in_range
                                && self.registry.distinct_range(type_name).is_some())
                        {
                            Type::Result {
                                ok: Box::new(target),
                                err: Box::new(Type::String),
                            }
                        } else {
                            target
                        };
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                }
            }
            // D-COLLBREADTH1=A: `Set.from([...])` → `Set<T>`.
            // T is inferred from the list argument's element type.
            if type_name == "Set" && method == "from" && args.len() == 1 {
                let arg_ty = self.infer(&mut args[0].expr);
                let elem_ty = match arg_ty {
                    Some(Type::List(inner)) => *inner,
                    _ => Type::Int,
                };
                if !self.hashable_type_eligible(&elem_ty) {
                    self.diags.push(Diagnostic::error(
                            "E0506",
                            format!("`Set<{}>` is not valid — `{}` is not hashable", elem_ty.name(), elem_ty.name()),
                            "Set elements must implement Hash and Eq; use Int, Bool, String, Char, or a named type".to_string(),
                            format!("change the element type to a hashable type, or use a `[{}]` list instead", elem_ty.name()),
                            Some(span),
                        ));
                }
                return Some(Type::Apply {
                    name: "Set".to_string(),
                    args: vec![elem_ty],
                });
            }
            // #1478: `Set.new()` → empty HashSet; elem from annotation/expected.
            if type_name == "Set" && method == "new" && args.is_empty() {
                let elem_ty = match &self.expected_type {
                    Some(Type::Apply { name, args, .. }) if name == "Set" && !args.is_empty() => {
                        args[0].clone()
                    }
                    _ => Type::Int,
                };
                if !self.hashable_type_eligible(&elem_ty) {
                    self.diags.push(Diagnostic::error(
                            "E0506",
                            format!("`Set<{}>` is not valid — `{}` is not hashable", elem_ty.name(), elem_ty.name()),
                            "Set elements must implement Hash and Eq; use Int, Bool, String, Char, or a named type".to_string(),
                            format!("change the element type to a hashable type, or use a `[{}]` list instead", elem_ty.name()),
                            Some(span),
                        ));
                }
                let ret = Type::Apply {
                    name: "Set".to_string(),
                    args: vec![elem_ty],
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            // #1477: `Map.new()` / `Map.from_keys(keys, default)` / `Map.fromkeys(keys, default)`.
            if type_name == Syntax::TYPE_MAP && method == "new" && args.is_empty() {
                let (key, value) = match &self.expected_type {
                    Some(Type::Map { key, value, .. }) => ((**key).clone(), (**value).clone()),
                    _ => (Type::Int, Type::Int),
                };
                let ret = Type::Map {
                    key: Box::new(key),
                    key_span: None,
                    value: Box::new(value),
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            if type_name == Syntax::TYPE_MAP
                && matches!(method, "from_keys" | "fromkeys")
                && args.len() == 2
            {
                let keys_ty = self.infer(&mut args[0].expr);
                let key = match keys_ty {
                    Some(Type::List(inner)) => *inner,
                    _ => Type::Int,
                };
                let value = self.infer(&mut args[1].expr).unwrap_or(Type::Int);
                let ret = Type::Map {
                    key: Box::new(key),
                    key_span: None,
                    value: Box::new(value),
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            // D-ITERTOOLS1=A: `Rank.from([...])` → `Rank<T>`.
            if type_name == Syntax::TYPE_RANK && method == "from" && args.len() == 1 {
                let arg_ty = self.infer(&mut args[0].expr);
                let elem_ty = match arg_ty {
                    Some(Type::List(inner)) => *inner,
                    _ => Type::Int,
                };
                return Some(Type::Apply {
                    name: Syntax::TYPE_RANK.to_string(),
                    args: vec![elem_ty],
                });
            }
            if type_name == Syntax::TYPE_RANK && method == "new" && args.is_empty() {
                let elem_ty = match &self.expected_type {
                    Some(Type::Apply { name, args, .. })
                        if name == Syntax::TYPE_RANK && !args.is_empty() =>
                    {
                        args[0].clone()
                    }
                    _ => Type::Int,
                };
                let ret = Type::Apply {
                    name: Syntax::TYPE_RANK.to_string(),
                    args: vec![elem_ty],
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            // D-ITERTOOLS1=A: `PriorityQueue.from([...])` / `.new()`.
            if type_name == Syntax::TYPE_PRIORITY_QUEUE && method == "from" && args.len() == 1 {
                let arg_ty = self.infer(&mut args[0].expr);
                let elem_ty = match arg_ty {
                    Some(Type::List(inner)) => *inner,
                    _ => Type::Int,
                };
                return Some(Type::Apply {
                    name: Syntax::TYPE_PRIORITY_QUEUE.to_string(),
                    args: vec![elem_ty],
                });
            }
            if type_name == Syntax::TYPE_PRIORITY_QUEUE && method == "new" && args.is_empty() {
                let elem_ty = match &self.expected_type {
                    Some(Type::Apply { name, args, .. })
                        if name == Syntax::TYPE_PRIORITY_QUEUE && !args.is_empty() =>
                    {
                        args[0].clone()
                    }
                    _ => Type::Int,
                };
                let ret = Type::Apply {
                    name: Syntax::TYPE_PRIORITY_QUEUE.to_string(),
                    args: vec![elem_ty],
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            // D-ITERTOOLS1=A: `Cache<K,V>.new(capacity)`, expected type supplies K/V.
            if type_name == Syntax::TYPE_LRU && method == "new" && args.len() == 1 {
                self.expect_core_arg("new", 0, &Type::Int, &mut args[0]);
                let (key_ty, value_ty) = match &self.expected_type {
                    Some(Type::Apply { name, args, .. })
                        if name == Syntax::TYPE_LRU && args.len() >= 2 =>
                    {
                        (args[0].clone(), args[1].clone())
                    }
                    _ => (Type::String, Type::Int),
                };
                let ret = Type::Apply {
                    name: Syntax::TYPE_LRU.to_string(),
                    args: vec![key_ty, value_ty],
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            if type_name == Syntax::TYPE_BITS && method == "new" && args.is_empty() {
                return Some(Type::Named(Syntax::TYPE_BITS.to_string()));
            }
            if type_name == Syntax::TYPE_BYTES && method == "new" && args.is_empty() {
                return Some(Type::Named(Syntax::TYPE_BYTES.to_string()));
            }
            if type_name == Syntax::TYPE_BYTES && method == "with_capacity" && args.len() == 1 {
                self.expect_core_arg("with_capacity", 0, &Type::Int, &mut args[0]);
                return Some(Type::Named(Syntax::TYPE_BYTES.to_string()));
            }
            if type_name == Syntax::TYPE_BYTES && method == "from" && args.len() == 1 {
                self.expect_core_arg(
                    "from",
                    0,
                    &Type::List(Box::new(Type::IntN {
                        signed: false,
                        bits: 8,
                    })),
                    &mut args[0],
                );
                return Some(Type::Named(Syntax::TYPE_BYTES.to_string()));
            }
            // D-COLLBREADTH1=A: `Queue.new()` → `Queue<T>`.
            // T is inferred from the type annotation's expected type.
            // D-COLLBREADTH1=A: `Queue.init([...])` → collect list into VecDeque.
            if type_name == Syntax::TYPE_QUEUE && method == "init" && args.len() == 1 {
                let arg_ty = self.infer(&mut args[0].expr);
                let elem_ty = match arg_ty {
                    Some(Type::List(inner)) => *inner,
                    _ => Type::Int,
                };
                return Some(Type::Apply {
                    name: Syntax::TYPE_QUEUE.to_string(),
                    args: vec![elem_ty],
                });
            }
            if type_name == Syntax::TYPE_QUEUE && method == "new" && args.is_empty() {
                let elem_ty = match &self.expected_type {
                    Some(Type::Apply { name, args, .. })
                        if name == Syntax::TYPE_QUEUE && !args.is_empty() =>
                    {
                        args[0].clone()
                    }
                    _ => Type::Int,
                };
                let ret = Type::Apply {
                    name: Syntax::TYPE_QUEUE.to_string(),
                    args: vec![elem_ty],
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            // D-TAG1: `Tally.new()` → `Tally<T>`. Turbofish / annotation supplies T.
            if type_name == Syntax::TYPE_TALLY && method == "new" && args.is_empty() {
                let elem_ty = type_args
                    .first()
                    .or_else(|| owner_type_args.first())
                    .cloned()
                    .unwrap_or_else(|| match &self.expected_type {
                        Some(Type::Apply { name, args, .. })
                            if name == Syntax::TYPE_TALLY && !args.is_empty() =>
                        {
                            args[0].clone()
                        }
                        _ => Type::Int,
                    });
                if !self.hashable_type_eligible(&elem_ty) {
                    self.diags.push(Diagnostic::error(
                            "E0506",
                            format!(
                                "`Tally<{}>` is not valid — `{}` is not hashable",
                                elem_ty.name(),
                                elem_ty.name()
                            ),
                            "Tally elements must implement Hash and Eq; use Int, Bool, String, Char, or a named type".to_string(),
                            format!(
                                "change the element type to a hashable type, or use a `[{}]` list instead",
                                elem_ty.name()
                            ),
                            Some(span),
                        ));
                }
                let ret = Type::Apply {
                    name: Syntax::TYPE_TALLY.to_string(),
                    args: vec![elem_ty],
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            // D-CONC-SHARE1=A (amends D-MEM1 S6 / D-SHARED-API1=A):
            // `shared x` — a lock-guarded shared value (`Arc<RwLock<T>>`
            // class). `T` is inferred from the constructed value, or from an
            // enclosing `Shared<T>` expectation when the value is shape-free.
            // The parser desugars the prefix form to this one constructor node
            // and teaches E1115 on the retired
            // `Shared.new(x)` call, so every tier keeps one shape.
            if type_name == "Shared" && method == "new" {
                self.record_memory_event(crate::Sema::MemoryEvent::new(
                    crate::Sema::MemoryEventKind::Allocation,
                    span,
                    "`shared` allocates shared storage",
                ));
                self.record_memory_event(crate::Sema::MemoryEvent::new(
                    crate::Sema::MemoryEventKind::RetainRelease,
                    span,
                    "`shared` introduces reference-counted ownership",
                ));
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!("`shared` takes exactly one value, got {}", args.len()),
                        "a `Shared<T>` wraps exactly one starting value".to_string(),
                        "write `shared value`".to_string(),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let expected_elem_ty =
                    owner_type_args
                        .first()
                        .cloned()
                        .or_else(|| match &self.expected_type {
                            Some(Type::Shared(inner)) => Some((**inner).clone()),
                            _ => None,
                        });
                let elem_ty = match expected_elem_ty {
                    Some(expected) => self
                        .infer_with_expected(&mut args[0].expr, &expected)
                        .unwrap_or(expected),
                    None => self.infer(&mut args[0].expr).unwrap_or(Type::Int),
                };
                if self.shared_storage_problem(&elem_ty).is_some() {
                    self.diags.push(Diagnostic::error(
                            "E1102",
                            format!(
                                "{} cannot be stored in `Shared<T>`",
                                elem_ty.show()
                            ),
                            "a Cell and its guards own one-thread borrow state; wrapping that state does not make it synchronized".to_string(),
                            "store the value itself in `Shared<T>`, then read and write its fields directly".to_string(),
                            Some(args[0].expr.span()),
                        ));
                }
                let ret = Type::Shared(Box::new(elem_ty));
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            // D-LOCALCELL1=A: `Cell.new(x)` constructs one thread-confined
            // interior-mutation handle. An annotation or explicit type
            // argument supplies `T` for `None`; otherwise `x` infers it.
            if type_name == "Cell" && method == "new" {
                self.record_memory_event(crate::Sema::MemoryEvent::new(
                    crate::Sema::MemoryEventKind::Allocation,
                    span,
                    "`Cell.new` allocates local storage",
                ));
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!("`Cell.new` takes exactly one value, got {}", args.len()),
                        "a `Cell<T>` stores exactly one starting value".to_string(),
                        "write `Cell.new(value)`".to_string(),
                        Some(span),
                    ));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                let expected = type_args
                    .first()
                    .cloned()
                    .or_else(|| match &self.expected_type {
                        Some(Type::Apply { name, args }) if name == "Cell" && !args.is_empty() => {
                            Some(args[0].clone())
                        }
                        _ => None,
                    });
                let saved_expected = self.expected_type.clone();
                self.expected_type = expected.clone();
                let inferred = self.infer(&mut args[0].expr).unwrap_or(Type::Int);
                self.expected_type = saved_expected;
                let elem_ty = expected.unwrap_or(inferred);
                let ret = Type::Apply {
                    name: "Cell".to_string(),
                    args: vec![elem_ty],
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            // D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>.new()` — an empty generational
            // arena. `T` comes from the call-site turbofish (`Pool<Player>.new()`)
            // or, failing that, the binding's type annotation — same fallback shape
            // as `Queue.new()`/`Tally.new()` above.
            if type_name == "Pool" && method == "new" && args.is_empty() {
                let elem_ty = type_args
                    .first()
                    .cloned()
                    .or_else(|| owner_type_args.first().cloned())
                    .unwrap_or_else(|| match &self.expected_type {
                        Some(Type::Apply { name, args, .. })
                            if name == "Pool" && !args.is_empty() =>
                        {
                            args[0].clone()
                        }
                        _ => Type::Int,
                    });
                let ret = Type::Apply {
                    name: "Pool".to_string(),
                    args: vec![elem_ty],
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            // D-PATHFS1: `Path.from(str)` — typed path constructor.
            if type_name == "Path" && method == "from" && !self.registry.contains("Path") {
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!("`Path.from` takes one string argument, got {}", args.len()),
                        "`Path.from` constructs a typed path from a string".to_string(),
                        "write `Path.from(some_string)`".to_string(),
                        Some(span),
                    ));
                }
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("Path".to_string()));
            }
            // D-SHIFT1 (c7shift): `Reader.over(bytes)` — bare static
            // constructor over `[U8]`, same shape as `Path.from` above (a
            // reserved core type name, no import needed; a user's own
            // `Reader` type always wins — `!self.registry.contains`).
            // Argument checking for all `Reader`/`Cursor` positions goes
            // through `check_shift_arg` (E0112 fallback included —
            // `check_type_assignable` alone lets a plain mismatch through).
            if type_name == "Reader" && method == "over" && !self.registry.contains("Reader") {
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`Reader.over` takes one `[U8]` argument, got {}",
                            args.len()
                        ),
                        "`Reader.over` wraps a byte list in a consuming, fallible cursor"
                            .to_string(),
                        "write `Reader.over(some_bytes)`".to_string(),
                        Some(span),
                    ));
                }
                if let Some(arg) = args.first_mut() {
                    let want = Type::List(Box::new(u8_ty()));
                    self.check_shift_arg("Reader.over", &want, arg);
                    arg.flags.owned_last_use = arg.convention == AccessConvention::Read
                        && self.proven_owned_last_use(&arg.expr, &want);
                }
                for a in args.iter_mut().skip(1) {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("Reader".to_string()));
            }
            // D-SHIFT1 (c7shift): `Cursor.over(s)` — bare static constructor
            // over `String`, same shape as `Reader.over` above.
            if type_name == "Cursor" && method == "over" && !self.registry.contains("Cursor") {
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`Cursor.over` takes one `String` argument, got {}",
                            args.len()
                        ),
                        "`Cursor.over` wraps a string in a consuming, fallible text cursor"
                            .to_string(),
                        "write `Cursor.over(some_string)`".to_string(),
                        Some(span),
                    ));
                }
                if let Some(arg) = args.first_mut() {
                    self.check_shift_arg("Cursor.over", &Type::String, arg);
                }
                for a in args.iter_mut().skip(1) {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("Cursor".to_string()));
            }
            // D-HOLE1: `Option.lift2(f, a, b)` — apply a two-argument function to
            // `a`/`b` only when both are present; `null` otherwise. A static
            // combinator (both optionals are plain arguments, not the receiver), so
            // it's resolved directly here, the same static-constructor shape as
            // `Set.from`/`Queue.new`/`Path.from` above.
            if type_name == "Option" && method == "lift2" && !self.registry.contains("Option") {
                return self.check_option_lift2(args, span, resolved_ret_out);
            }
            // D-SPACE-GEOMETRY1=A: stock constructors retain their nominal
            // space in the returned type; the scalar fields are ordinary
            // Float values and therefore need no heap-backed tag.
            if is_geometry_type(type_name) && !self.registry.contains(type_name) {
                if let Some(ret) = geometry_static_return_with_owner(
                    type_name,
                    method,
                    args.len(),
                    owner_type_args,
                ) {
                    if let Some(expected) =
                        geometry_static_arg_types(type_name, method, owner_type_args)
                    {
                        for (idx, (arg, want)) in
                            args.iter_mut().zip(expected.iter()).enumerate()
                        {
                            self.expect_core_arg(
                                &format!("{type_name}.{method}"),
                                idx,
                                want,
                                arg,
                            );
                        }
                    } else {
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                    }
                    *recv_type_out = Some(type_name.clone());
                    return Some(ret);
                }
            }

            // D-SIMD2 / D-LINALG1: a STATIC method on a built-in math type —
            // `F32x4.splat(x)` / `Vec3.from_array([…])`. The arg is elaborated
            // against the method's expected type (so a literal becomes the
            // component type / the `[T#N]` bridge).
            if is_math_type(type_name) && !self.registry.contains(type_name) {
                if let Some(ret) = math_static_return(type_name, method, args.len()) {
                    if let Some(want) = math_static_arg_ty(type_name, method) {
                        if let Some(arg) = args.first_mut() {
                            let old = self.expected_type.replace(want.clone());
                            let got = self.infer(&mut arg.expr);
                            self.expected_type = old;
                            if let Some(g) = got {
                                if g != want {
                                    self.diags.push(Diagnostic::error(
                                        "E0128",
                                        format!(
                                            "`{}.{}()` expects a `{}`, got `{}`",
                                            type_name,
                                            method,
                                            want.name(),
                                            g.name()
                                        ),
                                        format!(
                                            "`{}.{}` builds a `{}` from a `{}`",
                                            type_name,
                                            method,
                                            type_name,
                                            want.name()
                                        ),
                                        format!("pass a `{}` value", want.name()),
                                        Some(arg.expr.span()),
                                    ));
                                }
                            }
                        }
                    }
                    return Some(ret);
                }
            }
            // Built-in type identity does not depend on whether a particular
            // static method is registered. All valid builtin static shapes
            // returned above; an unknown one follows the ordinary E0102
            // method diagnostic instead of reinterpreting the type as a value.
            if self.lookup(type_name).is_none() && builtin_type_from_ident(type_name).is_some() {
                return self.check_static_method(type_name,
                method,
                span,
                owner_type_args,
                type_args,
                args, checked_callee_out);
            }
        }
        // D-VALIDATE1: `Validate.over(s).check(...)` and `.finish()` are
        // surface methods on the generated builder. Validate the rule at
        // this boundary, then lower the call through the ordinary generated
        // methods so AOT, JIT, interpreter, and web share one implementation.
        if matches!(method, "check" | "finish") {
            if let Some(recv_ty) = self.infer(receiver) {
                if let Type::Apply {
                    name,
                    args: type_args,
                } = &recv_ty
                {
                    if name == Syntax::TYPE_VALIDATE_BUILDER && type_args.len() == 1 {
                        let subject_ty = type_args[0].clone();
                        let builder_ty = recv_ty.clone();
                        *recv_type_out = Some(Syntax::TYPE_VALIDATE_BUILDER.to_string());
                        if method == "finish" {
                            if !args.is_empty() {
                                self.diags.push(wrong_core_arity(
                                    "Validate.finish",
                                    0,
                                    args.len(),
                                    span,
                                ));
                                for arg in args.iter_mut() {
                                    self.infer(&mut arg.expr);
                                }
                            }
                            let result = Type::Result {
                                ok: Box::new(subject_ty),
                                err: Box::new(decode_error_ty()),
                            };
                            *resolved_ret_out = Some(result.clone());
                            return Some(result);
                        }

                        if args.len() != 3 {
                            self.diags.push(crate::Sema::e0353_bad_validate_rule(span));
                            for arg in args.iter_mut() {
                                self.infer(&mut arg.expr);
                            }
                            *resolved_ret_out = Some(builder_ty.clone());
                            return Some(builder_ty);
                        }
                        let valid_shape = args[0].label.is_none()
                            && args[1]
                                .label
                                .as_ref()
                                .is_some_and(|(label, _)| label == "at")
                            && args[2].label.is_none();
                        let field_name = match &args[1].expr {
                            Expr::Ident(name, field_span) => Some((name.clone(), *field_span)),
                            // The first pass elaborates the bare field name to
                            // the String consumed by the generated method. A
                            // chained receiver can infer this call more than
                            // once, so accept that compiler-owned form only
                            // when the builder dispatch fact is already set.
                            Expr::Str(parts, field_span)
                                if recv_type_out.as_deref()
                                    == Some(Syntax::TYPE_VALIDATE_BUILDER)
                                    && matches!(parts.as_slice(), [StrPart::Lit(_)]) =>
                            {
                                let [StrPart::Lit(name)] = parts.as_slice() else {
                                    unreachable!("single literal checked above")
                                };
                                Some((name.clone(), *field_span))
                            }
                            _ => None,
                        };
                        let Some((field_name, field_span)) = field_name else {
                            self.diags
                                .push(crate::Sema::e0353_bad_validate_rule(args[1].span));
                            for arg in args.iter_mut() {
                                self.infer(&mut arg.expr);
                            }
                            *resolved_ret_out = Some(builder_ty.clone());
                            return Some(builder_ty);
                        };
                        if !valid_shape {
                            self.diags
                                .push(crate::Sema::e0353_bad_validate_rule(args[1].span));
                            for arg in args.iter_mut() {
                                self.infer(&mut arg.expr);
                            }
                            *resolved_ret_out = Some(builder_ty.clone());
                            return Some(builder_ty);
                        }
                        let subject_name = match &subject_ty {
                            Type::Named(name) | Type::Apply { name, .. } => name.as_str(),
                            _ => "",
                        };
                        let has_field = !subject_name.is_empty()
                            && self.struct_fields_for_type_name(subject_name).is_some_and(
                                |fields| fields.iter().any(|(name, _, _)| name == &field_name),
                            );
                        if !has_field {
                            self.diags.push(crate::Sema::e0354_unknown_validate_field(
                                &field_name,
                                field_span,
                            ));
                        }
                        // `at:` is a path label, not a runtime expression. Resolve
                        // the bare sibling field now so the generated method receives
                        // the canonical String expected by the one FieldError contract.
                        args[1].expr =
                            Expr::Str(vec![StrPart::Lit(field_name)], args[1].expr.span());
                        if let Some(condition_ty) = self.infer(&mut args[0].expr) {
                            self.check_type_assignable(
                                &Type::Bool,
                                &condition_ty,
                                args[0].expr.span(),
                            );
                        }
                        if let Some(reason_ty) = self.infer(&mut args[2].expr) {
                            self.check_type_assignable(
                                &Type::String,
                                &reason_ty,
                                args[2].expr.span(),
                            );
                        }
                        *resolved_ret_out = Some(builder_ty.clone());
                        return Some(builder_ty);
                    }
                }
            }
        }
        self.borrow_ctx = true;
        // D-MEM1 stage S5: chaining `.trim()`/`.after()`/`.before()` onto a
        // string-view name is the one builtin-method shape its bare `&str`
        // Rust place supports (the `_view` prelude helpers take `&str`) — the
        // general `Expr::Ident` E2307 check must not fire for THIS receiver
        // read. Every other method name reaches the same `self.infer(receiver)`
        // call with the flag left false, so it fires normally.
        let recv_is_exempt_view = matches!(receiver.as_ref(), Expr::Ident(n, _)
                if self.is_string_view(n) && matches!(method, "trim" | "after" | "before"));
        if recv_is_exempt_view {
            self.allow_string_view_read = true;
        }
        let recv_ty = if allocator_view_preserving_receiver {
            let saved = self.suppress_partial_move_root_read;
            self.suppress_partial_move_root_read = true;
            let recv_ty = self.infer(receiver);
            self.suppress_partial_move_root_read = saved;
            recv_ty
        } else {
            self.infer(receiver)
        };
        if recv_is_exempt_view {
            self.allow_string_view_read = false;
        }
        let recv_ty = recv_ty?;
        // D-MEM-COPYSEM1: any other method reads the view through a `String`
        // receiver, so the bare `&str` window is copied into one (the default
        // owning copy). The `&str` string kernels (`starts_with`, `ends_with`,
        // `contains`) read the window in place.
        if !recv_is_exempt_view
            && !matches!(method, "starts_with" | "ends_with" | "contains")
            && !self.copies_explicit()
            && matches!(receiver.as_ref(), Expr::Ident(n, _) if self.is_string_view(n))
        {
            let span = receiver.span();
            let old = std::mem::replace(receiver.as_mut(), Expr::Absent(span));
            **receiver = Expr::Copy(Box::new(old), span);
        }
        let receiver_is_clock = crate::Sema::Diagnostics::is_clock_type(&recv_ty);
        let clock_is_deterministic =
            crate::Sema::Diagnostics::is_deterministic_clock_type(&recv_ty);
        let expiring_clock_is_deterministic = matches!(
            &recv_ty,
            Type::Tagged { marker, inner }
                if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::DeterministicClock))
                    && matches!(
                        inner.as_ref(),
                        Type::Apply { name, .. }
                            if name == "ExpiringSecret"
                                || (jet_foundation::CoreModuleExports::core_source_owns_type(name)
                                    && name.ends_with("::Core/Crypto/Vault.jet::ExpiringSecret"))
                    )
        );
        // Most fact tags are type-transparent. These compiler-owned tags
        // carry method policy and must survive through method lookup.
        let recv_ty = match recv_ty {
            Type::Tagged {
                marker: crate::AST::TagMarker::Internal(crate::AST::InternalTag::SystemClock),
                inner,
            } if matches!(inner.as_ref(), Type::Named(name) | Type::Apply { name, .. }
                if self.core_source_declares_method(name, method)) => *inner,
            Type::Tagged { marker, inner }
                if matches!(
                    marker,
                    crate::AST::TagMarker::Internal(
                        crate::AST::InternalTag::SharedGuardRead
                            | crate::AST::InternalTag::SharedGuardEdit
                            | crate::AST::InternalTag::TerminalFactSet
                            | crate::AST::InternalTag::CoreCryptoNominal
                            | crate::AST::InternalTag::SystemClock
                    )
                ) =>
            {
                Type::Tagged { marker, inner }
            }
            Type::Tagged { inner, .. } => *inner,
            other => other,
        };
        // Core HTTP carriers may retain their import alias until receiver
        // inference. Normalize only the closed HTTP nominal family so method
        // lookup, field metadata, and return construction share one identity.
        let recv_ty = match recv_ty {
            Type::Named(name) => http_nominal_leaf(&name)
                .map_or(Type::Named(name.clone()), |leaf| Type::Named(leaf.to_string())),
            other => other,
        };
        // D-REALTIME1: typed Core handles contribute the same precise wait leaf
        // as plain Core rows. Only scalar Shared reads and the direct scalar
        // compatibility get/set/replace rail are wait-free; scalar edit still
        // acquires the exclusive protocol permit, and structured/guard/txn
        // paths remain blocking.
        let scalar_shared_atomic_access = matches!(
            (&recv_ty, method),
            (Type::Shared(inner), "read" | "get" | "set" | "replace") if inner.is_scalar()
        );
        let shared_protocol_access = matches!(
            (&recv_ty, method),
            (Type::Shared(_), "get" | "set" | "replace" | "read" | "edit"
                | "guard_read" | "guard_edit" | "read_txn" | "edit_txn")
        );
        if !scalar_shared_atomic_access {
            if shared_protocol_access {
                self.record_effect("Time.Wait", span);
            } else {
                let receiver_effect_type = match &recv_ty {
                    Type::Shared(_) => Some("Shared"),
                    Type::Named(name) | Type::Apply { name, .. } => Some(name.as_str()),
                    Type::Tagged { inner, .. } => match inner.as_ref() {
                        Type::Shared(_) => Some("Shared"),
                        Type::Named(name) | Type::Apply { name, .. } => Some(name.as_str()),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(type_name) = receiver_effect_type {
                    // Core source handle identities block exactly like their
                    // bare native spellings.
                    let type_name = crate::Sema::core_file_handle_dispatch_name(type_name)
                        .unwrap_or(type_name);
                    if let Some(effect) =
                        crate::Sema::Effects::receiver_effect_leaf(type_name, method)
                    {
                        self.record_effect(effect, span);
                    }
                }
            }
        }

        // D-TRACK-ORIGIN1=A: reject the retired runtime projection before
        // receiver-first `#Root` dispatch or any other compatibility path can
        // reinterpret `.origin()` as an ordinary call.
        let tracked_receiver = matches!(receiver.as_ref().without_parens(), Expr::Ident(name, _)
            if self.flow.origins.get(name).is_some_and(|origin| origin.is_readable()));
        if tracked_receiver && method == "origin" {
            self.diags.push(Diagnostic::error(
                "E0311",
                "`.origin()` is retired for tracked values".to_string(),
                "`#Track` origin is a compile-time fact, not runtime metadata".to_string(),
                "read `value.$origin` in comptime code".to_string(),
                Some(span),
            ));
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }
        // D-CALLVALUE1=B: `.call(...)` is the builtin projection for a
        // function-typed receiver. Keep a member named `call` on the
        // ordinary lookup path below — a user struct field, a user `impl`
        // method, or a Core handle's own method — so a real `call` shadows
        // the projection as promised by the surface decision.
        if method == Syntax::METHOD_CALL {
            let has_call_member = match &recv_ty {
                Type::Named(name) => {
                    self.struct_fields_for_type_name(name)
                        .is_some_and(|fields| {
                            fields
                                .iter()
                                .any(|(field, _, _)| field == Syntax::METHOD_CALL)
                        })
                        || self.resolve_method_sig(name, Syntax::METHOD_CALL).is_some()
                        || crate::Sema::core_handle_owns_method(name, Syntax::METHOD_CALL)
                }
                Type::Apply { name, args } => {
                    self.struct_fields_for_type_name(name)
                        .is_some_and(|fields| {
                            fields
                                .iter()
                                .any(|(field, _, _)| field == Syntax::METHOD_CALL)
                        })
                        || self.resolve_method_sig(name, Syntax::METHOD_CALL).is_some()
                        || core_generic_struct_field(name, Syntax::METHOD_CALL, args).is_some()
                }
                Type::TraitObject(traits) => traits.iter().any(|trait_name| {
                    self.trait_reg.trait_info(trait_name)
                        .is_some_and(|trait_def| {
                            trait_def.methods.contains_key(Syntax::METHOD_CALL)
                        })
                }),
                _ => false,
            };
            if !matches!(&recv_ty, Type::Fn { .. }) && !has_call_member {
                let mut callee = receiver.clone();
                let end = args
                    .last()
                    .map(|arg| arg.expr.span().end)
                    .unwrap_or(span.end);
                return self.infer_call_value(
                    &mut callee,
                    args,
                    Span::new(receiver.span().start, end),
                );
            }
            if matches!(&recv_ty, Type::Fn { .. }) {
                *recv_type_out = Some(Syntax::INTERNAL_CALL_VALUE.to_string());
                let end = args
                    .last()
                    .map(|arg| arg.expr.span().end)
                    .unwrap_or(span.end);
                let call_span = Span::new(receiver.span().start, end);
                return self.infer_call_value(receiver, args, call_span);
            }
        }
        // D-CALLDUAL1=E: a `#Root` free function is the one sanctioned
        // receiver-first spelling. Resolve it before ordinary method
        // lookup so the same function body handles both call forms.
        if let Some(selection) = self.select_root_call(method, &recv_ty, span) {
            let target = match selection {
                Ok(target) => target,
                Err(()) => {
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
            };
            return self.infer_root_call(
                target, receiver, span, type_args, args, recv_type_out, resolved_ret_out,
            );
        }
        if receiver_is_clock
            && !clock_is_deterministic
            && !crate::Sema::Diagnostics::is_system_clock_type(&recv_ty)
            && matches!(method, "now" | "tick" | "advance" | "wait")
        {
            self.record_effect(Effect::Time.name(), span);
            if self.in_pure && self.det_suppress == 0 {
                self.diags
                    .push(crate::Sema::e3403(&format!("Clock.{method}"), Some(span)));
            }
        }
        // D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D: only a
        // terminal-backed child carries a session. The optional field must
        // be unwrapped before the real TerminalSession method dispatch.
        if method == "resize"
            && matches!(
                &recv_ty,
                Type::Option(inner)
                    if matches!(
                        inner.as_ref(),
                        Type::Named(name) if name == Syntax::TYPE_TERMINAL_SESSION
                    )
            )
        {
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            self.diags.push(Diagnostic::error(
                "E0311",
                "`.resize()` needs `TerminalSession`, not `TerminalSession?`".to_string(),
                "a child has a terminal session only when a terminal-backed launch succeeds"
                    .to_string(),
                "unwrap the optional handle first, then call `session.resize(size)`".to_string(),
                Some(span),
            ));
            return None;
        }
        if matches!(&recv_ty, Type::Apply { name, .. } if name == crate::Syntax::TYPE_EVENT)
            && matches!(method, "emit_async" | "queued_count")
        {
            let (what, fix) = if method == "emit_async" {
                (
                        "`Event.emit_async` was retired",
                        "construct an `AsyncEvent<T, E>` with `event.async_result`, then call `.emit_async(payload)`",
                    )
            } else {
                (
                    "`Event.queued_count` was retired",
                    "use `.queued_count()` on an `AsyncEvent<T, E>`",
                )
            };
            self.diags.push(Diagnostic::error(
                    "E0102",
                    what.to_string(),
                    "synchronous `Event<T>` dispatches immediately; only `AsyncEvent<T, E>` owns a scheduler queue".to_string(),
                    fix.to_string(),
                    Some(span),
                ));
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }
        match &recv_ty {
            Type::Apply { name, .. } if name == "Pool" && method == "add" => {
                self.record_memory_event(crate::Sema::MemoryEvent::new(
                    crate::Sema::MemoryEventKind::ArenaBytes(None),
                    span,
                    "`Pool.add` has no proven byte bound",
                ));
            }
            Type::Named(name)
                if matches!(name.as_str(), "Arena" | "Bump")
                    && matches!(method, "new" | "alloc" | "alloc_slice") =>
            {
                if method == "new" {
                    let bound = args.first().and_then(|arg| match &arg.expr {
                        Expr::Int(value, _, _, _) if *value >= 0 => Some(*value as u64),
                        _ => None,
                    });
                    self.record_memory_event(crate::Sema::MemoryEvent::new(
                        crate::Sema::MemoryEventKind::ArenaBytes(bound),
                        span,
                        format!("`{name}.new` reserves bounded arena storage"),
                    ));
                }
                self.record_memory_event(crate::Sema::MemoryEvent::new(
                    crate::Sema::MemoryEventKind::Allocation,
                    span,
                    format!("`{name}.{method}` allocates from arena storage"),
                ));
            }
            _ => {}
        }
        if crate::Sema::Diagnostics::is_secret_bearing_crypto_type(&recv_ty)
            && matches!(method, "clone" | "encode" | "hash" | "debug" | "display")
        {
            let shown = recv_ty.show().trim_matches('`').to_string();
            self.diags.push(Diagnostic::error(
                    "E0311",
                    format!("`{method}` is forbidden on secret-bearing `{shown}`"),
                    "secret-bearing cryptographic values are move-only and expose no clone, hash, serialization, Debug, or Display operation".to_string(),
                    "move the value to its next owner; use only a named `core.crypto.expert` exposure inside an audited `#Unsafe` region when raw bytes are required".to_string(),
                    Some(span),
                ));
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }
        // D-AUTODERIVE1: `.debug()` is the Debug protocol method. Auto-derived
        // and handwritten impls share this spelling; TIR lowers it through the
        // same Debug text path `{value:Debug}` uses.
        if method == "debug" && args.is_empty() && type_args.is_empty() {
            if is_debuggable(&recv_ty, self.registry, self.trait_reg) {
                *recv_type_out = Some("__Debug__".to_string());
                return Some(Type::String);
            }
        }
        // D-SERDE2=A: Encode is one public protocol for hand and generated impls.
        if method == "encode" {
            if args.is_empty() && type_args.is_empty() && self.is_encodable(&recv_ty) {
                *recv_type_out = Some("__SerdeEncode__".to_string());
                return Some(Type::Named(Syntax::TYPE_DATA.to_string()));
            }
            if !self.is_encodable(&recv_ty) {
                let shown = recv_ty.show();
                let shown = shown.trim_matches('`');
                self.diags.push(Diagnostic::error(
                    "E0905",
                    format!("`{shown}` does not implement `Encode`"),
                    "`.encode()` can only call the value's Encode contract".to_string(),
                    format!("derive `Encode` on `{shown}`, or write `impl {shown}.Encode`"),
                    Some(span),
                ));
                return None;
            }
        }
        // D-SERDE16=A: public, target-directed Decode dispatch from an ordinary
        // Dyn subtree. This is the spelling generated derives emit too.
        if matches!(&recv_ty, Type::Named(n) if Syntax::is_data_type_name(n))
            && method == Syntax::METHOD_DYN_DECODE
        {
            *recv_type_out = Some(Syntax::TYPE_DATA.to_string());
            if !args.is_empty() || type_args.len() != 1 {
                self.diags
                    .push(wrong_core_arity("Dyn.decode<T>", 0, args.len(), span));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
            let target = self.resolve_type(type_args[0].clone());
            if !self.is_decodable(&target) {
                let shown = target.show();
                let shown = shown.trim_matches('`');
                self.diags.push(Diagnostic::error(
                    "E0905",
                    format!("`{shown}` does not implement `Decode`"),
                    format!(
                        "`Dyn.decode<{shown}>()` can only call the type's Decode contract"
                    ),
                    format!("derive `Decode` on `{shown}`, or write `impl {shown}.Decode`"),
                    Some(span),
                ));
            }
            let ret = Type::Result {
                ok: Box::new(target),
                err: Box::new(decode_error_ty()),
            };
            *resolved_ret_out = Some(ret.clone());
            return Some(ret);
        }
        if let Type::Named(ref n) | Type::Apply { name: ref n, .. } = &recv_ty {
            if n == Syntax::TYPE_TASKGROUP {
                return self.infer_taskgroup_method(receiver, method, span, args, recv_type_out);
            }
            // D-TYPEDTEXT1=D: inspect a checked `SQL`/`HTML` value. `.template()`/
            // `.params()` expose the bound-parameter split (SQL never re-embeds a
            // hole's text into the query string); `.text()` reads the escaped HTML.
            if n == "SQL" && matches!(method, "template" | "params") {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity(method, 0, args.len(), span));
                }
                *recv_type_out = Some(n.clone());
                return Some(if method == "template" {
                    Type::String
                } else {
                    Type::List(Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())))
                });
            }
            if n == "HTML" && method == "text" {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity(method, 0, args.len(), span));
                }
                *recv_type_out = Some(n.clone());
                return Some(Type::String);
            }
        }
        // D-FAIL-CARRIER1=A: the carrier's middle states, read from the
        // fallible view. Both live on the outcome value: an error type opts
        // in by carrying the surviving payload under `partial` and the
        // notes it collected under `notes`. Proving the field is here is
        // what makes the answer derivable from the receiver alone.
        if matches!(
            method,
            Syntax::METHOD_OUTCOME_PARTIAL | Syntax::METHOD_OUTCOME_NOTES
        ) {
            if let Type::Result { ok, err } = &recv_ty {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity(method, 0, args.len(), span));
                }
                // A missing field reports E0302 from the shared lookup; a
                // mismatched one reports the ordinary type mismatch.
                // Neither needs a code of its own.
                let notes = method == Syntax::METHOD_OUTCOME_NOTES;
                let (field, why, wanted) = if notes {
                    (
                        Syntax::FIELD_OUTCOME_NOTES,
                        "an outcome carries what it has to say as a field named `notes`",
                        Type::List(Box::new(Type::String)),
                    )
                } else {
                    (
                            Syntax::FIELD_OUTCOME_PARTIAL,
                            "an error type keeps part of its work by carrying it as a field named `partial`",
                            (**ok).clone(),
                        )
                };
                let carried = self.carrier_report_fact(err, field, why, span)?;
                self.check_type_assignable(&wanted, &carried, span);
                let ret = if notes {
                    wanted
                } else {
                    Type::Option(Box::new(wanted))
                };
                *recv_type_out = Some("__Carrier__".to_string());
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
        }

        // D-TYPE2-MEASURE1=A: non-mutating fixed-list join declares the
        // additive length rule on the shared measure substrate.
        if method == "concat" && args.len() == 1 {
            if let Type::FixedList {
                elem: left_elem,
                len: left_len,
            } = &recv_ty
            {
                if let Some(Type::FixedList {
                    elem: right_elem,
                    len: right_len,
                }) = self.infer(&mut args[0].expr)
                {
                    self.check_type_assignable(left_elem, &right_elem, args[0].expr.span());
                    if let Some(len) = left_len.combine(&right_len, crate::AST::MeasureRule::Add) {
                        // The call rides the ordinary builtin collection
                        // path (`recv_type` stays None — the TIR subset
                        // gate and lowering key builtins on that), while
                        // the recorded return carries the combined length
                        // measure so emit picks the fixed-shape concat.
                        let ret = Type::FixedList {
                            elem: left_elem.clone(),
                            len,
                        };
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                }
            }
        }
        // E0964: length-changing methods are forbidden on a fixed-size [T#N].
        if let Type::FixedList { .. } = &recv_ty {
            if matches!(method, "push" | "pop" | "insert" | "remove" | "clear") {
                self.diags.push(Diagnostic::error(
                        "E0964",
                        format!(
                            "`{}` changes a list's length, but this is a fixed-size {}",
                            method,
                            recv_ty.show()
                        ),
                        "the length of `[T#N]` is fixed at compile time and cannot change".to_string(),
                        "bind a growable list with `:=` (e.g. `r := [...]`) if you need to change its length".to_string(),
                        Some(span),
                    ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
        }
        // D-TXN3/D-TXN4: `<handle>.on_commit(() -> { … })` on a `#Transact`
        // transaction handle. Same shape as `scope.guard`: a zero-parameter
        // lambda, Drop-backed, run LIFO on a clean commit and dropped on a
        // `?`-failure/rollback. Returns a guard handle (`TransactionGuard`),
        // bound or discarded like a `scope.guard`. Sets `recv_type` so codegen
        // routes the node to the commit-guard lowering (I3).
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == crate::Syntax::TXN_HANDLE_TYPE && method == crate::Syntax::TXN_ON_COMMIT
            {
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`{}` takes one lambda, got {}",
                            crate::Syntax::TXN_ON_COMMIT,
                            args.len()
                        ),
                        "a post-commit hook registers a single cleanup lambda".to_string(),
                        format!(
                            "write `{}.{}(() -> {{ … }})`",
                            "<handle>",
                            crate::Syntax::TXN_ON_COMMIT
                        ),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return Some(Type::Named("TransactionGuard".to_string()));
                }
                let lam_ty = self.infer(&mut args[0].expr);
                match &lam_ty {
                    Some(Type::Fn { params, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(Diagnostic::error(
                                    "E0104",
                                    format!(
                                        "`{}` needs a zero-parameter lambda, got {} parameter{}",
                                        crate::Syntax::TXN_ON_COMMIT,
                                        params.len(),
                                        if params.len() == 1 { "" } else { "s" }
                                    ),
                                    "the hook body takes no arguments — it captures what it needs via closure".to_string(),
                                    format!("write `<handle>.{}(() -> {{ … }})` with no parameters", crate::Syntax::TXN_ON_COMMIT),
                                    Some(args[0].expr.span()),
                                ));
                        }
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`{}` needs a lambda, not {}",
                                crate::Syntax::TXN_ON_COMMIT,
                                other.show()
                            ),
                            "a post-commit hook runs a lambda only after the transaction commits"
                                .to_string(),
                            format!(
                                "write `<handle>.{}(() -> {{ … }})`",
                                crate::Syntax::TXN_ON_COMMIT
                            ),
                            Some(args[0].expr.span()),
                        ));
                    }
                    None => {}
                }
                *recv_type_out = Some(handle_ty.clone());
                return Some(Type::Named("TransactionGuard".to_string()));
            }
        }
        // D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(() -> { … })` on a
        // `#Transact` handle — the exact mirror of `on_commit`. A zero-parameter
        // lambda, Drop-backed, run LIFO on a `?`-failure/rollback and dropped on a
        // clean commit. Returns the same `TransactionGuard` handle.
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == crate::Syntax::TXN_HANDLE_TYPE
                && method == crate::Syntax::TXN_ON_ROLLBACK
            {
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`{}` takes one lambda, got {}",
                            crate::Syntax::TXN_ON_ROLLBACK,
                            args.len()
                        ),
                        "a rollback hook registers a single undo lambda".to_string(),
                        format!(
                            "write `{}.{}(() -> {{ … }})`",
                            "<handle>",
                            crate::Syntax::TXN_ON_ROLLBACK
                        ),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return Some(Type::Named("TransactionGuard".to_string()));
                }
                let lam_ty = self.infer(&mut args[0].expr);
                match &lam_ty {
                    Some(Type::Fn { params, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(Diagnostic::error(
                                    "E0104",
                                    format!(
                                        "`{}` needs a zero-parameter lambda, got {} parameter{}",
                                        crate::Syntax::TXN_ON_ROLLBACK,
                                        params.len(),
                                        if params.len() == 1 { "" } else { "s" }
                                    ),
                                    "the hook body takes no arguments — it captures what it needs via closure".to_string(),
                                    format!("write `<handle>.{}(() -> {{ … }})` with no parameters", crate::Syntax::TXN_ON_ROLLBACK),
                                    Some(args[0].expr.span()),
                                ));
                        }
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`{}` needs a lambda, not {}",
                                crate::Syntax::TXN_ON_ROLLBACK,
                                other.show()
                            ),
                            "a rollback hook runs a lambda only when the transaction rolls back"
                                .to_string(),
                            format!(
                                "write `<handle>.{}(() -> {{ … }})`",
                                crate::Syntax::TXN_ON_ROLLBACK
                            ),
                            Some(args[0].expr.span()),
                        ));
                    }
                    None => {}
                }
                *recv_type_out = Some(handle_ty.clone());
                return Some(Type::Named("TransactionGuard".to_string()));
            }
        }
        // D-DBDRIVER1: method calls on a `DBConnection` handle. A bespoke block
        // (like the `#Transact` handle above) rather than the generic
        // `file_handle_method_return` table, because `.query`/`.query_one`/
        // `.execute` need real expected-type-directed SQL argument elaboration.
        // The carrier keeps its template and ordered bindings together, so no
        // separate params literal is accepted at this boundary.
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == "DBConnection" {
                if let Some(ret) = self.check_db_connection_method(method, args, span) {
                    *recv_type_out = Some(handle_ty.clone());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            if handle_ty == "DBScope" {
                if let Some(ret) = self.check_db_scope_method(method, args, span) {
                    *recv_type_out = Some(handle_ty.clone());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            if handle_ty == "DBPool" {
                if let Some(ret) = self.check_db_pool_method(method, args, span) {
                    *recv_type_out = Some(handle_ty.clone());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            if handle_ty == "DBLease" {
                if let Some(ret) = self.check_db_lease_method(method, args, span) {
                    *recv_type_out = Some(handle_ty.clone());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            if handle_ty == "ServiceTree" {
                if let Some(ret) = self.check_service_tree_method(method, args, span) {
                    if matches!(
                        method,
                        "worker"
                            | "group"
                            | "set_restart"
                            | "set_delivery"
                            | "start"
                            | "stop"
                            | "send"
                            | "send_durable"
                            | "receive"
                            | "fail_worker"
                            | "drain_worker"
                            | "partition_worker"
                            | "reconcile_worker"
                            | "drain_dead_letters"
                            | "set_state_empty"
                            | "set_state_snapshot"
                            | "set_state_event_log"
                            | "commit_snapshot"
                            | "append_event"
                            | "workflow_start"
                            | "workflow_step"
                            | "directory_register"
                            | "handoff_generation"
                            | "rollback_generation"
                            | "chaos_fail"
                    ) {
                        self.check_mutating_method_receiver(receiver, method, span);
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            if handle_ty == "ServiceWorkflow" {
                if let Some(ret) = self.check_service_workflow_method(method, args, span) {
                    *recv_type_out = Some(handle_ty.clone());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            if handle_ty == "ServiceEndpoint" {
                if let Some(ret) = self.check_service_endpoint_method(method, args, span) {
                    *recv_type_out = Some(handle_ty.clone());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            if handle_ty == "ServiceRuntime" {
                if let Some(ret) = self.check_service_runtime_method(method, args, span) {
                    *recv_type_out = Some(handle_ty.clone());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            if handle_ty == "DeliveryState" {
                if let Some(ret) = self.check_service_delivery_state_method(method, args, span) {
                    *recv_type_out = Some(handle_ty.clone());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            if handle_ty == "Delivery" {
                if let Some(ret) = self.check_service_delivery_method(method, args, span) {
                    // D-SERVICE-RECEIPT2=A: every observation/control method
                    // consumes the bound handle and returns the next carrier
                    // where applicable. Observation never cancels the work;
                    // the move only discharges the local ownership duty.
                    if let Expr::Ident(name, name_span) = &**receiver {
                        self.mark_moved_by(
                            name.clone(),
                            *name_span,
                            format!("{handle_ty}.{method}"),
                        );
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
        }
        // D-DEP-WASM1=A / D-PLUGIN1=B (c81): method calls on a `Plugin`
        // handle resolve against the immutable sema-owned interface registry.
        // The receiver's identity is a nominal type argument, so a dynamic
        // `Plugin` value cannot invent exports.
        let plugin_identity = match &recv_ty {
            Type::Apply { name, args } if name == "Plugin" => args.first().and_then(|ty| {
                matches!(ty, Type::Named(_)).then(|| match ty {
                    Type::Named(identity) => identity.as_str(),
                    _ => unreachable!(),
                })
            }),
            Type::Named(name) if name == "Plugin" => None,
            _ => None,
        };
        if matches!(&recv_ty, Type::Named(name) if name == "Plugin")
            || matches!(&recv_ty, Type::Apply { name, .. } if name == "Plugin")
        {
            let interface = plugin_identity
                .and_then(|identity| self.plugin_interfaces.interface(identity))
                .cloned();
            if let Some(ret) =
                self.check_plugin_method(method, interface.as_ref(), args, span)
            {
                *recv_type_out = Some("Plugin".to_string());
                *resolved_ret_out = ret.clone();
                return ret;
            }
        }
        // D-LIB-CALLGRANT1=A: a loaded `Mod` has one typed scalar entry
        // point. Its grant and identity were checked at load; this method
        // only records the executable effect and return carrier.
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == "Mod" {
                if let Some(ret) = self.check_mod_method(method, args, span) {
                    *recv_type_out = Some(handle_ty.clone());
                    return ret;
                }
            }
        }
        if let Type::Named(handle_ty) = &recv_ty {
            let needs_edit = |checker: &mut Checker<'a>, api: &str| {
                if let Some(root) = expr_root_ident(receiver) {
                    if let Some(info) = checker.lookup(root) {
                        if !info.mutable {
                            checker.diags.push(Diagnostic::error(
                                    "E0202",
                                    format!("`{api}` needs edit access to `{root}`"),
                                    "game scene setup changes durable scene state".to_string(),
                                    format!("declare `{root} := game.Scene.new(...)` before calling `{api}`"),
                                    Some(receiver.span()),
                                ));
                        }
                    }
                }
            };
            match (handle_ty.as_str(), method) {
                ("GameScene", "on_frame") => {
                    needs_edit(self, "on_frame");
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity("on_frame", 1, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                    } else {
                        let expected_fn = Type::Fn {
                            params: vec![Type::Named("GameFrame".to_string())],
                            ret: None,
                            effect_bound: None,
                            return_view_provenance: None,
                            param_contract: None,
                            call_metadata: None,
                        };
                        let saved_esc = self.lambda_escapes;
                        let saved_exp = self.expected_type.clone();
                        self.lambda_escapes = true;
                        self.expected_type = Some(expected_fn);
                        self.infer(&mut args[0].expr);
                        super::super::super::check_game_frame_lambda(self, &mut args[0].expr);
                        self.lambda_escapes = saved_esc;
                        self.expected_type = saved_exp;
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return None;
                }
                ("GameScene", "component") => {
                    needs_edit(self, "component");
                    if args.is_empty() && type_args.len() == 1 {
                        args.push(Self::synthesized_string_arg(
                            Self::type_arg_name(&type_args[0]),
                            span,
                        ));
                    }
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity("component", 1, args.len(), span));
                    }
                    if let Some(arg) = args.get_mut(0) {
                        self.expect_core_arg("component", 0, &Type::String, arg);
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return None;
                }
                ("GameScene", "query") => {
                    if args.is_empty() && !type_args.is_empty() {
                        let names = type_args
                            .iter()
                            .map(Self::type_arg_name)
                            .collect::<Vec<_>>()
                            .join(",");
                        args.push(Self::synthesized_string_arg(names, span));
                    }
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity("query", 1, args.len(), span));
                    }
                    if let Some(arg) = args.get_mut(0) {
                        self.expect_core_arg("query", 0, &Type::String, arg);
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return Some(Type::List(Box::new(Type::String)));
                }
                ("GameAssets", "image" | "sound") => {
                    needs_edit(self, method);
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity(method, 1, args.len(), span));
                    }
                    if let Some(arg) = args.get_mut(0) {
                        self.expect_core_arg(method, 0, &Type::String, arg);
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    let ok = if method == "image" {
                        "GameImage"
                    } else {
                        "GameSound"
                    };
                    return Some(result_ty(Type::Named(ok.to_string()), Type::String));
                }
                ("GameInputMap", "bind") => {
                    needs_edit(self, "input.bind");
                    if args.len() != 2 {
                        self.diags
                            .push(wrong_core_arity("bind", 2, args.len(), span));
                    }
                    for i in 0..2 {
                        if let Some(arg) = args.get_mut(i) {
                            self.expect_core_arg("bind", i, &Type::String, arg);
                        }
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return None;
                }
                ("GameInputSnapshot", "pressed") => {
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity("pressed", 1, args.len(), span));
                    }
                    if let Some(arg) = args.get_mut(0) {
                        self.expect_core_arg("pressed", 0, &Type::String, arg);
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return Some(Type::Bool);
                }
                ("GameBackend", "should_continue") => {
                    if !args.is_empty() {
                        self.diags
                            .push(wrong_core_arity("should_continue", 0, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return Some(Type::Bool);
                }
                ("GameBackend", "present") => {
                    needs_edit(self, "present");
                    if !args.is_empty() {
                        self.diags
                            .push(wrong_core_arity("present", 0, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return None;
                }
                _ => {}
            }
        }
        // D-ENCSTREAM-SURFACE1=A: methods on opaque codec handles.
        if let Type::Named(handle_ty) = &recv_ty {
            if let Some(ret) = encoding_handle_method_return(handle_ty, method, args.len()) {
                if handle_ty == "JSONWriter" && method == "write" {
                    if let Some(arg) = args.first_mut() {
                        self.expect_core_arg(
                            "write",
                            0,
                            &Type::Named("DataEvent".to_string()),
                            arg,
                        );
                    }
                } else if handle_ty == "JSONLWriter" && method == "write" {
                    if let Some(arg) = args.first_mut() {
                        self.expect_core_arg("write", 0, &Type::Named("Dyn".to_string()), arg);
                    }
                } else if handle_ty == "CBORWriter" && method == "write" {
                    if let Some(arg) = args.first_mut() {
                        self.expect_core_arg(
                            "write",
                            0,
                            &Type::Named("DataEvent".to_string()),
                            arg,
                        );
                    }
                } else {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                }
                *recv_type_out = Some(handle_ty.clone());
                return ret;
            }
        }
        // D-FOUND-COREAPI1 / #2853: event-time stream operators are one
        // checked surface over the shared scheduler-backed Stream kernel.
        if let Type::Apply {
            name,
            args: stream_args,
        } = &recv_ty
        {
            if name == "Stream" && method == "with_event_time" {
                self.record_effect(Effect::IO.name(), span);
                *recv_type_out = Some("Stream".to_string());
                if stream_args.len() != 1 || args.len() != 1 {
                    self.diags.push(wrong_core_arity(
                        "Stream.with_event_time",
                        1,
                        args.len(),
                        span,
                    ));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                let element = stream_args[0].clone();
                let expected_fn = Type::Fn {
                    params: vec![element.clone()],
                    ret: None,
                    effect_bound: None,
                    return_view_provenance: None,
                    param_contract: None,
                    call_metadata: None,
                };
                let saved_esc = self.lambda_escapes;
                let saved_exp = self.expected_type.clone();
                self.lambda_escapes = true;
                self.expected_type = Some(expected_fn);
                let callback_ty = self.infer(&mut args[0].expr);
                if let Some(callback_ty) = callback_ty.as_ref() {
                    self.check_stream_callback_expr(&args[0].expr, callback_ty);
                }
                self.expected_type = saved_exp;
                self.lambda_escapes = saved_esc;
                let ret = match callback_ty {
                    Some(Type::Fn {
                        params,
                        ret: Some(event_ty),
                        ..
                    }) if params.len() == 1
                        && params[0] == element
                        && (event_ty.as_ref() == &Type::Int
                            || matches!(event_ty.as_ref(), Type::Named(type_name) if type_name == "DateTime")) =>
                    {
                        Type::Apply {
                            name: "StreamEventTime".to_string(),
                            args: vec![element],
                        }
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0108",
                            format!(
                                "`Stream.with_event_time` needs `fn({}) -> Int|DateTime`, got {}",
                                element.show(),
                                other.show()
                            ),
                            "the timestamp callback must return one event-time value for each stream element"
                                .to_string(),
                            "return an Int Unix timestamp or a DateTime".to_string(),
                            Some(args[0].expr.span()),
                        ));
                        return None;
                    }
                    None => return None,
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            if name == "StreamEventTime" && method == "key_by" {
                self.record_effect(Effect::IO.name(), span);
                *recv_type_out = Some("StreamEventTime".to_string());
                if stream_args.len() != 1 || args.len() != 1 {
                    self.diags.push(wrong_core_arity(
                        "StreamEventTime.key_by",
                        1,
                        args.len(),
                        span,
                    ));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                let element = stream_args[0].clone();
                let expected_fn = Type::Fn {
                    params: vec![element.clone()],
                    ret: None,
                    effect_bound: None,
                    return_view_provenance: None,
                    param_contract: None,
                    call_metadata: None,
                };
                let saved_esc = self.lambda_escapes;
                let saved_exp = self.expected_type.clone();
                self.lambda_escapes = true;
                self.expected_type = Some(expected_fn);
                let callback_ty = self.infer(&mut args[0].expr);
                if let Some(callback_ty) = callback_ty.as_ref() {
                    self.check_stream_callback_expr(&args[0].expr, callback_ty);
                }
                self.expected_type = saved_exp;
                self.lambda_escapes = saved_esc;
                let key = match callback_ty {
                    Some(Type::Fn {
                        params,
                        ret: Some(key),
                        ..
                    }) if params.len() == 1 && params[0] == element => key,
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0108",
                            format!(
                                "`StreamEventTime.key_by` needs `fn({}) -> K`, got {}",
                                element.show(),
                                other.show()
                            ),
                            "the key callback must return one key for each stream element"
                                .to_string(),
                            "return the field or value used as the event key".to_string(),
                            Some(args[0].expr.span()),
                        ));
                        return None;
                    }
                    None => return None,
                };
                let ret = Type::Apply {
                    name: "KeyedStream".to_string(),
                    args: vec![*key, element],
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            if name == "KeyedStream" && method == "window" {
                self.record_effect(Effect::IO.name(), span);
                *recv_type_out = Some("KeyedStream".to_string());
                if stream_args.len() != 2 || !(args.len() == 2 || args.len() == 3) {
                    self.diags.push(Diagnostic::error(
                        "E0102",
                        format!(
                            "`Stream.window` expects 2 or 3 arguments, got {}",
                            args.len()
                        ),
                        "a window needs a duration, a labelled watermark duration, and an optional late-event policy"
                            .to_string(),
                        "write `window(duration, watermark: duration)` or add `late: .Drop`/`.SideOutput`"
                            .to_string(),
                        Some(span),
                    ));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                if args.len() == 2 {
                    require_exact_labels(
                        "Stream.window",
                        args,
                        &[(1, "watermark")],
                        span,
                        &mut self.diags,
                    );
                    args.push(CallArg {
                        convention: AccessConvention::Read,
                        expr: Expr::EnumLit {
                            type_name: "LateEventDisposition".to_string(),
                            variant: "Drop".to_string(),
                            variant_span: None,
                            args: Vec::new(),
                            leading_dot: false,
                            span,
                        },
                        span,
                        flags: CallArgFlags::default(),
                        label: Some(("late".to_string(), span)),
                        spread: false,
                    });
                } else {
                    require_exact_labels(
                        "Stream.window",
                        args,
                        &[(1, "watermark"), (2, "late")],
                        span,
                        &mut self.diags,
                    );
                }
                self.expect_core_arg(
                    "Stream.window",
                    0,
                    &Type::Named("Duration".to_string()),
                    &mut args[0],
                );
                self.expect_core_arg(
                    "Stream.window",
                    1,
                    &Type::Named("Duration".to_string()),
                    &mut args[1],
                );
                self.expect_core_arg(
                    "Stream.window",
                    2,
                    &Type::Named("LateEventDisposition".to_string()),
                    &mut args[2],
                );
                let ret = Type::Apply {
                    name: "Stream".to_string(),
                    args: vec![Type::Apply {
                        name: "Window".to_string(),
                        args: vec![stream_args[0].clone(), stream_args[1].clone()],
                    }],
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
        }
        // D-DATAFLOW1=A: DataStream<T>.next() → ?T !DataError
        if let Type::Apply {
            name,
            args: type_args,
        } = &recv_ty
        {
            if data_nominal_leaf(name) == Some("DataStream") && method == "next" {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("DataStream.next", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                }
                let row = type_args
                    .first()
                    .cloned()
                    .unwrap_or(Type::Named("Unknown".to_string()));
                *recv_type_out = Some("DataStream".to_string());
                return Some(result_ty(
                    Type::Option(Box::new(row)),
                    Type::Named("DataError".to_string()),
                ));
            }
        }
        // D-DX-QUEUE1=A: a queue handle is a typed receiver, not a generic
        // service method. Keep the checked #Job identity and payload type at
        // this boundary; the provider/codegen layers only marshal the result.
        if let Type::Named(queue_type) = &recv_ty {
            if (queue_type == "JobQueue"
                || (jet_foundation::CoreModuleExports::core_source_owns_type(queue_type)
                    && queue_type.rsplit("::").next() == Some("JobQueue")))
                && matches!(method, "enqueue" | "delay")
            {
                let Some(method_ret) = job_queue_method_return(&Type::Named("JobQueue".to_string()), method, args.len()) else {
                    // Unknown methods continue to ordinary receiver lookup so
                    // user diagnostics and suggestions remain intact.
                    return None;
                };
                *recv_type_out = Some("JobQueue".to_string());
                self.record_call_receiver_access(receiver, AccessConvention::Write, span);
                let expected = if method == "enqueue" { 2 } else { 3 };
                let Some(ret) = method_ret else {
                    self.diags
                        .push(wrong_core_arity(method, expected, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                };
                if method == "enqueue" && args.len() == 3 {
                    require_exact_labels(
                        "JobQueue.enqueue",
                        args,
                        &[(2, "key")],
                        span,
                        &mut self.diags,
                    );
                } else if method == "delay" {
                    require_exact_labels(
                        "JobQueue.delay",
                        args,
                        &[(2, "for")],
                        span,
                        &mut self.diags,
                    );
                }
                let find_job = |items: &[crate::AST::Item], target: &str| {
                    items.iter().find_map(|item| match item {
                        crate::AST::Item::Func(function)
                            if function.name == target
                                && function.is_job
                                && function.params.len() == 1 =>
                        {
                            Some((function.name.clone(), function.params[0].ty.clone()))
                        }
                        _ => None,
                    })
                };
                let job = if matches!(method, "enqueue" | "delay") {
                    args.first().and_then(|arg| match arg.expr.without_parens() {
                        Expr::Ident(job_name, _) => find_job(self.items, job_name)
                            .or_else(|| {
                                self.unqualified
                                    .get(job_name)
                                    .and_then(|mangled| find_job(self.items, mangled))
                            })
                            .or_else(|| {
                                self.file_function_binding(job_name)
                                    .and_then(|(function_name, module_idx)| {
                                        self.modules
                                            .and_then(|modules| modules.get(module_idx))
                                            .and_then(|module| find_job(&module.items, &function_name))
                                    })
                            }),
                        _ => None,
                    })
                } else {
                    None
                };
                let payload_ty = job.as_ref().map(|(_, ty)| ty);
                if matches!(method, "enqueue" | "delay") {
                    if let Some((job_name, _)) = job.as_ref() {
                        if let Some(arg) = args.get_mut(0) {
                            let job_span = arg.expr.span();
                            arg.expr = Expr::Str(
                                vec![StrPart::Lit(job_name.clone())],
                                job_span,
                            );
                        }
                    } else if let Some(arg) = args.get_mut(0) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`JobQueue.{method}` expects a checked `#Job` function"
                            ),
                            "the queue identity is attached to the declared #Job and cannot be selected by an arbitrary function value"
                                .to_string(),
                            "pass the name of a #Job function with exactly one payload parameter"
                                .to_string(),
                            Some(arg.expr.span()),
                        ));
                        self.infer(&mut arg.expr);
                    }
                    if let Some(payload_ty) = payload_ty {
                        let payload_span = args
                            .get(1)
                            .map(|arg| arg.expr.span())
                            .unwrap_or(span);
                        self.check_encodable(payload_ty, payload_span);
                    }
                }
                match method {
                    "enqueue" | "delay" => {
                        if let Some(arg) = args.get_mut(1) {
                            if let Some(payload_ty) = payload_ty {
                                self.expect_core_arg(method, 1, payload_ty, arg);
                            } else {
                                self.infer(&mut arg.expr);
                            }
                        }
                        if method == "enqueue" {
                            if let Some(arg) = args.get_mut(2) {
                                self.expect_core_arg(
                                    "JobQueue.enqueue",
                                    2,
                                    &Type::Option(Box::new(Type::String)),
                                    arg,
                                );
                            }
                        } else if let Some(arg) = args.get_mut(2) {
                            self.expect_core_arg("JobQueue.delay", 2, &Type::Named("Duration".to_string()), arg);
                        }
                    }
                    _ => unreachable!("only enqueue/delay retain the checked #Job contract"),
                }
                let ret = self.resolve_type(ret);
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
        }
        // E2-M7: method calls on streaming file handles (D-IO2).
        if let Type::Named(handle_ty) = &recv_ty {
            if let Some(ret) =
                file_handle_method_return(handle_ty, method, args.len(), span, &mut self.diags)
            {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                *recv_type_out = Some(
                    crate::Sema::core_file_handle_dispatch_name(handle_ty)
                        .unwrap_or(handle_ty)
                        .to_string(),
                );
                if crate::Sema::is_mapped_file_view_method(handle_ty, method) {
                    if let Some(ret_ty) = ret.as_ref() {
                        *resolved_ret_out = Some(ret_ty.clone());
                    }
                }
                return ret;
            }
        }
        // E2-M10: method calls on net/http opaque types.
        if let Type::Named(handle_ty) = &recv_ty {
            if let Some(ret) =
                net_method_return(handle_ty, method, args.len(), span, &mut self.diags)
            {
                self.check_http_route_constant(handle_ty, method, args);
                if is_http_route_registration(handle_ty, method) {
                    self.check_http_route_registration(receiver, method, args, span);
                } else if handle_ty == "HTTPMux" && method == "middleware" {
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity(method, 1, args.len(), span));
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                    } else {
                        let middleware_ty = http_middleware_type();
                        self.expect_http_handler_arg(
                            method,
                            0,
                            &middleware_ty,
                            &mut args[0],
                        );
                    }
                }
                require_net_method_labels(handle_ty, method, args, span, &mut self.diags);
                if self.check_browser_method_args(handle_ty, method, args, span) {
                    // Browser handles share one exact argument checker.
                } else if handle_ty == "TLSClientConfig" && method == "with_alpn" {
                    if let Some(arg) = args.first_mut() {
                        self.expect_core_arg(
                            "with_alpn",
                            0,
                            &Type::List(Box::new(Type::String)),
                            arg,
                        );
                    }
                } else if handle_ty == "TLSClientConfig" && method == "with_trust" {
                    if let Some(arg) = args.first_mut() {
                        self.expect_core_arg(
                            "with_trust",
                            0,
                            &Type::Named("TLSClientTrust".to_string()),
                            arg,
                        );
                    }
                } else if handle_ty == "TLSClientConfig" && method == "with_client_identity" {
                    if let Some(arg) = args.first_mut() {
                        self.expect_core_arg(
                            "with_client_identity",
                            0,
                            &Type::Named("TLSClientIdentity".to_string()),
                            arg,
                        );
                    }
                } else if handle_ty == "TLSClientConfig" && method == "with_version_bounds" {
                    for (index, arg) in args.iter_mut().enumerate() {
                        self.expect_core_arg(
                            "with_version_bounds",
                            index,
                            &Type::Named("TLSVersion".to_string()),
                            arg,
                        );
                    }
                } else if handle_ty == "TLSStream" && method == "close_write" {
                    if let Some(arg) = args.first_mut() {
                        self.expect_core_arg(
                            "close_write",
                            0,
                            &Type::Named("Duration".to_string()),
                            arg,
                        );
                    }
                } else if handle_ty == "HTTPResponse" && method == "trailers" {
                    if let Some(arg) = args.first_mut() {
                        self.expect_core_arg_moving(
                            "Response.trailers",
                            0,
                            &Type::Named("Headers".to_string()),
                            arg,
                        );
                        arg.convention = AccessConvention::Move;
                    }
                } else {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                }
                *recv_type_out = Some(handle_ty.clone());
                return ret;
            }
        }
        // D-PATHFS1: method calls on `Path` typed handle.
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == "Path" {
                if let Some(ret) =
                    path_method_return(handle_ty, method, args.len(), span, &mut self.diags)
                {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some("Path".to_string());
                    return ret;
                }
            }
        }
        // D-SHIFT1 (c7shift): `cursor.take_pattern("…")` — argument-dependent
        // (the return shape comes from the pattern's holes), resolved
        // directly here, for the same reason `Arena.alloc` is above.
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == "Cursor" && method == Syntax::METHOD_TAKE_PATTERN {
                *recv_type_out = Some("Cursor".to_string());
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity(method, 1, args.len(), span));
                    return Some(result_ty(unit_ty(), Type::String));
                }
                let Expr::StrMatchLit(parts, lit_span) = &args[0].expr else {
                    self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs a literal pattern string", Syntax::METHOD_TAKE_PATTERN),
                            "the pattern is matched at compile time, so it can't be a computed `String` value"
                                .to_string(),
                            "write the pattern directly: `take_pattern(\"literal-{hole}-pattern\")`"
                                .to_string(),
                            Some(args[0].expr.span()),
                        ));
                    return Some(result_ty(unit_ty(), Type::String));
                };
                let _ = lit_span;
                let mut holes: Vec<(String, Type)> = Vec::new();
                for part in parts {
                    if let crate::AST::StrMatchPart::Hole {
                        name,
                        ty,
                        span: hole_span,
                    } = part
                    {
                        let bound_ty = self.str_match_hole_type(name, ty, *hole_span);
                        holes.push((name.clone(), bound_ty));
                    }
                }
                let ok_ty = if holes.is_empty() {
                    unit_ty()
                } else {
                    Type::Tuple(
                        holes
                            .iter()
                            .map(|(n, t)| (n.clone(), Box::new(t.clone())))
                            .collect(),
                    )
                };
                let out = result_ty(ok_ty, Type::String);
                *resolved_ret_out = Some(out.clone());
                return Some(out);
            }
        }
        // D-BINPAT1 / D-UNIFYLIT1=A: `reader.take_pattern([U8].{"…"})` —
        // the byte-mode sibling of `Cursor.take_pattern` above. Same
        // reason it's resolved directly here rather than through
        // `binary_reader_method_return`'s generic table: the return shape
        // comes from the pattern's holes (I8: one `take_pattern` mechanism,
        // dispatched on the literal kind the parser already committed to).
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == "Reader" && method == Syntax::METHOD_TAKE_PATTERN {
                *recv_type_out = Some("Reader".to_string());
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity(method, 1, args.len(), span));
                    return Some(result_ty(unit_ty(), Type::String));
                }
                let Expr::BinMatchLit(parts, lit_span) = &args[0].expr else {
                    self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs a literal binary pattern", Syntax::METHOD_TAKE_PATTERN),
                            "the pattern is matched at compile time, so it can't be a computed value"
                                .to_string(),
                            "write the pattern directly: `take_pattern([U8].{\"literal-{hole:U8}-pattern\"})`"
                                .to_string(),
                            Some(args[0].expr.span()),
                        ));
                    return Some(result_ty(unit_ty(), Type::String));
                };
                let _ = lit_span;
                let holes = self.bin_match_hole_types(parts, span, false);
                let ok_ty = if holes.is_empty() {
                    unit_ty()
                } else {
                    Type::Tuple(
                        holes
                            .iter()
                            .map(|(n, t)| (n.clone(), Box::new(t.clone())))
                            .collect(),
                    )
                };
                let out = result_ty(ok_ty, Type::String);
                *resolved_ret_out = Some(out.clone());
                return Some(out);
            }
        }
        // D-SHIFT1 (c7shift): `binary.Reader` instance methods (every read
        // fallible — bounds miss is an ordinary `?` error, not a panic).
        // D-BINREAD-LEN1=A: `take(n)` accepts Int plus U8/U16/U32 lengths;
        // the unsigned sized values widen internally. U64 stays explicit.
        if let Type::Named(handle_ty) = &recv_ty {
            if let Some(ret) = binary_reader_method_return(handle_ty, method, args.len()) {
                for a in args.iter_mut() {
                    if matches!(method, "take" | "seek" | "skip") {
                        self.check_shift_arg(&format!("Reader.{method}"), &Type::Int, a);
                    } else {
                        self.infer(&mut a.expr);
                    }
                }
                *recv_type_out = Some(handle_ty.clone());
                return ret;
            }
        }
        // D-SHIFT1: `text.Cursor` instance methods (excluding `take_pattern`,
        // handled above). `take_until(delim)` wants a `String`.
        if let Type::Named(handle_ty) = &recv_ty {
            if let Some(ret) = text_cursor_method_return(handle_ty, method, args.len()) {
                for a in args.iter_mut() {
                    if method == "take_until" {
                        self.check_shift_arg("Cursor.take_until", &Type::String, a);
                    } else {
                        self.infer(&mut a.expr);
                    }
                }
                *recv_type_out = Some(handle_ty.clone());
                return ret;
            }
        }
        // D-PENDING1=B: method calls on `Loadable<T,E>` handle.
        if let Some(ret) = loadable_method_return(&recv_ty, method, args.len()) {
            if method == "or_else" {
                let Type::Apply {
                    args: type_args, ..
                } = &recv_ty
                else {
                    unreachable!("loadable_method_return accepted a non-Loadable type")
                };
                self.expect_core_arg("Loadable.or_else", 0, &type_args[0], &mut args[0]);
            } else {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
            }
            *recv_type_out = Some("Loadable".to_string());
            return ret;
        }
        // D-NETDEP1=A / D-HTTPLIB1=A: method calls on HTTP types.
        if matches!(&recv_ty, Type::Named(name) if name == "Body") {
            let error = Type::Named("HTTPError".to_string());
            let result = |ok| Type::Result {
                ok: Box::new(ok),
                err: Box::new(error.clone()),
            };
            let special = match (method, args.len()) {
                ("json", 1) => {
                    self.expect_core_arg("Body.json", 0, &Type::Int, &mut args[0]);
                    let target = type_args
                        .first()
                        .cloned()
                        .or_else(|| match &self.expected_type {
                            Some(Type::Result { ok, .. }) => Some((**ok).clone()),
                            _ => None,
                        });
                    match target {
                        Some(target) => Some(result(target)),
                        None => {
                            self.diags.push(Diagnostic::error(
                                    "E0901",
                                    "`Body.json` needs a decode type".to_string(),
                                    "JSON bytes can decode to many types, so the target must be explicit".to_string(),
                                    "write `body.json<Type>(limit)`".to_string(),
                                    Some(span),
                                ));
                            Some(result(Type::Named("Unknown".to_string())))
                        }
                    }
                }
                ("copy_to", 2) => {
                    self.expect_core_arg_moving(
                        "Body.copy_to",
                        0,
                        &Type::Named("FileWriter".to_string()),
                        &mut args[0],
                    );
                    args[0].convention = AccessConvention::Move;
                    self.expect_core_arg("Body.copy_to", 1, &Type::Int, &mut args[1]);
                    Some(result(Type::Int))
                }
                ("bytes" | "text", 1) => {
                    self.expect_core_arg(method, 0, &Type::Int, &mut args[0]);
                    None
                }
                ("chunks", 0) => {
                    *recv_type_out = Some("Body".to_string());
                    return Some(Type::Named("HTTPBodyChunks".to_string()));
                }
                ("chunks", 1) => {
                    self.expect_core_arg("Body.chunks", 0, &Type::Int, &mut args[0]);
                    None
                }
                _ => None,
            };
            if let Some(ret) = special {
                *recv_type_out = Some("Body".to_string());
                return Some(ret);
            }
        }
        // D-HTTP-JSON1=A: `req.json<T>()` and `resp.json<T>(limit)` decode
        // the body through the same `#Codable` path the raw body uses.
        if method == "json"
            && matches!(&recv_ty, Type::Named(name)
                    if name == "HTTPRequest" || name == "HTTPResponse")
        {
            let is_request = matches!(&recv_ty, Type::Named(name) if name == "HTTPRequest");
            // D-HTTP-MSG1=A: a client request's one-argument JSON form is the
            // typed encoder path. Keep it distinct from request/response
            // decode-side inference, which uses a type argument and no value.
            if is_request && args.len() == 1 && type_args.is_empty() {
                let value_ty = self.infer(&mut args[0].expr);
                if let Some(value_ty) = value_ty {
                    self.check_encodable(&value_ty, args[0].expr.span());
                }
                *recv_type_out = Some("HTTPRequest".to_string());
                let ret = Type::Named("HTTPRequest".to_string());
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            let want = if is_request { 0 } else { 1 };
            if args.len() > want {
                self.diags
                    .push(wrong_core_arity("json", want, args.len(), span));
            }
            if let Some(arg) = args.first_mut() {
                self.expect_core_arg("json", 0, &Type::Int, arg);
            }
            let error = Type::Named("HTTPError".to_string());
            let target = type_args
                .first()
                .cloned()
                .or_else(|| match &self.expected_type {
                    Some(Type::Result { ok, .. }) => Some((**ok).clone()),
                    _ => None,
                });
            let target = match target {
                Some(target) => {
                    self.check_decodable(&target, span);
                    target
                }
                None => {
                    self.diags.push(Diagnostic::error(
                        "E0901",
                        format!(
                            "`{}.json` needs a decode type",
                            if is_request { "req" } else { "resp" }
                        ),
                        "JSON bytes can decode to many types, so the target must be explicit"
                            .to_string(),
                        if is_request {
                            "write `req.json<Type>()`".to_string()
                        } else {
                            "write `resp.json<Type>(limit)`".to_string()
                        },
                        Some(span),
                    ));
                    Type::Named("Unknown".to_string())
                }
            };
            *recv_type_out = Some(
                if is_request {
                    "HTTPRequest"
                } else {
                    "HTTPResponse"
                }
                .to_string(),
            );
            let ret = Type::Result {
                ok: Box::new(target),
                err: Box::new(error),
            };
            *resolved_ret_out = Some(ret.clone());
            return Some(ret);
        }
        if let Some(ret) = http_type_method_return(&recv_ty, method, args) {
            if let Type::Named(name) = &recv_ty {
                self.check_http_route_constant(name, method, args);
            }
            if matches!(&recv_ty, Type::Named(name) if name == "HTTPMux")
                && matches!(
                    method,
                    "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
                )
            {
                self.check_http_route_registration(receiver, method, args, span);
            } else if matches!(
                &recv_ty,
                Type::Named(name) if name == "HTTPRequest" || name == "HTTPResponse"
            ) && method == "text"
            {
                if args.len() > 1 {
                    self.diags
                        .push(wrong_core_arity(method, 1, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                } else if let Some(arg) = args.first_mut() {
                    self.expect_core_arg(method, 0, &Type::Int, arg);
                }
            } else if matches!(&recv_ty, Type::Named(name) if name == "HTTPClient") {
                let want = match method {
                    "cookies"
                    | "redirects"
                    | "send"
                    | "proxy"
                    | "tls"
                    | "allow_http_downgrade"
                    | "retries" => 1,
                    "protocols" => 3,
                    "timeouts" => 7,
                    "raw_encoding" => 0,
                    _ => args.len(),
                };
                if args.len() != want {
                    self.diags
                        .push(wrong_core_arity(method, want, args.len(), span));
                }
                let expected = match method {
                    "cookies" => Some(Type::Named("HTTPCookieJar".to_string())),
                    "protocols" | "allow_http_downgrade" => Some(Type::Bool),
                    "timeouts" => Some(Type::Int),
                    "redirects" => Some(Type::Named("HTTPRedirectPolicy".to_string())),
                    "retries" => Some(Type::Named("HTTPRetryPolicy".to_string())),
                    "send" => Some(Type::Named("HTTPRequest".to_string())),
                    "proxy" => Some(Type::Named("HTTPProxy".to_string())),
                    "tls" => Some(Type::Named("TLSClientConfig".to_string())),
                    _ => None,
                };
                for (index, arg) in args.iter_mut().enumerate() {
                    if let Some(expected) = &expected {
                        self.expect_core_arg(method, index, expected, arg);
                    } else {
                        self.infer(&mut arg.expr);
                    }
                }
            } else {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
            }
            *recv_type_out = match &recv_ty {
                Type::Named(n) => Some(n.clone()),
                _ => None,
            };
            *resolved_ret_out = ret.clone();
            return ret;
        }
        // D-EMAIL-SMTP-SURFACE1=A: exact envelope access/replacement methods.
        if let Some(ret) = email_method_return(&recv_ty, method, args.len()) {
            if method == "with_envelope" {
                self.expect_core_arg(
                    method,
                    0,
                    &Type::Named("Envelope".to_string()),
                    &mut args[0],
                );
            } else if method == "send" {
                self.expect_core_arg(method, 0, &Type::Named("Message".to_string()), &mut args[0]);
            }
            *recv_type_out = Some(match &recv_ty {
                Type::Named(name) => name.clone(),
                _ => "Message".to_string(),
            });
            return ret;
        }
        let recv_name = match &recv_ty {
            Type::Named(n) => n.as_str(),
            _ => "",
        };
        // D-REGEXENGINE1=A: method calls on compiled Regex and Match values.
        if let Some(ret) = regex_method_return(&recv_ty, method, args) {
            match (recv_name, method, args.len()) {
                ("Regex", "count", 1) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                }
                (
                    "Regex",
                    "is_match" | "full_match" | "match" | "find" | "find_all" | "matches" | "split",
                    1,
                ) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                }
                ("Regex", "replace" | "replace_first", 2) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    self.expect_core_arg(method, 1, &Type::String, &mut args[1]);
                }
                ("Regex", "split_limit", 2) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    self.expect_core_arg(method, 1, &Type::Int, &mut args[1]);
                }
                ("Regex", "replace_all_with", 2) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    let cb = Type::Fn {
                        params: vec![Type::Named("Match".to_string())],
                        ret: Some(Box::new(Type::String)),
                        effect_bound: None,
                        return_view_provenance: None,
                        param_contract: None,
                        call_metadata: None,
                    };
                    self.expect_core_arg(method, 1, &cb, &mut args[1]);
                }
                ("Match", "group" | "group_start" | "group_end", 1) => {
                    self.expect_core_arg(method, 0, &Type::Int, &mut args[0]);
                }
                ("Match", "name", 1) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                }
                _ => {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                }
            }
            *recv_type_out = Some(match &recv_ty {
                Type::Named(n) => n.clone(),
                _ => "Regex".to_string(),
            });
            // Preserve the exact Regex/Match return so a following method in
            // the same chain can recover its receiver type from this node.
            *resolved_ret_out = ret.clone();
            return ret;
        }
        // D-TIMEDEPTH1=A: method calls on the shared civil-time surface.
        let civil_recv_name = match &recv_ty {
            Type::Named(name) => Some(name.as_str()),
            _ => None,
        };
        if let Some(contract) =
            civil_recv_name.and_then(|name| civil_time_method_contract(name, method))
        {
            let params = contract
                .iter()
                .map(|param| crate::Sema::CallBinder::BindParam {
                    label: param.label,
                    name: param.label,
                    zone: param.zone,
                    default: None,
                    convention: AccessConvention::Read,
                    ty: None,
                    variadic: false,
                    core_default: param.default,
                })
                .collect::<Vec<_>>();
            if crate::Sema::CallBinder::bind_call_args(method, &params, args, span, &mut self.diags)
                .is_none()
            {
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                *recv_type_out = civil_recv_name.map(str::to_string);
                return None;
            }
            self.register_binder_refs(args);
        }
        if let Some(ret) = civil_time_method_return(&recv_ty, method, args) {
            let recv_name = match &recv_ty {
                Type::Named(n) => n.as_str(),
                _ => "",
            };
            match (recv_name, method, args.len()) {
                ("Date" | "LocalDate", "add_days" | "add_months", 1) => {
                    self.expect_core_arg(method, 0, &Type::Int, &mut args[0]);
                }
                ("Date" | "LocalDate", "diff_days", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("LocalDate".to_string()),
                        &mut args[0],
                    );
                }
                ("Date" | "LocalDate", "equal" | "compare", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("LocalDate".to_string()),
                        &mut args[0],
                    );
                }
                ("Date" | "LocalDate", "add_period" | "subtract_period", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("Period".to_string()),
                        &mut args[0],
                    );
                }
                ("Date" | "LocalDate", "truncate" | "format" | "format_checked", 1) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                }
                ("Date" | "LocalDate", "replace", 3) => {
                    for i in 0..3 {
                        self.expect_core_arg(method, i, &Type::Int, &mut args[i]);
                    }
                }
                ("Date" | "LocalDate", "with", argc) if argc == 3 || argc == 4 => {
                    for i in 0..3 {
                        self.expect_core_arg(method, i, &Type::Int, &mut args[i]);
                    }
                    if argc == 4 {
                        self.expect_core_arg(method, 3, &Type::String, &mut args[3]);
                    }
                }
                ("Date" | "LocalDate", "until" | "since", argc) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("LocalDate".to_string()),
                        &mut args[0],
                    );
                    for index in 1..argc.min(4) {
                        self.expect_core_arg(method, index, &Type::String, &mut args[index]);
                    }
                    if argc > 4 {
                        self.expect_core_arg(method, 4, &Type::Int, &mut args[4]);
                    }
                }
                ("DateTime", "plus_duration" | "subtract_duration", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("Duration".to_string()),
                        &mut args[0],
                    );
                }
                ("DateTime", "add_nanoseconds", 1) => {
                    self.expect_core_arg(method, 0, &Type::Int, &mut args[0]);
                }
                ("DateTime", "equal" | "compare", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("DateTime".to_string()),
                        &mut args[0],
                    );
                }
                ("DateTime", "difference", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("DateTime".to_string()),
                        &mut args[0],
                    );
                }
                ("DateTime", "add_period" | "subtract_period", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("Period".to_string()),
                        &mut args[0],
                    );
                }
                ("DateTime", "until" | "since", argc) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("DateTime".to_string()),
                        &mut args[0],
                    );
                    for index in 1..argc.min(4) {
                        self.expect_core_arg(method, index, &Type::String, &mut args[index]);
                    }
                    if argc > 4 {
                        self.expect_core_arg(method, 4, &Type::Int, &mut args[4]);
                    }
                }
                ("DateTime", "in_zone", 1) => {
                    self.expect_core_arg(method, 0, &Type::Named("Zone".to_string()), &mut args[0]);
                }
                ("DateTime", "truncate" | "floor" | "ceil", 1) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                }
                ("DateTime", "truncate" | "floor" | "ceil", 2) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    self.expect_core_arg(method, 1, &Type::Int, &mut args[1]);
                }
                ("DateTime", "round", argc) if (1..=3).contains(&argc) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    if argc > 1 {
                        self.expect_core_arg(method, 1, &Type::Int, &mut args[1]);
                    }
                    if argc > 2 {
                        self.expect_core_arg(method, 2, &Type::String, &mut args[2]);
                    }
                }
                ("DateTime", "format" | "format_checked", 1) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                }
                ("DateTime", "replace", 6) => {
                    for i in 0..6 {
                        self.expect_core_arg(method, i, &Type::Int, &mut args[i]);
                    }
                }
                ("DateTime", "with", argc) if argc == 6 || argc == 7 => {
                    for i in 0..6 {
                        self.expect_core_arg(method, i, &Type::Int, &mut args[i]);
                    }
                    if argc == 7 {
                        self.expect_core_arg(method, 6, &Type::String, &mut args[6]);
                    }
                }
                ("LocalTime", "add_duration" | "subtract_duration", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("Duration".to_string()),
                        &mut args[0],
                    );
                }
                ("LocalTime", "equal" | "compare", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("LocalTime".to_string()),
                        &mut args[0],
                    );
                }
                ("LocalTime", "truncate" | "floor" | "ceil", argc) if argc == 1 || argc == 2 => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    if argc == 2 {
                        self.expect_core_arg(method, 1, &Type::Int, &mut args[1]);
                    }
                }
                ("LocalTime", "round", argc) if (1..=3).contains(&argc) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    if argc > 1 {
                        self.expect_core_arg(method, 1, &Type::Int, &mut args[1]);
                    }
                    if argc > 2 {
                        self.expect_core_arg(method, 2, &Type::String, &mut args[2]);
                    }
                }
                ("LocalTime", "until" | "since", argc) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("LocalTime".to_string()),
                        &mut args[0],
                    );
                    for index in 1..argc.min(4) {
                        self.expect_core_arg(method, index, &Type::String, &mut args[index]);
                    }
                    if argc > 4 {
                        self.expect_core_arg(method, 4, &Type::Int, &mut args[4]);
                    }
                }
                ("ZonedDateTime", "add_duration" | "subtract_duration", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("Duration".to_string()),
                        &mut args[0],
                    );
                }
                ("ZonedDateTime", "add_period" | "subtract_period", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("Period".to_string()),
                        &mut args[0],
                    );
                }
                ("ZonedDateTime", "format" | "format_checked", 1) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                }
                ("ZonedDateTime", "format_rfc9557", 0) => {}
                ("ZonedDateTime", "with_time", argc) if argc == 1 || argc == 2 => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("LocalTime".to_string()),
                        &mut args[0],
                    );
                    if argc == 2 {
                        self.expect_core_arg(method, 1, &Type::String, &mut args[1]);
                    }
                }
                ("ZonedDateTime", "equal" | "compare", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("ZonedDateTime".to_string()),
                        &mut args[0],
                    );
                }
                ("ZonedDateTime", "with_zone", 1) => {
                    self.expect_core_arg(method, 0, &Type::Named("Zone".to_string()), &mut args[0]);
                }
                ("ZonedDateTime", "until" | "since", argc) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("ZonedDateTime".to_string()),
                        &mut args[0],
                    );
                    for index in 1..argc.min(4) {
                        self.expect_core_arg(method, index, &Type::String, &mut args[index]);
                    }
                    if argc > 4 {
                        self.expect_core_arg(method, 4, &Type::Int, &mut args[4]);
                    }
                }
                ("Zone", "next_transition" | "previous_transition", 1) => {
                    self.expect_core_arg(method, 0, &Type::Int, &mut args[0]);
                }
                ("Zone", "start_of_day" | "hours_in_day", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("LocalDate".to_string()),
                        &mut args[0],
                    );
                }
                ("ZonedDateTime", "start_of_day" | "hours_in_day" | "next_transition" | "previous_transition", 0) => {}
                ("Instant", "equal" | "compare", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("Instant".to_string()),
                        &mut args[0],
                    );
                }
                ("Period", "add" | "sub", 1) => {
                    self.expect_core_arg(
                        method,
                        0,
                        &Type::Named("Period".to_string()),
                        &mut args[0],
                    );
                }
                ("Period", "total_in", 2) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    let anchor_ty = crate::AST::canonicalize_union(vec![
                        Type::Named("Date".to_string()),
                        Type::Named("LocalDate".to_string()),
                        Type::Named("DateTime".to_string()),
                    ]);
                    self.expect_core_arg(method, 1, &anchor_ty, &mut args[1]);
                }
                ("Duration", "round", argc) if (1..=3).contains(&argc) => {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    if argc > 1 {
                        self.expect_core_arg(method, 1, &Type::Int, &mut args[1]);
                    }
                    if argc > 2 {
                        self.expect_core_arg(method, 2, &Type::String, &mut args[2]);
                    }
                }
                _ => {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                }
            }
            *recv_type_out = Some(match &recv_ty {
                Type::Named(n) => n.clone(),
                _ => "Date".to_string(),
            });
            return ret;
        }
        // D-ALLOC1/D-ALLOC-C/D-ALLOC-D (ratified 2026-06-19): method calls on
        // Arena/Bump/Pool/Fixed allocators. Reset invalidates live views;
        // universal `close(^allocator)` owns terminal release.
        if let Type::Named(handle_ty) = &recv_ty {
            let handle_ty_s = handle_ty.clone();
            if handle_ty_s == "Fixed" && matches!(method, "new" | "over") {
                *recv_type_out = Some(handle_ty_s.clone());
                *resolved_ret_out = Some(Type::Named(handle_ty_s.clone()));
                self.check_fixed_constructor(method, args, span);
                return Some(Type::Named(handle_ty_s));
            }
            if let Some(ret) =
                alloc_method_return(&handle_ty_s, method, args, span, &mut self.diags)
            {
                let recv_name = if let Expr::Ident(n, _) = &**receiver {
                    Some(n.clone())
                } else {
                    None
                };
                // D-ALLOC2: `reset` invalidates every value previously
                // allocated in this arena. Any view of it used afterward is
                // E0632 (use-after-reset) — the runtime `&mut self`
                // signatures would also reject, so Jet rejects first (I2).
                if method == "reset" {
                    if let Some(ref name) = recv_name {
                        if handle_ty_s == "Fixed" {
                            self.check_owner_change(name, "be reset", span);
                        }
                        self.kill_views_of_arena(name, method, span);
                    }
                }
                *recv_type_out = Some(handle_ty_s.clone());
                // For allocator value methods, infer the payload once. The
                // fallible spelling keeps the same payload inside one
                // `T !AllocError` carrier.
                if matches!(method, "alloc" | "try_alloc") {
                    if let Some(arg) = args.get_mut(0) {
                        let inferred = self.infer(&mut arg.expr);
                        let resolved = if method == "try_alloc" {
                            Some(result_ty(
                                Type::allocator_view(inferred.clone().unwrap_or(Type::Int)),
                                Type::Named(Syntax::TYPE_ALLOC_ERROR.to_string()),
                            ))
                        } else {
                            inferred
                        };
                        *resolved_ret_out = resolved.clone();
                        return resolved;
                    }
                    return None;
                }
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                *resolved_ret_out = ret.clone();
                return ret;
            }
        }
        // D-ARGS1: method calls on ArgsSpec / ParsedArgs (builder and result types).
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == "ArgsSpec" {
                if let Some(ret) =
                    args_spec_method_return(method, args.len(), span, &mut self.diags)
                {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some("ArgsSpec".to_string());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            if handle_ty == "ParsedArgs" {
                if let Some(ret) =
                    parsed_args_method_return(method, args.len(), span, &mut self.diags)
                {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some("ParsedArgs".to_string());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            // D-FOUND-REALTIME1=A: callback streams own a typed, bounded
            // lifecycle surface; receipt/deadline reads do not block.
            if handle_ty == "RealtimeStream" && self.resolve_opaque_method_sig(handle_ty, method).is_none() {
                if let Some(ret) = crate::Sema::realtime_stream_method_return(method, args.len()) {
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    *resolved_ret_out = ret.clone();
                    return ret;
                }
            }
            if handle_ty == "ProcessSpec" {
                if let Some(ret) =
                    process_spec_method_return(method, args.len(), span, &mut self.diags)
                {
                    if matches!(method, "run" | "spawn") {
                        self.record_effect(Effect::Exec.name(), span);
                    }
                    let stream_mode_ty = Type::Named("ProcessStreamMode".to_string());
                    match method {
                        "cwd" | "env_remove" | "arg" if args.len() == 1 => {
                            self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                        }
                        "args_extend" if args.len() == 1 => {
                            self.expect_core_arg(
                                method,
                                0,
                                &Type::List(Box::new(Type::String)),
                                &mut args[0],
                            );
                        }
                        "stdin" | "stdout" | "stderr" if args.len() == 1 => {
                            self.expect_core_arg(method, 0, &stream_mode_ty, &mut args[0]);
                        }
                        "env" if args.len() == 2 => {
                            self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                            self.expect_core_arg(method, 1, &Type::String, &mut args[1]);
                        }
                        "timeout" if args.len() == 1 => {
                            self.expect_core_arg(
                                method,
                                0,
                                &Type::Named("Duration".to_string()),
                                &mut args[0],
                            );
                        }
                        "cpu_time_limit" if args.len() == 1 => {
                            self.expect_core_arg(
                                method,
                                0,
                                &Type::Named("Duration".to_string()),
                                &mut args[0],
                            );
                        }
                        "output_limit" if args.len() == 1 => {
                            self.expect_core_arg(method, 0, &Type::Int, &mut args[0]);
                        }
                        "memory_limit" | "open_file_limit" if args.len() == 1 => {
                            self.expect_core_arg(method, 0, &Type::Int, &mut args[0]);
                        }
                        "terminal" if args.len() == 1 => {
                            self.expect_core_arg(
                                method,
                                0,
                                &Type::Named(Syntax::TYPE_TERMINAL_POLICY.to_string()),
                                &mut args[0],
                            );
                        }
                        "under" if args.len() == 1 => {
                            self.expect_core_arg(
                                method,
                                0,
                                &Type::Named(Syntax::TYPE_AUTHORITY.to_string()),
                                &mut args[0],
                            );
                        }
                        _ => {
                            for a in args.iter_mut() {
                                self.infer(&mut a.expr);
                            }
                        }
                    }
                    *recv_type_out = Some("ProcessSpec".to_string());
                    if method == "abilities" {
                        *resolved_ret_out = ret.clone();
                    }
                    return ret;
                }
            }
            if handle_ty == "ProcessChild" {
                if let Some(ret) =
                    process_child_method_return(method, args.len(), span, &mut self.diags)
                {
                    if method != "id" {
                        self.record_effect(Effect::Exec.name(), span);
                    }
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some("ProcessChild".to_string());
                    return ret;
                }
            }
            if handle_ty == Syntax::TYPE_TERMINAL_SESSION {
                if let Some(ret) =
                    terminal_session_method_return(method, args.len(), span, &mut self.diags)
                {
                    self.record_effect(Effect::Exec.name(), span);
                    if method == "resize" && args.len() == 1 {
                        self.expect_core_arg(
                            method,
                            0,
                            &Type::Named(Syntax::TYPE_TERMINAL_SIZE.to_string()),
                            &mut args[0],
                        );
                    } else {
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                    }
                    *recv_type_out = Some(Syntax::TYPE_TERMINAL_SESSION.to_string());
                    return ret;
                }
            }
            // D-PROCESS1=A / D-FOUND-LIFECYCLE1=A: `.write(text)`/`.close()` on the
            // `child.stdin` writer handle.
            if handle_ty == "ProcessStdin" {
                if let Some(ret) =
                    process_stdin_method_return(method, args.len(), span, &mut self.diags)
                {
                    self.record_effect(Effect::Exec.name(), span);
                    if method == "write" && args.len() == 1 {
                        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    } else {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                    }
                    *recv_type_out = Some("ProcessStdin".to_string());
                    return ret;
                }
            }
            // D-PROCESS1=A: `.lines()` on `child.stdout`/`child.stderr` streaming
            // reader handles (loop-source-only — E2502 in bindings.rs).
            if handle_ty == "ProcessStdoutStream" || handle_ty == "ProcessStderrStream" {
                if let Some(ret) =
                    process_stream_method_return(method, args.len(), span, &mut self.diags)
                {
                    self.record_effect(Effect::Exec.name(), span);
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return ret;
                }
            }
            // D-RENDERTGT2=A (c133 M1/M2): UI backend measure/layout/paint/on_event.
            if handle_ty == "NullBackend" || handle_ty == "TUIBackend" || handle_ty == "GtkBackend"
            {
                if let Some(ret) =
                    ui_backend_method_return(handle_ty, method, args.len(), span, &mut self.diags)
                {
                    // D-A11YGATE1=B (c134 Phase 6, E2931): duplicate accessible
                    // labels among an inline focus group's interactive nodes.
                    if method == "set_focus_group" {
                        self.check_a11y_focus_group_duplicates(args, span);
                    }
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some(handle_ty.to_string());
                    return ret;
                }
            }
            // c-devserver (owner-directed 2026-07-01): DevServer builder
            // methods (.html/.port/.serve).
            if handle_ty == "DevServer" {
                if let Some(ret) =
                    devserver_method_return(method, args.len(), span, &mut self.diags)
                {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some("DevServer".to_string());
                    return ret;
                }
            }
            // D-WEBAPP1=D: App builder chain (.route/.action/.mount/…).
            if handle_ty == "App" {
                if matches!(method, "route" | "page" | "layout") {
                    self.check_http_route_constant("App", method, args);
                }
                if let Some(ret) = app_method_return(method, args.len(), span, &mut self.diags) {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some("App".to_string());
                    return ret;
                }
            }
        }
        // D-DET1: methods on the deterministic injected Clock/Rng capability (and
        // Stopwatch). Reading time/randomness THROUGH the handle is reproducible.
        // Set `recv_type_out` so codegen routes the call to the handle-method op
        // (TIR shape (h)) rather than failing the typed-IR subset check.
        let handle_receiver = match &recv_ty {
            Type::Tagged {
                marker: crate::AST::TagMarker::Internal(crate::AST::InternalTag::SystemClock),
                inner,
            } => inner.as_ref(),
            other => other,
        };
        if let Type::Named(handle_ty) = handle_receiver {
            let source_owned = self.source_owns_nominal(handle_ty)
                || self.core_source_declares_method(handle_ty, method);
            // D-AUTHORITY-NAME1=A / D-AUTHORITY-WORD2=E: the carried
            // Authority value is ordinary data. Its only instance family is
            // the Prelude `with`/`without` narrowing pair.
            if handle_ty == crate::Syntax::TYPE_AUTHORITY {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let handle_ty = handle_ty.clone();
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some(handle_ty);
                    *resolved_ret_out = result.clone();
                    return result;
                }
            }
            // D-DET-CAPAPI: `rng.pick(list)` / `rng.shuffle(&list)` are GENERIC — the
            // element type comes from the `[T]` arg, mirroring the ambient
            // `random.pick`/`random.shuffle`. Resolve element-aware here (the
            // `builtin_method_return` table only carries Int placeholders).
            if !source_owned && handle_ty == crate::Syntax::RNG_TYPE
                && matches!(method, "pick" | "weighted_pick" | "sample" | "shuffle")
            {
                let handle_ty = handle_ty.clone();
                let result = self.finish_rng_generic(receiver, method, args, span);
                *recv_type_out = Some(handle_ty);
                *resolved_ret_out = result.clone();
                return result;
            }
            if matches!(
                handle_ty.as_str(),
                "Clock"
                    | "RNG"
                    | crate::Syntax::DETERMINISTIC_WORLD_TYPE
                    | "Duration"
                    | "TestSuite"
            ) && !source_owned {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let handle_ty = handle_ty.clone();
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some(handle_ty);
                    *resolved_ret_out = result.clone();
                    return result;
                }
            }
        }
        // D-REACT1=B: methods on the reactive handle types `Signal<T>`/`Derived<T>`
        // (`.get()`, `.set(v)`). Set `recv_type_out` to the handle name so codegen
        // routes the call to the reactive-method shape (keyed on recv_type, not the
        // bare method name — `get`/0 would otherwise alias a list `get`).
        if let Type::Apply { name, .. } = &recv_ty {
            if matches!(name.as_str(), "Signal" | "Derived" | "Computed") {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let handle_ty = name.clone();
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some(handle_ty);
                    return result;
                }
            }
        }
        // D-FFI-CALLBACK2=A: event.stop() closes admission without waiting.
        // The receiver is represented by a TLS event slot in the runtime
        // route, so lowering emits a zero-argument native helper.
        if let Type::Apply { name, .. } = &recv_ty {
            if name == "FfiCallbackEvent" && method == "stop" && args.is_empty() {
                *recv_type_out = Some(name.clone());
                return Some(Type::Tuple(vec![]));
            }
        }
        // D-EVENT1=D: methods on compiler-known Event<T>/Hook<T,R> values.
        // Set `recv_type_out` so TIR lowers these to the event runtime method
        // shape instead of the generic collection builtin fallback.
        if let Type::Apply { name, .. } = &recv_ty {
            if matches!(
                name.as_str(),
                crate::Syntax::TYPE_EVENT
                    | crate::Syntax::TYPE_ASYNC_EVENT
                    | crate::Syntax::TYPE_HOOK
                    | crate::Syntax::TYPE_DECISION_HOOK
                    | crate::Syntax::TYPE_DISPATCH_REPORT
            ) {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let handle_ty = name.clone();
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some(handle_ty);
                    return result;
                }
            }
        }
        if let Type::Named(name) = &recv_ty {
            if matches!(
                name.as_str(),
                crate::Syntax::TYPE_EFFECT
                    | crate::Syntax::TYPE_SUBSCRIPTION
                    | crate::Syntax::TYPE_EVENT_SCOPE
                    | crate::Syntax::TYPE_EVENT_TRACE
            ) {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let handle_ty = name.clone();
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some(handle_ty);
                    return result;
                }
            }
        }
        if let Type::Named(name) = &recv_ty {
            if matches!(
                name.as_str(),
                crate::Syntax::TYPE_WATCH_HANDLE | crate::Syntax::TYPE_WATCH_SET
            ) {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let handle_ty = name.clone();
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some(handle_ty);
                    return result;
                }
            }
        }
        // D-TYPE2-UNCERT1=A: a Measurement exposes its two observations.
        // Arithmetic is the ordinary operator surface; the old named
        // add/sub/mul/div methods are not a second spelling.
        if let Type::Apply { name, .. } = &recv_ty {
            if (name == crate::Syntax::TYPE_MEASUREMENT
                || (jet_foundation::CoreModuleExports::core_source_owns_type(name)
                    && name.ends_with("::Core/Units/Units.jet::Measurement")))
                && matches!((method, args.len()), ("value" | "uncertainty", 0))
            {
                *recv_type_out = Some(crate::Syntax::TYPE_MEASUREMENT.to_string());
                return Some(Type::Float);
            }
        }
        // D-MEM1 S6 (D-POOLID-API1=A / D-SHARED-API1=A): `Pool<T>.add/remove/ids`
        // and `Shared<T>.read/edit` set `recv_type_out` explicitly (unlike Task/
        // Sender/Receiver, whose method NAMES alone are globally unambiguous —
        // `add`/`remove`/`ids`/`read`/`edit` collide with Set/List/Map method
        // names, so codegen's subset gate needs `recv_type` to disambiguate the
        // receiver, the same reason Signal/Derived/Measurement set it above).
        // The actual dispatch (`finish_pool_add` etc.) already lives in
        // `finish_builtin_method`, reached the same way Signal/Derived reach it.
        if let Type::Apply { name, .. } = &recv_ty {
            if method == "force" && (name == crate::Syntax::EXPIRING_VALUE_TYPE
                || (jet_foundation::CoreModuleExports::core_source_owns_type(name)
                    && name.ends_with("::Core/Time/Time.jet::ExpiringValue")))
            {
                self.diags.push(Diagnostic::error(
                    "E0511",
                    "`ExpiringValue.force` bypasses expiry checking".to_string(),
                    "TTL-wrapped values must use fallible `get(clock)` so expired access is handled explicitly".to_string(),
                    "use `match item.get(clock) { .Ok(v) -> …; .Err(Expired) -> … }` instead".to_string(),
                    Some(span),
                ));
                return None;
            }
            let source_expiring_secret =
                jet_foundation::CoreModuleExports::core_source_owns_type(name)
                    && name.ends_with("::Core/Crypto/Vault.jet::ExpiringSecret");
            if (name == "ExpiringSecret" || source_expiring_secret) && method == "with" {
                if !expiring_clock_is_deterministic {
                    self.record_effect(Effect::Time.name(), span);
                    if self.in_pure && self.det_suppress == 0 {
                        self.diags
                            .push(crate::Sema::e3403("ExpiringSecret.with", Some(span)));
                    }
                }
                let inner = match &recv_ty {
                    Type::Apply { args, .. } => args.first().cloned().unwrap_or(Type::Int),
                    _ => unreachable!(),
                };
                let result = self.finish_expiring_secret_with(&inner, args, span);
                if !source_expiring_secret {
                    *recv_type_out = Some("ExpiringSecret".to_string());
                    return result.map(|ok| Type::Result {
                        ok: Box::new(ok),
                        err: Box::new(Type::Named("Expired".to_string())),
                    });
                }
                // Preserve the existing non-escaping read-loan check, but
                // let ordinary registered method inference/lowering invoke
                // the source body (no callback crosses a Rust host leaf).
                result?;
            }
            if name == "Pool" {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some("Pool".to_string());
                    return result;
                }
            }
            if matches!(name.as_str(), "Cell" | "CellReadGuard" | "CellEditGuard") {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    // Cell methods refine the builtin table's placeholder
                    // return from the checked receiver and callback types.
                    // Persist that exact fact for every later tier.
                    *resolved_ret_out = result.clone();
                    *recv_type_out = Some(name.clone());
                    return result;
                }
            }
        }
        if let Type::Shared(_) = &recv_ty {
            if let Some(ret) =
                Collections::builtin_method_return(&recv_ty, method, args.len(), false)
            {
                let result =
                    self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                *recv_type_out = Some("Shared".to_string());
                // D-STM1=A (card #506): a `Shared.edit` on the direct path of a
                // `#Transact` block joins the block's atomic commit — the write is
                // DEFERRED, so the call yields nothing (Unit), matching codegen's
                // `edit_txn(…) -> ()`. Consuming it (`x :: h.edit(…)`) then fails as an
                // ordinary type error against Unit. `txn_depth` is 0 inside a nested
                // lambda (an `on_commit` hook / a spawned task), where the edit stays
                // immediate and keeps its normal return — so this narrows exactly to
                // the edits the STM plane actually defers (the codegen routes the same
                // set, resetting `in_stm_transact` for lambda bodies).
                if method == "edit" && self.txn_depth > 0 {
                    let deferred = Type::Tuple(vec![]);
                    *resolved_ret_out = Some(deferred.clone());
                    return Some(deferred);
                }
                // D-CONC-SHARE1=A: `read`/`edit` answer whatever the closure
                // answers — for the plain-access desugar that is ONE FIELD of
                // the payload, not the payload. `shared_method_return`'s row is
                // a placeholder that says `T`, so persist the checked type the
                // way the Cell branch above does; otherwise every later tier
                // re-derives the placeholder and a chained call off the read
                // (`queue.jobs.first()`) dispatches against `T` and loses its
                // own result type.
                *resolved_ret_out = result.clone();
                return result;
            }
        }
        if let Type::Apply { name, .. } = &recv_ty {
            if name == crate::Syntax::TYPE_SHARED_SNAPSHOT {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some(crate::Syntax::TYPE_SHARED_SNAPSHOT.to_string());
                    *resolved_ret_out = result.clone();
                    return result;
                }
            }
        }
        if let Type::Apply { name, .. } = &recv_ty {
            if name == crate::Syntax::TYPE_SHARED_WEAK {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some(crate::Syntax::TYPE_SHARED_WEAK.to_string());
                    return result;
                }
            }
        }
        let shared_guard_ty = match &recv_ty {
            Type::Tagged { marker, inner }
                if matches!(
                    marker,
                    crate::AST::TagMarker::Internal(
                        crate::AST::InternalTag::SharedGuardRead
                            | crate::AST::InternalTag::SharedGuardEdit
                    )
                ) && matches!(
                    inner.as_ref(),
                    Type::Apply { name, .. }
                        if name == crate::Syntax::TYPE_SHARED_GUARD
                ) =>
            {
                Some(recv_ty.clone())
            }
            Type::Apply { name, .. } if name == crate::Syntax::TYPE_SHARED_GUARD => {
                let editable = match &**receiver {
                    Expr::Ident(name, _) => self
                        .lookup(name)
                        .is_some_and(|info| info.param_conv == Some(AccessConvention::Write)),
                    _ => false,
                };
                Some(Type::Tagged {
                    marker: crate::AST::TagMarker::Internal(if editable {
                        crate::AST::InternalTag::SharedGuardEdit
                    } else {
                        crate::AST::InternalTag::SharedGuardRead
                    }),
                    inner: Box::new(recv_ty.clone()),

                })
            }
            _ => None,
        };
        if let Some(shared_guard_ty) = shared_guard_ty {
            if let Some(result) =
                self.finish_shared_guard_method(receiver, &shared_guard_ty, method, args, span)
            {
                *recv_type_out = Some(crate::Syntax::TYPE_SHARED_GUARD.to_string());
                *resolved_ret_out = result.clone();
                return result;
            }
        }
        // D-SPACE-GEOMETRY1=A: generic and stock geometry methods share the
        // same closed return oracle.  This is the sema boundary that rejects
        // a mismatched transform middle space before any backend sees it.
        if let Some(base) = recv_ty.base_name() {
            if is_geometry_type(base)
                && (!self.registry.contains(base) || jet_foundation::CoreModuleExports::core_source_owns_type(base)) {
                let args_len = args.len();
                let mut arg_types = Vec::with_capacity(args_len);
                for (idx, arg) in args.iter_mut().enumerate() {
                    let depth_arg = idx == 1
                        && method == "point_at_depth"
                        && matches!(
                            &recv_ty,
                            Type::Apply { name, .. }
                                if matches!(crate::Sema::geometry_nominal_leaf(name), "Transform" | "Transform2")
                        )
                        && args_len == 2;
                    if depth_arg {
                        self.expect_core_arg(
                            "Transform.point_at_depth",
                            idx,
                            &Type::Float,
                            arg,
                        );
                        arg_types.push(Type::Float);
                    } else if let Some(arg_ty) = self.infer(&mut arg.expr) {
                        arg_types.push(arg_ty);
                    }
                }
                if let Some(result) = geometry_method_return(&recv_ty, method, &arg_types) {
                    *recv_type_out = Some(base.to_string());
                    match result {
                        Ok(ret) => {
                            *resolved_ret_out = Some(ret.clone());
                            return Some(ret);
                        }
                        Err(()) => {
                            if let Some((first, second, middle_a, middle_b)) =
                                geometry_transform_diagnostic(&recv_ty, method, &arg_types)
                            {
                                self.diags.push(Diagnostic::from_row(
                                    "E2521",
                                    &[
                                        ("first", first.as_str()),
                                        ("second", second.as_str()),
                                        ("middle_a", middle_a.as_str()),
                                        ("middle_b", middle_b.as_str()),
                                    ],
                                    Some(span),
                                ));
                            } else {
                                let expected = geometry_space(&recv_ty)
                                    .map(|space| space.show())
                                    .unwrap_or_else(|| recv_ty.show());
                                let actual = arg_types
                                    .first()
                                    .and_then(geometry_space)
                                    .map(|space| space.show())
                                    .or_else(|| arg_types.first().map(|ty| ty.show()))
                                    .unwrap_or_else(|| "<missing>".to_string());
                                let expected = if method == "add" && geometry_is_point(&recv_ty) {
                                    format!("a displacement in {expected}")
                                } else if method == "sub" && geometry_is_point(&recv_ty) {
                                    format!("a point or displacement in {expected}")
                                } else if geometry_is_delta(&recv_ty) {
                                    format!("a displacement in {expected}")
                                } else {
                                    expected
                                };
                                let operation = format!("{}.{method}", crate::Sema::geometry_nominal_leaf(base));
                                self.diags.push(Diagnostic::from_row(
                                    "E2520",
                                    &[
                                        ("operation", operation.as_str()),
                                        ("expected", expected.as_str()),
                                        ("actual", actual.as_str()),
                                    ],
                                    Some(span),
                                ));
                            }
                            return None;
                        }
                    }
                }
            }
        }

        // D-SIMD2 / D-LINALG1: methods on the built-in math value types
        // (`v.dot(w)`, `v.length()`, `v.sum()`, `v.reduce(.Max)`, `m.matmul(n)`).
        // Operator overloading on this closed family is blessed; named methods are
        // the rest of the surface. Set `recv_type_out` so codegen routes to the
        // math-method op (TIR handle-method path).
        if let Type::Named(math_ty) = &recv_ty {
            if is_math_type(math_ty) && !self.registry.contains(math_ty) {
                let math_ty = math_ty.clone();
                // D-REDUCE-VALUE1=A: the sole argument is a Core ReduceOp value.
                if method == "reduce" && is_simd_lane_type(&math_ty) {
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E2510",
                            format!("`reduce` takes one `ReduceOp` value, got {}", args.len()),
                            "a lane reduction uses the closed Core `ReduceOp` enum".to_string(),
                            "write `v.reduce(.Add)`, `.Mul`, `.Min`, `.Max`, or `.Avg`".to_string(),
                            Some(span),
                        ));
                    } else if let Expr::ReduceMarker(op, mspan) = &args[0].expr {
                        let replacement = crate::Policy::applied_rule(op)
                            .and_then(|row| match row.status {
                                crate::Policy::RuleStatus::Retired { replacement } => {
                                    Some(replacement)
                                }
                                crate::Policy::RuleStatus::Active => None,
                            })
                            .unwrap_or(".Add");
                        self.diags.push(Diagnostic::error(
                            "E2510",
                            format!("`#{op}` is retired as a reduce selector"),
                            "reduce operations are ordinary Core `ReduceOp` enum values"
                                .to_string(),
                            format!("write `v.reduce({replacement})`"),
                            Some(*mspan),
                        ));
                    } else if let Expr::EnumLit {
                        type_name,
                        variant,
                        args: enum_args,
                        span: value_span,
                        ..
                    } = &args[0].expr
                    {
                        let valid = (type_name.is_empty()
                            || type_name == crate::Syntax::TYPE_REDUCE_OP)
                            && enum_args.is_empty()
                            && simd_reduce_markers().contains(&variant.as_str());
                        let value_span = *value_span;
                        if valid {
                            let old = self.expected_type.take();
                            self.expected_type =
                                Some(Type::Named(crate::Syntax::TYPE_REDUCE_OP.to_string()));
                            self.infer(&mut args[0].expr);
                            self.expected_type = old;
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E2510",
                                "this value is not a `ReduceOp`".to_string(),
                                "`reduce` accepts `.Add`, `.Mul`, `.Min`, `.Max`, or `.Avg`"
                                    .to_string(),
                                "use a Core `ReduceOp` value".to_string(),
                                Some(value_span),
                            ));
                        }
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E2510",
                            "`reduce` takes a `ReduceOp` value".to_string(),
                            "the operation is an ordinary closed Core enum value".to_string(),
                            "write `v.reduce(.Add)`, `.Mul`, `.Min`, `.Max`, or `.Avg`".to_string(),
                            Some(args[0].expr.span()),
                        ));
                        self.infer(&mut args[0].expr);
                    }
                    *recv_type_out = Some(math_ty.clone());
                    return Some(math_scalar_ty(&math_ty));
                }
                if let Some(ret) = math_method_return(&math_ty, method, args.len()) {
                    // Check each arg against its expected type (so a literal
                    // elaborates), then return the method's result type.
                    for arg in args.iter_mut() {
                        let want = math_method_arg_ty(&math_ty, method);
                        let old = self.expected_type.take();
                        if let Some(w) = &want {
                            self.expected_type = Some(w.clone());
                        }
                        let got = self.infer(&mut arg.expr);
                        self.expected_type = old;
                        if let (Some(w), Some(g)) = (&want, &got) {
                            if g != w {
                                self.diags.push(Diagnostic::error(
                                    "E0128",
                                    format!(
                                        "`.{}()` on `{}` expects a `{}`, got `{}`",
                                        method,
                                        math_ty,
                                        w.name(),
                                        g.name()
                                    ),
                                    format!(
                                        "`{}.{}(…)` operates on a `{}`",
                                        math_ty,
                                        method,
                                        w.name()
                                    ),
                                    format!("pass a `{}` value", w.name()),
                                    Some(arg.expr.span()),
                                ));
                            }
                        }
                    }
                    *recv_type_out = Some(math_ty.clone());
                    return Some(ret);
                }
                // A math type but an unknown method — fall through to the generic
                // "no such method" path below, which prints a teaching diagnostic.
            }
        }
        if let Type::Named(n) = &recv_ty {
            if let Some(param) = self.type_param_scope.iter().find(|p| p.name == *n) {
                let bounds = param.bounds.clone();
                let known_method = bounds
                    .iter()
                    .find_map(|trait_name| {
                        self.trait_reg.trait_info(trait_name)
                            .and_then(|info| info.methods.get(method))
                            .map(|msig| (trait_name.clone(), msig.clone()))
                    })
                    .or_else(|| {
                        self.trait_reg.trait_infos()
                            .filter_map(|(trait_name, info)| {
                                info.methods.get(method).map(|msig| (trait_name, msig))
                            })
                            .min_by(|(left, _), (right, _)| left.cmp(right))
                            .map(|(trait_name, msig)| (trait_name.clone(), msig.clone()))
                    });
                if let Some((trait_name, mut msig)) = known_method {
                    // Inside a generic body the bounded parameter is the
                    // trait's `Self`: `left.compare(right)` with `T: Comparable`
                    // wants a `T`. Synthetic operator traits spell `Self` as
                    // the empty name.
                    let self_subst: HashMap<String, Type> = [
                        ("Self".to_string(), recv_ty.clone()),
                        (String::new(), recv_ty.clone()),
                    ]
                    .into_iter()
                    .collect();
                    for param in &mut msig.params {
                        param.ty = crate::Generics::substitute_type(&param.ty, &self_subst);
                    }
                    msig.return_type = msig
                        .return_type
                        .as_ref()
                        .map(|ret| crate::Generics::substitute_type(ret, &self_subst));
                    self.record_open_memory_dispatch(
                        span,
                        "generic trait dispatch has no sealed target set",
                    );
                    self.record_edge(
                        format!(
                            "{}::<static>",
                            crate::Sema::effect_key(Some(&trait_name), method)
                        ),
                        span,
                    );
                    if !bounds.iter().any(|bound| bound == &trait_name) {
                        self.diags.push(e0901(method, &trait_name, span));
                    }
                    *recv_type_out = Some(trait_name.clone());
                    let source = self.check_trait_method_args(method, &msig, receiver, args, span);
                    // The AST fact is the checked carrier for ordinary trait
                    // calls. Raw protocol traits retain their ABI declaration
                    // so auto-propagation cannot add `Try`.
                    let raw_protocol_return = crate::Sema::uses_raw_protocol_return(
                        Some(&trait_name),
                        false,
                        false,
                    );
                    let (resolved_ret, _carrier) =
                        self.checked_return_types(msig.return_type.clone(), raw_protocol_return);
                    *resolved_ret_out = Some(resolved_ret);
                    return source;
                }
            }
        }
        // D-SOA1: a `[S]` of a `#layout(columnar)` struct is stored
        // struct-of-arrays. v1 supports the core list surface (construct,
        // index-read, field-read, `len`, `is_empty`, `push`, iterate); the rest
        // is deferred — reject the method with a clear message rather than
        // silently miscompiling on columnar storage.
        if let Type::List(inner) = &recv_ty {
            if let Type::Named(elem) = inner.as_ref() {
                if self.registry.is_columnar(elem)
                    && !matches!(method, "len" | "is_empty" | "push")
                    && Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                        .is_some()
                {
                    self.diags.push(Diagnostic::error(
                            "E1108",
                            format!(
                                "`.{}()` isn't supported on a columnar list `{}` yet",
                                method,
                                recv_ty.show()
                            ),
                            "`#Layout(columnar)` lists support the core surface in v1: indexing, field access, `len`, `is_empty`, `push`, and iteration".to_string(),
                            format!(
                                "drop `#Layout(columnar)` from `{}` to use `.{}()`, or rewrite the loop with indexing",
                                elem, method
                            ),
                            Some(span),
                        ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
            }
        }
        // D-SERDE-ACCESS=B: accessor methods on Data/JSON/Dyn.
        if let Type::Named(ref tn) = recv_ty {
            if is_json_type_name(tn) {
                if let Some(ret) = dyn_method_return(method, args.len()) {
                    if method == "remove" {
                        self.check_mutating_method_receiver(receiver, method, span);
                    }
                    let json_ret = match ret {
                        Type::Result { ok, err } => Type::Result {
                            ok: if matches!(*ok, Type::Named(ref n) if n == "Dyn") {
                                Box::new(json_ty())
                            } else {
                                ok
                            },
                            err,
                        },
                        other => other,
                    };
                    let saved = self.expected_type.take();
                    for a in args.iter_mut() {
                        let expected = match method {
                            "has" | "remove" | "field" => Some(Type::String),
                            "at" => Some(Type::Int),
                            "equal_unordered" => Some(json_ty()),
                            _ => None,
                        };
                        let got = match expected.as_ref() {
                            Some(want) => self.infer_with_expected(&mut a.expr, want),
                            None => self.infer(&mut a.expr),
                        };
                        if let (Some(want), Some(got)) = (expected.as_ref(), got.as_ref()) {
                            self.check_type_assignable(want, got, a.span);
                        }
                        if method == "get" && got.as_ref().is_some_and(|ty| *ty != Type::String && *ty != Type::Int) {
                            self.diags.push(Diagnostic::error("E0505",
                                "Dyn.get requires a String key or Int index".to_string(),
                                "Dyn reads object keys and array positions".to_string(),
                                "use a String or Int".to_string(), Some(a.span)));
                        }
                    }
                    self.expected_type = saved;
                    *recv_type_out = Some("Dyn".to_string());
                    return Some(json_ret);
                }
            }
            // D-DBDRIVER1: accessor methods on `DBValue` (`.int()`/`.text()`/
            // `.bool()`/`.float()`/`.is_null()`) — read back a bound/column value.
            if is_db_value_type_name(tn) {
                if let Some(ret) = db_value_method_return(method, args.len()) {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some(tn.clone());
                    return Some(ret);
                }
            }
            // D-ANY-JAI1 (c7jaiany §6): accessor methods on `reflect.of(x)`'s
            // `Value`/`Field` handles — same zero-arg-getter shape as `DBValue`
            // above. `Value`/`Field` are common enough words a user struct might
            // reuse them (`Examples/features/memory/zerocopy.jet` already has its
            // own `struct Field`) — `!self.registry.contains(tn)` makes a
            // user-declared type of that name win, same principle as codegen's
            // `!self.type_names.contains(name)` guard on the Rust-type-name side.
            if is_reflect_type_name(tn) && !self.registry.contains(tn) {
                if let Some(ret) = reflect_method_return(tn, method, args.len()) {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some(tn.clone());
                    return Some(ret);
                }
            }
            // D-LAYOUT1 / D-LAYOUT-GATES1: methods on the built-in
            // `LayoutHandle`/`Constraint` types (`form.h("label","width")`,
            // `form.value(v)`, `form.suggest(v, 90.0)`, `c.medium()`, …).
            // `.h`/`.v`/`suggest`'s var argument (index 0 for `.value`/
            // `.suggest`) accepts any axis type (`HVar`/`VVar`/`LengthVar`),
            // so it's checked with `is_layout_axis_type` instead of the
            // single-fixed-`Type` table `layout_method_arg_ty` covers.
            if is_layout_type(tn) {
                if let Some(ret) = layout_method_return(tn, method, args.len()) {
                    for (i, arg) in args.iter_mut().enumerate() {
                        let axis_arg = (tn == "Layout"
                            && ((method == "value" && i == 0) || (method == "suggest" && i == 0)))
                            .then_some(());
                        let want = layout_method_arg_ty(method, i);
                        let old = self.expected_type.take();
                        if let Some(w) = &want {
                            self.expected_type = Some(w.clone());
                        }
                        let got = self.infer(&mut arg.expr);
                        self.expected_type = old;
                        if axis_arg.is_some() {
                            let ok = matches!(&got, Some(Type::Named(n)) if is_layout_axis_type(n));
                            if !ok {
                                self.diags.push(Diagnostic::error(
                                        "E0128",
                                        format!(
                                            "`.{}()` on `{}` expects a layout value (`HVar`/`VVar`/`LengthVar`), got `{}`",
                                            method,
                                            tn,
                                            got.as_ref().map(|t| t.name()).unwrap_or_default()
                                        ),
                                        "layout variables come from `handle.h(box, anchor)` / `handle.v(box, anchor)`, or a `box.anchor` read inside a `layout { … }` block".to_string(),
                                        "pass a layout variable, not a plain value".to_string(),
                                        Some(arg.expr.span()),
                                    ));
                            }
                        } else if let (Some(w), Some(g)) = (&want, &got) {
                            if g != w {
                                self.diags.push(Diagnostic::error(
                                    "E0128",
                                    format!(
                                        "`.{}()` on `{}` expects a `{}`, got `{}`",
                                        method,
                                        tn,
                                        w.name(),
                                        g.name()
                                    ),
                                    format!("`{}.{}(…)` operates on a `{}`", tn, method, w.name()),
                                    format!("pass a `{}` value", w.name()),
                                    Some(arg.expr.span()),
                                ));
                            }
                        }
                    }
                    *recv_type_out = Some(tn.clone());
                    return Some(ret);
                }
            }
        }
        // D-FLAGSHIP-WEBAPI1=A: generic web suite methods keep the receiver's
        // element type through sema; no engine-specific method table is needed.
        if let Some(ret) = web_method_return(&recv_ty, method, args.len()) {
            let (type_name, element) = match &recv_ty {
                Type::Named(name) => (name.as_str(), None),
                Type::Apply { name, args } => (name.as_str(), args.first().cloned()),
                _ => ("", None),
            };
            let expected = match (type_name, method) {
                ("JetDataPlot", "line" | "bar" | "point") => Some(0),
                ("JetDataPlot", "x" | "color" | "size" | "text" | "detail"
                    | "with_transform" | "with_scale" | "with_axis" | "with_legend"
                    | "with_layer" | "with_interaction" | "accessibility" | "layout"
                    | "select_indices") => Some(1),
                ("JetDataPlot", "y") => Some(2),
                ("JetDataPlot", "facet") => Some(1),
                ("WebTable", "with_column") => Some(1),
                ("WebTable", "paginate") => Some(2),
                ("WebTable", "page") => Some(0),
                ("WebStore", "set") => Some(1),
                ("WebStore", "signal" | "state_signal" | "facts_json")
                    | ("WebVirtualWindow", "facts_json") => Some(0),
                ("WebStorePatch", "generation" | "transaction" | "active" | "commit" | "rollback") => Some(0),
                ("WebStoreSubscription", "unsubscribe" | "active") => Some(0),
                (
                    "WebQuery",
                    "state" | "state_signal" | "mutation_state" | "mutation_signal"
                    | "invalidate" | "get" | "show" | "facts" | "cancel" | "refresh",
                ) => Some(0),
                ("WebFormTyped", "set_async_validator") => Some(4),
                ("WebFormTyped", "set") => Some(2),
                ("WebFormTyped", "blur" | "focus" | "set_action" | "post") => Some(1),
                ("WebFormTyped", "validate") if !args.is_empty() => Some(3),
                ("WebFormTyped", "validate") => Some(0),
                (
                    "WebFormTyped",
                    "cancel" | "submit" | "no_script" | "state" | "lifecycle"
                    | "errors" | "render" | "show",
                ) => Some(0),
                ("WebFormValidationChain", "render") => Some(0),
                _ => None,
            };
            let Some(expected) = expected else {
                return None;
            };
            if args.len() != expected {
                self.diags
                    .push(wrong_core_arity(method, expected, args.len(), span));
            }
            let row = element.unwrap_or(Type::Int);
            match (type_name, method) {
                ("JetDataPlot", _) => {
                    let column = Type::Apply {
                        name: "JetDataPlotColumn".to_string(),
                        args: vec![row.clone()],
                    };
                    match method {
                        "line" | "bar" | "point" => {}
                        "x" | "color" | "size" | "text" | "detail" => {
                            if let Some(arg) = args.get_mut(0) {
                                if !self.check_plot_column_selector(&row, arg) {
                                    self.expect_core_arg(method, 0, &column, arg);
                                }
                            }
                        }
                        "y" => {
                            if let Some(arg) = args.get_mut(0) {
                                if !self.check_plot_column_selector(&row, arg) {
                                    self.expect_core_arg(method, 0, &column, arg);
                                }
                            }
                            if let Some(arg) = args.get_mut(1) {
                                self.expect_core_arg(
                                    method,
                                    1,
                                    &Type::Named("JetDataPlotAggregate".to_string()),
                                    arg,
                                );
                            }
                        }
                        "with_transform" => {
                            if let Some(arg) = args.get_mut(0) {
                                self.expect_core_arg(
                                    method,
                                    0,
                                    &Type::Named("JetDataPlotTransform".to_string()),
                                    arg,
                                );
                            }
                        }
                        "with_scale" => {
                            if let Some(arg) = args.get_mut(0) {
                                self.expect_core_arg(
                                    method,
                                    0,
                                    &Type::Named("JetDataPlotScale".to_string()),
                                    arg,
                                );
                            }
                        }
                        "with_axis" => {
                            if let Some(arg) = args.get_mut(0) {
                                self.expect_core_arg(
                                    method,
                                    0,
                                    &Type::Named("JetDataPlotAxis".to_string()),
                                    arg,
                                );
                            }
                        }
                        "with_legend" => {
                            if let Some(arg) = args.get_mut(0) {
                                self.expect_core_arg(
                                    method,
                                    0,
                                    &Type::Named("JetDataPlotLegend".to_string()),
                                    arg,
                                );
                            }
                        }
                        "facet" => {
                            if let Some(arg) = args.get_mut(0) {
                                if !self.check_plot_column_selector(&row, arg) {
                                    self.expect_core_arg(method, 0, &column, arg);
                                }
                            }
                        }
                        "with_layer" => {
                            if let Some(arg) = args.get_mut(0) {
                                self.expect_core_arg(
                                    method,
                                    0,
                                    &Type::Named("JetDataPlotLayer".to_string()),
                                    arg,
                                );
                            }
                        }
                        "with_interaction" => {
                            if let Some(arg) = args.get_mut(0) {
                                self.expect_core_arg(
                                    method,
                                    0,
                                    &Type::Named("JetDataPlotInteraction".to_string()),
                                    arg,
                                );
                            }
                        }
                        "accessibility" => {
                            if let Some(arg) = args.get_mut(0) {
                                self.expect_core_arg(
                                    method,
                                    0,
                                    &Type::Named("JetDataPlotAccessibility".to_string()),
                                    arg,
                                );
                            }
                        }
                        "layout" => {
                            if let Some(arg) = args.get_mut(0) {
                                self.expect_core_arg(
                                    method,
                                    0,
                                    &Type::Named("JetDataPlotLayout".to_string()),
                                    arg,
                                );
                            }
                        }
                        "select_indices" => {
                            if let Some(arg) = args.get_mut(0) {
                                self.expect_core_arg(
                                    method,
                                    0,
                                    &Type::List(Box::new(Type::Int)),
                                    arg,
                                );
                            }
                        }
                        _ => unreachable!("web method return was checked above"),
                    }
                    self.record_call_receiver_access(receiver, AccessConvention::Read, span);
                }
                ("WebTable", "with_column") => {
                    let column = Type::Apply {
                        name: "WebTableColumn".to_string(),
                        args: vec![row],
                    };
                    if let Some(arg) = args.get_mut(0) {
                        self.expect_core_arg(method, 0, &column, arg);
                    }
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Move,
                        span,
                    );
                }
                ("WebTable", "paginate") => {
                    if let Some(arg) = args.get_mut(0) {
                        self.expect_core_arg(method, 0, &Type::Int, arg);
                    }
                    if let Some(arg) = args.get_mut(1) {
                        self.expect_core_arg(method, 1, &Type::Int, arg);
                    }
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Read,
                        span,
                    );
                }
                ("WebTable", "page") => {
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Read,
                        span,
                    );
                }
                ("WebStore", "set") => {
                    if let Some(arg) = args.get_mut(0) {
                        self.expect_core_arg(method, 0, &row, arg);
                    }
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Read,
                        span,
                    );
                }
                ("WebStore", "signal" | "state_signal" | "facts_json")
                | ("WebVirtualWindow", "facts_json") => {
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Read,
                        span,
                    );
                }
                ("WebStorePatch", "commit" | "rollback") => {
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Move,
                        span,
                    );
                }
                ("WebStoreSubscription", "unsubscribe" | "active") => {
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Read,
                        span,
                    );
                }
                ("WebQuery", _) => {
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Read,
                        span,
                    );
                }
                ("WebFormTyped", "validate") => {
                    if args.len() == 3 {
                        require_exact_labels(
                            "WebFormTyped.validate",
                            args,
                            &[(1, "on"), (2, "with")],
                            span,
                            &mut self.diags,
                        );
                    }
                    if let Some(arg) = args.get_mut(0) {
                        if !self.check_web_form_field_selector(arg) {
                            self.expect_core_arg(method, 0, &Type::String, arg);
                        }
                    }
                    if let Some(arg) = args.get_mut(1) {
                        self.expect_core_arg(
                            method,
                            1,
                            &Type::Named("WebFormValidationTiming".to_string()),
                            arg,
                        );
                    }
                    if let Some(arg) = args.get_mut(2) {
                        let validator = Type::Fn {
                            params: vec![Type::String],
                            ret: Some(Box::new(Type::Result {
                                ok: Box::new(unit_ty()),
                                err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
                            })),
                            effect_bound: None,
                            return_view_provenance: None,
                            param_contract: None,
                            call_metadata: None,
                        };
                        self.expect_core_arg(method, 2, &validator, arg);
                    }
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Read,
                        span,
                    );
                }
                ("WebFormTyped", "set_async_validator") => {
                    // ABI: form, field name, timing, debounce, callback.
                    let string = Type::String;
                    if let Some(arg) = args.get_mut(0) {
                        self.expect_core_arg(method, 0, &string, arg);
                    }
                    if let Some(arg) = args.get_mut(1) {
                        self.expect_core_arg(
                            method,
                            1,
                            &Type::Named("WebFormValidationTiming".to_string()),
                            arg,
                        );
                    }
                    if let Some(arg) = args.get_mut(2) {
                        self.expect_core_arg(method, 2, &Type::Int, arg);
                    }
                    if let Some(arg) = args.get_mut(3) {
                        let validator = Type::Fn {
                            params: vec![Type::String],
                            ret: Some(Box::new(Type::Result {
                                ok: Box::new(unit_ty()),
                                err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
                            })),
                            effect_bound: None,
                            return_view_provenance: None,
                            param_contract: None,
                            call_metadata: None,
                        };
                        self.expect_core_arg(method, 3, &validator, arg);
                    }
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Read,
                        span,
                    );
                }
                ("WebFormTyped", "set_action") => {
                    if let Some(arg) = args.get_mut(0) {
                        let handler = Type::Fn {
                            params: vec![Type::Named("WebFormDecodedInput".to_string())],
                            ret: Some(Box::new(Type::Result {
                                ok: Box::new(Type::String),
                                err: Box::new(Type::Named("WebFormActionError".to_string())),
                            })),
                            effect_bound: None,
                            return_view_provenance: None,
                            param_contract: None,
                            call_metadata: None,
                        };
                        self.expect_core_arg(method, 0, &handler, arg);
                    }
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Read,
                        span,
                    );
                }
                ("WebFormTyped", _) => {
                    for index in 0..expected {
                        if let Some(arg) = args.get_mut(index) {
                            self.expect_core_arg(method, index, &Type::String, arg);
                        }
                    }
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Read,
                        span,
                    );
                }
                ("WebFormValidationChain", "render") => {
                    self.record_call_receiver_access(
                        receiver,
                        AccessConvention::Read,
                        span,
                    );
                }
                _ => unreachable!("web method return was checked above"),
            }
            for arg in args.iter_mut().skip(expected) {
                self.infer(&mut arg.expr);
            }
            *recv_type_out = Some(type_name.to_string());
            if let Some(ret) = &ret {
                *resolved_ret_out = Some(ret.clone());
            }
            return ret;
        }
        // D-QUERY-RETAIN1=A: Query<T> methods preserve the deferred carrier;
        // only `collect` changes the value back to an ordinary list.
        // `group_by` yields the internal `DataGroupedQuery<T, K>` carrier whose
        // reducers return `Query<Group<K, V>>`; the key and value types are the
        // ordinary callback result types, never a String/Float projection.
        if let Type::Apply { name: head, args: query_args } = &recv_ty {
            if data_nominal_leaf(head) == Some("DataTracked") && query_args.len() == 2 {
                let row = query_args[0].clone();
                let key = query_args[1].clone();
                match method {
                    "query" => {
                        if !args.is_empty() {
                            self.diags
                                .push(wrong_core_arity(method, 0, args.len(), span));
                        }
                        let ret = Type::Apply {
                            name: "Query".to_string(),
                            args: vec![row.clone()],
                        };
                        *recv_type_out = Some("DataTracked".to_string());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    "insert" => {
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity(method, 1, args.len(), span));
                        }
                        if let Some(arg) = args.get_mut(0) {
                            self.expect_core_arg(method, 0, &row, arg);
                        }
                    }
                    "replace" => {
                        if args.len() != 2 {
                            self.diags
                                .push(wrong_core_arity(method, 2, args.len(), span));
                        }
                        if let Some(arg) = args.get_mut(0) {
                            self.expect_core_arg(method, 0, &key, arg);
                        }
                        if let Some(arg) = args.get_mut(1) {
                            self.expect_core_arg(method, 1, &row, arg);
                        }
                    }
                    "remove" => {
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity(method, 1, args.len(), span));
                        }
                        if let Some(arg) = args.get_mut(0) {
                            self.expect_core_arg(method, 0, &key, arg);
                        }
                    }
                    _ => {}
                }
                if matches!(method, "insert" | "replace" | "remove") {
                    let ret = Type::Result {
                        ok: Box::new(unit_ty()),
                        err: Box::new(Type::Named("DataError".to_string())),
                    };
                    *recv_type_out = Some("DataTracked".to_string());
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
            }
            if data_nominal_leaf(head) == Some("DataWatch") && query_args.len() == 1 {
                let row = query_args[0].clone();
                match method {
                    "get" => {
                        if !args.is_empty() {
                            self.diags
                                .push(wrong_core_arity(method, 0, args.len(), span));
                        }
                        let ret = Type::Result {
                            ok: Box::new(Type::List(Box::new(row))),
                            err: Box::new(Type::Named("DataError".to_string())),
                        };
                        *recv_type_out = Some("DataWatch".to_string());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    "status" => {
                        if !args.is_empty() {
                            self.diags
                                .push(wrong_core_arity(method, 0, args.len(), span));
                        }
                        let ret = Type::Named("DataWatchStatus".to_string());
                        *recv_type_out = Some("DataWatch".to_string());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    "cancel" => {
                        if !args.is_empty() {
                            self.diags
                                .push(wrong_core_arity(method, 0, args.len(), span));
                        }
                        let ret = unit_ty();
                        *recv_type_out = Some("DataWatch".to_string());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    _ => {}
                }
            }
            if data_nominal_leaf(head) == Some("Query") && query_args.len() == 1 {
                let row = query_args[0].clone();
                match method {
                    "filter" | "sort_by" => {
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity(method, 1, args.len(), span));
                        }
                        if let Some(callback) = args.get_mut(0) {
                            let callback_ty = Type::Fn {
                                params: vec![row.clone()],
                                ret: Some(Box::new(if method == "filter" {
                                    Type::Bool
                                } else {
                                    Type::String
                                })),
                                // Ordinary Query combinators collect later; empty
                                // effect bounds belong on live `.watch()` callbacks.
                                effect_bound: None,
                                return_view_provenance: None,
                                param_contract: None,
                                call_metadata: None,
                            };
                            self.expect_core_arg(method, 0, &callback_ty, callback);
                        }
                        let ret = Type::Apply {
                            name: head.clone(),
                            args: vec![row],
                        };
                        *recv_type_out = Some("Query".to_string());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    "map" | "min" | "max" => {
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity(method, 1, args.len(), span));
                        }
                        let value = args
                            .get_mut(0)
                            .and_then(|callback| self.infer_query_callback(method, &row, callback))
                            .unwrap_or_else(|| row.clone());
                        let ret = Type::Apply {
                            name: head.clone(),
                            args: vec![value],
                        };
                        *recv_type_out = Some("Query".to_string());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    "inner_join" | "left_join" => {
                        if args.len() != 3 {
                            self.diags
                                .push(wrong_core_arity(method, 3, args.len(), span));
                        }
                        let right_row = args.get_mut(0).and_then(|other| {
                            let other_ty = self.infer(&mut other.expr);
                            match other_ty.as_ref().map(Type::without_user_tags) {
                                Some(Type::Apply { name, args })
                                    if data_nominal_leaf(name) == Some("Query") && args.len() == 1 =>
                                {
                                    Some(args[0].clone())
                                }
                                Some(other_ty) => {
                                    self.diags.push(Diagnostic::error(
                                        "E0112",
                                        format!(
                                            "`{method}` wants a Query as argument 1, but this is {}",
                                            other_ty.show()
                                        ),
                                        "keyed joins combine two typed deferred queries".to_string(),
                                        "pass another `Query<Row>` value".to_string(),
                                        Some(other.expr.span()),
                                    ));
                                    None
                                }
                                None => None,
                            }
                        });
                        let right_row = right_row.unwrap_or_else(|| row.clone());
                        let _left_key = args.get_mut(1).and_then(|callback| {
                            self.infer_query_callback(method, &row, callback)
                        });
                        let _right_key = args.get_mut(2).and_then(|callback| {
                            self.infer_query_callback(method, &right_row, callback)
                        });
                        let joined_right = if method == "left_join" {
                            Type::Option(Box::new(right_row))
                        } else {
                            right_row
                        };
                        let ret = Type::Apply {
                            name: head.clone(),
                            args: vec![Type::Apply {
                                name: "DataJoin".to_string(),
                                args: vec![row, joined_right],
                            }],
                        };
                        *recv_type_out = Some("Query".to_string());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    "collect" => {
                        if args.len() != 0 {
                            self.diags
                                .push(wrong_core_arity(method, 0, args.len(), span));
                        }
                        let ret = Type::Result {
                            ok: Box::new(Type::List(Box::new(row))),
                            err: Box::new(Type::Named("DataError".to_string())),
                        };
                        *recv_type_out = Some("Query".to_string());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    "plan" => {
                        if !args.is_empty() {
                            self.diags
                                .push(wrong_core_arity(method, 0, args.len(), span));
                        }
                        let ret = Type::List(Box::new(Type::String));
                        *recv_type_out = Some("Query".to_string());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    "group_by" => {
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity(method, 1, args.len(), span));
                        }
                        let key = args
                            .get_mut(0)
                            .and_then(|callback| self.infer_query_callback(method, &row, callback))
                            .unwrap_or(Type::String);
                        let ret = Type::Apply {
                            name: "DataGroupedQuery".to_string(),
                            args: vec![row.clone(), key],
                        };
                        *recv_type_out = Some("Query".to_string());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    "watch" => {
                        if !args.is_empty() {
                            self.diags
                                .push(wrong_core_arity(method, 0, args.len(), span));
                        }
                        let ret = Type::Result {
                            ok: Box::new(Type::Apply {
                                name: "DataWatch".to_string(),
                                args: vec![row.clone()],
                            }),
                            err: Box::new(Type::Named("DataError".to_string())),
                        };
                        *recv_type_out = Some("Query".to_string());
                        *resolved_ret_out = Some(ret.clone());
                        return Some(ret);
                    }
                    _ => {}
                }
            }
            if data_nominal_leaf(head) == Some("DataGroupedQuery") && query_args.len() == 2 {
                let row = query_args[0].clone();
                let key = query_args[1].clone();
                let value = match method {
                    "count" => {
                        if !args.is_empty() {
                            self.diags
                                .push(wrong_core_arity(method, 0, args.len(), span));
                        }
                        Some(Type::Int)
                    }
                    "sum" | "mean" => {
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity(method, 1, args.len(), span));
                        }
                        let value = args
                            .get_mut(0)
                            .and_then(|callback| self.infer_query_callback(method, &row, callback));
                        if method == "sum"
                            && !matches!(
                                value,
                                Some(
                                    Type::Int
                                        | Type::IntN { .. }
                                        | Type::Float
                                        | Type::Float32
                                )
                            )
                        {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`sum` wants an Int, fixed-width Int, Float, or F32 result, but this is {}",
                                    value
                                        .as_ref()
                                        .map_or_else(|| "missing".to_string(), |ty| ty.show())
                                ),
                                "grouped sum follows the runtime numeric domain and does not add arbitrary values"
                                    .to_string(),
                                "return a numeric row value, or use `count` for cardinality".to_string(),
                                Some(span),
                            ));
                        }
                        if method == "mean" {
                            if let Some(value) = &value {
                                if !matches!(value, Type::Float) {
                                    self.diags.push(Diagnostic::error(
                                        "E0112",
                                        format!(
                                            "`mean` wants Float for argument 1's result, but this is {}",
                                            value.show()
                                        ),
                                        "a grouped mean follows the Float numeric rules; \
                                         it does not widen integers or units silently"
                                            .to_string(),
                                        "return a Float from the value callback, or use `sum`"
                                            .to_string(),
                                        Some(span),
                                    ));
                                }
                            }
                            Some(Type::Float)
                        } else {
                            Some(value.unwrap_or(Type::Int))
                        }
                    }
                    _ => None,
                };
                if let Some(value) = value {
                    let ret = Type::Apply {
                        name: "Query".to_string(),
                        args: vec![Type::Apply {
                            name: "Group".to_string(),
                            args: vec![key, value],
                        }],
                    };
                    *recv_type_out = Some("DataGroupedQuery".to_string());
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
            }
        }
        // D-ZIPPAD1: list/iterator zip-family calls use one variadic typed
        // contract for free and method spellings. Option.zip remains the
        // separate nullable combinator below.
        if method == "query" {
            if let Type::List(inner) = &recv_ty {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity(method, 1, args.len(), span));
                }
                if let Some(query) = args.get_mut(0) {
                    self.check_analytics_sql_schema(inner, &query.expr);
                    self.expect_core_arg(method, 0, &Type::Named("SQL".to_string()), query);
                }
                let ret = Type::Result {
                    ok: Box::new(Type::Apply {
                        name: "Query".to_string(),
                        args: vec![(*inner.clone())],
                    }),
                    err: Box::new(Type::List(Box::new(Type::Named("FieldError".to_string())))),
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
        }
        if matches!(method, "zip" | "zip_short" | "zip_pad") {
            if let Some(ret) = self.check_zip_family_method(
                receiver,
                method,
                &recv_ty,
                args,
                span,
                resolved_ret_out,
            ) {
                return Some(ret);
            }
        }

        // D-HOLE1: `.zip` on `T?` pairs two optionals into `(a: T, b: U)?` — present
        // only when both operands are present. `U` is independent of the receiver's
        // `T`, which doesn't fit `Collections::builtin_method_arg_types`'s
        // one-fixed-placeholder-type table (that table's `[T].zip([T])` entry works
        // around the same limitation by forcing the same element type instead);
        // handled directly here, the same shape as `finish_rng_generic`'s hand-rolled
        // generic dispatch below.
        if let Type::Option(a_inner) = &recv_ty {
            if method == "zip" {
                let a_inner = (**a_inner).clone();
                return self.finish_option_zip(a_inner, args, span, resolved_ret_out);
            }
        }
        // Int.abs is the canonical exact-Int source function, not the
        // width-polymorphic math builtin (which can select a Float route).
        if recv_ty == Type::Int && method == "abs" {
            if !args.is_empty() {
                self.diags.push(wrong_core_arity("Int.abs", 0, args.len(), span));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
            let source = jet_foundation::CoreModuleExports::core_source_module("core.math")
                .expect("canonical core.math source module");
            let owner = self.name_ledger.module_by_alias(source.alias)
                .filter(|&index| self.modules.is_some_and(|modules|
                    modules[index].registry.core_source_module
                        .is_some_and(|loaded| loaded.path == source.path)));
            let Some(owner) = owner else {
                self.diags.push(Diagnostic::error(
                    "E0003",
                    "Int.abs source module is not loaded".to_string(),
                    "this receiver operation executes core.math.abs on exact Int values".to_string(),
                    "load the registered Core math source module".to_string(),
                    Some(span),
                ));
                return None;
            };
            let receiver_span = receiver.span();
            let receiver_expr = std::mem::replace(receiver.as_mut(), Expr::Absent(receiver_span));
            let mut source_args = vec![CallArg {
                convention: AccessConvention::Read,
                expr: receiver_expr,
                span: receiver_span,
                flags: CallArgFlags::default(),
                label: None,
                spread: false,
            }];
            let result = self.infer_import_call(
                source.alias, owner, "abs", span, span, type_args,
                &mut source_args, resolved_ret_out,
            );
            **receiver = source_args.remove(0).expr;
            if result.is_some() {
                *recv_type_out = Some("Int".to_string());
            }
            return result;
        }
        // Compiler-private String source methods keep the existing primitive
        // contract and borrow/mutation checks, but mark the call for typed
        // source lowering. The source registry is intentionally consulted only
        // for the canonical primitive, never for user nominal types.
        if matches!(&recv_ty, Type::String)
            && self.modules.map_or(false, |modules| {
                crate::Sema::Registration::core_string_source_method(modules, self.name_ledger, method)
                    .is_some()
            })
        {
            if let Some(ret) =
                Collections::builtin_method_return(&recv_ty, method, args.len(), false)
            {
                *recv_type_out = Some(Syntax::TYPE_STRING.to_string());
                let result =
                    self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                if let Some(ty) = &result {
                    *resolved_ret_out = Some(ty.clone());
                }
                return result;
            }
        }
        if let Some(ret) = Collections::builtin_method_return(&recv_ty, method, args.len(), false)
            .filter(|_| !matches!(&recv_ty, Type::Named(name) | Type::Apply { name, .. }
                if self.source_owns_nominal(name) || self.core_source_declares_method(name, method)
                    || self.resolve_opaque_method_sig(name, method).is_some())) {
            let nominal_recv = match &recv_ty {
                Type::Named(name) => Some(name.as_str()),
                Type::Apply { name, args } if name == "Atomic" && args.len() == 1 => {
                    Some(name.as_str())
                }
                Type::Tagged { marker, inner }
                    if matches!(
                        marker,
                        crate::AST::TagMarker::Internal(crate::AST::InternalTag::CoreCryptoNominal)
                    ) =>
                {
                    match inner.as_ref() {
                        Type::Named(name) => Some(name.as_str()),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(name) = nominal_recv {
                // ProgramInfo / TypeInfo / compiler-API / crypto host methods
                // need the owner on the call so fragment and subset lowering
                // can dispatch them. Collection builtins, Ordering combinators,
                // and BuildContext must stay unset: their lowering key is
                // `recv_type.is_none()`.
                if matches!(
                    name,
                    crate::Syntax::TYPE_PROGRAM_INFO
                        | crate::Syntax::TYPE_TYPE_INFO
                        | "FieldInfo"
                        | "MethodInfo"
                        | "CompilerLexed"
                        | "CompilerSyntaxTree"
                        | "CompilerChecked"
                        | "CompilerSourceMap"
                        | "CompilerPackageError"
                        | "CompilerDependency"
                        | "CompilerPackageTarget"
                        | "CompilerPackageOutput"
                        | "CompilerBuildProfile"
                        | "CompilerManifest"
                        | "CompilerPackage"
                        | "CompilerLockedPackage"
                        | "CompilerLock"
                        | "CompilerKeyValue"
                        | "CompilerProfile"
                        | "CompilerProfileSet"
                        | "WrappedVaultKey"
                ) {
                    *recv_type_out = Some(name.to_string());
                }
            }
            // D-NUMOPS1: hand codegen the receiver's numeric width so it picks the
            // same widening/narrowing form sema just chose for the return type.
            if recv_ty.is_numeric() {
                *recv_type_out = Some(recv_ty.name());
            }
            // D-DECIMAL1 / D-NUMTYPE1: exact numeric handle methods.
            if matches!(
                &recv_ty,
                Type::Named(n)
                    if n == crate::Syntax::TYPE_DECIMAL
                        || n == crate::Syntax::TYPE_FRACTION
            ) {
                *recv_type_out = Some(recv_ty.name());
            }
            // S40: retain the receiver fact so the caller can lower the
            // Range spelling through the existing Slice expression path.
            if matches!(&recv_ty, Type::String) && method == "slice" && args.len() == 1 {
                *recv_type_out = Some(crate::Syntax::TYPE_STRING.to_string());
            }
            let result = self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
            // Lowering consumes the checked result, including ordinary nominal
            // methods; retaining only refinements loses their return contract.
            if let Some(ty) = &result {
                *resolved_ret_out = Some(ty.clone());
            }
            return result;
        }
        if recv_ty.is_numeric() {
            if let Some(target) = Syntax::retired_numeric_conversion_target(method) {
                let source = recv_ty.name();
                let from = Syntax::numeric_conversion_method(&source).unwrap_or("from_value");
                self.diags.push(Diagnostic::error(
                        "E0311",
                        format!("source-owned conversion `.{method}()` is retired"),
                        "explicit conversion belongs to the destination type, so the promised result is visible first".to_string(),
                        format!("write `{target}.{from}(value)`"),
                        Some(span),
                    ));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
        }
        // D-STR-DECLINE1=C (ratified 2026-08-07): `String.to_int`/`.to_float`
        // ship directly as `Int.parse`/`Float.parse` spellings (see
        // crates/jet-codegen/src/Codegen/TIR/lower/builtins.rs's
        // `("to_int", 0) if is_string => TBuiltinOp::ParseInt`) — no
        // "retired" guard here anymore; the builtin-method dispatch below
        // resolves the call.
        if let Type::TraitObject(trait_names) = &recv_ty {
            // D-ANY-JAI1: a multi-trait bound (`...[A, B]`) types its loop element as a
            // multi-name `TraitObject` — check EVERY bound trait for the method, not
            // just the first, so a call inside the body can reach a method on any of
            // them. First match wins (S48 single-trait dispatch is the n=1 case of
            // this loop, unchanged).
            let sig = trait_names.iter().find_map(|tn| {
                self.trait_reg.trait_info(tn)
                    .and_then(|t| t.methods.get(method))
                    .map(|msig| (tn.clone(), msig.clone()))
            });
            if let Some((trait_name, msig)) = sig {
                self.record_open_memory_dispatch(
                    span,
                    "trait-object dispatch has no sealed target set",
                );
                self.record_edge(crate::Sema::effect_key(Some(&trait_name), method), span);
                *recv_type_out = Some(trait_name.clone());
                let source = self.check_trait_method_args(method, &msig, receiver, args, span);
                // The AST fact is the checked carrier for ordinary trait
                // calls. Raw protocol traits retain their ABI declaration so
                // auto-propagation cannot add `Try`.
                let raw_protocol_return = crate::Sema::uses_raw_protocol_return(
                    Some(&trait_name),
                    false,
                    false,
                );
                let (resolved_ret, _carrier) =
                    self.checked_return_types(msig.return_type.clone(), raw_protocol_return);
                *resolved_ret_out = Some(resolved_ret);
                return source;
            }
            // Keep the original single-trait wording byte-for-byte (it's snapshot-
            // pinned product copy, Docs/spec/diagnostics.md) when there's only one
            // bound trait; only the multi-bound case needs the "none of" phrasing.
            let (headline, fix) = match trait_names.as_slice() {
                [only] => (
                    format!("trait `{only}` has no method `{method}`"),
                    format!("add `fn {method}(…)` to `trait {only}`"),
                ),
                many => (
                    format!(
                        "none of {} has a method `{method}`",
                        many.iter()
                            .map(|t| format!("`{t}`"))
                            .collect::<Vec<_>>()
                            .join(" + ")
                    ),
                    format!("add `fn {method}(…)` to one of the bound traits"),
                ),
            };
            self.diags.push(Diagnostic::error(
                "E0102",
                headline,
                "check the method name on this trait value".to_string(),
                fix,
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        // Only the retired raw-string Core helper is refused. User trait and
        // nominal methods named `read_dir` use ordinary checked resolution.
        if method == "read_dir" && recv_ty == Type::String {
            for argument in args.iter_mut() {
                self.infer(&mut argument.expr);
            }
            self.diags.push(Diagnostic::from_row("E0340", &[], Some(span)));
            return None;
        }
        // Operator hooks use operand-pair identity even with explicit dot syntax.
        // Their signatures are not ordinary name-only method candidates.
        let operator_trait = jet_foundation::Traits::operator_hook_trait(method);
        let mut call_access = self.call_access_frame();
        let mut pre_inferred_operator = None;
        let mut operator_signature = None;
        if let Some(trait_name) = operator_trait.filter(|_| args.len() == 1) {
            let (owner_type, _, traits) = self.capability_type_context(&recv_ty);
            if traits.operator_impls.iter().any(|row| row.left == owner_type.name() && row.trait_name == trait_name)
                || self.has_source_operator_left(&recv_ty, trait_name) {
                let rhs_place_type = self.place_expr_type(&args[0].expr).unwrap_or(Type::Bool);
                let rhs = self.with_call_access(&mut call_access, |checker| {
                    checker.check_call_argument_access(&mut args[0], AccessConvention::Read, &rhs_place_type, true);
                    checker.borrow_ctx = true;
                    let inferred = checker.infer(&mut args[0].expr);
                    checker.check_call_argument_captures(&args[0].expr);
                    inferred
                })?;
                let (rhs_type, _, _) = self.capability_type_context(&rhs);
                let (owner_type, registry, traits) = self.capability_type_context(&recv_ty);
                let source_row = self.source_operator_row(&recv_ty, trait_name, &rhs);
                if let Some(row) = source_row.map(|(_, row)| row).or_else(|| traits.operator_impl_for(&owner_type, trait_name, &rhs_type)) {
                    let key = jet_foundation::Traits::operator_method_identity(&row.left, trait_name, method, &row.rhs);
                    let owner = source_row.map(|(owner, _)| owner).unwrap_or_else(|| match &recv_ty {
                        Type::Named(name) | Type::Apply { name, .. } => {
                            let (namespace, leaf) = self.struct_type_name_parts(name);
                            self.struct_owner_module(leaf, namespace).unwrap_or(self.module_idx)
                        }
                        _ => self.module_idx,
                    });
                    let registry = source_row.and_then(|(owner, _)| self.modules.map(|modules| &modules[owner].registry)).unwrap_or(registry);
                    let signature = registry.operator_signatures.get(&key).cloned();
                    if let Some(mut signature) = signature {
                        self.qualify_method_signature(owner, &row.left, &mut signature);
                        *operator_rhs_out = Some(self.qualify_method_type(owner, &row.rhs, &std::collections::HashSet::new()));
                        operator_signature = Some((owner, signature));
                    }
                }
                if operator_signature.is_none() {
                    let diagnostic = self.missing_instance_method_diagnostic(
                        method, &recv_ty, None,
                        Self::missing_receiver_method_why(&recv_ty, method), span,
                    );
                    self.diags.push(diagnostic);
                    return None;
                }
                pre_inferred_operator = Some([Some(rhs)]);
            }
        }
        // Built-in value types have no user `struct`/`enum` registry entry,
        // but their method names still come from the canonical collection
        // table. Keep their ordinary missing-method errors on E0311 so the
        // explanation describes the actual receiver instead of claiming
        // that only nominal values can have methods. `Iter` retains its
        // materializer-specific E0102 recovery below.
        let builtin_methods = Collections::builtin_method_names(&recv_ty);
        if operator_signature.is_none() && !builtin_methods.is_empty()
            && !matches!(&recv_ty, Type::Named(name) | Type::Apply { name, .. }
                if self.source_owns_nominal(name) || self.core_source_declares_method(name, method)
                    || self.resolve_opaque_method_sig(name, method).is_some())
            && !matches!(
                &recv_ty,
                Type::Apply { name, .. }
                    if name == Syntax::TYPE_ITER || name == Syntax::TYPE_VIEW_ITER
            )
        {
            let diagnostic = Self::builtin_method_arity_diagnostic(&recv_ty, method, args.len(), span)
                .unwrap_or_else(|| self.missing_instance_method_diagnostic(
                    method,
                    &recv_ty,
                    None,
                    Self::missing_receiver_method_why(&recv_ty, method),
                    span,
                ));
            self.diags.push(diagnostic);
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        let type_name = match &recv_ty {
            Type::Named(n) | Type::Apply { name: n, .. } => n.clone(),
            Type::Option(inner) => match inner.as_ref() {
                Type::Named(n) | Type::Apply { name: n, .. } => n.clone(),
                _ => {
                    let diagnostic = self.missing_instance_method_diagnostic(
                        method,
                        &recv_ty,
                        None,
                        format!("this value does not expose an instance method named `{method}`"),
                        span,
                    );
                    self.diags.push(diagnostic);
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
            },
            _ if operator_signature.is_some() => recv_ty.name(),
            _ => {
                let diagnostic = self.missing_instance_method_diagnostic(
                    method,
                    &recv_ty,
                    None,
                    format!("this value does not expose an instance method named `{method}`"),
                    span,
                );
                self.diags.push(diagnostic);
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
        };
        let (_, dispatch_type_name) = self.struct_type_name_parts(&type_name);
        let display_type_name = self.display_type_name(&type_name, None);
        let field_name = method;
        let source_identity = if method == "bool" && args.len() == 1 {
            let (namespace, leaf) = self.struct_type_name_parts(&type_name);
            self.struct_owner_module(leaf, namespace)
                .map(|owner| self.canonical_nominal_name(owner, leaf))
        } else {
            None
        };
        let method = jet_foundation::CoreModuleExports::core_source_receiver_method(
            source_identity.as_deref().unwrap_or(&type_name), method, args.len(),
        );
        // D-MEMBER-NAMESPACE1=A: existence, not visibility or call validity,
        // selects the method namespace. Only an absent method permits a
        // callable field; normalize it to the existing function-value route
        // so lowering consumes sema's selection instead of guessing by name.
        let method_signature = operator_signature.or_else(|| self.resolve_method_sig(&type_name, method));
        if method_signature.is_none() {
            let core_field = match &recv_ty {
                Type::Apply { name, args } => core_generic_struct_field(name, field_name, args),
                _ => None,
            };
            let callable_field = core_field.as_ref().is_some_and(|ty| matches!(ty, Type::Fn { .. }))
                || self.struct_fields_for_type_name(&type_name).is_some_and(|fields| {
                    fields.iter().any(|(name, _, ty)| name == field_name && matches!(ty, Type::Fn { .. }))
                });
            if callable_field {
                *recv_type_out = Some(Syntax::INTERNAL_CALL_VALUE.to_string());
                let base = std::mem::replace(receiver, Box::new(Expr::Absent(span)));
                *receiver = Box::new(Expr::Field(base, field_name.to_string(), span));
                let end = args.last().map(|arg| arg.expr.span().end).unwrap_or(span.end);
                return self.infer_call_value(receiver, args, Span::new(span.start, end));
            }
        }
        let Some((owner_mod, mut msig)) = method_signature else {
            let iter_elem = match &recv_ty {
                Type::Apply { name, args }
                    if name == Syntax::TYPE_ITER || name == Syntax::TYPE_VIEW_ITER =>
                {
                    args.first()
                }
                _ => None,
            };
            let iter_positional_pick = iter_elem.is_some() && method == "nth";
            // #1887: `.to_list()` is the truthful fix only when the collected
            // list really carries the method that was asked for — a lazy view
            // has to be materialized before a positional read. A name the
            // list doesn't carry either is an ordinary misspelling, and falls
            // through to the near-miss candidate below.
            let collect_first = iter_elem
                .is_some_and(|elem| {
                    Collections::builtin_method_names(&Type::List(Box::new(elem.clone())))
                        .iter()
                        .any(|known| known == method)
                })
                .then(|| crate::Sema::Diagnostics::one_pass_materializer(&recv_ty))
                .flatten();
            // A cursor method is present in the Core signature table even
            // when its arity-specific return lookup misses. Recover that
            // method here so the fallback reports E0104 rather than suggesting
            // an extension to a protected Core type.
            if let Some(expected_args) = Self::protected_cursor_method_arity(&type_name, method) {
                if args.len() != expected_args {
                    let plural = if expected_args == 1 { "" } else { "s" };
                    let fix = if expected_args == 0 {
                        format!("call `{display_type_name}.{method}()` with no arguments")
                    } else {
                        format!(
                            "call `{display_type_name}.{method}(…)` with exactly {expected_args} argument{plural}"
                        )
                    };
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`{display_type_name}.{method}` expects {expected_args} argument{plural}, got {}",
                            args.len()
                        ),
                        format!(
                            "`{display_type_name}` is a consuming cursor; this operation advances its source"
                        ),
                        fix,
                        Some(span),
                    ));
                }
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
            // #1887: a hand-authored hint for a known receiver/method pair
            // outranks the generic edit-distance near-miss. `len` is one edit
            // away from `get` but semantically unrelated, and offering it
            // would displace the real explanation. The candidate list is the
            // fallback for a receiver nothing specific is known about, and it
            // also owns the suggested source edit: renaming the method is an
            // honest edit only when renaming is the fix being offered.
            let protected_core = (!self.registry.contains(&type_name))
                && (crate::Sema::CheckerCoreLib::core_type_known(&type_name)
                    || Collections::is_reserved_type(&type_name)
                    || crate::Sema::Diagnostics::is_one_pass_source(&recv_ty));
            let generic_bounds = match &recv_ty {
                Type::Named(name) => self
                    .type_param_scope
                    .iter()
                    .find(|param| param.name == *name)
                    .map(|param| param.bounds.clone()),
                _ => None,
            };
            let (why, fix, rename_edit) = if iter_positional_pick {
                (
                    "positional picks on `Iter` use one consuming path".to_string(),
                    "use `.skip(n).first()`; `nth` is not part of the API".to_string(),
                    None,
                )
            } else if let Some(materializer) = collect_first {
                (
                    format!(
                        "`{display_type_name}` is a one-pass source; `{method}` needs a materialized list, so collection consumes the source and allocates the list"
                    ),
                    format!(
                        "materialize it first with `{materializer}`; this consumes the source and allocates a list"
                    ),
                    None,
                )
            } else {
                let suggestion = self.method_suggestion(method, &recv_ty, Some(&type_name));
                if protected_core {
                    let fix = suggestion.as_ref().map_or_else(
                        || format!("use a documented method on `{display_type_name}`"),
                        |suggestion| suggestion.fix.clone(),
                    );
                    (
                        format!(
                            "`{display_type_name}` is a protected Core type with a closed method surface"
                        ),
                        fix,
                        suggestion.map(|suggestion| suggestion.name),
                    )
                } else if let Some(bounds) = generic_bounds {
                    let bound_names = bounds
                        .iter()
                        .map(|bound| format!("`{bound}`"))
                        .collect::<Vec<_>>()
                        .join(" + ");
                    let why = if bounds.is_empty() {
                        format!(
                            "type parameter `{type_name}` has no bound that provides `{method}`"
                        )
                    } else {
                        format!(
                            "none of type parameter `{type_name}`'s bounds ({bound_names}) provides `{method}`"
                        )
                    };
                    let fix = if bounds.is_empty() {
                        format!(
                            "add a trait bound that defines `{method}`, or use a method available on `{type_name}`"
                        )
                    } else {
                        format!(
                            "add a bound that defines `{method}`, or use a method from {bound_names}"
                        )
                    };
                    (why, fix, None)
                } else {
                    let fix = suggestion.as_ref().map_or_else(
                        || {
                            format!(
                                "define it inside `struct {display_type_name}` or `impl {display_type_name}`"
                            )
                        },
                        |suggestion| suggestion.fix.clone(),
                    );
                    (
                        "check the method name on this type".to_string(),
                        fix,
                        suggestion.map(|suggestion| suggestion.name),
                    )
                }
            };
            let mut diagnostic = Diagnostic::error(
                "E0102",
                format!("`{}` has no method `{}`", display_type_name, method),
                why,
                fix,
                Some(span),
            );
            if let Some(candidate) = rename_edit {
                diagnostic = diagnostic.with_edit(TextEdit {
                    span,
                    new_text: candidate,
                });
            }
            self.diags.push(diagnostic);
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        };
        let (_, method_type_name) = self.struct_type_name_parts(&type_name);
        let method_name = format!("{method_type_name}.{method}");
        let display_type_name = self.display_type_name(&type_name, Some(owner_mod));
        if owner_mod != self.module_idx
            && !self
                .name_ledger
                .visible(self.module_idx, owner_mod, &method_name)
        {
            self.diags
                .push(crate::Sema::Diagnostics::private_item(method, span));
        } else if owner_mod != self.module_idx
            && Syntax::classify_identifier(method) == Syntax::IdentifierClass::SoftPublic
        {
            self.diags
                .push(crate::Sema::Diagnostics::soft_public_use(method, span));
        }
        if let Some(dep) = msig.deprecation.as_ref() {
            self.check_deprecation(&method_name, dep, span);
        }
        let applied_args = match &recv_ty {
            Type::Apply { args, .. } => Some(args.as_slice()),
            Type::Option(inner) => match inner.as_ref() {
                Type::Apply { args, .. } => Some(args.as_slice()),
                _ => None,
            },
            _ => None,
        };
        if let Some(args) = applied_args {
            self.instantiate_method_sig(owner_mod, &type_name, &mut msig, args);
        }
        let raw_protocol_return = self.raw_protocol_return_for_method(&type_name, method);
        // D-APILABEL1=A: bind before inference — see `bind_method_args`.
        let method_diagnostics = self.diags.len();
        if !self.bind_method_args(method, &msig, args, span) {
            let (resolved_ret, return_type) =
                self.checked_return_types(msig.return_type.clone(), raw_protocol_return);
            let ret = Some(return_type);
            *resolved_ret_out = Some(resolved_ret);
            return ret;
        }
        self.normalize_method_variadic_call(method, &msig, args, span);
        let pre_inferred_method = self.instantiate_method_type_args(
            dispatch_type_name,
            method,
            &mut msig,
            type_args,
            args,
            span,
            &mut call_access,
        );
        self.record_method_reference(&type_name, method, span);
        self.record_edge(
            crate::Sema::effect_key(Some(dispatch_type_name), method),
            span,
        );
        if msig.is_static {
            self.diags.push(Diagnostic::error(
                "E0311",
                format!("`{}` is a static method on `{}`", method, display_type_name),
                "static methods belong to the type name, not a value".to_string(),
                format!("write `{}.{method}(...)` instead", display_type_name),
                Some(span),
            ));
        }
        *recv_type_out = Some(dispatch_type_name.to_string());
        // `mut self` methods change the receiver: it must be changeable,
        // free of an active `for` borrow, and not aliased by an argument.
        if msig.self_conv == Some(AccessConvention::Write) {
            self.check_mutating_method_receiver(receiver, method, span);
        }
        if msig.self_conv == Some(AccessConvention::Move) {
            if let Expr::Ident(n, nspan) = &**receiver {
                // A borrowed parameter can't be consumed (the generated Rust
                // would move out of a `&T`/`&mut T`).
                if let Some(info) = self.lookup(n) {
                    if !type_is_copy(&info.ty)
                        && matches!(
                            info.param_conv,
                            Some(AccessConvention::Read) | Some(AccessConvention::Write)
                        )
                    {
                        let source_ty = info.ty.clone();
                        let diagnostic = Diagnostic::error(
                            "E0120",
                            format!(
                                "`{}` was not moved here, so `.{}()` cannot take it with the move marker `^`",
                                n, method
                            ),
                            "this function has read access only and does not own the value; the move marker `^` is required".to_string(),
                            format!(
                                "call it on a copy: `{}.{}(...)` — or take ownership with the move marker `^`: `{}: {}{}`",
                                Syntax::copy_call(n),
                                method,
                                n,
                                Syntax::SIGIL_MOVE,
                                source_ty.name()
                            ),
                            Some(*nspan),
                        );
                        self.diags.push(self.with_ownership_copy_edit(
                            diagnostic,
                            *nspan,
                            Some(&source_ty),
                        ));
                    }
                }
                self.mark_moved_by(n.clone(), *nspan, format!("{dispatch_type_name}.{method}"));
            } else if let Some(place) = self.place_from_expr(receiver).filter(|place| {
                !place.projections.is_empty()
                    && place
                        .projections
                        .iter()
                        .all(|projection| matches!(projection, crate::Sema::ViewProjection::Field(_)))
            }) {
                // A taking call on a field moves that field out of its owner
                // (a partial move); siblings stay usable and a later read of
                // the field or its owner is E0121.
                if !self
                    .place_expr_type(receiver)
                    .is_some_and(|ty| type_is_copy(&ty))
                {
                    self.mark_moved_place_by(
                        place,
                        crate::Sema::ReceiverMarks::place_span(receiver),
                        format!("{dispatch_type_name}.{method}"),
                    );
                }
            }
        }
        self.check_method_args(
            dispatch_type_name,
            method,
            &msig,
            Some(receiver),
            args,
            span,
            pre_inferred_operator.as_ref().map(|types| types.as_slice())
                .or(pre_inferred_method.as_deref()),
            Some(call_access),
        )?;
        if !self.diags[method_diagnostics..]
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::Diagnostics::Severity::Error)
        {
            *checked_callee_out =
                self.checked_method_callee(owner_mod, method_type_name, &method_name);
        }
        let (resolved_ret, return_type) =
            self.checked_return_types(msig.return_type.clone(), raw_protocol_return);
        let ret = Some(return_type);
        *resolved_ret_out = Some(resolved_ret);
        ret
    }
}

fn checker_hardware_path(
    expr: &Expr,
    core_imports: &HashMap<String, String>,
) -> Option<Vec<String>> {
    fn append(expr: &Expr, path: &mut Vec<String>) -> bool {
        match expr {
            Expr::Ident(name, _) => {
                path.push(name.clone());
                true
            }
            Expr::Field(base, member, _) => {
                if !append(base, path) {
                    return false;
                }
                path.push(member.clone());
                true
            }
            Expr::Paren(inner, _) => append(inner, path),
            _ => false,
        }
    }
    let mut path = Vec::new();
    append(expr, &mut path).then_some(path).filter(|path| {
        path.first()
            .and_then(|alias| core_imports.get(alias))
            .is_some_and(|module| module.starts_with("board."))
    })
}
