//! Trait registration & metadata, auto-derive checking, and trait codegen.

use crate::AST::{
    AccessConvention, EnumDef, Func, ImplDef, Item, StructDef, TraitDef, TraitImplBlock,
    TraitMethodSig, Type, TypeParam,
};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{
    self, e0902, e0903, e0906, e0907, e0908, sig_matches_trait, substitute_type, unify_types,
    BUILTIN_TRAITS, COMPARABLE, EQUATABLE, PRINTABLE, SERIALIZE,
};
use crate::Sema::FuncSig;
use crate::Syntax;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default)]
pub struct TraitRegistry {
    pub traits: HashMap<String, TraitInfo>,
    pub trait_impls: HashSet<(String, String)>,
    pub struct_params: HashMap<String, Vec<TypeParam>>,
    pub enum_params: HashMap<String, Vec<TypeParam>>,
    pub fn_params: HashMap<String, Vec<TypeParam>>,
    pub derives: HashMap<String, HashSet<String>>,
    pub local_types: HashSet<String>,
    pub local_traits: HashSet<String>,
    pub auto_printable: HashSet<String>,
    pub auto_equatable: HashSet<String>,
    /// D-ERR-CONV: registered `(from_ty, to_ty)` error conversions.
    /// Maps (source_type_name, target_type_name) → the span where it was declared.
    /// Used for duplicate detection and orphan-rule checking.
    pub error_conversions: HashMap<(String, String), Span>,
}

#[derive(Debug, Clone)]
pub struct TraitInfo {
    pub methods: HashMap<String, TraitMethodSig>,
    pub span: Span,
}

impl TraitRegistry {
    pub fn register_items(&mut self, items: &[Item], diags: &mut Vec<Diagnostic>) {
        for item in items {
            match item {
                Item::Trait(t) => self.register_trait(t, diags),
                Item::Struct(s) => self.register_struct_meta(s),
                Item::Enum(e) => {
                    self.local_types.insert(e.name.clone());
                    self.enum_params
                        .insert(e.name.clone(), e.type_params.clone());
                    for (t, _) in &e.derives {
                        self.derives
                            .entry(e.name.clone())
                            .or_default()
                            .insert(t.clone());
                    }
                }
                Item::Func(f) => {
                    if !f.type_params.is_empty() {
                        self.fn_params.insert(f.name.clone(), f.type_params.clone());
                    }
                }
                Item::Impl(i) => self.register_impl(i, diags),
                Item::ErrorConv(ec) => {
                    self.register_error_conv(&ec.from_ty, &ec.to_ty, ec.from_span, diags);
                }
                _ => {}
            }
        }
        for item in items {
            if let Item::Struct(s) = item {
                self.register_struct_trait_impls(s, diags);
            }
            if let Item::Enum(e) = item {
                for block in &e.trait_impls {
                    self.register_trait_impl_block(&e.name, block, diags);
                }
            }
        }
        self.compute_auto_derives(items);
    }

    fn register_trait(&mut self, t: &TraitDef, diags: &mut Vec<Diagnostic>) {
        if BUILTIN_TRAITS.contains(&t.name.as_str()) {
            diags.push(Diagnostic::error(
                "E0106",
                format!("the name `{}` is built in and can't be redefined", t.name),
                "built-in traits like Printable and Comparable are always available".to_string(),
                "choose a different trait name".to_string(),
                Some(t.name_span),
            ));
            return;
        }
        if self.traits.contains_key(&t.name) {
            diags.push(Diagnostic::error(
                "E0105",
                format!("`{}` is defined twice", t.name),
                "every trait needs a unique name".to_string(),
                "rename or remove one of the definitions".to_string(),
                Some(t.name_span),
            ));
            return;
        }
        let methods = t
            .methods
            .iter()
            .map(|m| (m.name.clone(), m.clone()))
            .collect();
        self.local_traits.insert(t.name.clone());
        self.traits.insert(
            t.name.clone(),
            TraitInfo {
                methods,
                span: t.name_span,
            },
        );
    }

    fn register_struct_meta(&mut self, s: &StructDef) {
        self.local_types.insert(s.name.clone());
        if !s.type_params.is_empty() {
            self.struct_params
                .insert(s.name.clone(), s.type_params.clone());
        }
        for (t, _) in &s.derives {
            self.derives
                .entry(s.name.clone())
                .or_default()
                .insert(t.clone());
        }
    }

    fn register_impl(&mut self, i: &ImplDef, diags: &mut Vec<Diagnostic>) {
        if let Some(trait_name) = &i.trait_name {
            if i.delegation_field.is_some() {
                // S62: delegation — just register the impl pair; method completeness
                // is satisfied by the field, not by the methods vec (which is empty).
                // Full validation (field existence + field type implements trait) happens
                // in sema.rs after type registration.
                let key = (i.type_name.clone(), trait_name.clone());
                let _ = self.trait_impls.insert(key); // may already be there; ignore dup
            } else {
                self.validate_trait_impl(&i.type_name, trait_name, &i.methods, i.type_span, diags);
            }
        }
    }

    fn register_struct_trait_impls(&mut self, s: &StructDef, diags: &mut Vec<Diagnostic>) {
        for block in &s.trait_impls {
            self.register_trait_impl_block(&s.name, block, diags);
        }
    }

    fn register_trait_impl_block(
        &mut self,
        type_name: &str,
        block: &TraitImplBlock,
        diags: &mut Vec<Diagnostic>,
    ) {
        self.validate_trait_impl(
            type_name,
            &block.trait_name,
            &block.methods,
            block.trait_span,
            diags,
        );
    }

    fn validate_trait_impl(
        &mut self,
        type_name: &str,
        trait_name: &str,
        methods: &[Func],
        span: Span,
        diags: &mut Vec<Diagnostic>,
    ) {
        if Generics::is_builtin_trait(trait_name)
            && (trait_name == COMPARABLE || trait_name == EQUATABLE)
        {
            diags.push(e0903(trait_name, span));
            return;
        }
        let key = (type_name.to_string(), trait_name.to_string());
        if !self.trait_impls.insert(key) {
            diags.push(e0908(type_name, trait_name, span));
            return;
        }
        let local_type = !type_name.contains('.') && self.local_types.contains(type_name);
        let local_trait = !trait_name.contains('.')
            && (self.local_traits.contains(trait_name) || Generics::is_builtin_trait(trait_name));
        if !local_type && !local_trait {
            diags.push(e0902(span));
            return;
        }
        if let Some(trait_info) = self.traits.get(trait_name) {
            let provided: HashSet<String> = methods.iter().map(|m| m.name.clone()).collect();
            // D-LIB2: methods that have a default body in the trait don't need to be
            // provided by the implementor.
            let missing: Vec<String> = trait_info
                .methods
                .iter()
                .filter(|(k, sig)| !provided.contains(*k) && sig.default_body.is_none())
                .map(|(k, _)| k.clone())
                .collect();
            if !missing.is_empty() {
                diags.push(e0906(trait_name, &missing, span));
            }
            for m in methods {
                if let Some(sig) = trait_info.methods.get(&m.name) {
                    let params: Vec<_> = m
                        .params
                        .iter()
                        .map(|p| (p.convention, p.ty.clone()))
                        .collect();
                    if !sig_matches_trait(&params, &m.return_type, m.is_view_return, sig) {
                        diags.push(e0907(trait_name, &m.name, m.name_span));
                    }
                }
            }
        } else if !Generics::is_builtin_trait(trait_name) {
            diags.push(Diagnostic::error(
                "E0119",
                format!("there's no trait called `{trait_name}`"),
                "impl blocks need a trait that has been declared".to_string(),
                format!("add `trait {trait_name} {{ … }}` first"),
                Some(span),
            ));
        }
    }

    fn compute_auto_derives(&mut self, items: &[Item]) {
        for item in items {
            match item {
                Item::Struct(s) if struct_auto_derive_ok(s) => {
                    self.auto_printable.insert(s.name.clone());
                    self.auto_equatable.insert(s.name.clone());
                }
                Item::Enum(e) if enum_auto_derive_ok(e) => {
                    self.auto_printable.insert(e.name.clone());
                    self.auto_equatable.insert(e.name.clone());
                }
                _ => {}
            }
        }
    }

    pub fn is_trait_name(&self, name: &str) -> bool {
        self.traits.contains_key(name) || Generics::is_builtin_trait(name)
    }

    pub fn resolve_type_name(&self, name: &str, type_param_scope: &[TypeParam]) -> Type {
        if type_param_scope.iter().any(|p| p.name == name) {
            Type::Named(name.to_string())
        } else if self.is_trait_name(name) {
            Type::TraitObject(name.to_string())
        } else {
            Type::Named(name.to_string())
        }
    }

    pub fn implements_trait(&self, type_name: &str, trait_name: &str) -> bool {
        if matches!(
            type_name,
            Syntax::TYPE_INT
                | Syntax::TYPE_FLOAT
                | Syntax::TYPE_BOOL
                | Syntax::TYPE_STRING
                | Syntax::TYPE_CHAR
        ) {
            return true;
        }
        if self
            .trait_impls
            .contains(&(type_name.to_string(), trait_name.to_string()))
        {
            return true;
        }
        match trait_name {
            PRINTABLE if self.auto_printable.contains(type_name) => true,
            EQUATABLE if self.auto_equatable.contains(type_name) => true,
            COMPARABLE | SERIALIZE => self
                .derives
                .get(type_name)
                .is_some_and(|d| d.contains(trait_name)),
            _ => false,
        }
    }

    pub fn infer_fn_subst(
        &self,
        sig: &FuncSig,
        arg_types: &[Type],
        type_params: &[TypeParam],
        expected_ret: Option<&Type>,
    ) -> Result<HashMap<String, Type>, String> {
        if type_params.is_empty() {
            return Ok(HashMap::new());
        }
        let mut subst = HashMap::new();
        for (i, (_, pty)) in sig.params.iter().enumerate() {
            if let Some(arg_ty) = arg_types.get(i) {
                if !unify_types(pty, arg_ty, &mut subst) {
                    return Err(type_params
                        .first()
                        .map(|p| p.name.clone())
                        .unwrap_or_default());
                }
            }
        }
        if let Some(expected) = expected_ret {
            if let Some(ret) = &sig.return_type {
                let inst_ret = substitute_type(ret, &subst);
                let _ = unify_types(&inst_ret, expected, &mut subst);
            }
        }
        for p in type_params {
            if !subst.contains_key(&p.name) {
                return Err(p.name.clone());
            }
            for b in &p.bounds {
                if let Type::Named(concrete) = subst.get(&p.name).unwrap() {
                    if !self.implements_trait(concrete, b) {
                        return Err(p.name.clone());
                    }
                } else if let Type::Apply { name, .. } = subst.get(&p.name).unwrap() {
                    if !self.implements_trait(name, b) {
                        return Err(p.name.clone());
                    }
                }
            }
        }
        Ok(subst)
    }

    pub fn instantiate_type(&self, ty: &Type, subst: &HashMap<String, Type>) -> Type {
        substitute_type(ty, subst)
    }

    /// D-ERR-CONV: register a typed error conversion (Source → Target).
    /// Returns `Err(prev_span)` if a conversion for this pair already exists (E2405).
    /// Checks the orphan rule: at least one of `from_ty`/`to_ty` must be local.
    pub fn register_error_conv(
        &mut self,
        from_ty: &str,
        to_ty: &str,
        span: Span,
        diags: &mut Vec<Diagnostic>,
    ) {
        // Orphan rule (S28 analogue): at least one side must be defined in this program.
        let from_local = self.local_types.contains(from_ty);
        let to_local = self.local_types.contains(to_ty);
        if !from_local && !to_local {
            diags.push(Diagnostic::error(
                "E2406",
                format!(
                    "can't declare `impl {} -> {}` — neither type is defined in this program",
                    from_ty, to_ty
                ),
                "error conversions obey the same orphan rule as trait impls (S28): \
                 at least one of `Source` or `Target` must be a type you defined"
                    .to_string(),
                format!(
                    "define one of these types locally, or use `{}` (D-ERR2) if you don't own either type",
                    crate::Syntax::TRAIT_FALLIBLE
                ),
                Some(span),
            ));
            return;
        }
        let key = (from_ty.to_string(), to_ty.to_string());
        if let Some(prev) = self.error_conversions.get(&key) {
            let prev = *prev;
            diags.push(Diagnostic::error(
                "E2405",
                format!(
                    "duplicate error conversion: `impl {} -> {}` is already declared",
                    from_ty, to_ty
                ),
                "there can be at most one declared way to convert a `Source` error into a `Target`"
                    .to_string(),
                "remove one of the two `impl … -> …` blocks".to_string(),
                Some(span),
            ));
            let _ = prev; // the previous span could be added to the note in a future diagnostic upgrade
            return;
        }
        self.error_conversions.insert(key, span);
    }

    /// D-ERR-CONV: returns true if a declared `impl from_ty -> to_ty` exists.
    pub fn has_error_conv(&self, from_ty: &str, to_ty: &str) -> bool {
        self.error_conversions
            .contains_key(&(from_ty.to_string(), to_ty.to_string()))
    }
}

fn struct_auto_derive_ok(s: &StructDef) -> bool {
    !s.fields.is_empty()
        && s.fields
            .iter()
            .all(|f| !f.is_stored_ref && field_auto_ok(&f.ty, &s.name))
}

fn enum_auto_derive_ok(e: &EnumDef) -> bool {
    use crate::AST::VariantPayload;
    e.variants.iter().all(|v| match &v.payload {
        VariantPayload::Unit => true,
        VariantPayload::Single(t, _) => field_auto_ok(t, &e.name),
        VariantPayload::Named(fs) => fs.iter().all(|f| field_auto_ok(&f.ty, &e.name)),
    })
}

fn field_auto_ok(ty: &Type, owner: &str) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
        Type::List(inner) | Type::Option(inner) => field_auto_ok(inner, owner),
        Type::Named(n) => n != owner,
        Type::Apply { .. } => true,
        _ => false,
    }
}

pub fn rust_type_name(ty: &Type) -> String {
    match ty {
        Type::Int => "i64".to_string(),
        Type::Float => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "String".to_string(),
        Type::Char => "char".to_string(),
        Type::List(inner) => format!("Vec<{}>", rust_type_name(inner)),
        Type::Named(n) if n.is_empty() => "Self".to_string(),
        Type::Named(n) => format!("user_{n}"),
        Type::Apply { name, args } => format!(
            "user_{name}<{}>",
            args.iter()
                .map(rust_type_name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::TraitObject(t) => format!("Box<dyn user_{t}>"),
        Type::Option(inner) => format!("Option<{}>", rust_type_name(inner)),
        Type::Map { key, value } => format!(
            "std::collections::BTreeMap<{}, {}>",
            rust_type_name(key),
            rust_type_name(value)
        ),
        Type::Fn { .. } => "Box<dyn Fn()>".to_string(),
        _ => "()".to_string(),
    }
}

pub fn emit_trait_def(t: &TraitDef, out: &mut String) {
    out.push_str(&format!("pub trait user_{} {{\n", t.name));
    for m in &t.methods {
        // Thread is_view_return into the declared return type so the trait
        // declaration renders `-> &T`, matching the impl side's
        // rust_return_type(cx, t, is_view). Without this the trait decl emits
        // `-> T` while the impl emits `-> &T` → rustc E0053.
        let ret = m
            .return_type
            .as_ref()
            .map(|t| {
                let base = rust_type_name(t);
                if m.is_view_return {
                    format!("&{base}")
                } else {
                    base
                }
            })
            .unwrap_or_else(|| "()".to_string());
        let params: Vec<String> = m
            .params
            .iter()
            .map(|p| {
                if p.name == Syntax::KW_SELF {
                    // D-MUTSELF1: a `mut self` trait method declares `&mut self` so its
                    // impl may mutate the receiver in place (`(*self).field = v`); the
                    // impl side (emit_trait_method) renders the same receiver. `self` /
                    // `take self` stay `&self` / `self`.
                    match p.convention {
                        AccessConvention::Mutate => "&mut self".to_string(),
                        AccessConvention::Move => "self".to_string(),
                        AccessConvention::Read => "&self".to_string(),
                    }
                } else {
                    // Match the convention applied by emit_trait_method / rust_param_type.
                    let base = rust_type_name(&p.ty);
                    let rust_ty = match p.convention {
                        AccessConvention::Read if p.ty.is_scalar() => base,
                        AccessConvention::Read => format!("&{}", base),
                        AccessConvention::Mutate => format!("&mut {}", base),
                        AccessConvention::Move => base,
                    };
                    format!("_{}: {}", p.name, rust_ty)
                }
            })
            .collect();
        out.push_str(&format!(
            "    fn {}({}) -> {};\n",
            m.name,
            params.join(", "),
            ret
        ));
    }
    out.push_str("}\n\n");
}
