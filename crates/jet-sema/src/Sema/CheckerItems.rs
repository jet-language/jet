use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{
    AccessConvention, BinOp, Call, EnumLitArg, Expr, Pattern, StrMatchPart, StructPatField, Type,
    VariantPayload,
};
use std::collections::{HashMap, HashSet};

impl<'a> Checker<'a> {
    pub(crate) fn check_static_method(
        &mut self,
        type_name: &str,
        method: &str,
        span: Span,
        args: &mut Vec<crate::AST::CallArg>,
    ) -> Option<Type> {
        let Some(msig) = self.registry.method(type_name, method).cloned() else {
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("`{}` has no method `{}`", type_name, method),
                "check the method name on this type".to_string(),
                format!("define it inside `struct {type_name}` or `impl {type_name}`"),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        };
        if !msig.is_static {
            self.diags.push(Diagnostic::error(
                "E0311",
                format!("`{}` is an instance method on `{}`", method, type_name),
                "instance methods need a value before the dot".to_string(),
                format!("call it on a `{type_name}` value: `x.{method}(...)`"),
                Some(span),
            ));
        }
        self.check_method_args(type_name, method, &msig, args, span)
    }

    pub(crate) fn check_method_args(
        &mut self,
        type_name: &str,
        method: &str,
        sig: &MethodSig,
        args: &mut Vec<crate::AST::CallArg>,
        span: Span,
    ) -> Option<Type> {
        let _ = (type_name, method, span);
        let expected_args = if sig.self_conv.is_some() {
            sig.params.len().saturating_sub(1)
        } else {
            sig.params.len()
        };

        // D-NARG-D4 (S61, E0125): label validation — if a call arg has
        // `name: val`, verify it matches the parameter name at that position.
        // Labels never reorder. param_info is already self-excluded.
        if !sig.param_info.is_empty() {
            let all_param_names: Vec<&str> =
                sig.param_info.iter().map(|(n, _)| n.as_str()).collect();
            for (i, arg) in args.iter().enumerate() {
                if let Some((label, label_span)) = &arg.label {
                    if let Some((param_name, _)) = sig.param_info.get(i) {
                        if label != param_name {
                            // Is the label a real param name at a different position?
                            if all_param_names.contains(&label.as_str()) {
                                // Transposed: label names a real param, but wrong position.
                                self.diags.push(Diagnostic::error(
                                    "E0125",
                                    format!(
                                        "label `{}:` doesn't match the parameter `{}` here",
                                        label, param_name
                                    ),
                                    "labels are checked documentation — each names the parameter at its own position, and arguments stay in the order they're declared".to_string(),
                                    format!(
                                        "write `{}:` here, or drop the label",
                                        param_name
                                    ),
                                    Some(*label_span),
                                ));
                            } else {
                                // Unknown: label doesn't name any parameter.
                                self.diags.push(Diagnostic::error(
                                    "E0125",
                                    format!(
                                        "`{}` has no parameter named `{}`",
                                        method, label
                                    ),
                                    format!(
                                        "a label must name the parameter at its position; `{}` takes {}",
                                        method,
                                        all_param_names.join(", ")
                                    ),
                                    format!(
                                        "use one of `{}`'s parameter names, or drop the label",
                                        method
                                    ),
                                    Some(*label_span),
                                ));
                            }
                        }
                    }
                }
            }
        }

        // D-NARG-D2 (S61): default-value filling — append defaults for omitted
        // trailing params. Earlier-param refs in defaults are substituted with
        // the supplied argument expression so codegen never sees an unresolved
        // identifier (invariant I2).
        if args.len() < expected_args && !sig.defaults.is_empty() {
            let provided = args.len();
            let required: usize = sig.defaults.iter().take_while(|d| d.is_none()).count();
            if provided >= required {
                // Build earlier_names incrementally so a default like `d = h`
                // can reference an already-filled synthetic arg `h`.
                let all_param_names: Vec<String> =
                    sig.param_info.iter().map(|(n, _)| n.clone()).collect();
                for i in provided..expected_args {
                    if let Some(Some(default_expr)) = sig.defaults.get(i) {
                        // earlier_names covers all params up to (not including) i.
                        let earlier_names: Vec<String> =
                            all_param_names.iter().take(i).cloned().collect();
                        // Substitute any earlier-param idents with the supplied arg.
                        let resolved = super::substitute_param_refs(
                            default_expr.clone(),
                            &earlier_names,
                            args,
                        );
                        // Use the first non-self param conv (offset by self).
                        let param_idx = if sig.self_conv.is_some() { i + 1 } else { i };
                        let conv = sig
                            .params
                            .get(param_idx)
                            .map(|(c, _)| *c)
                            .unwrap_or(crate::AST::AccessConvention::Read);
                        args.push(crate::AST::CallArg {
                            convention: conv,
                            expr: resolved,
                            span,
                            flags: Default::default(),
                            label: None,
                            spread: false,
                        });
                    }
                }
            }
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
                let arg_ty = self.infer(&mut arg.expr);
                if let Some(arg_ty) = arg_ty {
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
                if arg.convention == AccessConvention::Write
                    && !matches!(arg.expr, Expr::Ident(_, _))
                {
                    self.diags.push(Diagnostic::error(
                        "E0202",
                        format!(
                            "`{}` needs a plain named binding after it",
                            Syntax::SIGIL_MUTATE
                        ),
                        "write access (`~`) can only be granted to a named binding, not an expression".to_string(),
                        format!(
                            "bind the value first: `x {} ...` then pass `{}x`",
                            Syntax::SIGIL_BIND_MUT,
                            Syntax::SIGIL_MUTATE
                        ),
                        Some(arg.span),
                    ));
                }
                // Same ownership rules as plain calls (E0201/E0202/E0203).
                match (param_conv, arg.convention) {
                    (AccessConvention::Move, AccessConvention::Read) => {
                        if let Expr::Ident(name, span) = &arg.expr {
                            if is_cloneable(param_ty, self.registry, self.structs) {
                                arg.flags.implicit_clone = true;
                                // D-L0201: only warn when the value is dead after
                                // this call (a wasteful clone). If it's still used
                                // later the clone is necessary — stay silent.
                                if !self.is_name_live_after(name) {
                                    self.diags.push(Diagnostic::lint(
                                        "L0201",
                                        format!(
                                            "implicit clone of `{}`; write `{}{}` to transfer ownership or `.clone()` to silence this warning",
                                            name,
                                            Syntax::SIGIL_MOVE,
                                            name
                                        ),
                                        format!(
                                            "`{}` expects to take ownership of this value",
                                            method
                                        ),
                                        format!(
                                            "write `{}{}` to move, or `{}.clone()` to copy explicitly",
                                            Syntax::SIGIL_MOVE,
                                            name,
                                            name
                                        ),
                                        Some(*span),
                                    ));
                                }
                            } else {
                                self.diags.push(Diagnostic::error(
                                    "E0201",
                                    format!(
                                        "`{}` needs `{}` here — this value can't be copied",
                                        method,
                                        Syntax::SIGIL_MOVE
                                    ),
                                    format!(
                                        "parameter {} takes ownership (`^`); passing `{}` without `{}` would have to copy it, but this type can't be copied",
                                        arg_idx + 1,
                                        name,
                                        Syntax::SIGIL_MOVE
                                    ),
                                    format!(
                                        "write `{}{}` to move ownership to `{}`",
                                        Syntax::SIGIL_MOVE,
                                        name,
                                        method
                                    ),
                                    Some(*span),
                                ));
                            }
                        }
                    }
                    (AccessConvention::Move, AccessConvention::Move) => {
                        if let Expr::Ident(name, span) = &arg.expr {
                            if !param_ty.is_scalar() {
                                self.mark_moved(name.clone(), *span);
                            }
                        }
                    }
                    (AccessConvention::Write, AccessConvention::Read) => {
                        if let Expr::Ident(name, nspan) = &arg.expr {
                            self.diags.push(Diagnostic::error(
                                "E0202",
                                format!(
                                    "parameter `{}` requires write access (`~`) at the call site",
                                    name
                                ),
                                format!(
                                    "`{method}` needs to edit (`~`) this value; passing it without `{}` grants only read access",
                                    Syntax::SIGIL_MUTATE
                                ),
                                format!(
                                    "write `{}{}` when calling `{method}`",
                                    Syntax::SIGIL_MUTATE,
                                    name
                                ),
                                Some(*nspan),
                            ));
                        }
                    }
                    (AccessConvention::Write, AccessConvention::Write) => {
                        if let Expr::Ident(name, span) = &arg.expr {
                            if let Some(info) = self.lookup(name) {
                                if !info.mutable {
                                    self.diags.push(Diagnostic::error(
                                        "E0111",
                                        format!(
                                            "`{}` was made with `{}`, so it can't be changed",
                                            name,
                                            Syntax::SIGIL_BIND_IMMUT
                                        ),
                                        format!(
                                            "`{}` will change this value, so it must be mutable (`{}`)",
                                            method,
                                            Syntax::SIGIL_BIND_MUT
                                        ),
                                        format!(
                                            "declare it with `{} {} ...`",
                                            name,
                                            Syntax::SIGIL_BIND_MUT
                                        ),
                                        Some(*span),
                                    ));
                                }
                            }
                        }
                    }
                    (AccessConvention::Read | AccessConvention::Write, AccessConvention::Move) => {
                        self.diags.push(Diagnostic::error(
                            "E0203",
                            format!(
                                "`{}` passed to a parameter that does not consume",
                                Syntax::SIGIL_MOVE
                            ),
                            "only move (`^`) parameters accept a moved value at the call site"
                                .to_string(),
                            format!(
                                "remove `{}` or change the parameter to take ownership (`{}`)",
                                Syntax::SIGIL_MOVE,
                                Syntax::SIGIL_MOVE
                            ),
                            Some(arg.span),
                        ));
                    }
                    _ => {}
                }
                arg_idx += 1;
            }
        }
        sig.return_type.clone()
    }

    pub(crate) fn struct_owner_module(
        &self,
        type_name: &str,
        import_ns: Option<&str>,
    ) -> Option<usize> {
        if let Some(alias) = import_ns {
            let mod_idx = *self.imports.get(alias)?;
            let mods = self.modules?;
            if mods[mod_idx].registry.contains(type_name) {
                return Some(mod_idx);
            }
            return None;
        }
        if self.registry.contains(type_name) {
            return Some(self.module_idx);
        }
        let mods = self.modules?;
        let mut found = None;
        for (idx, st) in mods.iter().enumerate() {
            if st.registry.contains(type_name) && self.type_is_pub_in(idx, type_name) {
                found = Some(idx);
            }
        }
        found
    }

    pub(crate) fn struct_fields_of(
        &self,
        owner_mod: usize,
        type_name: &str,
    ) -> Option<&[(String, Span, Type, bool, bool)]> {
        if owner_mod == self.module_idx {
            self.registry.struct_fields(type_name)
        } else {
            self.modules?
                .get(owner_mod)?
                .registry
                .struct_fields(type_name)
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
        None
    }

    pub(crate) fn same_package_scope(&self, owner_mod: usize) -> bool {
        if owner_mod == self.module_idx {
            return true;
        }
        let Some(mods) = self.modules else {
            return false;
        };
        let (Some(owner), Some(current)) = (mods.get(owner_mod), mods.get(self.module_idx)) else {
            return false;
        };
        owner.package_scope == current.package_scope
    }

    pub(crate) fn field_is_pub_in(&self, owner_mod: usize, type_name: &str, field: &str) -> bool {
        if owner_mod == self.module_idx {
            return true;
        }
        let Some(mods) = self.modules else {
            return false;
        };
        let Some(st) = mods.get(owner_mod) else {
            return false;
        };
        let key = (type_name.to_string(), field.to_string());
        st.field_pub.get(&key).copied().unwrap_or(false)
            || (self.same_package_scope(owner_mod)
                && st.field_pkg_pub.get(&key).copied().unwrap_or(false))
    }

    pub(crate) fn type_is_pub_in(&self, owner_mod: usize, type_name: &str) -> bool {
        if owner_mod == self.module_idx {
            return true;
        }
        let Some(mods) = self.modules else {
            return false;
        };
        let Some(st) = mods.get(owner_mod) else {
            return false;
        };
        st.type_pub.get(type_name).copied().unwrap_or(false)
            || (self.same_package_scope(owner_mod)
                && st.type_pkg_pub.get(type_name).copied().unwrap_or(false))
    }

    pub(crate) fn check_struct_lit(
        &mut self,
        type_name: &str,
        type_args: &[Type],
        import_ns: Option<&str>,
        fields: &mut [(String, Span, Expr)],
        span: Span,
    ) -> Type {
        // E2-M10: compiler-known constructable struct types (HttpRequest, HttpResponse).
        // These have no user-module owner but are valid in struct literals.
        if let Some(core_fields) = core_constructable_fields(type_name) {
            let str_map_ty = Type::Map {
                key: Box::new(Type::String),
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
                self.infer(fexpr);
                self.expected_type = saved;
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
        }
        let def_fields: Vec<(String, Span, Type, bool, bool)> = self
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
        let subst = self.struct_subst(type_name, type_args);
        let field_names: Vec<String> = def_fields.iter().map(|(n, ..)| n.clone()).collect();
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
            }
            let field_def = def_fields.iter().find(|(n, ..)| n == name);
            let saved_expected = self.expected_type.clone();
            let saved_esc = self.lambda_escapes;
            let skip_clone = field_def.is_some_and(|(_, _, _, is_ref, _)| *is_ref)
                && self
                    .registry
                    .ref_field_label(type_name, name)
                    .is_some_and(|owner| {
                        super::CheckerOwnership::ref_field_init_root(expr)
                            .is_some_and(|root| root == owner)
                    });
            if let Some((_, _, fty, _, _)) = field_def {
                let inst = self.trait_reg.instantiate_type(fty, &subst);
                self.expected_type = Some(inst);
            }
            if matches!(expr, Expr::Lambda(_)) {
                self.lambda_escapes = true;
            }
            // A `#Ref(owner)` fill from `owner` (or `owner.field`) is a borrow slot;
            // suppress `field_read_to_clone` during infer so we don't wrap `.clone()`.
            let saved_borrow = self.borrow_ctx;
            if skip_clone {
                self.borrow_ctx = true;
            }
            let et = self.infer(expr);
            self.borrow_ctx = saved_borrow;
            self.expected_type = saved_expected;
            self.lambda_escapes = saved_esc;
            if !skip_clone {
                // A struct-lit field VALUE is an owning position. A bare borrowed-in-env
                // non-`Copy` ident (a `read`/`mut` param → `&T`/`&mut T`) can't be moved
                // into the field — codegen would emit `(*user_n)` → rustc E0507.
                self.clone_borrowed_struct_field_value(expr);
            }
            // D-ALLOC2: E0631 — storing an arena `view` in a struct field would
            // let the struct (which can outlive the region) keep a dangling
            // borrow into the arena.
            if let Expr::Ident(vname, vspan) = expr {
                if self.is_arena_view(vname) {
                    self.report_view_escape(vname, "be stored in a struct field", *vspan);
                }
                // D-DYNARRAY1: E2305 — storing a `View<T>` in a struct field
                // would let the struct outlive the list it borrows from.
                if self.is_list_view(vname) {
                    self.report_list_view_escape(vname, "be stored in a struct field", *vspan);
                }
            }
            if let Some((_, _, fty, _, _)) = field_def {
                let inst = self.trait_reg.instantiate_type(fty, &subst);
                if let Some(et) = et {
                    self.check_type_assignable(&inst, &et, expr.span());
                }
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
            .filter(|(n, _, _, is_ref, _)| !*is_ref && !provided.contains_key(n))
            .map(|(n, ..)| n.clone())
            .collect();
        if !missing.is_empty() {
            self.diags.push(Diagnostic::error(
                "E0303",
                format!(
                    "struct literal for `{}` is missing fields: {}",
                    type_name,
                    missing.join(", ")
                ),
                "every non-`ref` field must appear exactly once".to_string(),
                format!("add: {}", missing.join(", ")),
                Some(span),
            ));
        }
        if !type_args.is_empty() {
            Type::Apply {
                name: type_name.to_string(),
                args: type_args.to_vec(),
            }
        } else if self
            .trait_reg
            .struct_params
            .get(type_name)
            .is_some_and(|p| !p.is_empty())
        {
            Type::Apply {
                name: type_name.to_string(),
                args: self
                    .trait_reg
                    .struct_params
                    .get(type_name)
                    .unwrap()
                    .iter()
                    .map(|p| Type::Named(p.name.clone()))
                    .collect(),
            }
        } else {
            Type::Named(type_name.to_string())
        }
    }

    /// Rewrite a struct-literal field VALUE that is a bare borrowed-in-env non-`Copy`
    /// ident into a `.clone()` MethodCall. A `read`/`mut` param is `&T`/`&mut T`, so
    /// using it directly as the (owning) field value would emit `(*user_n)` →
    /// rustc E0507 ("cannot move out of `*user_n`"). This mirrors `field_read_to_clone`
    /// (which clones an owning *field read*) for the bare-ident case. Owned locals,
    /// `take` params, `Copy` types, and non-ident values are left untouched (no clone).
    /// A type-VAR param (`a: T`) is emitted BY VALUE in codegen (`is_type_param` →
    /// `Move`/no deref, Items.rs), not by reference, so it must NOT be cloned even though
    /// its convention reads as `Read` — cloning would diverge from the AST path.
    fn clone_borrowed_struct_field_value(&mut self, expr: &mut Expr) {
        let should_clone = match expr {
            Expr::Ident(name, _) => self.lookup(name).is_some_and(|info| {
                // A type-var-typed param is by-value (codegen's `is_type_param`), never
                // borrowed — exclude it (matches the AST path's owned move).
                let is_type_var = matches!(&info.ty,
                    Type::Named(n) if self.type_param_scope.iter().any(|p| &p.name == n));
                !type_is_copy(&info.ty)
                    && !is_type_var
                    && matches!(
                        info.param_conv,
                        Some(AccessConvention::Read) | Some(AccessConvention::Write)
                    )
            }),
            _ => false,
        };
        if should_clone {
            let span = expr.span();
            let old = std::mem::replace(expr, Expr::Absent(span));
            *expr = Expr::MethodCall {
                receiver: Box::new(old),
                method: "clone".to_string(),
                method_span: span,
                type_args: Vec::new(),
                args: Vec::new(),
                recv_type: None,
                resolved_ret: None,
            };
        }
    }

    pub(crate) fn check_enum_lit(
        &mut self,
        type_name: &str,
        variant: &str,
        args: &mut [EnumLitArg],
        span: Span,
    ) -> Type {
        let ty = Type::Named(type_name.to_string());
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
                    if let Some(et) = self.infer(e) {
                        self.check_type_assignable(expected, &et, e.span());
                    }
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

    /// S31: `subject == Red` when `Red` is a unit variant, not a variable.
    pub(crate) fn eq_unit_variant_pattern(
        &self,
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
        let Type::Named(enum_name) = subj_ty else {
            return None;
        };
        let variant_known = self
            .resolve_enum_variants_cloned(enum_name)
            .is_some_and(|variants| variants.contains_key(variant));
        if !variant_known {
            return None;
        }
        Some(Pattern::Variant {
            variant: variant.clone(),
            bindings: Vec::new(),
            span: *rhs_span,
        })
    }

    /// S31: pattern carried as `PatternTest` or as `subject == UnitVariant`.
    pub(crate) fn switch_arm_pattern(
        &self,
        cond: &Expr,
        subject_name: Option<&str>,
        subj_ty: &Type,
    ) -> Option<Pattern> {
        match cond {
            // D-PARSESTR1: a str-match pattern is never provably exhaustive
            // (literal text might not match; a typed hole's parse can fail),
            // so it must never join the `all_pattern` provable-coverage path —
            // it always needs its own E0148 (missing `else`) check instead.
            Expr::PatternTest {
                pattern: Pattern::StrMatch { .. },
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
            // D-TERM1 (ratified 2026-06-22): `Key` arm heads written as bare
            // variant name (unit: `Enter`, `Up`, …) or call-like (payload:
            // `Char(c)`, `F(n)`, `Ctrl(c)`). The parser produces a Call node
            // for `Variant(binding)` arms and an Ident node for unit variants.
            // Recognise them as patterns when the subject is a `Key`.
            //
            // Also handles JSON patterns written in the arm-head position
            // (the same call-as-pattern reuse JSON already relies on).
            Expr::Ident(variant, span) if subject_name.is_some() => {
                let Type::Named(enum_name) = subj_ty else {
                    return None;
                };
                let variants = self.resolve_enum_variants_cloned(enum_name)?;
                let (_, payload) = variants.get(variant.as_str())?;
                if !matches!(payload, crate::AST::VariantPayload::Unit) {
                    return None;
                }
                // Only accept if the variant is NOT a live local (avoids shadowing).
                if self.lookup(variant).is_some() {
                    return None;
                }
                Some(Pattern::Variant {
                    variant: variant.clone(),
                    bindings: vec![],
                    span: *span,
                })
            }
            Expr::Call(call) if subject_name.is_some() => {
                // Only a single positional binding arg is the pattern form.
                if call.args.len() != 1 {
                    return None;
                }
                let Expr::Ident(binding, _) = &call.args[0].expr else {
                    return None;
                };
                if self.lookup(binding).is_some() {
                    return None;
                } // it's a real local
                let Type::Named(enum_name) = subj_ty else {
                    return None;
                };
                let variants = self.resolve_enum_variants_cloned(enum_name)?;
                let (_, payload) = variants.get(call.name.as_str())?;
                if !matches!(payload, crate::AST::VariantPayload::Single(..)) {
                    return None;
                }
                Some(Pattern::Variant {
                    variant: call.name.clone(),
                    bindings: vec![crate::AST::PatSlot::Bind(binding.clone())],
                    span: call.name_span,
                })
            }
            _ => None,
        }
    }

    pub(crate) fn check_pattern_test(
        &mut self,
        subject: &mut Box<Expr>,
        pattern: &Pattern,
        span: Span,
    ) -> HashMap<String, Type> {
        let subj_ty = self.infer(subject);
        let Some(st) = subj_ty else {
            return HashMap::new();
        };
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
        if let Expr::Ident(n, nspan) = subject {
            if n != Syntax::KW_IT && self.lookup(n).is_some() {
                self.mark_moved(n.clone(), *nspan);
            }
        }
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
                    "use `== ok(...)` or `== err(...)` on a fallible result".to_string(),
                    format!(
                        "write `== {}(...)` or `== {}(...)` instead",
                        Syntax::LIT_OK,
                        Syntax::LIT_ERR
                    ),
                    Some(span),
                ));
                HashMap::new()
            }
            (
                Type::Named(type_name)
                | Type::Apply {
                    name: type_name, ..
                },
                Pattern::Struct { fields, rest, .. },
            ) => {
                let all_fields: Option<Vec<String>> = self
                    .struct_owner_module(type_name, None)
                    .and_then(|m| self.struct_fields_of(m, type_name))
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
                            crate::AST::PatSlot::Bind(name) => {
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
                            crate::AST::PatSlot::Bind(name) => {
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
                        crate::AST::PatSlot::Bind(name) => {
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
                    let StrMatchPart::Hole { name, ty, span: hole_span } = part else {
                        continue;
                    };
                    let bound_ty = match ty {
                        None => Type::String,
                        Some(t @ (Type::Int | Type::Float | Type::Bool | Type::String)) => {
                            t.clone()
                        }
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
                                Some(*hole_span),
                            ));
                            Type::String
                        }
                    };
                    result.insert(name.clone(), bound_ty);
                }
                result
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
                } else if !types_comparable(&lt, self.registry) {
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
