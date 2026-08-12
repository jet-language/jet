use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::Sema::Diagnostics::soft_public_use;
use crate::AST::{
    AccessConvention, BinOp, Call, EnumLitArg, Expr, Pattern, StrMatchPart, StructPatField, Type,
    VariantPayload,
};
use std::collections::{HashMap, HashSet};

/// D-BINPAT1: the unsigned integer type a fixed-width bit hole binds — the
/// smallest standard width (`U8`/`U16`/`U32`/`U64`) that holds `width` bits.
pub(crate) fn bin_bits_type(width: u8) -> Type {
    let bits = if width <= 8 {
        8
    } else if width <= 16 {
        16
    } else if width <= 32 {
        32
    } else {
        64
    };
    Type::IntN { signed: false, bits }
}

impl<'a> Checker<'a> {
    fn qualify_method_type(
        &self,
        owner_mod: usize,
        ty: &Type,
        generic_names: &HashSet<String>,
    ) -> Type {
        let qualify_name = |name: &str| {
            if generic_names.contains(name) || name.contains("::") {
                return name.to_string();
            }
            if owner_mod != self.module_idx
                && self
                    .modules
                    .is_some_and(|modules| modules[owner_mod].registry.contains(name))
            {
                return self.canonical_nominal_name(owner_mod, name);
            }
            match self.resolve_type(Type::Named(name.to_string())) {
                Type::Named(resolved) => resolved,
                _ => name.to_string(),
            }
        };
        match ty {
            Type::Named(name) => Type::Named(qualify_name(name)),
            Type::Apply { name, args } => Type::Apply {
                name: qualify_name(name),
                args: args
                    .iter()
                    .map(|arg| self.qualify_method_type(owner_mod, arg, generic_names))
                    .collect(),
            },
            Type::List(inner) => Type::List(Box::new(self.qualify_method_type(
                owner_mod,
                inner,
                generic_names,
            ))),
            Type::Shared(inner) => Type::Shared(Box::new(self.qualify_method_type(
                owner_mod,
                inner,
                generic_names,
            ))),
            Type::Option(inner) => Type::Option(Box::new(self.qualify_method_type(
                owner_mod,
                inner,
                generic_names,
            ))),
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(self.qualify_method_type(owner_mod, ok, generic_names)),
                err: Box::new(self.qualify_method_type(owner_mod, err, generic_names)),
            },
            Type::Map {
                key,
                key_span,
                value,
            } => Type::Map {
                key: Box::new(self.qualify_method_type(owner_mod, key, generic_names)),
                key_span: *key_span,
                value: Box::new(self.qualify_method_type(owner_mod, value, generic_names)),
            },
            Type::Fn {
                params,
                ret,
                effect_bound,
                param_contract,
                call_metadata,
                return_view_provenance,
            } => Type::Fn {
                params: params
                    .iter()
                    .map(|param| self.qualify_method_type(owner_mod, param, generic_names))
                    .collect(),
                ret: ret.as_ref().map(|ret| {
                    Box::new(self.qualify_method_type(owner_mod, ret, generic_names))
                }),
                effect_bound: effect_bound.clone(),
                param_contract: param_contract.clone(),
                call_metadata: call_metadata.clone(),
                return_view_provenance: return_view_provenance.clone(),
            },
            Type::Tuple(fields) => Type::Tuple(
                fields
                    .iter()
                    .map(|(name, field)| {
                        (
                            name.clone(),
                            Box::new(self.qualify_method_type(owner_mod, field, generic_names)),
                        )
                    })
                    .collect(),
            ),
            Type::FixedList {
                elem,
                len,
                len_symbol,
            } => Type::FixedList {
                elem: Box::new(self.qualify_method_type(owner_mod, elem, generic_names)),
                len: *len,
                len_symbol: len_symbol.clone(),
            },
            Type::Tagged { marker, inner } => Type::Tagged {
                marker: marker.clone(),
                inner: Box::new(self.qualify_method_type(owner_mod, inner, generic_names)),
            },
            Type::Union(members) => crate::AST::canonicalize_union(
                members
                    .iter()
                    .map(|member| self.qualify_method_type(owner_mod, member, generic_names))
                    .collect(),
            ),
            Type::Quantity { base, dimension } => Type::Quantity {
                base: Box::new(self.qualify_method_type(owner_mod, base, generic_names)),
                dimension: dimension.clone(),
            },
            Type::TraitObject(bounds) => Type::TraitObject(bounds.clone()),
            other => other.clone(),
        }
    }

    fn qualify_method_signature(
        &self,
        owner_mod: usize,
        owner_name: &str,
        sig: &mut MethodSig,
    ) {
        if owner_mod == self.module_idx {
            return;
        }
        let mut generic_names: HashSet<String> = sig
            .type_params
            .iter()
            .map(|param| param.name.clone())
            .collect();
        if let Some(module) = self.modules.and_then(|modules| modules.get(owner_mod)) {
            if let Some(params) = module
                .trait_reg
                .struct_params
                .get(owner_name)
                .or_else(|| module.trait_reg.enum_params.get(owner_name))
            {
                generic_names.extend(params.iter().map(|param| param.name.clone()));
            }
        }
        for (_, ty) in &mut sig.params {
            *ty = self.qualify_method_type(owner_mod, ty, &generic_names);
        }
        if let Some(ret) = &mut sig.return_type {
            *ret = self.qualify_method_type(owner_mod, ret, &generic_names);
        }
    }

    pub(crate) fn resolve_method_sig(
        &self,
        type_name: &str,
        method: &str,
    ) -> Option<(usize, MethodSig)> {
        let (import_ns, leaf) = Self::split_type_name(type_name);
        let owner = self.struct_owner_module(leaf, import_ns)?;
        let mut sig = if owner == self.module_idx {
            self.registry.method(leaf, method).cloned()?
        } else {
            self.modules
                .and_then(|modules| modules.get(owner))
                .and_then(|module| module.registry.method(leaf, method).cloned())?
        };
        self.qualify_method_signature(owner, leaf, &mut sig);
        Some((owner, sig))
    }

    pub(crate) fn instantiate_method_sig(
        &self,
        owner_mod: usize,
        type_name: &str,
        sig: &mut MethodSig,
        args: &[Type],
    ) {
        let (_, leaf) = Self::split_type_name(type_name);
        let declared = if owner_mod == self.module_idx {
            self.trait_reg
                .struct_params
                .get(leaf)
                .or_else(|| self.trait_reg.enum_params.get(leaf))
        } else {
            self.modules.and_then(|modules| {
                modules.get(owner_mod).and_then(|module| {
                    module
                        .trait_reg
                        .struct_params
                        .get(leaf)
                        .or_else(|| module.trait_reg.enum_params.get(leaf))
                })
            })
        };
        let Some(params) = declared else {
            return;
        };
        Self::instantiate_method_sig_from(sig, params, args);
    }

    fn instantiate_method_sig_from(
        sig: &mut MethodSig,
        params: &[crate::AST::TypeParam],
        args: &[Type],
    ) {
        let subst: HashMap<String, Type> = params
            .iter()
            .zip(args)
            .map(|(param, arg)| (param.name.clone(), arg.clone()))
            .collect();
        for (_, ty) in &mut sig.params {
            *ty = crate::Generics::substitute_type(ty, &subst);
        }
        if let Some(ret) = &mut sig.return_type {
            *ret = crate::Generics::substitute_type(ret, &subst);
        }
    }

    pub(crate) fn reject_borrowed_param_subplace(
        &mut self,
        expr: &Expr,
        ty: Option<&Type>,
        destination: &str,
    ) -> bool {
        if ty.is_none_or(type_is_copy) || !matches!(expr, Expr::Field(..) | Expr::Index { .. }) {
            return false;
        }
        let Some(root) = crate::Sema::Diagnostics::expr_root_ident(expr) else {
            return false;
        };
        let borrowed = self.lookup(root).is_some_and(|info| {
            matches!(
                info.param_conv,
                Some(AccessConvention::Read) | Some(AccessConvention::Write)
            )
        });
        if !borrowed {
            return false;
        }
        self.diags.push(Diagnostic::error(
            "E0120",
            format!("`{root}` was not moved here, so this part cannot {destination}"),
            "the function can access this parameter, but it does not own any part of it"
                .to_string(),
            format!(
                "copy the selected value explicitly by prefixing it with `{}`",
                Syntax::SIGIL_COPY
            ),
            Some(expr.span()),
        ));
        true
    }

    pub(crate) fn check_static_method(
        &mut self,
        type_name: &str,
        method: &str,
        span: Span,
        owner_type_args: &mut Vec<Type>,
        method_type_args: &[Type],
        args: &mut Vec<crate::AST::CallArg>,
    ) -> Option<Type> {
        if matches!(type_name, "Arena" | "Bump") && method == "new" {
            let bound = args.first().and_then(|arg| match &arg.expr {
                Expr::Int(value, _, _, _) if *value >= 0 => Some(*value as u64),
                _ => None,
            });
            self.record_memory_event(crate::Sema::MemoryEvent::new(
                crate::Sema::MemoryEventKind::ArenaBytes(bound),
                span,
                format!("`{type_name}.new` reserves arena storage"),
            ));
            self.record_memory_event(crate::Sema::MemoryEvent::new(
                crate::Sema::MemoryEventKind::Allocation,
                span,
                format!("`{type_name}.new` allocates arena backing storage"),
            ));
        }
        if type_name == "EncodingLimits" && method == "safe" {
            if !args.is_empty() {
                self.diags.push(Diagnostic::error("E0101", format!("`EncodingLimits.safe` takes 0 arguments, got {}", args.len()), "the safe encoding limits are fixed defaults".to_string(), "remove the arguments".to_string(), Some(span)));
                for arg in args { self.infer(&mut arg.expr); }
            }
            return Some(Type::Named("EncodingLimits".to_string()));
        }
        if type_name == "CBOROptions" && method == "safe" {
            if !args.is_empty() {
                self.diags.push(Diagnostic::error("E0101", format!("`CBOROptions.safe` takes 0 arguments, got {}", args.len()), "safe CBOR limits and interoperability defaults are fixed".to_string(), "remove the arguments".to_string(), Some(span)));
            }
            return Some(Type::Named("CBOROptions".to_string()));
        }
        if matches!(type_name, "XMLLimits" | "XMLParseOptions" | "XMLRenderOptions") && method == "safe" {
            if !args.is_empty() {
                self.diags.push(Diagnostic::error("E0101", format!("`{type_name}.safe` takes 0 arguments, got {}", args.len()), "safe XML limits and entity defaults are fixed".to_string(), "remove the arguments".to_string(), Some(span)));
            }
            return Some(Type::Named(type_name.to_string()));
        }
        if type_name == "Limits" && method == "safe" {
            if !args.is_empty() {
                self.diags.push(Diagnostic::error("E0101", format!("`Limits.safe` takes 0 arguments, got {}", args.len()), "safe SMTP limits are fixed defaults".to_string(), "remove the arguments".to_string(), Some(span)));
                for arg in args { self.infer(&mut arg.expr); }
            }
            return Some(Type::Named("Limits".to_string()));
        }
        if type_name == "DataLimits" && method == "safe" {
            if !args.is_empty() {
                self.diags.push(Diagnostic::error(
                    "E0101",
                    format!("`DataLimits.safe` takes 0 arguments, got {}", args.len()),
                    "safe typed-data limits are fixed defaults".to_string(),
                    "remove the arguments".to_string(),
                    Some(span),
                ));
                for arg in args {
                    self.infer(&mut arg.expr);
                }
            }
            return Some(Type::Named("DataLimits".to_string()));
        }
        let Some((owner_mod, mut msig)) = self.resolve_method_sig(type_name, method) else {
            let builtin = crate::Sema::Diagnostics::builtin_type_from_ident(type_name).is_some();
            self.diags.push(Diagnostic::error(
                "E0102",
                if builtin {
                    format!("`{type_name}` has no static method `{method}`")
                } else {
                    format!("`{type_name}` has no method `{method}`")
                },
                if builtin {
                    "built-in types expose only their documented static methods".to_string()
                } else {
                    "check the method name on this type".to_string()
                },
                if builtin {
                    format!(
                        "call a documented static method on `{type_name}`, or call `.{method}()` on a value that provides it"
                    )
                } else {
                    format!("define it inside `struct {type_name}` or `impl {type_name}`")
                },
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        };
        let (_, method_type_name) = self.struct_type_name_parts(type_name);
        let method_name = format!("{method_type_name}.{method}");
        if owner_mod != self.module_idx
            && !self
                .name_ledger
                .visible(self.module_idx, owner_mod, &method_name)
        {
            self.diags.push(private_item(method, span));
        } else if owner_mod != self.module_idx
            && Syntax::classify_identifier(method) == Syntax::IdentifierClass::SoftPublic
        {
            self.diags.push(soft_public_use(method, span));
        }
        let (_, dispatch_type_name) = Self::split_type_name(type_name);
        let declared = if owner_mod == self.module_idx {
            self.trait_reg
                .struct_params
                .get(dispatch_type_name)
                .or_else(|| self.trait_reg.enum_params.get(dispatch_type_name))
                .cloned()
        } else {
            self.modules.and_then(|modules| {
                modules.get(owner_mod).and_then(|module| {
                    module
                        .trait_reg
                        .struct_params
                        .get(dispatch_type_name)
                        .or_else(|| module.trait_reg.enum_params.get(dispatch_type_name))
                        .cloned()
                })
            })
        }
        .unwrap_or_default();
        let mut call_access = self.call_access_frame();
        let mut pre_inferred = None;
        // Bind before receiver-type inference too: generic constructors use
        // their arguments to infer the owner type, so labels must already be
        // in declaration order when that inference walks them.
        if !self.bind_method_args(method, &msig, args, span) {
            return msig.return_type.clone();
        }
        self.normalize_method_variadic_call(method, &msig, args, span);
        if method == "new" && owner_type_args.is_empty() {
            if !declared.is_empty() {
                let expected = self.expected_type.clone();
                let mut inference_args = Vec::new();
                let mut inferred = self.trait_reg.infer_subst_without_bounds(
                    &msig.params,
                    msig.return_type.as_ref(),
                    &[],
                    &declared,
                    expected.as_ref(),
                );
                if inferred.is_err() {
                    let saved_expected = self.expected_type.take();
                    let actual = args
                        .iter_mut()
                        .enumerate()
                        .map(|(index, arg)| {
                            self.with_call_access(&mut call_access, |checker| {
                                if let Some((param_conv, param_ty)) = msig.params.get(index) {
                                    checker.check_call_argument_access(
                                        arg,
                                        *param_conv,
                                        param_ty,
                                        true,
                                    );
                                }
                                let inferred = checker.infer(&mut arg.expr);
                                checker.check_call_argument_captures(&arg.expr);
                                inferred
                            })
                        })
                        .collect::<Vec<_>>();
                    self.expected_type = saved_expected;
                    if actual.iter().all(Option::is_some) {
                        let actual_types = actual.iter().flatten().cloned().collect::<Vec<_>>();
                        inference_args = actual_types.clone();
                        inferred = self.trait_reg.infer_subst_without_bounds(
                            &msig.params,
                            msig.return_type.as_ref(),
                            &actual_types,
                            &declared,
                            expected.as_ref(),
                        );
                        pre_inferred = Some(actual);
                    } else {
                        return None;
                    }
                }
                let subst = match inferred {
                    Ok(subst) => subst,
                    Err(_) => {
                        let mut unbounded = declared.clone();
                        for param in &mut unbounded {
                            param.bounds.clear();
                        }
                        if let Ok(subst) = self.trait_reg.infer_subst_without_bounds(
                            &msig.params,
                            msig.return_type.as_ref(),
                            &inference_args,
                            &unbounded,
                            expected.as_ref(),
                        ) {
                            for param in &declared {
                                let Some(concrete) = subst.get(&param.name) else {
                                    continue;
                                };
                                if let Some(bound) = param
                                    .bounds
                                    .iter()
                                    .find(|bound| {
                                        !self.type_satisfies_bound(concrete, bound)
                                    })
                                {
                                    let concrete_name = concrete.name();
                                    self.diags.push(crate::Generics::e0905(
                                        &concrete_name,
                                        bound,
                                        span,
                                        false,
                                    ));
                                    return None;
                                }
                            }
                        }
                        let params = declared
                            .iter()
                            .map(|param| param.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        self.diags.push(Diagnostic::error(
                            "E0904",
                            format!("`{type_name}.new` needs its generic receiver type here"),
                            "constructor inputs and the surrounding expected type must determine one concrete receiver type".to_string(),
                            format!("write the full receiver, such as `{type_name}<{params}>.new(...)`"),
                            Some(span),
                        ));
                        return None;
                    }
                };
                owner_type_args.extend(
                    declared
                        .iter()
                        .filter_map(|param| subst.get(&param.name).cloned()),
                );
            }
        }
        Self::instantiate_method_sig_from(&mut msig, &declared, owner_type_args);
        let pre_inferred_method = self.instantiate_method_type_args(
            dispatch_type_name,
            method,
            &mut msig,
            method_type_args,
            args,
            span,
            &mut call_access,
        );
        self.record_method_reference(type_name, method, span);
        self.record_edge(super::effect_key(Some(dispatch_type_name), method), span);
        if !msig.is_static {
            self.diags.push(Diagnostic::error(
                "E0311",
                format!("`{}` is an instance method on `{}`", method, type_name),
                "instance methods need a value before the dot".to_string(),
                format!("call it on a `{type_name}` value: `x.{method}(...)`"),
                Some(span),
            ));
        }
        self.check_method_args(
            dispatch_type_name,
            method,
            &msig,
            None,
            args,
            span,
            pre_inferred_method
                .as_deref()
                .or(pre_inferred.as_deref()),
            Some(call_access),
        )
    }

    /// D-GENERIC-CALL1=A: resolve method-owned call arguments after the receiver
    /// type has been instantiated. Receiver arguments and method arguments are
    /// deliberately separate so `Box<Int>.make<String>(...)` has one clear
    /// substitution for each binder.
    pub(crate) fn instantiate_method_type_args(
        &mut self,
        type_name: &str,
        method: &str,
        sig: &mut MethodSig,
        type_args: &[Type],
        args: &mut [crate::AST::CallArg],
        span: Span,
        call_access: &mut CallAccessFrame,
    ) -> Option<Vec<Option<Type>>> {
        if sig.type_params.is_empty() {
            if !type_args.is_empty() {
                self.diags.push(Diagnostic::error(
                    "E0119",
                    format!("{type_name}.{method} is not generic"),
                    "only methods declared with type parameters accept call-site type arguments"
                        .to_string(),
                    format!("call {type_name}.{method}(...) without type arguments"),
                    Some(span),
                ));
            }
            return None;
        }

        if !type_args.is_empty() {
            if type_args.len() != sig.type_params.len() {
                self.diags.push(Diagnostic::error(
                    "E0119",
                    format!(
                        "{type_name}.{method} expects {} type argument{}, got {}",
                        sig.type_params.len(),
                        if sig.type_params.len() == 1 { "" } else { "s" },
                        type_args.len()
                    ),
                    "a generic method call must provide one type for every declared type parameter"
                        .to_string(),
                    format!("write {type_name}.{method}<…>(...) with the declared types"),
                    Some(span),
                ));
                return None;
            }
            let mut subst = HashMap::new();
            for (param, actual) in sig.type_params.iter().zip(type_args) {
                let actual = self.resolve_type(actual.clone());
                self.check_declared_type(&actual, span);
                for bound in &param.bounds {
                    if !self.type_satisfies_bound(&actual, bound) {
                        self.diags.push(crate::Generics::e0905(
                            &actual.name(),
                            bound,
                            span,
                            false,
                        ));
                    }
                }
                subst.insert(param.name.clone(), actual);
            }
            for (_, ty) in &mut sig.params {
                *ty = crate::Generics::substitute_type(ty, &subst);
            }
            if let Some(ret) = &mut sig.return_type {
                *ret = crate::Generics::substitute_type(ret, &subst);
            }
            return None;
        }

        let params: Vec<(AccessConvention, Type)> = sig
            .params
            .iter()
            .enumerate()
            .filter(|(index, _)| !(sig.self_conv.is_some() && *index == 0))
            .map(|(_, param)| param.clone())
            .collect();
        let mut pre_inferred = Vec::with_capacity(args.len());
        for (index, arg) in args.iter_mut().enumerate() {
            pre_inferred.push(self.with_call_access(call_access, |checker| {
                if let Some((param_conv, param_ty)) = params.get(index) {
                    checker.check_call_argument_access(arg, *param_conv, param_ty, true);
                }
                let saved_expected = checker.expected_type.clone();
                if let Some((_, param_ty)) = params.get(index) {
                    checker.expected_type = Some(param_ty.clone());
                }
                let inferred = checker.infer(&mut arg.expr);
                checker.expected_type = saved_expected;
                checker.check_call_argument_captures(&arg.expr);
                inferred
            }));
        }
        let arg_types: Vec<Type> = pre_inferred.iter().filter_map(Clone::clone).collect();
        if arg_types.len() != args.len() {
            return Some(pre_inferred);
        }
        let subst = match self.trait_reg.infer_subst_without_bounds(
            &params,
            sig.return_type.as_ref(),
            &arg_types,
            &sig.type_params,
            self.expected_type.as_ref(),
        ) {
                    Ok(subst) => {
                        for param in &sig.type_params {
                            let Some(concrete) = subst.get(&param.name) else {
                                continue;
                            };
                            if let Some(bound) = param
                                .bounds
                                .iter()
                                .find(|bound| !self.type_satisfies_bound(concrete, bound))
                            {
                                let concrete_name = concrete.name();
                                self.diags.push(crate::Generics::e0905(
                                    &concrete_name,
                                    bound,
                                    span,
                                    false,
                                ));
                                return None;
                            }
                        }
                        subst
                    }
            Err(param) => {
                self.diags.push(crate::Generics::e0904(span, &param));
                return Some(pre_inferred);
            }
        };
        for param in &sig.type_params {
            if let Some(actual) = subst.get(&param.name) {
                if let Some(bound) = param
                    .bounds
                    .iter()
                    .find(|bound| !self.type_satisfies_bound(actual, bound))
                {
                    self.diags.push(crate::Generics::e0905(
                        &actual.name(),
                        bound,
                        span,
                        false,
                    ));
                }
            }
        }
        for (_, ty) in &mut sig.params {
            *ty = crate::Generics::substitute_type(ty, &subst);
        }
        if let Some(ret) = &mut sig.return_type {
            *ret = crate::Generics::substitute_type(ret, &subst);
        }
        let _ = (type_name, method);
        Some(pre_inferred)
    }

    /// D-APILABEL1=A: methods bind through the same one binder as free calls —
    /// labels bind by name, zones decide whether a label is forbidden or
    /// required, and skipped defaults are filled here.
    ///
    /// This runs **before** type-argument inference, not inside
    /// `check_method_args`. Inference walks the argument list positionally and
    /// hands back a `pre_inferred` vector indexed by position, so binding
    /// afterwards would leave that vector describing the source order while
    /// every later read assumed declaration order — which rejected correct
    /// generic calls such as `s.put(value: 3, key: "k")`.
    ///
    /// Returns false when nothing could be bound and a diagnostic was reported.
    pub(crate) fn bind_method_args(
        &mut self,
        method: &str,
        sig: &MethodSig,
        args: &mut Vec<crate::AST::CallArg>,
        span: Span,
    ) -> bool {
        // `params` is self-first when there is a receiver; `param_info` already
        // excludes it, so the conventions line up after the skip.
        let self_offset = usize::from(sig.self_conv.is_some());
        let params: Vec<crate::Sema::CallBinder::BindParam<'_>> = (0..sig.param_info.len())
            .map(|index| crate::Sema::CallBinder::BindParam {
                label: sig
                    .param_call
                    .get(index)
                    .map(|(label, _)| label.as_str())
                    .unwrap_or(sig.param_info[index].0.as_str()),
                name: sig.param_info[index].0.as_str(),
                zone: sig
                    .param_call
                    .get(index)
                    .map(|(_, zone)| *zone)
                    .unwrap_or(crate::AST::ParamZone::Either),
                default: sig.defaults.get(index).and_then(|d| d.as_ref()),
                convention: sig
                    .params
                    .get(index + self_offset)
                    .map(|(convention, _)| *convention)
                    .unwrap_or(crate::AST::AccessConvention::Read),
                ty: sig.params.get(index + self_offset).map(|(_, ty)| ty),
                variadic: sig.param_variadic.get(index).copied().unwrap_or(false),
                core_default: None,
            })
            .collect();
        let bound =
            crate::Sema::CallBinder::bind_call_args(method, &params, args, span, &mut self.diags);
        self.register_binder_refs(args);
        if bound.is_none() {
            // Nothing bound, so every later check would be about slots that do
            // not exist. Report each argument's own problems only.
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return false;
        }
        true
    }

    /// Normalize a method's homogeneous rest tail before generic inference.
    /// Method calls share the free-call binder, but their method signature is
    /// stored without a `FuncSig`; this adapter gives the one variadic
    /// normalizer the same declaration-order list slot.
    pub(crate) fn normalize_method_variadic_call(
        &mut self,
        method: &str,
        sig: &MethodSig,
        args: &mut Vec<crate::AST::CallArg>,
        span: Span,
    ) {
        if !sig.param_variadic.last().copied().unwrap_or(false) {
            return;
        }
        let self_offset = usize::from(sig.self_conv.is_some());
        let mut packed = crate::AST::Call {
            name: method.to_string(),
            name_span: span,
            type_args: Vec::new(),
            args: std::mem::take(args),
            resolved_ret: None,
            range_checked: false,
            widen_approx: false,
        };
        let fake_sig = crate::AST::FuncSig {
            params: sig.params[self_offset..].to_vec(),
            root_param: false,
            return_type: sig.return_type.clone(),
            return_view_provenance: crate::AST::ViewProvenanceCell::new(),
            is_extern: false,
            is_unsafe: false,
            is_pure: false,
            is_foreign_thread_safe: false,
            is_sanitizer: false,
            is_must_use: sig.must_use,
            is_c_abi: false,
            c_abi_name: None,
            foreign_effect_root: None,
            param_info: sig.param_info.clone(),
            param_call: sig.param_call.clone(),
            defaults: sig.defaults.clone(),
            param_variadic: sig.param_variadic.clone(),
            variadic_bounds: None,
            param_view_from_names: Vec::new(),
        };
        self.normalize_variadic_call(&mut packed, &fake_sig);
        *args = packed.args;
    }

    pub(crate) fn check_method_args(
        &mut self,
        type_name: &str,
        method: &str,
        sig: &MethodSig,
        receiver: Option<&Expr>,
        args: &mut Vec<crate::AST::CallArg>,
        span: Span,
        pre_inferred: Option<&[Option<Type>]>,
        call_access: Option<CallAccessFrame>,
    ) -> Option<Type> {
        let _ = (type_name, method, span);
        let expected_args = if sig.self_conv.is_some() {
            sig.params.len().saturating_sub(1)
        } else {
            sig.params.len()
        };

        let mut call_access = call_access.unwrap_or_else(|| self.call_access_frame());
        if let (Some(receiver), Some(convention)) = (receiver, sig.self_conv) {
            self.with_call_access(&mut call_access, |checker| {
                if convention == AccessConvention::Write {
                    checker.record_call_receiver_reservation(receiver, span);
                } else {
                    checker.record_call_receiver_access(receiver, convention, span);
                }
            });
        }

        if args.len() != expected_args {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`{}` expects {} argument{}, got {}",
                    method,
                    expected_args,
                    if expected_args == 1 { "" } else { "s" },
                    args.len()
                ),
                if sig.self_conv.is_some() {
                    "every argument must match a parameter (not counting `self`)".to_string()
                } else {
                    "every argument must match a parameter".to_string()
                },
                format!("check the definition of `{method}` on `{type_name}`"),
                Some(span),
            ));
        }
        let mut arg_idx = 0;
        for (i, (param_conv, param_ty)) in sig.params.iter().enumerate() {
            if i == 0 && sig.self_conv.is_some() {
                continue;
            }
            if let Some(arg) = args.get_mut(arg_idx) {
                if matches!(param_conv, AccessConvention::Read) && !param_ty.is_scalar() {
                    self.borrow_ctx = true;
                }
                let pre_inferred_ty = pre_inferred
                    .and_then(|types| types.get(arg_idx))
                    .cloned()
                    .flatten();
                let arg_ty = if pre_inferred_ty.is_some() {
                    pre_inferred_ty
                } else {
                    self.with_call_access(&mut call_access, |checker| {
                        checker.check_call_argument_access(arg, *param_conv, param_ty, true);
                        let saved_expected = checker.expected_type.clone();
                        checker.expected_type = Some(param_ty.clone());
                        let inferred = checker.infer(&mut arg.expr);
                        checker.expected_type = saved_expected;
                        checker.check_call_argument_captures(&arg.expr);
                        inferred
                    })
                };
                if let Some(arg_ty) = arg_ty {
                    let arg_ty = self.widen_numeric_argument(
                        &mut arg.expr,
                        arg_ty,
                        param_ty,
                        *param_conv,
                    );
                    let reported = self.check_type_assignable(param_ty, &arg_ty, arg.expr.span());
                    if !reported && arg_ty != *param_ty {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`{}` wants {} for argument {}, but this is {}",
                                method,
                                param_ty.show(),
                                arg_idx + 1,
                                arg_ty.show()
                            ),
                            "every argument must match its parameter's type".to_string(),
                            type_fix_hint(param_ty, &arg_ty),
                            Some(arg.expr.span()),
                        ));
                    }
                }
                self.check_callable_argument_ownership(
                    method,
                    arg_idx,
                    *param_conv,
                    param_ty,
                    arg,
                );
                arg_idx += 1;
            }
        }
        self.activate_call_reservations(&call_access, span);
        sig.return_type.clone()
    }

    pub(crate) fn check_trait_method_args(
        &mut self,
        method: &str,
        sig: &crate::AST::TraitMethodSig,
        receiver: &Expr,
        args: &mut Vec<crate::AST::CallArg>,
        span: Span,
    ) -> Option<Type> {
        let (receiver_param, params) = sig
            .params
            .split_first()
            .filter(|(receiver, _)| receiver.name == Syntax::KW_SELF)
            .map_or((None, sig.params.as_slice()), |(receiver, params)| {
                (Some(receiver), params)
            });

        // Trait-object dispatch has no `FuncSig`, but its `Param` rows carry
        // the same public contract. Feed those rows to the one binder before
        // any dynamic-dispatch arity or type work, just as static methods do.
        let bind_params: Vec<crate::Sema::CallBinder::BindParam<'_>> = params
            .iter()
            .map(|param| crate::Sema::CallBinder::BindParam {
                label: param.call_label(),
                name: &param.name,
                zone: param.zone,
                default: param.default.as_deref(),
                convention: param.convention,
                ty: Some(&param.ty),
                variadic: param.variadic,
                core_default: None,
            })
            .collect();
        if crate::Sema::CallBinder::bind_call_args(
            method,
            &bind_params,
            args,
            span,
            &mut self.diags,
        )
        .is_none()
        {
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return sig.return_type.clone();
        }
        self.register_binder_refs(args);

        // `TraitMethodSig` stores a variadic parameter's element type, while
        // the emitted method ABI receives the normalized list. Use the same
        // normalizer as free and concrete methods for homogeneous rest
        // parameters. Trait-bounded heterogeneous rests remain source-sized
        // here; their dynamic ABI has no single list element type.
        let variadic = params.last().is_some_and(|param| param.variadic);
        let homogeneous_variadic = variadic
            && params
                .last()
                .is_none_or(|param| param.variadic_bound_list.is_none());
        if homogeneous_variadic {
            let fake_sig = crate::AST::FuncSig {
                params: params
                    .iter()
                    .map(|param| {
                        let ty = if param.variadic {
                            crate::AST::Type::List(Box::new(param.ty.clone()))
                        } else {
                            param.ty.clone()
                        };
                        (param.convention, ty)
                    })
                    .collect(),
                root_param: false,
                return_type: sig.return_type.clone(),
                return_view_provenance: crate::AST::ViewProvenanceCell::new(),
                is_extern: false,
                is_unsafe: false,
                is_pure: sig.is_pure,
                is_foreign_thread_safe: false,
                is_sanitizer: false,
                is_must_use: false,
                is_c_abi: false,
                c_abi_name: None,
                foreign_effect_root: None,
                param_info: params
                    .iter()
                    .map(|param| (param.name.clone(), param.default.is_some()))
                    .collect(),
                param_call: params
                    .iter()
                    .map(|param| (param.call_label().to_string(), param.zone))
                    .collect(),
                defaults: params
                    .iter()
                    .map(|param| param.default.as_deref().cloned())
                    .collect(),
                param_variadic: params.iter().map(|param| param.variadic).collect(),
                variadic_bounds: None,
                param_view_from_names: Vec::new(),
            };
            let mut packed = crate::AST::Call {
                name: method.to_string(),
                name_span: span,
                type_args: Vec::new(),
                args: std::mem::take(args),
                resolved_ret: None,
                range_checked: false,
                widen_approx: false,
            };
            self.normalize_variadic_call(&mut packed, &fake_sig);
            *args = packed.args;
        }

        let expected_args = params.len();
        let fixed = params.len().saturating_sub(1);
        let arity_ok = if variadic && !homogeneous_variadic {
            args.len() >= fixed
        } else {
            args.len() == expected_args
        };
        if !arity_ok {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`{method}` expects {} argument{}, got {}",
                    expected_args,
                    if expected_args == 1 { "" } else { "s" },
                    args.len()
                ),
                "every argument must match a parameter (not counting `self`)".to_string(),
                format!("check the definition of `{method}` on the trait"),
                Some(span),
            ));
        }
        let mut call_access = self.call_access_frame();
        if let Some(receiver_param) = receiver_param {
            if receiver_param.convention == AccessConvention::Write {
                self.check_mutating_method_receiver(receiver, method, span);
            }
            self.with_call_access(&mut call_access, |checker| {
                if receiver_param.convention == AccessConvention::Write {
                    checker.record_call_receiver_reservation(receiver, receiver.span());
                } else {
                    checker.record_call_receiver_access(
                        receiver,
                        receiver_param.convention,
                        receiver.span(),
                    );
                }
            });
        }
        for (index, arg) in args.iter_mut().enumerate() {
            let Some(param) = params
                .get(if variadic && index >= fixed { fixed } else { index })
            else {
                continue;
            };
            let param_ty = if variadic && homogeneous_variadic && index >= fixed {
                Type::List(Box::new(param.ty.clone()))
            } else {
                param.ty.clone()
            };
            if param.convention == AccessConvention::Read && !param_ty.is_scalar() {
                self.borrow_ctx = true;
            }
            let saved_expected = self.expected_type.clone();
            self.expected_type = Some(param_ty.clone());
            let arg_ty = self.with_call_access(&mut call_access, |checker| {
                checker.check_call_argument_access(arg, param.convention, &param_ty, true);
                let inferred = checker.infer(&mut arg.expr);
                checker.check_call_argument_captures(&arg.expr);
                inferred
            });
            self.expected_type = saved_expected;
            if let Some(arg_ty) = arg_ty {
                let arg_ty = self.widen_numeric_argument(
                    &mut arg.expr,
                    arg_ty,
                    &param_ty,
                    param.convention,
                );
                let reported =
                    self.check_type_assignable(&param_ty, &arg_ty, arg.expr.span());
                if !reported && arg_ty != param_ty {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{method}` wants {}, but this is {}",
                            param_ty.show(),
                            arg_ty.show()
                        ),
                        "every argument must match its parameter's type".to_string(),
                        type_fix_hint(&param_ty, &arg_ty),
                        Some(arg.expr.span()),
                    ));
                }
            }
            self.check_callable_argument_ownership(
                method,
                index,
                param.convention,
                &param_ty,
                arg,
            );
        }
        self.activate_call_reservations(&call_access, span);
        sig.return_type.clone()
    }

    pub(crate) fn struct_owner_module(
        &self,
        type_name: &str,
        import_ns: Option<&str>,
    ) -> Option<usize> {
        if let Some(namespace) = import_ns {
            if namespace.contains("::") {
                let identity = format!("{namespace}::{type_name}");
                let mod_idx = self.name_ledger.nominal_module(&identity)?;
                let mods = self.modules?;
                return mods
                    .get(mod_idx)
                    .filter(|module| module.registry.contains(type_name))
                    .map(|_| mod_idx);
            }
            let mod_idx = *self.imports.get(namespace)?;
            let mods = self.modules?;
            if mods[mod_idx].registry.contains(type_name) {
                return Some(mod_idx);
            }
            return None;
        }
        if type_name.contains("::") {
            let (namespace, leaf) = type_name.rsplit_once("::")?;
            return self.struct_owner_module(leaf, Some(namespace));
        }
        if self.registry.contains(type_name) {
            return Some(self.module_idx);
        }
        let mods = self.modules?;
        if let Some(alias) = self.name_ledger.alias(self.module_idx, type_name) {
            if let Some(owner) = alias.target_module {
                let leaf = alias
                    .target
                    .rsplit_once("::")
                    .or_else(|| alias.target.rsplit_once('.'))
                    .map_or(alias.target.as_str(), |(_, leaf)| leaf);
                if mods
                    .get(owner)
                    .is_some_and(|module| module.registry.contains(leaf))
                    && self.type_is_pub_in(owner, leaf)
                {
                    return Some(owner);
                }
            }
        }
        let mut found = None;
        for (idx, st) in mods.iter().enumerate() {
            if st.registry.contains(type_name) && self.type_is_pub_in(idx, type_name) {
                if found.is_some_and(|previous| previous != idx) {
                    return None;
                }
                found = Some(idx);
            }
        }
        found
    }

    pub(crate) fn split_type_name(type_name: &str) -> (Option<&str>, &str) {
        type_name
            .rsplit_once("::")
            .or_else(|| type_name.rsplit_once('.'))
            .map_or((None, type_name), |(namespace, leaf)| (Some(namespace), leaf))
    }

    pub(crate) fn struct_type_name_parts<'b>(
        &self,
        type_name: &'b str,
    ) -> (Option<&'b str>, &'b str) {
        Self::split_type_name(type_name)
    }

    pub(crate) fn canonical_nominal_name(&self, owner: usize, leaf: &str) -> String {
        self.name_ledger
            .nominal_identity(owner, leaf)
            .expect("name ledger must contain every loaded module")
    }

    pub(crate) fn struct_fields_for_type_name(
        &self,
        type_name: &str,
    ) -> Option<&[(String, Span, Type)]> {
        let (import_ns, leaf) = Self::split_type_name(type_name);
        let owner = self.struct_owner_module(leaf, import_ns)?;
        self.struct_fields_of(owner, leaf)
    }

    pub(crate) fn type_implements_trait_for_name(
        &self,
        type_name: &str,
        trait_name: &str,
    ) -> bool {
        let (import_ns, leaf) = Self::split_type_name(type_name);
        let Some(owner) = self.struct_owner_module(leaf, import_ns) else {
            return self.trait_reg.implements_trait(type_name, trait_name);
        };
        if owner == self.module_idx {
            self.trait_reg.implements_trait(leaf, trait_name)
        } else {
            self.modules
                .and_then(|modules| modules.get(owner))
                .is_some_and(|module| module.trait_reg.implements_trait(leaf, trait_name))
        }
    }

    pub(crate) fn capability_type_context(
        &self,
        ty: &Type,
    ) -> (
        Type,
        &crate::Sema::TypeRegistry,
        &crate::Traits::TraitRegistry,
    ) {
        let (name, args) = match ty {
            Type::Named(name) => (name.as_str(), None),
            Type::Apply { name, args } => (name.as_str(), Some(args.as_slice())),
            _ => return (ty.clone(), self.registry, self.trait_reg),
        };
        let (import_ns, leaf) = Self::split_type_name(name);
        let Some(owner) = self.struct_owner_module(leaf, import_ns) else {
            return (ty.clone(), self.registry, self.trait_reg);
        };
        let normalized = match args {
            None => Type::Named(leaf.to_string()),
            Some(args) => Type::Apply {
                name: leaf.to_string(),
                args: args.to_vec(),
            },
        };
        if owner == self.module_idx {
            (normalized, self.registry, self.trait_reg)
        } else if let Some(module) = self.modules.and_then(|modules| modules.get(owner)) {
            (normalized, &module.registry, &module.trait_reg)
        } else {
            (ty.clone(), self.registry, self.trait_reg)
        }
    }

    pub(crate) fn is_equatable_type(&self, ty: &Type) -> bool {
        let (normalized, registry, trait_reg) = self.capability_type_context(ty);
        crate::Sema::Diagnostics::is_equatable(&normalized, registry, trait_reg)
    }

    pub(crate) fn types_comparable_type(&self, ty: &Type) -> bool {
        let (normalized, registry, _) = self.capability_type_context(ty);
        crate::Sema::Diagnostics::types_comparable(&normalized, registry)
    }

    pub(crate) fn incomparable_field_type(&self, ty: &Type) -> Option<String> {
        let (normalized, registry, _) = self.capability_type_context(ty);
        crate::Sema::Diagnostics::incomparable_field(&normalized, registry)
    }

    pub(crate) fn struct_fields_of(
        &self,
        owner_mod: usize,
        type_name: &str,
    ) -> Option<&[(String, Span, Type)]> {
        if owner_mod == self.module_idx {
            self.registry.struct_fields(type_name)
        } else {
            self.modules?
                .get(owner_mod)?
                .registry
                .struct_fields(type_name)
        }
    }

    pub(crate) fn type_contains_observable_clock(&self, ty: &Type) -> bool {
        fn contains(
            checker: &Checker<'_>,
            ty: &Type,
            context_mod: usize,
            seen: &mut HashSet<(usize, String)>,
        ) -> bool {
            match ty {
                Type::Tagged { marker, inner }
                    if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::DeterministicClock))
                        && crate::Sema::Diagnostics::is_clock_type(inner) =>
                {
                    false
                }
                Type::Named(name) if name == crate::Syntax::CLOCK_TYPE => true,
                Type::Named(name) | Type::Apply { name, .. } => {
                    if let Type::Apply { args, .. } = ty {
                        if args
                            .iter()
                            .any(|arg| contains(checker, arg, context_mod, seen))
                        {
                            return true;
                        }
                    }
                    let context_registry = if context_mod == checker.module_idx {
                        Some(checker.registry)
                    } else {
                        checker
                            .modules
                            .and_then(|modules| modules.get(context_mod))
                            .map(|state| &state.registry)
                    };
                    let owner_mod = if context_registry
                        .is_some_and(|registry| registry.contains(name))
                    {
                        context_mod
                    } else if let Some(owner) = checker.struct_owner_module(name, None) {
                        owner
                    } else {
                        return false;
                    };
                    if !seen.insert((owner_mod, name.clone())) {
                        return false;
                    }
                    let registry = if owner_mod == checker.module_idx {
                        checker.registry
                    } else if let Some(registry) = checker
                        .modules
                        .and_then(|modules| modules.get(owner_mod))
                        .map(|state| &state.registry)
                    {
                        registry
                    } else {
                        return false;
                    };
                    let result = match registry.types.get(name) {
                        Some(TypeDef::Struct { fields, .. }) => fields.iter().any(
                            |(_, _, field_ty)| {
                                contains(checker, field_ty, owner_mod, seen)
                            },
                        ),
                        Some(TypeDef::Enum { variants, .. }) => {
                            variants.values().any(|(_, payload)| match payload {
                                VariantPayload::Unit => false,
                                VariantPayload::Single(payload_ty, _) => {
                                    contains(checker, payload_ty, owner_mod, seen)
                                }
                                VariantPayload::Named(fields) => fields.iter().any(|field| {
                                    contains(checker, &field.ty, owner_mod, seen)
                                }),
                            })
                        }
                        Some(TypeDef::Distinct { base, .. }) => {
                            contains(checker, base, owner_mod, seen)
                        }
                        Some(TypeDef::Alias { target, .. }) => {
                            contains(checker, target, owner_mod, seen)
                        }
                        None => false,
                    };
                    seen.remove(&(owner_mod, name.clone()));
                    result
                }
                Type::Option(inner)
                | Type::List(inner)
                | Type::Shared(inner)
                | Type::Tagged { inner, .. } => contains(checker, inner, context_mod, seen),
                Type::Result { ok, err } => {
                    contains(checker, ok, context_mod, seen)
                        || contains(checker, err, context_mod, seen)
                }
                Type::Map { key, value, .. } => {
                    contains(checker, key, context_mod, seen)
                        || contains(checker, value, context_mod, seen)
                }
                Type::Tuple(fields) => fields
                    .iter()
                    .any(|(_, field_ty)| contains(checker, field_ty, context_mod, seen)),
                Type::FixedList { elem, .. } => contains(checker, elem, context_mod, seen),
                _ => false,
            }
        }

        contains(self, ty, self.module_idx, &mut HashSet::new())
    }

    /// D-FIELDPOL1: `type_name`'s computed fields (name → span + declared
    /// type) in `owner_mod`, or `None` when it has none / isn't a struct.
    pub(crate) fn computed_field_types_of(
        &self,
        owner_mod: usize,
        type_name: &str,
    ) -> Option<&HashMap<String, (Span, Type)>> {
        if owner_mod == self.module_idx {
            self.registry.computed_field_types(type_name)
        } else {
            self.modules?
                .get(owner_mod)?
                .registry
                .computed_field_types(type_name)
        }
    }

    /// Check if `enum_name` is a known enum in the current or any imported module.
    pub(crate) fn is_known_enum(&self, enum_name: &str) -> bool {
        if self.registry.enum_variants(enum_name).is_some() {
            return true;
        }
        if let Some(mods) = self.modules {
            for &idx in self.imports.values() {
                if self.type_is_pub_in(idx, enum_name)
                    && mods[idx].registry.enum_variants(enum_name).is_some()
                {
                    return true;
                }
            }
        }
        // D-TERM1 (ratified 2026-06-22): `Key` is a core enum, not in user registry.
        if enum_name == crate::Syntax::TYPE_KEY {
            return true;
        }
        // D-PROCESS1=A: `ProcessStreamMode` is a core dot-literal enum.
        if enum_name == "ProcessStreamMode" {
            return true;
        }
        // D-TEXTWIDTH1=B: `TextWidth`'s two field enums.
        if matches!(enum_name, "TextWidthAmbiguous" | "TextWidthControls") {
            return true;
        }
        if enum_name == Syntax::DURATION_UNIT_TYPE {
            return true;
        }
        if matches!(enum_name, "Overflow" | "FailurePolicy" | "DispatchState" | "HookPolicy" | "HookDecision" | "HookOutcome") {
            return true;
        }
        if matches!(enum_name, "NetShutdown" | "NetReadyInterest") {
            return true;
        }
        if matches!(enum_name, "NetError" | "NetDnsError") {
            return true;
        }
        if is_io_error_type_name(enum_name) || enum_name == Syntax::TYPE_IO_OPERATION {
            return true;
        }
        if matches!(enum_name, "SMTPSecurity" | "RecipientPolicy" | "EmailError") {
            return true;
        }
        if enum_name == "AuthError" {
            return true;
        }
        if matches!(enum_name, "ServiceReceipt" | "ServiceError") {
            return true;
        }
        false
    }

    /// Resolve enum variants for `enum_name`, returning a cloned copy.
    /// Checks current registry and imported file-module registries.
    pub(crate) fn resolve_enum_variants_cloned(
        &self,
        enum_name: &str,
    ) -> Option<HashMap<String, (Span, VariantPayload)>> {
        if let Some(v) = self.registry.enum_variants(enum_name) {
            return Some(v.clone());
        }
        if let Some(mods) = self.modules {
            for &idx in self.imports.values() {
                if self.type_is_pub_in(idx, enum_name) {
                    if let Some(v) = mods[idx].registry.enum_variants(enum_name) {
                        return Some(v.clone());
                    }
                }
            }
        }
        // D-TERM1 (ratified 2026-06-22): `Key` is a core enum (not in user registry).
        // Synthesise its variant table here so `Key.Char(c)` / `Key.Enter` literals work.
        if enum_name == crate::Syntax::TYPE_KEY {
            return Some(core_key_variants());
        }
        if let Some(v) = core_task_failure_variants(enum_name) {
            return Some(v);
        }
        // D-PROCESS1=A: `ProcessStreamMode` is a core enum — synthesise its variant
        // table so `.Stream`/`.Inherit`/`.Capture` dot-literals resolve (D-ENUMDOT2).
        if enum_name == "ProcessStreamMode" {
            return Some(core_process_stream_mode_variants());
        }
        if enum_name == Syntax::TYPE_TERMINAL_MODE {
            return Some(core_terminal_mode_variants());
        }
        if enum_name == "EnvError" {
            return Some(core_env_error_variants());
        }
        if let Some(v) = core_service_receipt_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_service_error_variants(enum_name) {
            return Some(v);
        }
        // D-TEXTWIDTH1=B: `TextWidthAmbiguous`/`TextWidthControls` — synthesise
        // their variant table so `.Narrow`/`.Wide`/`.Zero`/`.Reject` dot-literals
        // resolve (D-ENUMDOT2), same mechanism as `ProcessStreamMode`.
        if let Some(v) = core_text_width_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_duration_unit_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_event_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_reduce_op_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_net_control_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_net_error_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_http_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_io_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_encoding_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_lang_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_fact_kind_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_email_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_tls_variants(enum_name) {
            return Some(v);
        }
        if let Some(v) = core_auth_variants(enum_name) {
            return Some(v);
        }
        None
    }

    /// D-TAG1: fold a field chain rooted at an enum type name into
    /// `(enum_name, dotted_path)` — `Damage.Fire.Burn` → `("Damage",
    /// "Fire.Burn")`. Returns `None` for a bare `Ident` (no hop — the
    /// single-segment `Enum.Variant` routes stay untouched), for chains not
    /// rooted at a known enum, or when a local shadows the type name.
    pub(crate) fn fold_enum_variant_path(&self, e: &Expr) -> Option<(String, String)> {
        fn walk<'e>(e: &'e Expr) -> Option<(&'e str, Vec<&'e str>)> {
            match e {
                Expr::Ident(name, _) => Some((name, Vec::new())),
                Expr::Field(inner, member, _) => {
                    let (root, mut segs) = walk(inner)?;
                    segs.push(member);
                    Some((root, segs))
                }
                _ => None,
            }
        }
        let (root, segs) = walk(e)?;
        if segs.is_empty() {
            return None;
        }
        if self.lookup(root).is_some() || !self.is_known_enum(root) {
            return None;
        }
        Some((root.to_string(), segs.join(".")))
    }

    /// D-TAG1: the enum's variant groups (group path → span + ordered leaf paths),
    /// resolved like `resolve_enum_variants_cloned` (local registry, then imports).
    /// `None` when the type isn't an enum; an empty map for a flat enum.
    pub(crate) fn resolve_enum_groups_cloned(
        &self,
        enum_name: &str,
    ) -> Option<HashMap<String, (Span, Vec<String>)>> {
        if let Some(g) = self.registry.enum_groups(enum_name) {
            return Some(g.clone());
        }
        if let Some(mods) = self.modules {
            for &idx in self.imports.values() {
                if self.type_is_pub_in(idx, enum_name) {
                    if let Some(g) = mods[idx].registry.enum_groups(enum_name) {
                        return Some(g.clone());
                    }
                }
            }
        }
        None
    }

    pub(crate) fn field_is_pub_in(&self, owner_mod: usize, type_name: &str, field: &str) -> bool {
        if owner_mod == self.module_idx {
            return true;
        }
        let name = format!("{type_name}.{field}");
        self.name_ledger.visible(self.module_idx, owner_mod, &name)
    }

    pub(crate) fn type_is_pub_in(&self, owner_mod: usize, type_name: &str) -> bool {
        if owner_mod == self.module_idx {
            return true;
        }
        self.name_ledger.visible(self.module_idx, owner_mod, type_name)
    }

    pub(crate) fn check_struct_lit(
        &mut self,
        type_name: &str,
        type_args: &[Type],
        import_ns: Option<&str>,
        fields: &mut Vec<(String, Span, Expr)>,
        span: Span,
    ) -> Type {
        // A canonical nominal arrives here from an expected type as
        // `package::path::Type`. Field registries and visibility tables are
        // keyed by the owner-local leaf, while the returned type below keeps
        // the canonical identity. Split only for this local lookup; never
        // reintroduce the short name as semantic identity.
        let (qualified_namespace, leaf_name) = Self::split_type_name(type_name);
        let import_ns = import_ns.or(qualified_namespace);
        let type_name = leaf_name;
        // D-HTTP-CORE2=A: shared HTTP messages enforce typed headers and a
        // single-use byte Body. The old public-field literals cannot preserve
        // those invariants and ended at this core API break.
        if matches!(type_name, "HTTPRequest" | "HTTPResponse") {
            let (what, fix) = if type_name == "HTTPResponse" {
                (
                    "`HTTPResponse.{ ... }` was replaced by the shared HTTP response constructor",
                    "use `server.response(status, text)`, then add headers with `.header(...)`",
                )
            } else {
                (
                    "`HTTPRequest.{ ... }` was replaced by the shared HTTP request constructor",
                    "use `client.request(method, url)`, then configure it with request methods",
                )
            };
            self.diags.push(Diagnostic::error(
                "E2002",
                what.to_string(),
                "HTTP messages now contain validated typed fields and a byte-native, single-use Body; public-field literals cannot preserve those invariants".to_string(),
                fix.to_string(),
                Some(span),
            ));
            return Type::Named(type_name.to_string());
        }
        // E2-M10: compiler-known constructable struct types (HTTPRequest, HTTPResponse).
        // These have no user-module owner but are valid in struct literals.
        if let Some(core_fields) = core_constructable_fields(type_name) {
            let str_map_ty = Type::Map {
                key: Box::new(Type::String),
                key_span: None,
                value: Box::new(Type::String),
            };
            let provided_names: std::collections::HashSet<String> =
                fields.iter().map(|(n, ..)| n.clone()).collect();
            for (fname, _, fexpr) in fields.iter_mut() {
                let expected_ty: Option<Type> = core_fields
                    .iter()
                    .find(|(n, _)| n == fname)
                    .map(|(_, t)| t.clone());
                let saved = self.expected_type.clone();
                if let Some(et) = expected_ty.as_ref() {
                    self.expected_type = Some(et.clone());
                }
                let got = self.infer(fexpr);
                self.expected_type = saved;
                if type_name == Syntax::TYPE_ERR {
                    if let (Some(expected), Some(got)) = (expected_ty.as_ref(), got.as_ref()) {
                        self.check_type_assignable(expected, got, fexpr.span());
                    }
                }
                let _ = (&str_map_ty, &expected_ty);
            }
            // Report missing fields.
            let missing: Vec<_> = core_fields
                .iter()
                .filter(|(n, _)| !provided_names.contains(n))
                .map(|(n, _)| n.clone())
                .collect();
            if !missing.is_empty() {
                self.diags.push(Diagnostic::error(
                    "E0303",
                    format!(
                        "struct literal for `{}` is missing fields: {}",
                        type_name,
                        missing.join(", ")
                    ),
                    "every field must appear exactly once".to_string(),
                    format!("add: {}", missing.join(", ")),
                    Some(span),
                ));
            }
            return Type::Named(type_name.to_string());
        }
        let Some(owner_mod) = self.struct_owner_module(type_name, import_ns) else {
            self.diags.push(Diagnostic::error(
                "E0119",
                format!("there's no type called `{}`", type_name),
                "struct literals need a struct type name".to_string(),
                "define the struct first, or check the spelling".to_string(),
                Some(span),
            ));
            for (_, _, e) in fields.iter_mut() {
                self.infer(e);
            }
            return Type::Named(type_name.to_string());
        };
        if owner_mod != self.module_idx && !self.type_is_pub_in(owner_mod, type_name) {
            self.diags.push(private_item(type_name, span));
        } else if owner_mod != self.module_idx
            && Syntax::classify_identifier(type_name) == Syntax::IdentifierClass::SoftPublic
        {
            self.diags.push(soft_public_use(type_name, span));
        }
        let def_fields: Vec<(String, Span, Type)> = self
            .struct_fields_of(owner_mod, type_name)
            .map(|fields| fields.to_vec())
            .unwrap_or_default();
        if def_fields.is_empty() {
            self.diags.push(Diagnostic::error(
                "E0119",
                format!("there's no type called `{}`", type_name),
                "struct literals need a struct type name".to_string(),
                "define the struct first, or check the spelling".to_string(),
                Some(span),
            ));
            for (_, _, e) in fields.iter_mut() {
                self.infer(e);
            }
            return Type::Named(type_name.to_string());
        };
        let subst = self.struct_subst_for_owner(owner_mod, type_name, type_args);
        let field_names: Vec<String> = def_fields.iter().map(|(n, ..)| n.clone()).collect();
        // D-PATCH1: `T.Patch` literals are partial — omitted fields mean "unchanged"
        // (encoded as `null` / `None`); provided values use the inner field type.
        let is_patch_lit = type_name
            .strip_suffix(".Patch")
            .is_some_and(|base| super::patch_type_name(base) == type_name)
            && def_fields
                .iter()
                .all(|(_, _, ty)| matches!(ty, Type::Option(_)));
        let mut provided = HashMap::new();
        for (name, name_span, expr) in fields.iter_mut() {
            if provided.insert(name.clone(), ()).is_some() {
                self.diags.push(Diagnostic::error(
                    "E0303",
                    format!("field `{}` appears more than once", name),
                    "each field may be written only once in a struct literal".to_string(),
                    "remove the duplicate field".to_string(),
                    Some(*name_span),
                ));
            }
            if owner_mod != self.module_idx && !self.field_is_pub_in(owner_mod, type_name, name) {
                self.diags.push(private_item(name, *name_span));
            } else if owner_mod != self.module_idx
                && Syntax::classify_identifier(name) == Syntax::IdentifierClass::SoftPublic
            {
                self.diags.push(soft_public_use(name, *name_span));
            }
            let field_def = def_fields.iter().find(|(n, ..)| n == name);
            let saved_expected = self.expected_type.clone();
            let saved_esc = self.lambda_escapes;
            if let Some((_, _, fty)) = field_def {
                let inst = self.instantiate_type_for_owner(owner_mod, fty, &subst);
                self.expected_type = if is_patch_lit {
                    inst.unwrap_option().cloned()
                } else {
                    Some(inst)
                };
            }
            let string_view_field = self.expected_type.as_ref().is_some_and(|ty| {
                matches!(
                    ty,
                    Type::Apply { name, args }
                        if name == "View"
                            && matches!(args.as_slice(), [Type::Named(inner)] if inner == "str")
                )
            });
            if matches!(expr, Expr::Lambda(_)) {
                self.lambda_escapes = true;
            }
            let saved_string_view_read = self.allow_string_view_read;
            if string_view_field {
                self.allow_string_view_read = true;
            }
            let et = self.infer(expr);
            self.allow_string_view_read = saved_string_view_read;
            self.expected_type = saved_expected;
            self.lambda_escapes = saved_esc;
            // A struct-lit field VALUE is an owning position. A bare borrowed-in-env
            // non-`Copy` ident (a `read`/`mut` param → `&T`/`&mut T`) can't be moved
            // into the field — codegen would emit `(*user_n)` → rustc E0507.
            self.clone_borrowed_struct_field_value(expr, et.as_ref());
            // D-ALLOC2: E0631 — storing an arena `view` in a struct field would
            // let the struct (which can outlive the region) keep a dangling
            // borrow into the arena.
            if let Expr::Ident(vname, vspan) = expr {
                if self.is_arena_view(vname) || self.is_fixed_backing_view(vname) {
                    self.report_view_escape(vname, "be stored in a struct field", *vspan);
                }
                // D-DYNARRAY1: E2305 — storing a `View<T>` in a struct field
                // would let the struct outlive the list it borrows from.
                if self.is_list_view(vname) {
                    if !self.named_view_has_stable_owner(vname) {
                        self.report_view_escape(vname, "be stored in a struct field", *vspan);
                    }
                }
                if string_view_field && self.is_string_view(vname) {
                    if !self.named_view_has_stable_owner(vname) {
                        self.report_string_view_unsupported_use(
                            vname,
                            "be stored in a struct field",
                            *vspan,
                        );
                    }
                }
            }
            if let Some((_, _, fty)) = field_def {
                let inst = self.instantiate_type_for_owner(owner_mod, fty, &subst);
                if let Some(et) = et {
                    // `View<str>` fields accept only live string-view bindings
                    // (or trim/after/before calls). A plain owned `String` is
                    // not a view — allowing it here false-greened into an ICE
                    // when codegen emitted `String` into an `&str` slot (#1163).
                    let string_view_compatible = string_view_field
                        && et == Type::String
                        && (matches!(
                            expr,
                            Expr::Ident(name, _) if self.is_string_view(name)
                        ) || self.string_view_call_source(expr).is_some());
                    if string_view_field
                        && et == Type::String
                        && !string_view_compatible
                    {
                        // #1164: owned String into View<str> is a teaching
                        // ceiling, not a missing-provenance case.
                        self.report_owned_string_as_view_str(expr.span());
                    } else if is_patch_lit {
                        if let Some(inner) = inst.unwrap_option() {
                            self.check_type_assignable(&inner, &et, expr.span());
                        }
                    } else if !string_view_compatible {
                        self.check_type_assignable(&inst, &et, expr.span());
                    }
                }
            } else if self
                .computed_field_types_of(owner_mod, type_name)
                .is_some_and(|c| c.contains_key(name))
            {
                // D-FIELDPOL1: a computed field is never settable — it's
                // recomputed on every read, so a struct literal can't give it
                // a value.
                self.diags.push(Diagnostic::error(
                    "E0339",
                    format!(
                        "`{}` is a computed field on `{}` — it can't be set",
                        name, type_name
                    ),
                    format!(
                        "`{}` is declared `{} => …` — its value always comes from that formula, never from a struct literal",
                        name, name
                    ),
                    format!("remove `{}` from this `.{{ … }}`", name),
                    Some(*name_span),
                ));
            } else {
                self.diags.push(Diagnostic::error(
                    "E0303",
                    format!("struct literal for `{}` has no field `{}`", type_name, name),
                    "struct literals may only set fields that exist on the type".to_string(),
                    suggest_field(name, &field_names)
                        .map(|s| format!("did you mean `{}`?", s))
                        .unwrap_or_else(|| "remove this field".to_string()),
                    Some(*name_span),
                ));
            }
        }
        let missing: Vec<_> = def_fields
            .iter()
            .filter(|(n, _, _)| !provided.contains_key(n))
            .map(|(n, ..)| n.clone())
            .collect();
        // D-FIELDDEF1=C: omitted fields with `=` defaults are filled in.
        let defaults = if owner_mod == self.module_idx {
            self.registry
                .field_defaults(type_name)
                .map(|d| d.clone())
        } else {
            self.modules
                .and_then(|mods| mods.get(owner_mod))
                .and_then(|m| m.registry.field_defaults(type_name))
                .map(|d| d.clone())
        };
        let mut still_missing = Vec::new();
        for name in missing {
            let Some(expr) = defaults.as_ref().and_then(|d| d.get(&name)).cloned() else {
                still_missing.push(name);
                continue;
            };
            let name_span = def_fields
                .iter()
                .find(|(n, ..)| n == &name)
                .map(|(_, sp, ..)| *sp)
                .unwrap_or(span);
            let mut filled = expr;
            let field_def = def_fields.iter().find(|(n, ..)| n == &name);
            let saved_expected = self.expected_type.clone();
            if let Some((_, _, fty)) = field_def {
                let inst = self.instantiate_type_for_owner(owner_mod, fty, &subst);
                self.expected_type = Some(inst);
            }
            let et = self.infer(&mut filled);
            self.expected_type = saved_expected;
            if let (Some((_, _, fty)), Some(et)) = (field_def, et) {
                let inst = self.instantiate_type_for_owner(owner_mod, fty, &subst);
                self.check_type_assignable(&inst, &et, filled.span());
            }
            fields.push((name, name_span, filled));
        }
        if !still_missing.is_empty() && !is_patch_lit {
            self.diags.push(Diagnostic::error(
                "E0303",
                format!(
                    "struct literal for `{}` is missing fields: {}",
                    type_name,
                    still_missing.join(", ")
                ),
                "every field without an `=` default must appear exactly once".to_string(),
                format!("add: {}", still_missing.join(", ")),
                Some(span),
            ));
        }
        let nominal_name = if owner_mod == self.module_idx {
            type_name.to_string()
        } else {
            self.canonical_nominal_name(owner_mod, type_name)
        };
        if !type_args.is_empty() {
            Type::Apply {
                name: nominal_name,
                args: type_args.to_vec(),
            }
        } else if self
            .modules
            .and_then(|modules| modules.get(owner_mod))
            .map_or_else(
                || self.trait_reg.struct_params.get(type_name),
                |module| module.trait_reg.struct_params.get(type_name),
            )
            .is_some_and(|p| !p.is_empty())
        {
            let params = if owner_mod == self.module_idx {
                self.trait_reg.struct_params.get(type_name)
            } else {
                self.modules
                    .and_then(|modules| modules.get(owner_mod))
                    .and_then(|module| module.trait_reg.struct_params.get(type_name))
            }
            .expect("generic struct parameters exist");
            Type::Apply {
                name: nominal_name,
                args: params
                    .iter()
                    .map(|p| Type::Named(p.name.clone()))
                    .collect(),
            }
        } else {
            Type::Named(nominal_name)
        }
    }

    /// Rewrite a struct-literal (or enum-payload, via the same call from
    /// `check_enum_lit`) field VALUE that is a bare non-`Copy` ident into an
    /// `Expr::Copy` node (D-CAP2 — the same node `copy x` desugars to), when
    /// leaving it a move would either (a) not
    /// type-check (a borrowed param) or (b) silently move an OWNED LOCAL that
    /// is still read later — both would otherwise reach rustc as a raw,
    /// unreported E0507/E0382 (I2). Two cases:
    ///   - a `read`/`mut` param is `&T`/`&mut T`, so using it directly as the
    ///     (owning) field value would emit `(*user_n)` → rustc E0507. Reject it
    ///     before codegen and require an explicit copy.
    ///   - an OWNED local (no param convention) moves for real in the
    ///     generated Rust; if a later statement in the same/enclosing block
    ///     still reads it, that would be rustc E0382 ("use after move") with
    ///     no jet-level diagnostic ever raised — clone it instead, matching
    ///     the same "auto-clone at an owning slot" rule `check_take_arg_ownership`
    ///     already applies to a plain function-call argument. An owned local
    ///     that is NOT read again keeps moving (no wasted clone; unchanged
    ///     from prior behavior).
    /// `take` params, `Copy` types, and non-ident values are left untouched (no clone).
    fn clone_borrowed_struct_field_value(&mut self, expr: &mut Expr, ty: Option<&Type>) {
        if self.reject_borrowed_param_subplace(expr, ty, "fill an owned field") {
            return;
        }
        if let Expr::Ident(name, span) = expr {
            let borrowed = self.lookup(name).is_some_and(|info| {
                !type_is_copy(&info.ty)
                    && matches!(
                        info.param_conv,
                        Some(AccessConvention::Read) | Some(AccessConvention::Write)
                    )
            });
            if borrowed {
                self.diags.push(Diagnostic::error(
                    "E0120",
                    format!("`{name}` was not moved here, so it cannot fill an owned field"),
                    "this function has read access only and does not own the value".to_string(),
                    format!("copy it explicitly with `{}{name}`", Syntax::SIGIL_COPY),
                    Some(*span),
                ));
                return;
            }
        }
        let should_clone = match expr {
            Expr::Ident(name, _) => {
                let name = name.clone();
                self.lookup(&name).is_some_and(|info| {
                    if type_is_copy(&info.ty) {
                        return false;
                    }
                    match info.param_conv {
                        Some(AccessConvention::Read) | Some(AccessConvention::Write) => true,
                        _ => self.is_name_live_after(&name),
                    }
                })
            }
            _ => false,
        };
        if should_clone {
            // D-CAP2 (D-MEM1/S4): same node `copy x` desugars to — one
            // mechanism for "duplicate this value".
            let span = expr.span();
            let old = std::mem::replace(expr, Expr::Absent(span));
            *expr = Expr::Copy(Box::new(old), span);
        }
    }

    pub(crate) fn check_enum_lit(
        &mut self,
        type_name: &str,
        variant: &str,
        args: &mut [EnumLitArg],
        span: Span,
    ) -> Type {
        let contextual_ty = self.expected_type.clone().filter(|ty| {
            matches!(ty, Type::Apply { name, .. } if name == type_name)
        });
        let ty = contextual_ty.clone().unwrap_or_else(|| Type::Named(type_name.to_string()));
        let Some(variants) = self.resolve_enum_variants_cloned(type_name) else {
            self.diags.push(Diagnostic::error(
                "E0119",
                format!("there's no enum called `{}`", type_name),
                "enum literals need an enum type name".to_string(),
                "define the enum first, or check the spelling".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                match a {
                    EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => {
                        self.infer(e);
                    }
                }
            }
            return ty;
        };
        let Some((_, payload)) = variants.get(variant) else {
            // D-TAG1: a group name in value position — a value is always a leaf.
            if let Some((_, leaves)) = self
                .resolve_enum_groups_cloned(type_name)
                .and_then(|g| g.get(variant).cloned())
            {
                self.diags.push(Diagnostic::error(
                    "E0332",
                    format!("`{}` is a group, not a value", variant),
                    "a group names its whole subtree in patterns; an actual value is always a leaf variant (D-TAG1)".to_string(),
                    format!(
                        "pick a leaf: {}",
                        leaves
                            .iter()
                            .map(|l| format!("`{type_name}.{l}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    match a {
                        EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => {
                            self.infer(e);
                        }
                    }
                }
                return ty;
            }
            let mut fix = "check the variant name".to_string();
            if let Some(s) = suggest_field(variant, &variants.keys().cloned().collect::<Vec<_>>()) {
                fix = format!("did you mean `{}`?", s);
            }
            self.diags.push(Diagnostic::error(
                "E0304",
                format!("`{}` has no variant `{}`", type_name, variant),
                "enum literals must name a variant on the type".to_string(),
                fix,
                Some(span),
            ));
            for a in args.iter_mut() {
                match a {
                    EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => {
                        self.infer(e);
                    }
                }
            }
            return ty;
        };
        match payload {
            VariantPayload::Unit => {
                if !args.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0303",
                        format!("variant `{}` takes no payload", variant),
                        "unit variants are written without parentheses".to_string(),
                        format!("write `{type_name}.{variant}` with no `(...)`"),
                        Some(span),
                    ));
                }
            }
            VariantPayload::Single(expected, _) => {
                let contextual_payload = match (&contextual_ty, type_name, variant) {
                    (Some(Type::Apply { args, .. }), "HookDecision" | "HookOutcome", "Transform" | "Continue") => args.first(),
                    (Some(Type::Apply { args, .. }), "HookDecision" | "HookOutcome", "Fail") => args.get(1),
                    _ => None,
                };
                let expected = contextual_payload.unwrap_or(expected);
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0303",
                        format!("variant `{}` expects one value", variant),
                        "single-payload variants take one positional argument (S30)".to_string(),
                        format!("write `{type_name}.{variant}(...)`"),
                        Some(span),
                    ));
                }
                if let Some(EnumLitArg::Positional(e)) = args.first_mut() {
                    let et = self.infer(e);
                    if let Some(et) = &et {
                        if contextual_payload.is_some() && et != expected {
                            self.diags.push(Diagnostic::error(
                                "E0108",
                                format!("`{type_name}.{variant}` needs {}, not {}", expected.show(), et.show()),
                                "the decision payload must match the hook's declared payload or error type".to_string(),
                                type_fix_hint(expected, et),
                                Some(e.span()),
                            ));
                        } else {
                            self.check_type_assignable(expected, et, e.span());
                        }
                    }
                    // D-EPPAYLOAD1 (I2 fix): same owning-position clone-insertion
                    // as a struct-lit field value — see `clone_borrowed_struct_field_value`.
                    // An enum payload is an owning slot exactly like a struct field;
                    // without this, an owned local moved here and read afterward
                    // reached rustc as a raw, unreported E0382.
                    self.clone_borrowed_struct_field_value(e, et.as_ref());
                } else if let Some(EnumLitArg::Named { label, .. }) = args.first() {
                    self.diags.push(Diagnostic::error(
                        "E0303",
                        format!(
                            "variant `{}` expects a positional value, not `{}:`",
                            variant, label
                        ),
                        "single-payload variants use positional args only (S30)".to_string(),
                        format!("write `{type_name}.{variant}(value)`"),
                        Some(span),
                    ));
                }
            }
            VariantPayload::Named(fields) => {
                let mut seen = HashSet::new();
                for a in args.iter_mut() {
                    match a {
                        EnumLitArg::Positional(_) => {
                            self.diags.push(Diagnostic::error(
                                "E0303",
                                format!("variant `{}` requires labeled fields", variant),
                                "named-payload variants construct with the dot-brace form \
                                 (D-UITREE1/D-DOTCTOR1), matching struct construction"
                                    .to_string(),
                                format!("write `{type_name}.{variant}.{{ w: 1.0, h: 2.0 }}`"),
                                Some(span),
                            ));
                        }
                        EnumLitArg::Named { label, expr } => {
                            if !seen.insert(label.clone()) {
                                self.diags.push(Diagnostic::error(
                                    "E0303",
                                    format!("field `{}` appears more than once", label),
                                    "each payload field may be written only once".to_string(),
                                    "remove the duplicate label".to_string(),
                                    Some(expr.span()),
                                ));
                            }
                            let et = self.infer(expr);
                            // D-EPPAYLOAD1 (I2 fix): see the positional-payload call above.
                            self.clone_borrowed_struct_field_value(expr, et.as_ref());
                            if let Some(f) = fields.iter().find(|f| f.name == *label) {
                                if let Some(et) = et {
                                    self.check_type_assignable(&f.ty, &et, expr.span());
                                }
                            } else {
                                self.diags.push(Diagnostic::error(
                                    "E0302",
                                    format!("variant `{}` has no field `{}`", variant, label),
                                    "check the field names on this variant".to_string(),
                                    suggest_field(
                                        label,
                                        &fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
                                    )
                                    .map(|s| format!("did you mean `{}`?", s))
                                    .unwrap_or_else(|| "remove this label".to_string()),
                                    Some(expr.span()),
                                ));
                            }
                        }
                    }
                }
                let missing: Vec<_> = fields
                    .iter()
                    .filter(|f| !seen.contains(&f.name))
                    .map(|f| f.name.clone())
                    .collect();
                if !missing.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0303",
                        format!(
                            "variant `{}` is missing fields: {}",
                            variant,
                            missing.join(", ")
                        ),
                        "every payload field must appear exactly once".to_string(),
                        format!("add: {}", missing.join(", ")),
                        Some(span),
                    ));
                }
            }
        }
        ty
    }

    /// S31 / D-ENUMDOT1: bare `subject == Red` for a unit variant is E0367 —
    /// patterns need a leading `.`. Still return the pattern so recovery can
    /// typecheck the rest of the table.
    pub(crate) fn eq_unit_variant_pattern(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        subject_name: Option<&str>,
        subj_ty: &Type,
    ) -> Option<Pattern> {
        if !subject_name.is_some_and(|n| expr_is_same_ident(lhs, n)) {
            return None;
        }
        let Expr::Ident(variant, rhs_span) = rhs else {
            return None;
        };
        if self.lookup(variant).is_some() || self.consts.contains_key(variant) {
            return None;
        }
        let enum_name = match subj_ty {
            Type::Named(name) | Type::Apply { name, .. } => name,
            _ => return None,
        };
        let variant_known = self
            .resolve_enum_variants_cloned(enum_name)
            .is_some_and(|variants| variants.contains_key(variant))
            // D-TAG1: a group name is pattern-shaped too (matches its subtree).
            || self
                .resolve_enum_groups_cloned(enum_name)
                .is_some_and(|groups| groups.contains_key(variant));
        if !variant_known {
            return None;
        }
        self.diags.push(Diagnostic::error(
            "E0367",
            format!("pattern `{variant}` needs a leading `.`"),
            "match patterns take a leading dot so the name isn't read as a variable or call (D-ENUMDOT1)"
                .to_string(),
            format!("write `.{variant}`"),
            Some(*rhs_span),
        ));
        Some(Pattern::Variant {
            variant: variant.clone(),
            bindings: Vec::new(),
            leading_dot: false,
            span: *rhs_span,
        })
    }

    /// S31: pattern carried as `PatternTest` or as bare `subject == UnitVariant`
    /// (the latter is accepted only to emit E0367 and recover).
    pub(crate) fn switch_arm_pattern(
        &mut self,
        cond: &Expr,
        subject_name: Option<&str>,
        subj_ty: &Type,
    ) -> Option<Pattern> {
        match cond {
            // D-PARSESTR1 / D-BINPAT1: a str-match or binary pattern is never
            // provably exhaustive (fixed text/bytes might not match; a subject
            // might be too short), so it must never join the `all_pattern`
            // provable-coverage path — it always needs its own E0148 (missing
            // `else`) check instead.
            Expr::PatternTest {
                pattern: Pattern::StrMatch { .. } | Pattern::BinMatch { .. },
                ..
            } => None,
            Expr::PatternTest {
                subject, pattern, ..
            } => {
                if subject_name.is_some_and(|n| expr_is_same_ident(subject, n)) {
                    Some(pattern.clone())
                } else {
                    None
                }
            }
            Expr::Binary(BinOp::Eq, lhs, rhs, _) => {
                self.eq_unit_variant_pattern(lhs, rhs, subject_name, subj_ty)
            }
            // D-ENUMDOT1: bare unit / call-shaped arm heads are the old S31
            // spelling. Accept only to emit E0367 and recover — canonical form
            // is `.Variant` / `.Variant(binding)`.
            Expr::Ident(variant, span) if subject_name.is_some() => {
                let enum_name = match subj_ty {
                    Type::Named(name) | Type::Apply { name, .. } => name,
                    _ => return None,
                };
                let variants = self.resolve_enum_variants_cloned(enum_name)?;
                // D-TAG1: a bare group name is a unit-shaped subtree pattern.
                let is_group = self
                    .resolve_enum_groups_cloned(enum_name)
                    .is_some_and(|g| g.contains_key(variant.as_str()));
                if !is_group {
                    let (_, payload) = variants.get(variant.as_str())?;
                    if !matches!(payload, crate::AST::VariantPayload::Unit) {
                        return None;
                    }
                }
                // Only accept if the variant is NOT a live local (avoids shadowing).
                if self.lookup(variant).is_some() {
                    return None;
                }
                self.diags.push(Diagnostic::error(
                    "E0367",
                    format!("pattern `{variant}` needs a leading `.`"),
                    "match patterns take a leading dot so the name isn't read as a variable or call (D-ENUMDOT1)"
                        .to_string(),
                    format!("write `.{variant}`"),
                    Some(*span),
                ));
                Some(Pattern::Variant {
                    variant: variant.clone(),
                    bindings: vec![],
                    leading_dot: false,
                    span: *span,
                })
            }
            Expr::Call(call) if subject_name.is_some() => {
                // Only a single positional binding arg is the pattern form.
                if call.args.len() != 1 {
                    return None;
                }
                let Expr::Ident(binding, binding_span) = &call.args[0].expr else {
                    return None;
                };
                if self.lookup(binding).is_some() {
                    return None;
                } // it's a real local
                let enum_name = match subj_ty {
                    Type::Named(name) | Type::Apply { name, .. } => name,
                    Type::Union(members)
                        if members
                            .iter()
                            .any(|m| crate::AST::union_member_tag(m) == call.name) =>
                    {
                        self.diags.push(Diagnostic::error(
                            "E0367",
                            format!("pattern `{}` needs a leading `.`", call.name),
                            "match patterns take a leading dot so the name isn't read as a variable or call (D-ENUMDOT1)"
                                .to_string(),
                            format!("write `.{}(…)`", call.name),
                            Some(call.name_span),
                        ));
                        return Some(Pattern::Variant {
                            variant: call.name.clone(),
                            bindings: vec![crate::AST::PatSlot::Bind {
                                name: binding.clone(),
                                span: *binding_span,
                            }],
                            leading_dot: false,
                            span: call.name_span,
                        });
                    }
                    _ => return None,
                };
                let variants = self.resolve_enum_variants_cloned(enum_name)?;
                let (_, payload) = variants.get(call.name.as_str())?;
                if !matches!(payload, crate::AST::VariantPayload::Single(..)) {
                    return None;
                }
                self.diags.push(Diagnostic::error(
                    "E0367",
                    format!("pattern `{}` needs a leading `.`", call.name),
                    "match patterns take a leading dot so the name isn't read as a variable or call (D-ENUMDOT1)"
                        .to_string(),
                    format!("write `.{}(…)`", call.name),
                    Some(call.name_span),
                ));
                Some(Pattern::Variant {
                    variant: call.name.clone(),
                    bindings: vec![crate::AST::PatSlot::Bind {
                        name: binding.clone(),
                        span: *binding_span,
                    }],
                    leading_dot: false,
                    span: call.name_span,
                })
            }
            _ => None,
        }
    }

    pub(crate) fn check_pattern_test(
        &mut self,
        subject: &mut Box<Expr>,
        pattern: &mut Pattern,
        span: Span,
    ) -> HashMap<String, Type> {
        let subj_ty = self.infer(subject);
        let Some(st) = subj_ty else {
            return HashMap::new();
        };
        super::CheckerCore::normalize_contextual_pattern(pattern, &st);
        let bindings = self.validate_pattern(&st, pattern, span);
        if !matches!(pattern, Pattern::Struct { .. }) {
            self.mark_pattern_subject_moved(subject, &bindings);
        }
        bindings
    }

    /// Binding a non-copy payload out of a pattern gives the subject away in
    /// the generated Rust (`if let` / `matches!` move the place), so the old
    /// name must stop being usable — otherwise rustc rejects the output (I2).
    pub(crate) fn mark_pattern_subject_moved(
        &mut self,
        subject: &Expr,
        bindings: &HashMap<String, Type>,
    ) {
        if bindings.values().all(type_is_copy) {
            return;
        }
        if let Expr::Ident(name, name_span) = subject {
            if self.is_borrowed_binding(name)
                && self
                    .lookup(name)
                    .is_some_and(|info| !is_cloneable(&info.ty, self.registry))
            {
                if self
                    .diags
                    .iter()
                    .any(|diag| diag.code == "E0120" && diag.span == Some(*name_span))
                {
                    return;
                }
                let ty = self
                    .lookup(name)
                    .map(|info| info.ty.show().trim_matches('`').to_string())
                    .unwrap_or_else(|| "value".to_string());
                self.diags.push(Diagnostic::error(
                    "E0120",
                    format!("`{name}` was not moved here, so its contents cannot be taken apart"),
                    format!(
                        "`{name}` grants read-only access to `{ty}`, so its non-copyable parts cannot be taken apart with the move-capability marker `^`"
                    ),
                    format!(
                        "change the parameter to `{name}: {}{ty}` with the move-capability marker `^`, or inspect it without binding its contents",
                        Syntax::SIGIL_MOVE
                    ),
                    Some(*name_span),
                ));
                return;
            }
        }
        if self.transfer_mutable_pattern_subject(subject) {
            return;
        }
        if let Expr::Ident(n, nspan) = subject {
            if n != Syntax::KW_IT && self.lookup(n).is_some() {
                self.mark_moved(n.clone(), *nspan);
            }
        }
    }

    /// D-PARSESTR1 (shared with D-SHIFT1's `take_pattern` — I8, one hole-type
    /// rule, not two): the bound type of a str-match pattern hole. `None`
    /// binds `String`; a typed hole must be `Int`/`Float`/`Bool`/`String` —
    /// the only four types that can read out of matched text — else E0305.
    pub(crate) fn str_match_hole_type(
        &mut self,
        name: &str,
        ty: &Option<Type>,
        hole_span: Span,
    ) -> Type {
        match ty {
            None => Type::String,
            Some(t @ (Type::Int | Type::Float | Type::Bool | Type::String)) => t.clone(),
            Some(other) => {
                self.diags.push(Diagnostic::error(
                    "E0305",
                    format!(
                        "`{{{}:{}}}` can't read text as {} — only `Int`, `Float`, `Bool`, or `String` can come out of a pattern hole",
                        name,
                        other.show(),
                        other.show()
                    ),
                    "a typed hole reads the matched text into that type, and only these four types know how to read text".to_string(),
                    "use `Int`, `Float`, `Bool`, or `String`, or drop the type to bind `String`".to_string(),
                    Some(hole_span),
                ));
                Type::String
            }
        }
    }

    /// D-BINPAT1 (shared with card #506 follow-up's `take_pattern` — I8, one
    /// hole-type rule, not two): walk a binary pattern's parts, folding the
    /// running bit offset and pushing alignment/endian diagnostics (E1008,
    /// E1011), and return the ORDERED `(name, Type)` list for every hole
    /// (literal parts contribute no entry). Order matches source order —
    /// `validate_pattern`'s arm-head bindings collect this into a
    /// name-keyed `HashMap` (binding order doesn't matter for a switch arm);
    /// `take_pattern` needs the order preserved, since it becomes a
    /// canonical result tuple, so it keeps the `Vec` as-is.
    pub(crate) fn bin_match_hole_types(
        &mut self,
        parts: &[crate::AST::BinMatchPart],
        span: Span,
    ) -> Vec<(String, Type)> {
        let mut off: usize = 0;
        let mut holes: Vec<(String, Type)> = Vec::new();
        for part in parts {
            match part {
                crate::AST::BinMatchPart::Lit(bytes) => {
                    if off % 8 != 0 {
                        self.diags.push(self.bin_align_diag(span));
                    }
                    off += bytes.len() * 8;
                }
                crate::AST::BinMatchPart::Hole { name, spec, span: hole_span } => {
                    match spec {
                        crate::AST::BinSpec::Rest => {
                            if off % 8 != 0 {
                                self.diags.push(self.bin_align_diag(*hole_span));
                            }
                            holes.push((
                                name.clone(),
                                Type::List(Box::new(Type::IntN { signed: false, bits: 8 })),
                            ));
                        }
                        crate::AST::BinSpec::Bits { width, endian } => {
                            if matches!(endian, crate::AST::BinEndian::Little) && *width % 8 != 0
                            {
                                self.diags.push(Diagnostic::error(
                                    "E1008",
                                    format!(
                                        "a little-endian read `U{width}le` must be a whole number of bytes"
                                    ),
                                    "byte order only reorders whole bytes, so a `le` read's width must be a multiple of 8"
                                        .to_string(),
                                    "use a multiple-of-8 width (`U16le`, `U32le`) or big-endian `be`".to_string(),
                                    Some(*hole_span),
                                ));
                            }
                            holes.push((name.clone(), bin_bits_type(*width)));
                            off += *width as usize;
                        }
                    }
                }
            }
        }
        holes
    }

    /// D-BINPAT1: a fixed byte literal or a rest capture must start on a byte
    /// boundary — the preceding bit-typed holes must sum to a multiple of 8.
    fn bin_align_diag(&self, span: Span) -> Diagnostic {
        Diagnostic::error(
            "E1011",
            "this part of the binary pattern doesn't start on a byte boundary".to_string(),
            "fixed bytes and a `{rest:...}` capture are read whole-byte, so every bit-field before them must add up to a multiple of 8 bits"
                .to_string(),
            "pad the preceding holes to a byte boundary (e.g. add a reserved `{_:U4}`)".to_string(),
            Some(span),
        )
    }

    pub(crate) fn validate_pattern(
        &mut self,
        subject_ty: &Type,
        pattern: &Pattern,
        span: Span,
    ) -> HashMap<String, Type> {
        match (subject_ty, pattern) {
            (Type::Option(inner), Pattern::Present { binding, .. }) => {
                let mut map = HashMap::new();
                map.insert(binding.clone(), (**inner).clone());
                map
            }
            (Type::Option(_), Pattern::Absent(_)) => HashMap::new(),
            (Type::Result { ok, .. }, Pattern::Ok { binding, .. }) => {
                let mut map = HashMap::new();
                map.insert(binding.clone(), (**ok).clone());
                map
            }
            (Type::Result { err, .. }, Pattern::Err { binding, .. }) => {
                let mut map = HashMap::new();
                map.insert(binding.clone(), (**err).clone());
                map
            }
            (Type::Result { .. }, Pattern::Present { .. } | Pattern::Absent(_)) => {
                self.diags.push(Diagnostic::error(
                    "E0305",
                    format!(
                        "this pattern belongs to an optional value, not {}",
                        subject_ty.name()
                    ),
                    "use `== .Ok(...)` or `== .Err(...)` on a fallible result".to_string(),
                    format!(
                        "write `== .{}(...)` or `== .{}(...)` instead",
                        Syntax::LIT_OK,
                        Syntax::LIT_ERR
                    ),
                    Some(span),
                ));
                HashMap::new()
            }
            // D-UNIONTYPE1=A: arm head is the member type name; binds that member.
            (
                Type::Union(members),
                Pattern::Variant {
                    variant,
                    bindings,
                    ..
                },
            ) => {
                let Some(member_ty) = members
                    .iter()
                    .find(|m| crate::AST::union_member_tag(m) == *variant)
                    .cloned()
                else {
                    self.diags.push(Diagnostic::error(
                        "E0305",
                        format!(
                            "pattern `{}` doesn't belong to `{}`",
                            variant,
                            subject_ty.name()
                        ),
                        "union match arms name a member type of the subject's union".to_string(),
                        format!(
                            "members are {}",
                            members
                                .iter()
                                .map(crate::AST::union_member_tag)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        Some(span),
                    ));
                    return HashMap::new();
                };
                if bindings.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0306",
                        format!(
                            "pattern `{}` expects 1 binding, got {}",
                            variant,
                            bindings.len()
                        ),
                        "each union member arm binds the member value once".to_string(),
                        format!("write `.{}(v)`", variant),
                        Some(span),
                    ));
                }
                let mut result = HashMap::new();
                if let Some(crate::AST::PatSlot::Bind { name, .. }) = bindings.first() {
                    result.insert(name.clone(), member_ty);
                }
                result
            }
            (
                Type::Named(type_name)
                | Type::Apply {
                    name: type_name, ..
                },
                Pattern::Struct { fields, rest, .. },
            ) => {
                let (import_ns, lookup_name) = self.struct_type_name_parts(type_name);
                let all_fields: Option<Vec<String>> = self
                    .struct_owner_module(lookup_name, import_ns)
                    .and_then(|m| self.struct_fields_of(m, lookup_name))
                    .map(|fs| fs.iter().map(|(name, ..)| name.clone()).collect());
                let Some(all_fields) = all_fields else {
                    self.diags.push(Diagnostic::error(
                        "E0313",
                        format!(
                            "`{{ … }}` can only match a struct value, but this is {}",
                            subject_ty.show()
                        ),
                        "a struct pattern reads named fields from a struct value".to_string(),
                        "match a struct value, or use an enum/optional pattern instead".to_string(),
                        Some(span),
                    ));
                    return HashMap::new();
                };
                let mut named = HashSet::new();
                let mut result = HashMap::new();
                for f in fields {
                    let field_name = f.field_name();
                    named.insert(field_name.to_string());
                    let fty = self
                        .field_type(subject_ty, field_name, f.field_span())
                        .unwrap_or(Type::Int);
                    match f {
                        StructPatField::Bind {
                            local, local_span, ..
                        } => {
                            result.insert(local.clone(), fty);
                            let _ = local_span;
                        }
                        StructPatField::Value { value, .. } => {
                            let mut value = (**value).clone();
                            let saved_expected = self.expected_type.clone();
                            self.expected_type = Some(fty.clone());
                            let got = self.infer(&mut value);
                            self.expected_type = saved_expected;
                            if let Some(got) = got {
                                let reported = self.check_type_assignable(&fty, &got, value.span());
                                if !reported && got != fty {
                                    self.diags.push(Diagnostic::error(
                                        "E0108",
                                        format!(
                                            "field `{}` is {}, but this pattern value is {}",
                                            field_name,
                                            fty.show(),
                                            got.show()
                                        ),
                                        "a field pattern value must have the field's type"
                                            .to_string(),
                                        type_fix_hint(&fty, &got),
                                        Some(value.span()),
                                    ));
                                }
                            }
                        }
                    }
                }
                if rest.is_none() && named.len() < all_fields.len() {
                    self.diags.push(Diagnostic::error(
                        "E0326",
                        format!("this pattern leaves out fields of `{}`", type_name),
                        "a destructure that doesn't name every field must end with `..` so the skipped fields are visible at a glance".to_string(),
                        "add `, ..` before the closing `}`, or name the remaining fields".to_string(),
                        Some(span),
                    ));
                } else if let Some(rest_span) = rest {
                    if named.len() >= all_fields.len() && !all_fields.is_empty() {
                        self.diags.push(Diagnostic::error(
                            "E0327",
                            "this `..` is redundant".to_string(),
                            format!("the pattern already names every field of `{}`", type_name),
                            "remove `..` or leave out at least one field".to_string(),
                            Some(*rest_span),
                        ));
                    }
                }
                result
            }
            (_, Pattern::Struct { .. }) => {
                self.diags.push(Diagnostic::error(
                    "E0313",
                    format!(
                        "`{{ … }}` can only match a struct value, but this is {}",
                        subject_ty.show()
                    ),
                    "a struct pattern reads named fields from a struct value".to_string(),
                    "match a struct value, or use an enum/optional pattern instead".to_string(),
                    Some(span),
                ));
                HashMap::new()
            }
            (
                Type::Apply { name: enum_name, args },
                Pattern::Variant { variant, bindings, .. },
            ) if matches!(enum_name.as_str(), "HookDecision" | "HookOutcome") => {
                let payload_ty = match (enum_name.as_str(), variant.as_str()) {
                    ("HookDecision", "Continue" | "Cancel") | ("HookOutcome", "Cancel") => None,
                    ("HookDecision", "Transform") | ("HookOutcome", "Continue") => args.first().cloned(),
                    ("HookDecision" | "HookOutcome", "Fail") => args.get(1).cloned(),
                    _ => {
                        self.diags.push(Diagnostic::error(
                            "E0305",
                            format!("pattern `{variant}` doesn't belong to `{enum_name}`"),
                            "pattern tests must name a variant on the value's enum type".to_string(),
                            "check the variant spelling".to_string(),
                            Some(span),
                        ));
                        return HashMap::new();
                    }
                };
                let expected_count = usize::from(payload_ty.is_some());
                if bindings.len() != expected_count {
                    self.diags.push(Diagnostic::error(
                        "E0306",
                        format!("pattern `{variant}` expects {expected_count} binding{}, got {}", if expected_count == 1 { "" } else { "s" }, bindings.len()),
                        "each payload field needs its own binding name".to_string(),
                        if expected_count == 1 { format!("write `{variant}(value)`") } else { format!("write `{variant}`") },
                        Some(span),
                    ));
                }
                let mut result = HashMap::new();
                if let (Some(ty), Some(crate::AST::PatSlot::Bind { name, .. })) = (payload_ty, bindings.first()) {
                    result.insert(name.clone(), ty);
                }
                result
            }
            (
                Type::Named(enum_name),
                Pattern::Variant {
                    variant, bindings, ..
                },
            ) => {
                if is_json_type_name(enum_name) {
                    let Some(expected) = core_json_pattern_types(variant) else {
                        self.diags.push(Diagnostic::error(
                            "E0305",
                            format!(
                                "pattern `{}` doesn't belong to `{}`",
                                variant,
                                Syntax::TYPE_DATA
                            ),
                            "pattern tests must name a variant on the value's enum type"
                                .to_string(),
                            "the `Data` variants are Null/Bool/Int/Float/Text/Array/Object"
                                .to_string(),
                            Some(span),
                        ));
                        return HashMap::new();
                    };
                    if bindings.len() != expected.len() {
                        self.diags.push(Diagnostic::error(
                            "E0306",
                            format!(
                                "pattern `{}` expects {} binding{}, got {}",
                                variant,
                                expected.len(),
                                if expected.len() == 1 { "" } else { "s" },
                                bindings.len()
                            ),
                            "each payload field needs its own binding name".to_string(),
                            format!(
                                "write `{}({})",
                                variant,
                                (0..expected.len())
                                    .map(|i| format!("v{i}"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            Some(span),
                        ));
                    }
                    // JSON pattern: only Bind slots generate bindings; wildcard/range are invalid here.
                    let mut result = HashMap::new();
                    for (slot, ty) in bindings.iter().zip(expected.iter()) {
                        match slot {
                            crate::AST::PatSlot::Bind { name, .. } => {
                                result.insert(name.clone(), ty.clone());
                            }
                            crate::AST::PatSlot::Wildcard => {}
                            crate::AST::PatSlot::Range { .. } => {
                                self.diags.push(Diagnostic::error(
                                    "E0316",
                                    "range patterns are not supported in JSON variant payloads".to_string(),
                                    "JSON variants have flexible payload types; use a binding and check at runtime".to_string(),
                                    "write a name like `n` instead of a range".to_string(),
                                    Some(span),
                                ));
                            }
                        }
                    }
                    return result;
                }
                // D-TERM1 (ratified 2026-06-22): `Key` is a core enum — resolve
                // its variants without a user-defined type registry entry.
                if enum_name == crate::Syntax::TYPE_KEY {
                    let Some(expected) = core_key_pattern_types(variant) else {
                        self.diags.push(Diagnostic::error(
                            "E0305",
                            format!(
                                "pattern `{}` doesn't belong to `Key`",
                                variant
                            ),
                            "pattern tests must name a variant on the value's enum type".to_string(),
                            "valid `Key` variants: `Char(c)`, `Enter`, `Escape`, `Backspace`, `Tab`, `Delete`, `Up`, `Down`, `Left`, `Right`, `F(n)`, `Ctrl(c)`, `Unknown`".to_string(),
                            Some(span),
                        ));
                        return HashMap::new();
                    };
                    if bindings.len() != expected.len() {
                        self.diags.push(Diagnostic::error(
                            "E0306",
                            format!(
                                "pattern `{}` expects {} binding{}, got {}",
                                variant,
                                expected.len(),
                                if expected.len() == 1 { "" } else { "s" },
                                bindings.len()
                            ),
                            "each payload field needs its own binding name".to_string(),
                            format!(
                                "write `{}({})",
                                variant,
                                (0..expected.len())
                                    .map(|i| format!("v{i}"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            Some(span),
                        ));
                    }
                    let mut result = HashMap::new();
                    for (slot, ty) in bindings.iter().zip(expected.iter()) {
                        match slot {
                            crate::AST::PatSlot::Bind { name, .. } => {
                                result.insert(name.clone(), ty.clone());
                            }
                            crate::AST::PatSlot::Wildcard => {}
                            crate::AST::PatSlot::Range { .. } => {
                                self.diags.push(Diagnostic::error(
                                    "E0316",
                                    "range patterns are not supported in `Key` variant payloads".to_string(),
                                    "`Key` payload types are `Char` and `Int`; use a binding and check at runtime".to_string(),
                                    "write a name like `c` instead of a range".to_string(),
                                    Some(span),
                                ));
                            }
                        }
                    }
                    return result;
                }
                let Some(variants) = self.resolve_enum_variants_cloned(enum_name) else {
                    self.diags.push(Diagnostic::error(
                        "E0305",
                        format!("pattern `{}` doesn't match this value's type", variant),
                        format!("`{}` is a struct, not an enum", enum_name),
                        "use a struct field access instead of a variant pattern".to_string(),
                        Some(span),
                    ));
                    return HashMap::new();
                };
                let Some((_, payload)) = variants.get(variant) else {
                    // D-TAG1: a group name matches its whole subtree — a unit-shaped
                    // pattern with no bindings (groups carry no payload, E0331).
                    if self
                        .resolve_enum_groups_cloned(enum_name)
                        .is_some_and(|g| g.contains_key(variant))
                    {
                        if !bindings.is_empty() {
                            self.diags.push(Diagnostic::error(
                                "E0306",
                                format!(
                                    "pattern `{}` expects 0 bindings, got {}",
                                    variant,
                                    bindings.len()
                                ),
                                "a group has no payload of its own — payloads live on leaf variants (D-TAG1)".to_string(),
                                format!("write `.{}` with no `(...)`, or match a specific leaf to bind its payload", variant),
                                Some(span),
                            ));
                        }
                        return HashMap::new();
                    }
                    self.diags.push(Diagnostic::error(
                        "E0305",
                        format!("pattern `{}` doesn't belong to `{}`", variant, enum_name),
                        "pattern tests must name a variant on the value's enum type".to_string(),
                        "check the variant spelling".to_string(),
                        Some(span),
                    ));
                    return HashMap::new();
                };
                let expected = pattern_binding_types(payload);
                if bindings.len() != expected.len() {
                    self.diags.push(Diagnostic::error(
                        "E0306",
                        format!(
                            "pattern `{}` expects {} binding{}, got {}",
                            variant,
                            expected.len(),
                            if expected.len() == 1 { "" } else { "s" },
                            bindings.len()
                        ),
                        "each payload field needs its own binding name".to_string(),
                        format!(
                            "write `{}({})",
                            variant,
                            (0..expected.len())
                                .map(|i| format!("v{i}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        Some(span),
                    ));
                }
                // Build bindings: Bind(name) → insert; Wildcard → skip; Range → validate type + skip.
                let mut result = HashMap::new();
                for (slot, ty) in bindings.iter().zip(expected.iter()) {
                    match slot {
                        crate::AST::PatSlot::Bind { name, .. } => {
                            result.insert(name.clone(), ty.clone());
                        }
                        crate::AST::PatSlot::Wildcard => {}
                        crate::AST::PatSlot::Range { lo, hi } => {
                            // D-PATR: field must be Int or Char; lo must be <= hi.
                            if !matches!(ty, Type::Int | Type::Char) {
                                self.diags.push(Diagnostic::error(
                                    "E0316",
                                    format!(
                                        "range pattern `{}..{}` used on a non-integer field (type `{}`)",
                                        lo, hi, ty.show()
                                    ),
                                    "range patterns only work on `Int` or `Char` payload fields".to_string(),
                                    "use a binding name and add a condition in the arm body instead".to_string(),
                                    Some(span),
                                ));
                            } else if lo > hi {
                                self.diags.push(Diagnostic::error(
                                    "E0316",
                                    format!("range pattern `{}..{}` is empty — lower bound exceeds upper bound", lo, hi),
                                    "the lower end of a range must be ≤ the upper end".to_string(),
                                    format!("swap to `{}..{}`", hi, lo),
                                    Some(span),
                                ));
                            }
                        }
                    }
                }
                result
            }
            (_, Pattern::Variant { variant, .. }) => {
                self.diags.push(Diagnostic::error(
                    "E0305",
                    format!("pattern `{}` doesn't match {}", variant, subject_ty.show()),
                    "variant patterns only work on enum values".to_string(),
                    format!(
                        "test an enum value, or use `{}` / `{}` for optionals",
                        Syntax::LIT_VALUE,
                        Syntax::LIT_NULL
                    ),
                    Some(span),
                ));
                HashMap::new()
            }
            (Type::Named(_), Pattern::Present { .. } | Pattern::Absent(_)) => {
                self.diags.push(Diagnostic::error(
                    "E0305",
                    "this pattern doesn't match the value's type".to_string(),
                    format!(
                        "`{}` / `{}` patterns work on `T?` values only",
                        Syntax::LIT_VALUE,
                        Syntax::LIT_NULL
                    ),
                    "use a variant pattern for enum values".to_string(),
                    Some(span),
                ));
                HashMap::new()
            }
            (_, Pattern::Ok { .. } | Pattern::Err { .. }) => {
                self.diags.push(Diagnostic::error(
                    "E0305",
                    format!(
                        "this pattern belongs to a fallible result, not {}",
                        subject_ty.name()
                    ),
                    format!(
                        "use `== {}(...)` or `== {}(...)` on `T ? E`",
                        Syntax::LIT_OK,
                        Syntax::LIT_ERR
                    ),
                    "check the type of the value being tested".to_string(),
                    Some(span),
                ));
                HashMap::new()
            }
            // D-PATR (ratified 2026-06-19): arm-head range pattern.
            (_, Pattern::Range { lo, hi, .. }) => {
                if !matches!(subject_ty, Type::Int | Type::Char) {
                    self.diags.push(Diagnostic::error(
                        "E0316",
                        format!(
                            "range pattern `{}..{}` used on `{}` — only `Int` or `Char` values support range patterns",
                            lo, hi, subject_ty.show()
                        ),
                        "range arms match a subject that is an integer or character".to_string(),
                        "use a regular condition (`subject >= lo && subject <= hi`) for other types".to_string(),
                        Some(span),
                    ));
                } else if lo > hi {
                    self.diags.push(Diagnostic::error(
                        "E0316",
                        format!(
                            "range pattern `{}..{}` is empty — lower bound exceeds upper bound",
                            lo, hi
                        ),
                        "the lower end of a range must be ≤ the upper end".to_string(),
                        format!("swap to `{}..{}`", hi, lo),
                        Some(span),
                    ));
                }
                HashMap::new() // range patterns bind nothing at arm-head level
            }
            // D-PARSESTR1: the same interpolation literal that formats a
            // string can sit in pattern position — matches the fixed text
            // and binds each hole. Always refutable; E0148 (missing `else`)
            // is enforced in `check_switch`, not here.
            (_, Pattern::StrMatch { parts, .. }) => {
                if !matches!(subject_ty, Type::String) {
                    self.diags.push(Diagnostic::error(
                        "E0305",
                        format!(
                            "this pattern matches text, but the subject is {}",
                            subject_ty.show()
                        ),
                        "a string-interpolation pattern only matches a `String` value".to_string(),
                        "match a `String` subject, or use a pattern for this type instead"
                            .to_string(),
                        Some(span),
                    ));
                    return HashMap::new();
                }
                let mut result = HashMap::new();
                for part in parts {
                    let StrMatchPart::Hole {
                        name,
                        ty,
                        span: hole_span,
                    } = part
                    else {
                        continue;
                    };
                    let bound_ty = self.str_match_hole_type(name, ty, *hole_span);
                    result.insert(name.clone(), bound_ty);
                }
                result
            }
            // D-BINPAT1 / D-UNIFYLIT1=A: `[U8].{"…"}` binary pattern matches a
            // `[U8]` subject. Each fixed-width hole binds an unsigned integer; a
            // `{rest:...}` hole binds `[U8]`. Always refutable; E0148 (missing
            // `else`) is enforced in `check_switch`.
            (_, Pattern::BinMatch { parts, .. }) => {
                let is_bytes = matches!(subject_ty, Type::List(elem)
                    if matches!(elem.as_ref(), Type::IntN { signed: false, bits: 8 }));
                if !is_bytes {
                    self.diags.push(Diagnostic::error(
                        "E1010",
                        format!(
                            "this pattern matches raw bytes, but the subject is {}",
                            subject_ty.show()
                        ),
                        "an `[U8].{\"…\"}` binary pattern only matches a `[U8]` byte buffer"
                            .to_string(),
                        "match a `[U8]` subject, or use a pattern for this type instead"
                            .to_string(),
                        Some(span),
                    ));
                    return HashMap::new();
                }
                // Static bit-offset fold: literals and the rest capture must be
                // byte-aligned, and a little-endian read must be byte-multiple.
                self.bin_match_hole_types(parts, span)
                    .into_iter()
                    .collect::<HashMap<_, _>>()
            }
            // D-PATO (ratified 2026-06-19): structural or-pattern `A(x) | B(x)`.
            (_, Pattern::Or(alts, _)) => {
                if alts.is_empty() {
                    return HashMap::new();
                }
                // Check each alternative and collect its bindings.
                let first_bindings = self.validate_pattern(subject_ty, &alts[0], span);
                for alt in &alts[1..] {
                    let alt_bindings = self.validate_pattern(subject_ty, alt, span);
                    // E0317: alternatives must bind the same names at the same types.
                    let names_match = first_bindings.len() == alt_bindings.len()
                        && first_bindings
                            .iter()
                            .all(|(k, v)| alt_bindings.get(k) == Some(v));
                    if !names_match {
                        self.diags.push(Diagnostic::error(
                            "E0317",
                            "or-pattern alternatives bind different names or types".to_string(),
                            "every arm of `A(x) | B(y)` must bind the same names at the same types"
                                .to_string(),
                            "rename bindings so both alternatives bind identical names".to_string(),
                            Some(span),
                        ));
                    }
                }
                first_bindings
            }
            _ => HashMap::new(),
        }
    }

    pub(crate) fn check_panic_call(&mut self, call: &mut Call) {
        if call.args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0103",
                format!("`{}` needs exactly one message", Syntax::BUILTIN_PANIC),
                "a panic report needs something to show the user".to_string(),
                format!("e.g. {}(\"something went wrong\")", Syntax::BUILTIN_PANIC),
                Some(call.name_span),
            ));
        }
        for arg in call.args.iter_mut() {
            self.borrow_ctx = true; // panic shows the message via `.jet_show()`
            let t = self.infer(&mut arg.expr);
            if let Some(t) = t {
                if t != Type::String {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` needs text, but this is {}",
                            Syntax::BUILTIN_PANIC,
                            t.show()
                        ),
                        "the panic message is shown to the user as text".to_string(),
                        "put the message in quotes".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
            }
        }
    }

    pub(crate) fn check_require_call(&mut self, call: &mut Call) {
        if call.args.is_empty() || call.args.len() > 2 {
            self.diags.push(Diagnostic::error(
                "E0103",
                format!(
                    "`{}` needs one condition, or a condition and a message",
                    Syntax::BUILTIN_REQUIRE
                ),
                "require checks a yes/no condition and stops when it's false".to_string(),
                format!(
                    "e.g. {}(x > 0) or {}(x > 0, \"x must be positive\")",
                    Syntax::BUILTIN_REQUIRE,
                    Syntax::BUILTIN_REQUIRE
                ),
                Some(call.name_span),
            ));
        }
        if let Some(arg) = call.args.first_mut() {
            let t = self.infer(&mut arg.expr);
            if let Some(t) = t {
                if t != Type::Bool {
                    self.diags.push(Diagnostic::error(
                        "E0110",
                        format!(
                            "`{}` needs {}, but this is {}",
                            Syntax::BUILTIN_REQUIRE,
                            Type::Bool.show(),
                            t.show()
                        ),
                        "the condition must be true or false".to_string(),
                        "compare values first, e.g. `x > 0`".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
            }
        }
        if let Some(arg) = call.args.get_mut(1) {
            let t = self.infer(&mut arg.expr);
            if let Some(t) = t {
                if t != Type::String {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` message must be text, but this is {}",
                            Syntax::BUILTIN_REQUIRE,
                            t.show()
                        ),
                        "the optional message is shown when the condition is false".to_string(),
                        "put the message in quotes".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
            }
        }
    }

    pub(crate) fn check_require_eq_call(&mut self, call: &mut Call) {
        if call.args.len() != 2 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`{}` needs exactly two values to compare",
                    Syntax::BUILTIN_REQUIRE_EQ
                ),
                "require_eq checks that two values are equal".to_string(),
                format!("e.g. {}(got, expected)", Syntax::BUILTIN_REQUIRE_EQ),
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return;
        }
        // require_eq compares and shows by reference in the generated Rust.
        self.borrow_ctx = true;
        let lt = self.infer(&mut call.args[0].expr);
        self.borrow_ctx = true;
        let rt = self.infer(&mut call.args[1].expr);
        match (lt, rt) {
            (Some(lt), Some(rt)) => {
                if lt != rt {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "`{}` compared {} and {}, which don't match",
                            Syntax::BUILTIN_REQUIRE_EQ,
                            lt.show(),
                            rt.show()
                        ),
                        "both sides must be the same type to compare them".to_string(),
                        "convert one side, or compare fields that have the same type".to_string(),
                        Some(call.name_span),
                    ));
                } else if !crate::Sema::Diagnostics::is_equatable(
                    &lt,
                    self.registry,
                    self.trait_reg,
                ) {
                    if let Some(field) = incomparable_field(&lt, self.registry) {
                        self.diags.push(Diagnostic::error(
                            "E0312",
                            format!(
                                "`{}` can't compare values of type `{}` (field `{}` isn't comparable)",
                                Syntax::BUILTIN_REQUIRE_EQ,
                                lt.name(),
                                field
                            ),
                            "equality needs types whose fields can all be compared".to_string(),
                            "compare the fields you care about instead".to_string(),
                            Some(call.name_span),
                        ));
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E0312",
                            format!(
                                "`{}` can't compare values of type `{}`",
                                Syntax::BUILTIN_REQUIRE_EQ,
                                lt.show()
                            ),
                            "this type doesn't support `==`".to_string(),
                            "compare fields individually, or use a different check".to_string(),
                            Some(call.name_span),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}
