use crate::AST::{AccessConvention, Expr, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::{private_item, soft_public_use, type_fix_hint};
use crate::Sema::FFI::e3211;
use crate::Syntax;
use std::collections::HashMap;
impl<'a> Checker<'a> {
        /// D-MOD2: check a call `alias.method(args)` where `alias` is an inline code module.
        /// The function was registered as `__jet_{alias}__{method}` in `self.funcs`.
        pub(crate) fn infer_code_module_call(
            &mut self,
            alias: &str,
            mangled: &str,
            alias_span: Span,
            span: Span,
            type_args: &[Type],
            args: &mut Vec<crate::AST::CallArg>,
        ) -> Option<Type> {
            let member_prefix = jet_foundation::Names::member_name(alias, "");
            // D-NAME-WALK1=A: a qualified call through an inline module's
            // `pub use` follows the exported item to its real owner. Keep this
            // before the ordinary mangled-signature lookup because the inline
            // module itself does not emit a forwarding function.
            let item = mangled.strip_prefix(&member_prefix).unwrap_or(mangled).to_string();
            if let Some((real_alias, real_mangled)) = self
                .inline_reexport_inline
                .get(&(alias.to_string(), item.clone()))
                .cloned()
            {
                return self.infer_code_module_call(
                    &real_alias,
                    &real_mangled,
                    alias_span,
                    span,
                    type_args,
                    args,
                );
            }
            if let Some((real_name, real_idx)) = self
                .inline_reexport_file
                .get(&(alias.to_string(), item.clone()))
                .cloned()
            {
                return self.infer_import_call(
                    real_idx,
                    &real_name,
                    alias_span,
                    span,
                    type_args,
                    args,
                );
            }
            if let Some((module, real_item)) = self
                .inline_reexport_core
                .get(&(alias.to_string(), item.clone()))
                .cloned()
            {
                return self.infer_core_call(
                    &module,
                    &real_item,
                    alias_span,
                    span,
                    type_args,
                    args,
                );
            }
            let Some(sig) = self.funcs.get(mangled).cloned() else {
                self.diags.push(Diagnostic::error(
                    "E0608",
                    format!(
                        "`{}` is not defined in module `{}`",
                        mangled.strip_prefix(&member_prefix).unwrap_or(mangled),
                        alias
                    ),
                    "check the module body for the function you're calling".to_string(),
                    "make sure the function name is spelled correctly".to_string(),
                    Some(alias_span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            };
            self.record_current_function_reference(mangled, span);
            self.record_edge(mangled.to_string(), span);
            if let Some(identity) = self.code_module_identities.get(alias).cloned() {
                self.record_semantic_reference(alias_span, identity.clone());
                let method = mangled.strip_prefix(&member_prefix).unwrap_or(mangled);
                self.record_semantic_reference(span, format!("fn:{identity}::{method}"));
            }
            // D-MOD2/3: a qualified `M.item` call from outside the module reaches only
            // its `pub` items — a bare private item escapes its module otherwise.
            if !self.name_ledger.exported(self.module_idx, mangled) {
                let item = mangled.strip_prefix(&member_prefix).unwrap_or(mangled);
                self.diags.push(Diagnostic::error(
                    "E0609",
                    format!("`{}` is private in module `{}`", item, alias),
                    "only `pub` items in an inline module are reachable from outside it".to_string(),
                    format!("add `pub` before `fn {}` in module `{}`", item, alias),
                    Some(alias_span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
            let item = mangled.strip_prefix(&member_prefix).unwrap_or(mangled);
            if Syntax::classify_identifier(item) == Syntax::IdentifierClass::SoftPublic {
                self.diags.push(soft_public_use(item, span));
            }
            // D-APILABEL1=A: inline-module calls use the same binder as local
            // and file-module calls. The mangled registration is only an
            // implementation name; its public labels remain the source
            // function's contract.
            let params = crate::Sema::CallBinder::bind_params_from_sig(&sig);
            if crate::Sema::CallBinder::bind_call_args(
                item,
                &params,
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
            // Homogeneous rest parameters are lowered as one list slot after
            // binding. Trait-bounded heterogeneous rests retain their source
            // tail for the dedicated per-arity path.
            if sig.param_variadic.last().copied().unwrap_or(false)
                && sig.variadic_bounds.is_none()
            {
                let mut normalized = crate::AST::Call {
                    name: item.to_string(),
                    name_span: span,
                    type_args: type_args.to_vec(),
                    args: std::mem::take(args),
                    resolved_ret: None,
                    range_checked: false,
                    widen_approx: false,
                };
                self.normalize_variadic_call(&mut normalized, &sig);
                *args = normalized.args;
            }
            if args.len() != sig.params.len() {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!(
                        "`{}` expects {} argument{}, got {}",
                        mangled.strip_prefix(&member_prefix).unwrap_or(mangled),
                        sig.params.len(),
                        if sig.params.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                    "every argument must match a parameter".to_string(),
                    format!(
                        "check the definition of `{}` in module `{}`",
                        mangled.strip_prefix(&member_prefix).unwrap_or(mangled),
                        alias
                    ),
                    Some(span),
                ));
            }
            let type_params = self.trait_reg.fn_params.get(mangled).cloned().unwrap_or_default();
            let mut subst = HashMap::new();
            let mut call_access = self.call_access_frame();
            let mut pre_inferred = Vec::new();
            if !type_args.is_empty() {
                if type_params.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0119",
                        format!("{alias}.{item} is not generic"),
                        "only functions declared with type parameters accept call-site type arguments"
                            .to_string(),
                        format!("call {alias}.{item}(...) without type arguments"),
                        Some(span),
                    ));
                } else if type_args.len() != type_params.len() {
                    self.diags.push(Diagnostic::error(
                        "E0119",
                        format!(
                            "{alias}.{item} expects {} type argument{}, got {}",
                            type_params.len(),
                            if type_params.len() == 1 { "" } else { "s" },
                            type_args.len()
                        ),
                        "a generic call must provide one type for every declared type parameter"
                            .to_string(),
                        format!("write {alias}.{item}<…>(...) with the declared types"),
                        Some(span),
                    ));
                } else {
                    for (param, actual) in type_params.iter().zip(type_args) {
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
                }
            } else if !type_params.is_empty() {
                for (index, arg) in args.iter_mut().enumerate() {
                    pre_inferred.push(self.with_call_access(&mut call_access, |checker| {
                        if let Some((param_conv, param_ty)) = sig.params.get(index) {
                            checker.check_call_argument_access(arg, *param_conv, param_ty, true);
                        }
                        let inferred = checker.infer(&mut arg.expr);
                        checker.check_call_argument_captures(&arg.expr);
                        inferred
                    }));
                }
                let arg_types = pre_inferred.iter().filter_map(Clone::clone).collect::<Vec<_>>();
                if arg_types.len() == args.len() {
                    match self.trait_reg.infer_fn_subst_without_bounds(
                        &sig,
                        &arg_types,
                        &type_params,
                        self.expected_type.as_ref(),
                    ) {
                        Ok(inferred) => {
                            if let Some((ty, bound)) = type_params.iter().find_map(|param| {
                                let ty = inferred.get(&param.name)?;
                                param
                                    .bounds
                                    .iter()
                                    .find(|bound| !self.type_satisfies_bound(ty, bound))
                                    .map(|bound| (ty, bound))
                            }) {
                                self.diags.push(crate::Generics::e0905(
                                    &ty.name(),
                                    bound,
                                    span,
                                    false,
                                ));
                            }
                            subst = inferred;
                        }
                        Err(param) => self.diags.push(crate::Generics::e0904(span, &param)),
                    }
                }
            }
            let effective_params: Vec<(AccessConvention, Type)> = sig
                .params
                .iter()
                .map(|(conv, ty)| (*conv, self.trait_reg.instantiate_type(ty, &subst)))
                .collect();
            for (index, (arg, (pconv, pty))) in args
                .iter_mut()
                .zip(effective_params.iter())
                .enumerate()
            {
                if matches!(pconv, AccessConvention::Read) && !pty.is_scalar() {
                    self.borrow_ctx = true;
                }
                // D-SG9: a fixed-width literal argument adopts the parameter's width.
                let saved = self.expected_type.clone();
                self.expected_type = Some(pty.clone());
                let aty = pre_inferred.get(index).cloned().unwrap_or_else(|| {
                    self.with_call_access(&mut call_access, |checker| {
                        checker.check_call_argument_access(arg, *pconv, pty, true);
                        let inferred = checker.infer(&mut arg.expr);
                        checker.check_call_argument_captures(&arg.expr);
                        inferred
                    })
                });
                self.expected_type = saved;
                if let Some(aty) = aty {
                    let aty =
                        self.widen_numeric_argument(&mut arg.expr, aty, pty, *pconv);
                    let arg_span = arg.expr.span();
                    if sig.is_pure
                        && crate::Sema::Diagnostics::is_clock_type(pty)
                        && !crate::Sema::Diagnostics::is_deterministic_clock_type(&aty)
                    {
                        self.diags.push(crate::Sema::e3403(
                            &format!(
                                "an unproven Clock passed to pure `{}`",
                                mangled.strip_prefix(&member_prefix).unwrap_or(mangled)
                            ),
                            Some(arg_span),
                        ));
                    }
                    self.check_type_assignable(pty, &aty, arg_span);
                }
                self.check_write_arg_change(arg);
            }
            sig.return_type
                .as_ref()
                .map(|ty| self.trait_reg.instantiate_type(ty, &subst))
        }

        pub(crate) fn infer_import_call(
            &mut self,
            mod_idx: usize,
            name: &str,
            alias_span: Span,
            span: Span,
            type_args: &[Type],
            args: &mut Vec<crate::AST::CallArg>,
        ) -> Option<Type> {
            self.infer_import_call_with_warning(
                mod_idx, name, alias_span, span, type_args, args, true,
            )
        }

        fn infer_import_call_with_warning(
            &mut self,
            mod_idx: usize,
            name: &str,
            alias_span: Span,
            span: Span,
            type_args: &[Type],
            args: &mut Vec<crate::AST::CallArg>,
            warn_soft_public: bool,
        ) -> Option<Type> {
            let Some(mods) = self.modules else {
                return None;
            };
            let target = &mods[mod_idx];
            // D-MOD4: `pub use` re-export — `thismod.Item` where Item is defined in a
            // submodule and re-exported. Redirect to the real definition.
            if let Some((real_name, real_idx)) = target.reexports.get(name) {
                if warn_soft_public
                    && mod_idx != self.module_idx
                    && Syntax::classify_identifier(name) == Syntax::IdentifierClass::SoftPublic
                {
                    self.diags.push(soft_public_use(name, span));
                }
                let (real_name, real_idx) = (real_name.clone(), *real_idx);
                return self.infer_import_call_with_warning(
                    real_idx,
                    &real_name,
                    alias_span,
                    span,
                    type_args,
                    args,
                    false,
                );
            }
            if target.funcs.contains_key(name) {
                let is_pub = self.name_ledger.visible(self.module_idx, mod_idx, name);
                if !is_pub && mod_idx != self.module_idx {
                    self.diags.push(private_item(name, span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                if warn_soft_public
                    && mod_idx != self.module_idx
                    && is_pub
                    && Syntax::classify_identifier(name) == Syntax::IdentifierClass::SoftPublic
                {
                    self.diags.push(soft_public_use(name, span));
                }
                let sig = target.funcs.get(name).unwrap().clone();
                // D-APILABEL1=A: a call across a module boundary binds by the
                // same law as a local one. Without this a label here was read
                // positionally, so `other.schedule(delay: 1, task: 2)` bound
                // the values the wrong way round and said nothing.
                {
                    let params = crate::Sema::CallBinder::bind_params_from_sig(&sig);
                    if crate::Sema::CallBinder::bind_call_args(
                        name,
                        &params,
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
                }
                // Homogeneous rest parameters are one declaration slot in the
                // binder and one list slot at the module boundary. Keep the
                // same normalization as direct calls before generic inference
                // and per-parameter checking. Heterogeneous trait-bounded
                // rests retain their individual tail for their special path.
                if sig.param_variadic.last().copied().unwrap_or(false)
                    && sig.variadic_bounds.is_none()
                {
                    let mut normalized = crate::AST::Call {
                        name: name.to_string(),
                        name_span: span,
                        type_args: type_args.to_vec(),
                        args: std::mem::take(args),
                        resolved_ret: None,
                        range_checked: false,
                        widen_approx: false,
                    };
                    self.normalize_variadic_call(&mut normalized, &sig);
                    *args = normalized.args;
                }
                let mut call_access = self.call_access_frame();
                let type_params = target.trait_reg.fn_params.get(name).cloned().unwrap_or_default();
                let mut subst = HashMap::new();
                let mut pre_inferred = Vec::new();
                if !type_args.is_empty() {
                    if type_params.is_empty() {
                        self.diags.push(Diagnostic::error(
                            "E0119",
                            format!("{name} is not generic"),
                            "only functions declared with type parameters accept call-site type arguments"
                                .to_string(),
                            format!("call {name}(...) without type arguments"),
                            Some(span),
                        ));
                    } else if type_args.len() != type_params.len() {
                        self.diags.push(Diagnostic::error(
                            "E0119",
                            format!(
                                "{name} expects {} type argument{}, got {}",
                                type_params.len(),
                                if type_params.len() == 1 { "" } else { "s" },
                                type_args.len()
                            ),
                            "a generic call must provide one type for every declared type parameter"
                                .to_string(),
                            format!("write {name}<…>(...) with the declared types"),
                            Some(span),
                        ));
                    } else {
                        for (param, actual) in type_params.iter().zip(type_args) {
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
                    }
                } else if !type_params.is_empty() {
                    for (index, arg) in args.iter_mut().enumerate() {
                        pre_inferred.push(self.with_call_access(&mut call_access, |checker| {
                            if let Some((param_conv, param_ty)) = sig.params.get(index) {
                                checker.check_call_argument_access(
                                    arg,
                                    *param_conv,
                                    param_ty,
                                    !sig.is_extern,
                                );
                            }
                            let inferred = checker.infer(&mut arg.expr);
                            checker.check_call_argument_captures(&arg.expr);
                            inferred
                        }));
                    }
                    let arg_types = pre_inferred.iter().filter_map(|ty| ty.clone()).collect::<Vec<_>>();
                    if arg_types.len() == args.len() {
                        match target.trait_reg.infer_fn_subst_without_bounds(
                            &sig,
                            &arg_types,
                            &type_params,
                            self.expected_type.as_ref(),
                        ) {
                            Ok(inferred) => {
                                if let Some((ty, bound)) = type_params.iter().find_map(|param| {
                                    let ty = inferred.get(&param.name)?;
                                    param
                                        .bounds
                                        .iter()
                                        .find(|bound| !self.type_satisfies_bound(ty, bound))
                                        .map(|bound| (ty, bound))
                                }) {
                                    self.diags.push(crate::Generics::e0905(
                                        &ty.name(),
                                        bound,
                                        span,
                                        false,
                                    ));
                                }
                                subst = inferred;
                            }
                            Err(param) => self.diags.push(crate::Generics::e0904(span, &param)),
                        }
                    }
                }
                let qualify_unit = |ty: Type| ty.map_named_types(&|name| {
                    target.registry.unit_dimension(name).map(|_| {
                        format!("{}.{}", target.module_alias, name)
                    })
                });
                let effective_params: Vec<(AccessConvention, Type)> = sig.params.iter().map(|(conv, ty)| {
                    (*conv, qualify_unit(self.trait_reg.instantiate_type(ty, &subst)))
                }).collect();
                let target_alias = target.module_alias.clone();
                self.record_edge(format!("{target_alias}.{name}"), span);
                self.record_function_reference(mod_idx, name, span);
                if args.len() != sig.params.len() {
                    self.diags.push(Diagnostic::error(
                        "E0104",
                        format!(
                            "`{}` expects {} argument{}, got {}",
                            name,
                            sig.params.len(),
                            if sig.params.len() == 1 { "" } else { "s" },
                            args.len()
                        ),
                        "every argument must match a parameter".to_string(),
                        format!("check the definition of `{}` in the imported file", name),
                        Some(span),
                    ));
                }
                for (index, (arg, (pconv, pty))) in args.iter_mut().zip(effective_params.iter()).enumerate() {
                    if matches!(pconv, AccessConvention::Read) && !pty.is_scalar() {
                        self.borrow_ctx = true;
                    }
                    // E3211 (card #436): a `String` literal with a known
                    // interior NUL byte can't cross into a C-boundary
                    // function (`CString::new` would fail — C strings are
                    // NUL-terminated, not length-prefixed). Only literals
                    // (no interpolation) are checked here — a runtime-built
                    // String is caught by a codegen panic instead (see
                    // `Codegen/CModule.rs`'s `NUL_PANIC`).
                    if sig.is_c_abi && matches!(pty, Type::String) {
                        if let Expr::Str(parts, str_span) = &arg.expr {
                            let literal: Option<String> = parts
                                .iter()
                                .map(|p| match p {
                                    crate::AST::StrPart::Lit(s) => Some(s.clone()),
                                    crate::AST::StrPart::Interp(..) => None,
                                })
                                .collect();
                            if literal.is_some_and(|text| text.contains('\0')) {
                                self.diags.push(e3211(*str_span));
                            }
                        }
                    }
                    // D-SG9: a fixed-width literal argument adopts the parameter's width.
                    let saved = self.expected_type.clone();
                    self.expected_type = Some(pty.clone());
                    let aty = pre_inferred
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| {
                            self.with_call_access(&mut call_access, |checker| {
                                checker.check_call_argument_access(
                                    arg,
                                    *pconv,
                                    pty,
                                    !sig.is_extern,
                                );
                                let inferred = checker.infer(&mut arg.expr);
                                checker.check_call_argument_captures(&arg.expr);
                                inferred
                            })
                        });
                    self.expected_type = saved;
                    if sig.is_pure
                        && crate::Sema::Diagnostics::is_clock_type(pty)
                        && aty.as_ref().is_some_and(|ty| {
                            !crate::Sema::Diagnostics::is_deterministic_clock_type(ty)
                        })
                    {
                        self.diags.push(crate::Sema::e3403(
                            &format!("an unproven Clock passed to pure `{name}`"),
                            Some(arg.expr.span()),
                        ));
                    }
                    if crate::Sema::FFI::is_callback_boundary_param(sig.is_c_abi, pty) {
                        let safe = match &arg.expr {
                            Expr::Ident(callback, _) => self.funcs.get(callback).is_some_and(|f| {
                                !f.is_extern && f.is_foreign_thread_safe
                            }) || aty.as_ref().is_some_and(|ty| {
                                crate::Sema::FFI::cpp_callback_abi_type(ty).is_some()
                            }),
                            Expr::Lambda(lam) => crate::Sema::foreign_thread_safe_lambda(lam),
                            _ => false,
                        };
                        if safe {
                            arg.flags.c_callback_symbol = true;
                        } else {
                            self.diags.push(crate::Sema::FFI::e3203(pty, arg.expr.span()));
                        }
                    }
                    if let Some(aty) = aty {
                        let aty =
                            self.widen_numeric_argument(&mut arg.expr, aty, pty, *pconv);
                        let span = arg.expr.span();
                        let loan_param_ty = match pty {
                            Type::Named(qualified) => qualified
                                .split_once('.')
                                .and_then(|(alias, leaf)| {
                                    target.core_imports.get(alias).and_then(|module| {
                                        if matches!(
                                            module.as_str(),
                                            "core.crypto"
                                        ) && matches!(
                                            leaf,
                                            "Secret" | "SigningKey" | "X25519SecretKey"
                                        ) {
                                            Some(crate::Sema::Diagnostics::core_crypto_nominal(
                                                Type::Named(leaf.to_string()),
                                            ))
                                        } else {
                                            None
                                        }
                                    })
                                })
                                .unwrap_or_else(|| pty.clone()),
                            _ => pty.clone(),
                        };
                        let reads_expiring_secret_loan = *pconv == AccessConvention::Read
                            && arg.convention == AccessConvention::Read
                            && crate::Sema::Diagnostics::expiring_secret_loan_matches(
                                &loan_param_ty,
                                &aty,
                            );
                        let reported = reads_expiring_secret_loan
                            || self.check_type_assignable(pty, &aty, span);
                        if !reported && aty != *pty && !reads_expiring_secret_loan {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`{}` wants {} here, but this is {}",
                                    name,
                                    pty.show(),
                                    aty.show()
                                ),
                                "every argument must match its parameter's type".to_string(),
                                type_fix_hint(pty, &aty),
                                Some(span),
                            ));
                        }
                    }
                    // Cross-file calls follow the same ownership rules.
                    if let Expr::Ident(n, nspan) = &arg.expr {
                        match (pconv, arg.convention) {
                            (AccessConvention::Move, AccessConvention::Move) => {
                                if !pty.is_scalar() {
                                    self.mark_moved(n.clone(), *nspan);
                                }
                            }
                            (AccessConvention::Move, AccessConvention::Read) => {
                                if !pty.is_scalar() {
                                    arg.flags.implicit_clone = true;
                                }
                            }
                            (AccessConvention::Write, AccessConvention::Read) => {
                                self.diags.push(Diagnostic::error(
                                    "E0202",
                                    format!(
                                        "parameter `{}` requires the write-capability marker `&` at the call site",
                                        n
                                    ),
                                    format!(
                                        "`{}` needs to edit this value with the write-capability marker `&`; passing it without that marker grants only read access",
                                        name
                                    ),
                                    format!(
                                        "write the write-capability marker `&` (`{}{}`) when calling `{}`",
                                        Syntax::SIGIL_WRITE,
                                        n,
                                        name
                                    ),
                                    Some(*nspan),
                                ));
                            }
                            _ => {}
                        }
                    }
                    self.check_write_arg_change(arg);
                }
                return sig.return_type.as_ref().map(|ty| qualify_unit(self.resolve_type(
                    self.trait_reg.instantiate_type(ty, &subst)
                )));
            }
            if target.registry.contains(name) {
                let is_pub = self.type_is_pub_in(mod_idx, name);
                if !is_pub && mod_idx != self.module_idx {
                    self.diags.push(private_item(name, span));
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0102",
                        format!("nothing named `{}` exists in this import", name),
                        "only `pub` functions and types from the other file are reachable here"
                            .to_string(),
                        "check the spelling, or mark the item `pub` in its file".to_string(),
                        Some(span),
                    ));
                }
            } else {
                self.diags.push(Diagnostic::error(
                    "E0102",
                    format!("nothing named `{}` exists in this import", name),
                    "only `pub` functions and types from the other file are reachable here".to_string(),
                    "check the spelling, or mark the item `pub` in its file".to_string(),
                    Some(alias_span),
                ));
            }
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            None
        }
    
}
