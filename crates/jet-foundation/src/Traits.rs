//! Trait registration & metadata, auto-derive checking, and trait codegen.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{
    self, e0902, e0903, e0906, e0907, e0908, e0913, sig_matches_trait, substitute_type,
    unify_types, BUILTIN_TRAITS, COMPARABLE, DEBUG, DECODE, DISPLAY, ENCODE, EQUATABLE, PRINTABLE,
    RENDERABLE, SERIALIZE,
};
use crate::Syntax;
use crate::AST::FuncSig;
use crate::AST::{
    AccessConvention, EnumDef, Func, ImplDef, Item, StructDef, TraitDef, TraitImplBlock,
    TraitMethodSig, Type, TypeParam,
};
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
    /// D-SERDE9/10: for each Codable type, the indices of its type params that
    /// reach the wire (a non-`#[Skip]` field type mentions them). Used at the use
    /// site so a non-codable type argument fails there (E2411), not on the
    /// emitted impl (I2). A param absent here is phantom/skip-only and is never
    /// required to be codable — `Id<Kind>` serializes regardless of `Kind`.
    pub serde_wire_params: HashMap<String, Vec<usize>>,
    pub local_traits: HashSet<String>,
    /// D-QUAL2: names declared with the `tag` keyword. A tag is a marker that
    /// erases at runtime and carries no methods, so it may not be `derive`d or
    /// used as a dispatching trait bound (E0731).
    pub local_tags: HashSet<String>,
    pub auto_printable: HashSet<String>,
    pub auto_debug: HashSet<String>,
    pub auto_equatable: HashSet<String>,
    /// D-ITER-HOOK: collection type → element type for `for x in coll`.
    pub iterable_items: HashMap<String, Type>,
    /// D-INDEX-HOOK: type → (key, value) for expert `[]` indexing.
    pub index_types: HashMap<String, (Type, Type)>,
    /// D-INDEX-HOOK: types that also implement `IndexMut`.
    pub index_mutable: HashSet<String>,
    /// D-INDEX-HOOK: `Index` assoc bindings per type for `IndexMut` validation.
    pub index_key_value: HashMap<String, (Type, Type)>,
    /// D-ERR-CONV: registered `(from_ty, to_ty)` error conversions.
    /// Maps (source_type_name, target_type_name) → the span where it was declared.
    /// Used for duplicate detection and orphan-rule checking.
    pub error_conversions: HashMap<(String, String), Span>,
}

#[derive(Debug, Clone)]
pub struct TraitInfo {
    pub methods: HashMap<String, TraitMethodSig>,
    /// D-LIB2: associated type names declared in the trait body (`type Item`).
    pub assoc_types: Vec<String>,
    pub span: Span,
}

impl TraitRegistry {
    pub fn register_items(&mut self, items: &[Item], diags: &mut Vec<Diagnostic>) {
        for item in items {
            match item {
                Item::Trait(t) => self.register_trait(t, diags),
                // D-QUAL2: record tag names so derives / bounds can reject them,
                // and flag any method written in a tag body (E0732).
                Item::Tag(t) => {
                    self.local_tags.insert(t.name.clone());
                    for m in &t.methods {
                        diags.push(Generics::e0732(&t.name, &m.name, m.name_span));
                    }
                }
                Item::Struct(s) => self.register_struct_meta(s),
                Item::Enum(e) => {
                    self.local_types.insert(e.name.clone());
                    self.enum_params
                        .insert(e.name.clone(), e.type_params.clone());
                    if !e.type_params.is_empty() {
                        // D-SERDE9/10: every variant payload type reaches the wire.
                        let wire_types: Vec<&Type> = e
                            .variants
                            .iter()
                            .flat_map(|v| match &v.payload {
                                crate::AST::VariantPayload::Unit => Vec::new(),
                                crate::AST::VariantPayload::Single(t, _) => vec![t],
                                crate::AST::VariantPayload::Named(fs) => {
                                    fs.iter().map(|f| &f.ty).collect()
                                }
                            })
                            .collect();
                        self.serde_wire_params.insert(
                            e.name.clone(),
                            wire_param_indices(&e.type_params, &wire_types),
                        );
                    }
                    for (t, _) in &e.derives {
                        self.derives
                            .entry(e.name.clone())
                            .or_default()
                            .insert(t.clone());
                    }
                }
                Item::TypeAlias(a) => {
                    self.local_types.insert(a.name.clone());
                    if !a.type_params.is_empty() {
                        self.struct_params
                            .insert(a.name.clone(), a.type_params.clone());
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
                // D-CAPBUNDLE1: a distinct type's capability bundles re-expose
                // base operations while keeping nominal identity (I8: reuse the
                // existing Display/derive machinery rather than a second
                // mechanism). `@Numeric` arithmetic is a separate, older gate
                // (D-DIST3/E0127) left untouched — see CheckerInfer/binary.rs.
                Item::Distinct(d) => {
                    if d.is_printable {
                        self.trait_impls
                            .insert((d.name.clone(), DISPLAY.to_string()));
                    }
                    if d.is_comparable {
                        self.derives
                            .entry(d.name.clone())
                            .or_default()
                            .insert(COMPARABLE.to_string());
                    }
                    if d.is_codable_as_base {
                        self.derives
                            .entry(d.name.clone())
                            .or_default()
                            .insert(ENCODE.to_string());
                        self.derives
                            .entry(d.name.clone())
                            .or_default()
                            .insert(DECODE.to_string());
                    }
                }
                _ => {}
            }
        }
        // D-QUAL2: `derive X` attaches method impls, so X must be a trait. A tag
        // has no methods — deriving one is E0731.
        for item in items {
            let derives: &[(String, Span)] = match item {
                Item::Struct(s) => &s.derives,
                Item::Enum(e) => &e.derives,
                _ => continue,
            };
            for (name, span) in derives {
                if self.local_tags.contains(name) {
                    diags.push(Generics::e0731(name, "`derive`", *span));
                }
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
        self.collect_iter_index_metadata(items);
    }

    fn collect_iter_index_metadata(&mut self, items: &[Item]) {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for item in items {
            match item {
                Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_ITERABLE) => {
                    if let Some(Type::Named(iter)) =
                        trait_assoc(items, &i.type_name, Syntax::TRAIT_ITERABLE, "Iter")
                    {
                        pairs.push((i.type_name.clone(), iter));
                    }
                }
                Item::Struct(s) => {
                    if let Some(Type::Named(iter)) =
                        trait_assoc(items, &s.name, Syntax::TRAIT_ITERABLE, "Iter")
                    {
                        pairs.push((s.name.clone(), iter));
                    }
                }
                Item::Enum(e) => {
                    if let Some(Type::Named(iter)) =
                        trait_assoc(items, &e.name, Syntax::TRAIT_ITERABLE, "Iter")
                    {
                        pairs.push((e.name.clone(), iter));
                    }
                }
                _ => {}
            }
        }
        for (coll, iter) in pairs {
            if let Some(item_ty) = trait_assoc(items, &iter, Syntax::TRAIT_ITERATOR, "Item") {
                self.iterable_items.insert(coll, item_ty);
            }
        }
        for item in items {
            let type_name = match item {
                Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_INDEX) => {
                    Some(i.type_name.clone())
                }
                Item::Struct(s)
                    if s.trait_impls
                        .iter()
                        .any(|b| b.trait_name == Syntax::TRAIT_INDEX) =>
                {
                    Some(s.name.clone())
                }
                Item::Enum(e)
                    if e.trait_impls
                        .iter()
                        .any(|b| b.trait_name == Syntax::TRAIT_INDEX) =>
                {
                    Some(e.name.clone())
                }
                _ => None,
            };
            if let Some(type_name) = type_name {
                let key = trait_assoc(items, &type_name, Syntax::TRAIT_INDEX, "Key");
                let value = trait_assoc(items, &type_name, Syntax::TRAIT_INDEX, "Value");
                if let (Some(key), Some(value)) = (key, value) {
                    self.index_types.insert(type_name.clone(), (key, value));
                    if self.implements_trait(&type_name, Syntax::TRAIT_INDEX_MUT) {
                        self.index_mutable.insert(type_name);
                    }
                }
            }
        }
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
                assoc_types: t.assoc_types.iter().map(|(n, _)| n.clone()).collect(),
                span: t.name_span,
            },
        );
    }

    fn register_struct_meta(&mut self, s: &StructDef) {
        self.local_types.insert(s.name.clone());
        if !s.type_params.is_empty() {
            self.struct_params
                .insert(s.name.clone(), s.type_params.clone());
            // D-SERDE9/10: record which params reach the wire (a non-`#[Skip]`
            // field type mentions them) for use-site codability checks.
            let wire_types: Vec<&Type> = s
                .fields
                .iter()
                .filter(|f| !f.serde_markers.iter().any(|m| m.name == Syntax::ATTR_SKIP))
                .map(|f| &f.ty)
                .collect();
            self.serde_wire_params.insert(
                s.name.clone(),
                wire_param_indices(&s.type_params, &wire_types),
            );
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
                self.validate_trait_impl(
                    &i.type_name,
                    trait_name,
                    &i.methods,
                    &i.assoc_type_impls,
                    i.type_span,
                    diags,
                );
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
            &block.assoc_type_impls,
            block.trait_span,
            diags,
        );
    }

    fn validate_trait_impl(
        &mut self,
        type_name: &str,
        trait_name: &str,
        methods: &[Func],
        assoc_type_impls: &[(String, Span, Type)],
        span: Span,
        diags: &mut Vec<Diagnostic>,
    ) {
        // D-QUAL2: an `impl Type: Tag { … }` attaches methods and dispatches, but
        // a tag is a marker with no methods — E0731.
        if self.local_tags.contains(trait_name) {
            diags.push(Generics::e0731(trait_name, "an `impl` block", span));
            return;
        }
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
            // D-LIB2: an impl supplies a concrete type for each of the trait's
            // associated types (`type Item = Int`). Resolve them so the abstract
            // method signatures match the impl's concrete ones.
            let assoc: HashMap<String, Type> = if trait_name == Syntax::TRAIT_INDEX_MUT {
                let mut merged: HashMap<String, Type> = assoc_type_impls
                    .iter()
                    .filter(|(name, _, _)| trait_info.assoc_types.contains(name))
                    .map(|(name, _, ty)| (name.clone(), ty.clone()))
                    .collect();
                if let Some((key, value)) = self.index_key_value.get(type_name) {
                    merged
                        .entry("Key".to_string())
                        .or_insert_with(|| key.clone());
                    merged
                        .entry("Value".to_string())
                        .or_insert_with(|| value.clone());
                }
                merged
            } else {
                assoc_type_impls
                    .iter()
                    .filter(|(name, _, _)| trait_info.assoc_types.contains(name))
                    .map(|(name, _, ty)| (name.clone(), ty.clone()))
                    .collect()
            };
            // Every associated type the trait declares must be bound by the impl.
            let missing_assoc: Vec<String> = trait_info
                .assoc_types
                .iter()
                .filter(|n| !assoc.contains_key(*n))
                .cloned()
                .collect();
            if !missing_assoc.is_empty() {
                // An unbound associated type leaves method sigs unresolvable, which
                // would spuriously trip E0907 on every method — report only E0913.
                diags.push(e0913(trait_name, &missing_assoc, span));
            } else {
                if trait_name == Syntax::TRAIT_INDEX {
                    if let (Some(key), Some(value)) = (assoc.get("Key"), assoc.get("Value")) {
                        self.index_key_value
                            .insert(type_name.to_string(), (key.clone(), value.clone()));
                    }
                }
                for m in methods {
                    if let Some(sig) = trait_info.methods.get(&m.name) {
                        let params: Vec<_> = m
                            .params
                            .iter()
                            .map(|p| (p.convention, p.ty.clone()))
                            .collect();
                        if !sig_matches_trait(&params, &m.return_type, sig, &assoc) {
                            diags.push(e0907(trait_name, &m.name, m.name_span));
                        }
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
                    self.auto_debug.insert(s.name.clone());
                    self.auto_equatable.insert(s.name.clone());
                }
                Item::Enum(e) if enum_auto_derive_ok(e) => {
                    self.auto_printable.insert(e.name.clone());
                    self.auto_debug.insert(e.name.clone());
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
            Type::TraitObject(vec![name.to_string()])
        } else {
            Type::Named(name.to_string())
        }
    }

    pub fn implements_trait(&self, type_name: &str, trait_name: &str) -> bool {
        // A primitive auto-satisfies the *built-in* trait family (Printable,
        // Comparable, Renderable, …) — that's always true for every scalar. It
        // must NOT auto-satisfy an arbitrary user-declared trait (D-ANY-JAI1
        // fix, c7jaiany): a primitive can't carry a user `impl Int.Loud { }`
        // (there's nothing to register it under), so falling through to the
        // ordinary `trait_impls` lookup below correctly says no.
        if matches!(
            type_name,
            Syntax::TYPE_INT
                | Syntax::TYPE_FLOAT
                | Syntax::TYPE_BOOL
                | Syntax::TYPE_STRING
                | Syntax::TYPE_CHAR
        ) && Generics::is_builtin_trait(trait_name)
        {
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
            DEBUG if self.auto_debug.contains(type_name) => true,
            DISPLAY => self
                .trait_impls
                .contains(&(type_name.to_string(), DISPLAY.to_string())),
            EQUATABLE if self.auto_equatable.contains(type_name) => true,
            COMPARABLE | SERIALIZE | ENCODE | DECODE => self
                .derives
                .get(type_name)
                .is_some_and(|d| d.contains(trait_name)),
            // D-CLIFLAG1: `@[Cli]` is a derive-trait name like the others above,
            // just not one of Generics's built-in constants (it's CLI-parsing
            // specific, not a wire/comparison trait) — same `derives` lookup.
            _ if trait_name == Syntax::CONTRACT_CLI => self
                .derives
                .get(type_name)
                .is_some_and(|d| d.contains(trait_name)),
            // D-ANY-JAI1: `Renderable` = anything codegen already gives a
            // `JetDisplay` impl — the S55 auto-printable derive, or an explicit
            // `impl Type.Display`.
            RENDERABLE => {
                self.auto_printable.contains(type_name)
                    || self
                        .trait_impls
                        .contains(&(type_name.to_string(), DISPLAY.to_string()))
            }
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
        // c148: build the declared-param name set so `unify_types` can recognize
        // multi-char type params (e.g. `Kind`) in addition to single-char ones.
        let tp_set: HashSet<String> = type_params.iter().map(|p| p.name.clone()).collect();
        let mut subst = HashMap::new();
        for (i, (_, pty)) in sig.params.iter().enumerate() {
            if let Some(arg_ty) = arg_types.get(i) {
                if !unify_types(pty, arg_ty, &mut subst, &tp_set) {
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
                let _ = unify_types(&inst_ret, expected, &mut subst, &tp_set);
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

    /// D-TXN-ROLLBACK layer 2: register the synthetic `Rollback` trait so
    /// `impl T: Rollback` is accepted + validated without the user writing
    /// `trait Rollback { … }`. Called BEFORE `register_items` on each module
    /// so the trait is already known when user impl blocks are validated.
    /// Guard: if the user already wrote `trait Rollback { … }`, skip.
    pub fn register_synthetic_rollback(&mut self) {
        if self.traits.contains_key(crate::Syntax::TRAIT_ROLLBACK) {
            return;
        }
        let dummy = Span { start: 0, end: 0 };
        // snapshot(self) -> Snapshot
        let snapshot_sig = TraitMethodSig {
            name: "snapshot".to_string(),
            name_span: dummy,
            params: vec![crate::AST::Param {
                name: crate::Syntax::KW_SELF.to_string(),
                name_span: dummy,
                ty: Type::Named(String::new()), // self placeholder
                ty_span: dummy,
                convention: AccessConvention::Read,
                default: None,
                variadic: false,
                variadic_bound_list: None,
            }],
            return_type: Some(Type::Named("Snapshot".to_string())),
            span: dummy,
            default_body: None,
            is_pure: false,
            declared_effects: None,
        };
        // restore(&self, snap: ^Snapshot)
        let restore_sig = TraitMethodSig {
            name: "restore".to_string(),
            name_span: dummy,
            params: vec![
                crate::AST::Param {
                    name: crate::Syntax::KW_SELF.to_string(),
                    name_span: dummy,
                    ty: Type::Named(String::new()),
                    ty_span: dummy,
                    convention: AccessConvention::Write,
                    default: None,
                    variadic: false,
                    variadic_bound_list: None,
                },
                crate::AST::Param {
                    name: "snap".to_string(),
                    name_span: dummy,
                    ty: Type::Named("Snapshot".to_string()),
                    ty_span: dummy,
                    convention: AccessConvention::Move,
                    default: None,
                    variadic: false,
                    variadic_bound_list: None,
                },
            ],
            return_type: None,
            span: dummy,
            default_body: None,
            is_pure: false,
            declared_effects: None,
        };
        let mut methods = HashMap::new();
        methods.insert("snapshot".to_string(), snapshot_sig);
        methods.insert("restore".to_string(), restore_sig);
        self.local_traits
            .insert(crate::Syntax::TRAIT_ROLLBACK.to_string());
        self.traits.insert(
            crate::Syntax::TRAIT_ROLLBACK.to_string(),
            TraitInfo {
                methods,
                assoc_types: vec!["Snapshot".to_string()],
                span: dummy,
            },
        );
    }

    /// D-DISPLAYDBG1: register synthetic `Display` + `Debug` protocol hooks.
    pub fn register_synthetic_display_debug(&mut self) {
        self.register_synthetic_trait_method(
            crate::Syntax::TRAIT_DISPLAY,
            "display",
            Some(Type::String),
            AccessConvention::Move,
        );
        // Debug is auto-derived; manual impl is allowed but uncommon.
        self.register_synthetic_trait_method(
            crate::Syntax::TRAIT_DEBUG,
            "debug",
            Some(Type::String),
            AccessConvention::Move,
        );
    }

    /// D-ITER-HOOK / D-INDEX-HOOK: register Iterable/Iterator/Index/IndexMut hooks.
    pub fn register_synthetic_iter_index(&mut self) {
        let dummy = Span { start: 0, end: 0 };
        if !self.traits.contains_key(crate::Syntax::TRAIT_ITERATOR) {
            let next_sig = TraitMethodSig {
                name: "next".to_string(),
                name_span: dummy,
                params: vec![crate::AST::Param {
                    name: crate::Syntax::KW_SELF.to_string(),
                    name_span: dummy,
                    ty: Type::Named(String::new()),
                    ty_span: dummy,
                    convention: AccessConvention::Write,
                    default: None,
                    variadic: false,
                    variadic_bound_list: None,
                }],
                return_type: Some(Type::Option(Box::new(Type::Named("Item".to_string())))),
                span: dummy,
                default_body: None,
                is_pure: false,
                declared_effects: None,
            };
            let mut methods = HashMap::new();
            methods.insert("next".to_string(), next_sig);
            self.local_traits
                .insert(crate::Syntax::TRAIT_ITERATOR.to_string());
            self.traits.insert(
                crate::Syntax::TRAIT_ITERATOR.to_string(),
                TraitInfo {
                    methods,
                    assoc_types: vec!["Item".to_string()],
                    span: dummy,
                },
            );
        }
        if !self.traits.contains_key(crate::Syntax::TRAIT_ITERABLE) {
            let iter_sig = TraitMethodSig {
                name: "iter".to_string(),
                name_span: dummy,
                params: vec![crate::AST::Param {
                    name: crate::Syntax::KW_SELF.to_string(),
                    name_span: dummy,
                    ty: Type::Named(String::new()),
                    ty_span: dummy,
                    convention: AccessConvention::Move,
                    default: None,
                    variadic: false,
                    variadic_bound_list: None,
                }],
                return_type: Some(Type::Named("Iter".to_string())),
                span: dummy,
                default_body: None,
                is_pure: false,
                declared_effects: None,
            };
            let mut methods = HashMap::new();
            methods.insert("iter".to_string(), iter_sig);
            self.local_traits
                .insert(crate::Syntax::TRAIT_ITERABLE.to_string());
            self.traits.insert(
                crate::Syntax::TRAIT_ITERABLE.to_string(),
                TraitInfo {
                    methods,
                    assoc_types: vec!["Iter".to_string()],
                    span: dummy,
                },
            );
        }
        if !self.traits.contains_key(crate::Syntax::TRAIT_INDEX) {
            let get_sig = TraitMethodSig {
                name: "get".to_string(),
                name_span: dummy,
                params: vec![
                    crate::AST::Param {
                        name: crate::Syntax::KW_SELF.to_string(),
                        name_span: dummy,
                        ty: Type::Named(String::new()),
                        ty_span: dummy,
                        convention: AccessConvention::Read,
                        default: None,
                        variadic: false,
                        variadic_bound_list: None,
                    },
                    crate::AST::Param {
                        name: "k".to_string(),
                        name_span: dummy,
                        ty: Type::Named("Key".to_string()),
                        ty_span: dummy,
                        convention: AccessConvention::Move,
                        default: None,
                        variadic: false,
                        variadic_bound_list: None,
                    },
                ],
                return_type: Some(Type::Option(Box::new(Type::Named("Value".to_string())))),
                span: dummy,
                default_body: None,
                is_pure: false,
                declared_effects: None,
            };
            let mut methods = HashMap::new();
            methods.insert("get".to_string(), get_sig);
            self.local_traits
                .insert(crate::Syntax::TRAIT_INDEX.to_string());
            self.traits.insert(
                crate::Syntax::TRAIT_INDEX.to_string(),
                TraitInfo {
                    methods,
                    assoc_types: vec!["Key".to_string(), "Value".to_string()],
                    span: dummy,
                },
            );
        }
        if !self.traits.contains_key(crate::Syntax::TRAIT_INDEX_MUT) {
            let set_sig = TraitMethodSig {
                name: "set".to_string(),
                name_span: dummy,
                params: vec![
                    crate::AST::Param {
                        name: crate::Syntax::KW_SELF.to_string(),
                        name_span: dummy,
                        ty: Type::Named(String::new()),
                        ty_span: dummy,
                        convention: AccessConvention::Write,
                        default: None,
                        variadic: false,
                        variadic_bound_list: None,
                    },
                    crate::AST::Param {
                        name: "k".to_string(),
                        name_span: dummy,
                        ty: Type::Named("Key".to_string()),
                        ty_span: dummy,
                        convention: AccessConvention::Move,
                        default: None,
                        variadic: false,
                        variadic_bound_list: None,
                    },
                    crate::AST::Param {
                        name: "v".to_string(),
                        name_span: dummy,
                        ty: Type::Named("Value".to_string()),
                        ty_span: dummy,
                        convention: AccessConvention::Move,
                        default: None,
                        variadic: false,
                        variadic_bound_list: None,
                    },
                ],
                return_type: None,
                span: dummy,
                default_body: None,
                is_pure: false,
                declared_effects: None,
            };
            let mut methods = HashMap::new();
            methods.insert("set".to_string(), set_sig);
            self.local_traits
                .insert(crate::Syntax::TRAIT_INDEX_MUT.to_string());
            self.traits.insert(
                crate::Syntax::TRAIT_INDEX_MUT.to_string(),
                TraitInfo {
                    methods,
                    assoc_types: vec![],
                    span: dummy,
                },
            );
        }
    }

    fn register_synthetic_trait_method(
        &mut self,
        trait_name: &str,
        method: &str,
        ret: Option<Type>,
        self_conv: AccessConvention,
    ) {
        if self.traits.contains_key(trait_name) {
            return;
        }
        let dummy = Span { start: 0, end: 0 };
        let sig = TraitMethodSig {
            name: method.to_string(),
            name_span: dummy,
            params: vec![crate::AST::Param {
                name: crate::Syntax::KW_SELF.to_string(),
                name_span: dummy,
                ty: Type::Named(String::new()),
                ty_span: dummy,
                convention: self_conv,
                default: None,
                variadic: false,
                variadic_bound_list: None,
            }],
            return_type: ret,
            span: dummy,
            default_body: None,
            is_pure: false,
            declared_effects: None,
        };
        let mut methods = HashMap::new();
        methods.insert(method.to_string(), sig);
        self.local_traits.insert(trait_name.to_string());
        self.traits.insert(
            trait_name.to_string(),
            TraitInfo {
                methods,
                assoc_types: Vec::new(),
                span: dummy,
            },
        );
    }
}

fn struct_auto_derive_ok(s: &StructDef) -> bool {
    !s.fields.is_empty() && s.fields.iter().all(|f| field_auto_ok(&f.ty, &s.name))
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

/// D-SERDE10: the indices of `params` that any of `wire_types` mentions — i.e.
/// the type params that reach the wire. A param no field type mentions is
/// phantom/skip-only and is omitted (it needs no serde bound).
fn wire_param_indices(params: &[TypeParam], wire_types: &[&Type]) -> Vec<usize> {
    let names: HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
    let mut mentioned: HashSet<String> = HashSet::new();
    for ty in wire_types {
        Generics::collect_type_param_mentions(ty, &names, &mut mentioned);
    }
    params
        .iter()
        .enumerate()
        .filter(|(_, p)| mentioned.contains(&p.name))
        .map(|(i, _)| i)
        .collect()
}

pub fn rust_type_name(ty: &Type) -> String {
    rust_type_name_assoc(ty, &HashSet::new())
}

/// Like `rust_type_name`, but renders a name in `assoc` (a trait's associated
/// types) as `Self::Name` rather than `user_Name`. Used inside a `trait`
/// declaration where `type Item` is in scope (D-LIB2).
pub fn rust_type_name_assoc(ty: &Type, assoc: &HashSet<String>) -> String {
    match ty {
        Type::Int => "i64".to_string(),
        Type::Float => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "String".to_string(),
        Type::Char => "char".to_string(),
        Type::List(inner) => format!("Vec<{}>", rust_type_name_assoc(inner, assoc)),
        Type::Named(n) if n.is_empty() => "Self".to_string(),
        Type::Named(n) if assoc.contains(n) => format!("Self::{n}"),
        Type::Named(n) => format!("user_{n}"),
        Type::Apply { name, args } => format!(
            "user_{name}<{}>",
            args.iter()
                .map(|a| rust_type_name_assoc(a, assoc))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        // Codegen only ever constructs a singleton `TraitObject` (see the type's
        // doc comment) — join defensively rather than assume, never panic on I2.
        Type::TraitObject(t) => format!(
            "Box<dyn {}>",
            t.iter().map(|n| format!("user_{n}")).collect::<Vec<_>>().join(" + ")
        ),
        Type::Option(inner) => format!("Option<{}>", rust_type_name_assoc(inner, assoc)),
        Type::Map { key, value } => format!(
            "std::collections::BTreeMap<{}, {}>",
            rust_type_name_assoc(key, assoc),
            rust_type_name_assoc(value, assoc)
        ),
        Type::Fn { .. } => "Box<dyn Fn()>".to_string(),
        _ => "()".to_string(),
    }
}

pub fn emit_trait_def(t: &TraitDef, out: &mut String) {
    out.push_str(&format!("pub trait user_{} {{\n", t.name));
    // D-LIB2: declare each associated type; method sigs below render uses of it
    // as `Self::Name`, and each impl emits `type Name = <concrete>;`.
    let assoc: HashSet<String> = t.assoc_types.iter().map(|(n, _)| n.clone()).collect();
    for (name, _) in &t.assoc_types {
        out.push_str(&format!("    type {name};\n"));
    }
    for m in &t.methods {
        let ret = m
            .return_type
            .as_ref()
            .map(|t| rust_type_name_assoc(t, &assoc))
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
                        AccessConvention::Write => "&mut self".to_string(),
                        AccessConvention::Move => "self".to_string(),
                        // D-CAP9: Share/Raw follow Read until specialized.
                        AccessConvention::Read | AccessConvention::Share | AccessConvention::Raw => {
                            "&self".to_string()
                        }
                    }
                } else {
                    // Match the convention applied by emit_trait_method / rust_param_type.
                    let base = rust_type_name_assoc(&p.ty, &assoc);
                    let rust_ty = match p.convention {
                        AccessConvention::Read | AccessConvention::Share | AccessConvention::Raw
                            if p.ty.is_scalar() =>
                        {
                            base
                        }
                        AccessConvention::Read | AccessConvention::Share | AccessConvention::Raw => {
                            format!("&{}", base)
                        }
                        AccessConvention::Write => format!("&mut {}", base),
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

fn trait_assoc(items: &[Item], type_name: &str, trait_name: &str, assoc: &str) -> Option<Type> {
    for item in items {
        match item {
            Item::Impl(i)
                if i.type_name == type_name && i.trait_name.as_deref() == Some(trait_name) =>
            {
                return i
                    .assoc_type_impls
                    .iter()
                    .find(|(n, _, _)| n == assoc)
                    .map(|(_, _, t)| t.clone());
            }
            Item::Struct(s) if s.name == type_name => {
                for block in &s.trait_impls {
                    if block.trait_name == trait_name {
                        return block
                            .assoc_type_impls
                            .iter()
                            .find(|(n, _, _)| n == assoc)
                            .map(|(_, _, t)| t.clone());
                    }
                }
            }
            Item::Enum(e) if e.name == type_name => {
                for block in &e.trait_impls {
                    if block.trait_name == trait_name {
                        return block
                            .assoc_type_impls
                            .iter()
                            .find(|(n, _, _)| n == assoc)
                            .map(|(_, _, t)| t.clone());
                    }
                }
            }
            _ => {}
        }
    }
    None
}
