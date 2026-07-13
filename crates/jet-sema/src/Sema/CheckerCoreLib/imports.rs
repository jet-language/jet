use crate::AST::{AccessConvention, Expr, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::{private_item, type_fix_hint};
use crate::Sema::FFI::e3211;
use crate::Syntax;
impl<'a> Checker<'a> {
        /// D-MOD2: check a call `alias.method(args)` where `alias` is an inline code module.
        /// The function was registered as `{alias}__{method}` in `self.funcs`.
        pub(crate) fn infer_code_module_call(
            &mut self,
            alias: &str,
            mangled: &str,
            alias_span: Span,
            span: Span,
            args: &mut [crate::AST::CallArg],
        ) -> Option<Type> {
            let Some(sig) = self.funcs.get(mangled).cloned() else {
                self.diags.push(Diagnostic::error(
                    "E0608",
                    format!(
                        "`{}` is not defined in module `{}`",
                        &mangled[alias.len() + 2..],
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
            if let Some(identity) = self.code_module_identities.get(alias).cloned() {
                self.record_semantic_reference(alias_span, identity.clone());
                let method = mangled.strip_prefix(&format!("{alias}__")).unwrap_or(mangled);
                self.record_semantic_reference(span, format!("fn:{identity}::{method}"));
            }
            // D-MOD2/3: a qualified `M.item` call from outside the module reaches only
            // its `pub` items — a bare private item escapes its module otherwise.
            if !self.func_pub.get(mangled).copied().unwrap_or(false)
                && !self.func_pkg_pub.get(mangled).copied().unwrap_or(false)
            {
                let item = &mangled[alias.len() + 2..];
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
            if args.len() != sig.params.len() {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!(
                        "`{}` expects {} argument{}, got {}",
                        &mangled[alias.len() + 2..],
                        sig.params.len(),
                        if sig.params.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                    "every argument must match a parameter".to_string(),
                    format!(
                        "check the definition of `{}` in module `{}`",
                        &mangled[alias.len() + 2..],
                        alias
                    ),
                    Some(span),
                ));
            }
            for (arg, (pconv, pty)) in args.iter_mut().zip(sig.params.iter()) {
                if matches!(pconv, AccessConvention::Read) && !pty.is_scalar() {
                    self.borrow_ctx = true;
                }
                // D-SG9: a fixed-width literal argument adopts the parameter's width.
                let saved = self.expected_type.clone();
                self.expected_type = Some(pty.clone());
                let aty = self.infer(&mut arg.expr);
                self.expected_type = saved;
                if let Some(aty) = aty {
                    let arg_span = arg.expr.span();
                    self.check_type_assignable(pty, &aty, arg_span);
                }
            }
            sig.return_type
        }

        pub(crate) fn infer_import_call(
            &mut self,
            mod_idx: usize,
            name: &str,
            alias_span: Span,
            span: Span,
            args: &mut [crate::AST::CallArg],
        ) -> Option<Type> {
            let Some(mods) = self.modules else {
                return None;
            };
            let target = &mods[mod_idx];
            // D-MOD4: `pub use` re-export — `thismod.Item` where Item is defined in a
            // submodule and re-exported. Redirect to the real definition.
            if let Some((real_name, real_idx)) = target.reexports.get(name) {
                let (real_name, real_idx) = (real_name.clone(), *real_idx);
                return self.infer_import_call(real_idx, &real_name, alias_span, span, args);
            }
            if target.funcs.contains_key(name) {
                let is_pub = target.func_pub.get(name).copied().unwrap_or(false)
                    || (self.same_package_scope(mod_idx)
                        && target.func_pkg_pub.get(name).copied().unwrap_or(false));
                if !is_pub && mod_idx != self.module_idx {
                    self.diags.push(private_item(name, span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let sig = target.funcs.get(name).unwrap().clone();
                let target_alias = target.module_alias.clone();
                self.record_edge(format!("{target_alias}.{name}"));
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
                for (arg, (pconv, pty)) in args.iter_mut().zip(sig.params.iter()) {
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
                    let aty = self.infer(&mut arg.expr);
                    self.expected_type = saved;
                    if sig.is_c_abi && matches!(pty, Type::Fn { .. }) {
                        let safe = match &arg.expr {
                            Expr::Ident(callback, _) => self.funcs.get(callback).is_some_and(|f| {
                                !f.is_extern && f.is_foreign_thread_safe
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
                        let span = arg.expr.span();
                        let reported = self.check_type_assignable(pty, &aty, span);
                        if !reported && aty != *pty {
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
                                        "parameter `{}` requires `{}` at the call site",
                                        n,
                                        Syntax::SIGIL_WRITE
                                    ),
                                    format!(
                                        "`{}` needs to edit (`&`) this value; passing it without `{}` grants only read access",
                                        name,
                                        Syntax::SIGIL_WRITE
                                    ),
                                    format!(
                                        "write `{}{}` when calling `{}`",
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
                }
                return sig.return_type.clone();
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
