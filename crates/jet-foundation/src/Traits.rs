//! Trait registration & metadata, auto-derive checking, and trait codegen.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{
    self, e0902, e0906, e0907, e0908, e0913, sig_matches_trait, substitute_type,
    unify_types, BUILTIN_TRAITS, COMPARABLE, DEBUG, DECODE, DISPLAY, ENCODE, EQUATABLE, PRINTABLE,
    CLOSE, RENDERABLE, SERIALIZE,
};
use crate::Syntax;
use crate::AST::FuncSig;
use crate::AST::{
    AccessConvention, EnumDef, Func, ImplDef, Item, ProgramBundle, StructDef, TraitDef,
    TraitImplBlock, TraitMethodSig, Type, TypeParam,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default)]
pub struct TraitRegistry {
    pub traits: HashMap<String, TraitInfo>,
    pub trait_impls: HashSet<(String, String)>,
    pub struct_params: HashMap<String, Vec<TypeParam>>,
    /// Generic struct parameters mentioned by stored fields. Structural
    /// Equatable/Comparable bridges constrain only these; phantom parameters
    /// do not participate in equality or ordering.
    pub structural_params: HashMap<String, Vec<usize>>,
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
    /// Compute automatic structural traits for one standalone item group.
    pub fn auto_derives_for_items(items: &[Item]) -> TraitRegistry {
        let mut registry = Self::default();
        registry.register_synthetic_display_debug();
        registry.register_items(items, &mut Vec::new());
        registry
    }

    /// Compute the three automatic structural traits for each bundle module.
    /// Module index is the nominal identity; an imported `alias.Type` resolves
    /// through the bundle's import-target table before its leaf-name facts are
    /// consulted. Bare names never borrow facts from another module.
    pub fn bundle_auto_derives(bundle: &ProgramBundle) -> Vec<TraitRegistry> {
        let mut registries: Vec<_> = bundle
            .modules
            .iter()
            .map(|module| Self::auto_derives_for_items(&module.items))
            .collect();
        loop {
            let snapshot = registries.clone();
            let mut changed = false;
            for (module_idx, module) in bundle.modules.iter().enumerate() {
                let imports: HashMap<String, usize> = module
                    .imports
                    .iter()
                    .filter_map(|import| {
                        bundle
                            .import_targets
                            .get(&(module_idx, import.span))
                            .copied()
                            .map(|target| (import.import_alias(), target))
                    })
                    .collect();
                changed |= registries[module_idx].compute_auto_derives_with(
                    &module.items,
                    |name, trait_name| {
                        let (alias, leaf) = name.split_once('.')?;
                        let target = *imports.get(alias)?;
                        Some(snapshot[target].implements_trait(leaf, trait_name))
                    },
                );
            }
            if !changed {
                break;
            }
        }
        let snapshot = registries.clone();
        for (module_idx, module) in bundle.modules.iter().enumerate() {
            for import in &module.imports {
                let Some(&target) = bundle.import_targets.get(&(module_idx, import.span)) else {
                    continue;
                };
                let alias = import.import_alias();
                registries[module_idx].auto_printable.extend(
                    snapshot[target]
                        .auto_printable
                        .iter()
                        .map(|leaf| format!("{alias}.{leaf}")),
                );
                registries[module_idx].auto_debug.extend(
                    snapshot[target]
                        .auto_debug
                        .iter()
                        .map(|leaf| format!("{alias}.{leaf}")),
                );
                registries[module_idx].auto_equatable.extend(
                    snapshot[target]
                        .auto_equatable
                        .iter()
                        .map(|leaf| format!("{alias}.{leaf}")),
                );
            }
        }
        registries
    }

    pub fn merge_auto_derives(&mut self, source: &TraitRegistry) {
        self.auto_printable
            .extend(source.auto_printable.iter().cloned());
        self.auto_debug.extend(source.auto_debug.iter().cloned());
        self.auto_equatable
            .extend(source.auto_equatable.iter().cloned());
    }

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
                // mechanism). `#Numeric` arithmetic is a separate, older gate
                // (D-DIST3/E0127) left untouched — see CheckerInfer/binary.rs.
                Item::Distinct(d) => {
                    self.auto_equatable.insert(d.name.clone());
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
                    if d.is_numeric && d.range.is_none() {
                        for trait_name in [
                            Syntax::TRAIT_ADD,
                            Syntax::TRAIT_SUB,
                            Syntax::TRAIT_MUL,
                            Syntax::TRAIT_DIV,
                        ] {
                            self.trait_impls
                                .insert((d.name.clone(), trait_name.to_string()));
                        }
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
            let (derives, markers): (&[(String, Span)], &[crate::AST::Marker]) = match item {
                Item::Struct(s) => (&s.derives, &s.type_markers),
                Item::Enum(e) => (&e.derives, &e.type_markers),
                _ => continue,
            };
            for (name, span) in derives {
                if self.local_tags.contains(name) {
                    diags.push(Generics::e0731(name, "`derive`", *span));
                } else if name == DEBUG
                    && !markers.iter().any(|marker| {
                        marker.name == DEBUG && !marker.negated && marker.name_span == *span
                    })
                {
                    diags.push(Generics::e0922(*span));
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
        self.reject_partial_comparable_derives(items, diags);
        self.compute_auto_derives(items);
        self.collect_iter_index_metadata(items);
    }

    fn reject_partial_comparable_derives(&self, items: &[Item], diags: &mut Vec<Diagnostic>) {
        for item in items {
            let (types, span) = match item {
                Item::Struct(s) => {
                    let Some((_, span)) = s.derives.iter().find(|(name, _)| name == COMPARABLE)
                    else {
                        continue;
                    };
                    (
                        s.fields
                            .iter()
                            .filter(|field| field.computed.is_none())
                            .map(|field| &field.ty)
                            .collect::<Vec<_>>(),
                        *span,
                    )
                }
                Item::Enum(e) => {
                    let Some((_, span)) = e.derives.iter().find(|(name, _)| name == COMPARABLE)
                    else {
                        continue;
                    };
                    let types = e
                        .variants
                        .iter()
                        .flat_map(|variant| match &variant.payload {
                            crate::AST::VariantPayload::Unit => Vec::new(),
                            crate::AST::VariantPayload::Single(ty, _) => vec![ty],
                            crate::AST::VariantPayload::Named(fields) => {
                                fields.iter().map(|field| &field.ty).collect()
                            }
                        })
                        .collect();
                    (types, *span)
                }
                Item::Distinct(d) if d.is_comparable => {
                    let span = d.comparable_span.unwrap_or(d.name_span);
                    (vec![&d.base], span)
                }
                _ => continue,
            };
            if let Some(offender) = types.iter().find_map(|ty| {
                self.partial_comparable_offender(ty, items, &mut HashSet::new())
            }) {
                diags.push(Generics::e0905(&offender, COMPARABLE, span, false));
            }
        }
    }

    fn partial_comparable_offender(
        &self,
        ty: &Type,
        items: &[Item],
        visiting: &mut HashSet<String>,
    ) -> Option<String> {
        match ty {
            Type::Int | Type::IntN { .. } | Type::Bool | Type::String | Type::Char => None,
            Type::Float | Type::Float32 => Some(ty.name()),
            Type::List(inner) | Type::Option(inner) | Type::FixedList { elem: inner, .. } => {
                self.partial_comparable_offender(inner, items, visiting)
            }
            Type::Result { ok, err } => self
                .partial_comparable_offender(ok, items, visiting)
                .or_else(|| self.partial_comparable_offender(err, items, visiting)),
            Type::Tuple(fields) => fields.iter().find_map(|(_, field)| {
                self.partial_comparable_offender(field, items, visiting)
            }),
            Type::Tagged { inner, .. } => {
                self.partial_comparable_offender(inner, items, visiting)
            }
            Type::Named(name) => {
                if name == Syntax::TYPE_FLOAT || name == "F32" {
                    return Some(name.clone());
                }
                let explicit = self
                    .trait_impls
                    .contains(&(name.clone(), COMPARABLE.to_string()));
                if explicit {
                    return None;
                }
                if !visiting.insert(name.clone()) {
                    return None;
                }
                let result = match items.iter().find(|item| match item {
                    Item::Struct(s) => s.name == *name,
                    Item::Enum(e) => e.name == *name,
                    Item::Distinct(d) => d.name == *name,
                    Item::TypeAlias(alias) => alias.name == *name,
                    _ => false,
                }) {
                    Some(Item::Struct(s)) => {
                        if !s.derives.iter().any(|(derive, _)| derive == COMPARABLE) {
                            Some(name.clone())
                        } else {
                            s.fields
                                .iter()
                                .filter(|field| field.computed.is_none())
                                .find_map(|field| {
                                    self.partial_comparable_offender(&field.ty, items, visiting)
                                })
                        }
                    }
                    Some(Item::Enum(e)) => {
                        if !e.derives.iter().any(|(derive, _)| derive == COMPARABLE) {
                            Some(name.clone())
                        } else {
                            e.variants.iter().find_map(|variant| match &variant.payload {
                                crate::AST::VariantPayload::Unit => None,
                                crate::AST::VariantPayload::Single(field, _) => {
                                    self.partial_comparable_offender(field, items, visiting)
                                }
                                crate::AST::VariantPayload::Named(fields) => fields.iter().find_map(
                                    |field| self.partial_comparable_offender(&field.ty, items, visiting),
                                ),
                            })
                        }
                    }
                    Some(Item::Distinct(d)) => {
                        if d.is_comparable {
                            self.partial_comparable_offender(&d.base, items, visiting)
                        } else {
                            Some(name.clone())
                        }
                    }
                    Some(Item::TypeAlias(alias)) => {
                        self.partial_comparable_offender(&alias.target, items, visiting)
                    }
                    _ => None,
                };
                visiting.remove(name);
                result
            }
            Type::Apply { name, args } => {
                if self
                    .trait_impls
                    .contains(&(name.clone(), COMPARABLE.to_string()))
                {
                    return None;
                }
                if !self.implements_trait(name, COMPARABLE) {
                    return Some(name.clone());
                }
                let Some(params) = self
                    .struct_params
                    .get(name)
                    .or_else(|| self.enum_params.get(name))
                else {
                    return self.partial_comparable_offender(
                        &Type::Named(name.clone()),
                        items,
                        visiting,
                    );
                };
                let subst: HashMap<String, Type> = params
                    .iter()
                    .zip(args)
                    .map(|(param, arg)| (param.name.clone(), arg.clone()))
                    .collect();
                if let Some(Item::Struct(s)) = items
                    .iter()
                    .find(|item| matches!(item, Item::Struct(s) if s.name == *name))
                {
                    return s
                        .fields
                        .iter()
                        .filter(|field| field.computed.is_none())
                        .find_map(|field| {
                            self.partial_comparable_offender(
                                &substitute_type(&field.ty, &subst),
                                items,
                                visiting,
                            )
                        });
                }
                self.partial_comparable_offender(&Type::Named(name.clone()), items, visiting)
            }
            other => Some(other.name()),
        }
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
            let stored_types: Vec<&Type> = s
                .fields
                .iter()
                .filter(|field| field.computed.is_none())
                .map(|field| &field.ty)
                .collect();
            self.structural_params.insert(
                s.name.clone(),
                wire_param_indices(&s.type_params, &stored_types),
            );
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
        let key = (type_name.to_string(), trait_name.to_string());
        if !self.trait_impls.insert(key) {
            diags.push(e0908(type_name, trait_name, span));
            return;
        }
        let local_type = !type_name.contains('.') && self.local_types.contains(type_name);
        let local_trait = !trait_name.contains('.')
            && (self.local_traits.contains(trait_name) || Generics::is_builtin_trait(trait_name));
        let operator_trait = matches!(
            trait_name,
            Syntax::TRAIT_ADD
                | Syntax::TRAIT_SUB
                | Syntax::TRAIT_MUL
                | Syntax::TRAIT_DIV
                | Syntax::TRAIT_EQUATABLE
                | Syntax::TRAIT_COMPARABLE
        );
        if operator_trait && !local_type {
            diags.push(e0902(span));
            return;
        }
        if !local_type && !local_trait {
            diags.push(e0902(span));
            return;
        }
        // D-SERDE2 (card #131 S1-bridge): `Encode`/`Decode` are built-in traits with no
        // entry in `self.traits`, so the generic signature check below never runs for a
        // hand `impl T.Encode`/`impl T.Decode`. Validate their fixed shapes here — a wrong
        // shape must be a sema error (E0906/E0907) BEFORE codegen, or the internal codec
        // bridge would emit Rust rustc rejects (I2/I4).
        if trait_name == ENCODE || trait_name == DECODE {
            self.check_serde_impl_methods(type_name, trait_name, methods, span, diags);
            return;
        }
        if operator_trait {
            let expected_method = match trait_name {
                Syntax::TRAIT_ADD => "add",
                Syntax::TRAIT_SUB => "sub",
                Syntax::TRAIT_MUL => "mul",
                Syntax::TRAIT_DIV => "div",
                Syntax::TRAIT_EQUATABLE => "equal",
                _ => "compare",
            };
            let Some(method) = methods.iter().find(|method| method.name == expected_method) else {
                diags.push(e0906(trait_name, &[expected_method.to_string()], span));
                for extra in methods {
                    diags.push(e0907(trait_name, expected_method, extra.name_span));
                }
                return;
            };
            let self_ok = method.params.first().is_some_and(|param| {
                param.name == Syntax::KW_SELF
                    && param.convention == AccessConvention::Read
                    && !param.variadic
                    && param.default.is_none()
            });
            let rhs_ok = method.params.get(1).is_some_and(|param| {
                param.convention == AccessConvention::Read
                    && param.ty == Type::Named(type_name.to_string())
                    && !param.variadic
                    && param.default.is_none()
            });
            let ret_ok = match trait_name {
                Syntax::TRAIT_EQUATABLE => method.return_type == Some(Type::Bool),
                Syntax::TRAIT_COMPARABLE => {
                    method.return_type
                        == Some(Type::Named(Syntax::TYPE_ORDERING.to_string()))
                }
                _ => method.return_type == Some(Type::Named(type_name.to_string())),
            };
            if methods.len() != 1
                || !self_ok
                || !rhs_ok
                || !ret_ok
                || !method.type_params.is_empty()
            {
                diags.push(e0907(trait_name, expected_method, method.name_span));
            }
            for extra in methods.iter().filter(|candidate| !std::ptr::eq(*candidate, method)) {
                diags.push(e0907(trait_name, expected_method, extra.name_span));
            }
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
        self.compute_auto_derives_with(items, |_, _| None);
    }

    fn compute_auto_derives_with(
        &mut self,
        items: &[Item],
        foreign_supports: impl Fn(&str, &str) -> Option<bool>,
    ) -> bool {
        let mut any_changed = false;
        for trait_name in [PRINTABLE, EQUATABLE, DEBUG] {
            loop {
                let mut changed = false;
                for item in items {
                    let (name, markers, default, qualifies) = match item {
                        Item::Struct(s) => (
                            &s.name,
                            &s.type_markers,
                            s.auto_derive_default,
                            struct_auto_derive_ok(s),
                        ),
                        Item::Enum(e) => (
                            &e.name,
                            &e.type_markers,
                            e.auto_derive_default,
                            enum_auto_derive_ok(e),
                        ),
                        _ => continue,
                    };
                    if !qualifies
                        || !auto_derive_requested(markers, trait_name, default)
                        || self
                            .trait_impls
                            .contains(&(name.clone(), trait_name.to_string()))
                        || !self.auto_derive_dependencies_ready(
                            item,
                            trait_name,
                            &foreign_supports,
                        )
                    {
                        continue;
                    }
                    changed |= match trait_name {
                        PRINTABLE => self.auto_printable.insert(name.clone()),
                        EQUATABLE => self.auto_equatable.insert(name.clone()),
                        DEBUG => self.auto_debug.insert(name.clone()),
                        _ => false,
                    };
                }
                any_changed |= changed;
                if !changed {
                    break;
                }
            }
        }
        any_changed
    }

    fn auto_derive_dependencies_ready(
        &self,
        item: &Item,
        trait_name: &str,
        foreign_supports: &impl Fn(&str, &str) -> Option<bool>,
    ) -> bool {
        let type_params = match item {
            Item::Struct(s) => &s.type_params,
            Item::Enum(e) => &e.type_params,
            _ => return false,
        };
        let supports = |ty: &Type| {
            self.auto_derive_type_ready(ty, trait_name, type_params, foreign_supports)
        };
        match item {
            Item::Struct(s) => s
                .fields
                .iter()
                .filter(|field| field.computed.is_none())
                .all(|field| supports(&field.ty)),
            Item::Enum(e) => e.variants.iter().all(|variant| match &variant.payload {
                crate::AST::VariantPayload::Unit => true,
                crate::AST::VariantPayload::Single(ty, _) => supports(ty),
                crate::AST::VariantPayload::Named(fields) => {
                    fields.iter().all(|field| supports(&field.ty))
                }
            }),
            _ => false,
        }
    }

    fn auto_derive_type_ready(
        &self,
        ty: &Type,
        trait_name: &str,
        type_params: &[TypeParam],
        foreign_supports: &impl Fn(&str, &str) -> Option<bool>,
    ) -> bool {
        match ty {
            Type::List(inner) | Type::Option(inner) | Type::FixedList { elem: inner, .. } => {
                self.auto_derive_type_ready(inner, trait_name, type_params, foreign_supports)
            }
            Type::Result { ok, err } => {
                self.auto_derive_type_ready(ok, trait_name, type_params, foreign_supports)
                    && self.auto_derive_type_ready(
                        err,
                        trait_name,
                        type_params,
                        foreign_supports,
                    )
            }
            Type::Map { key, value, .. } => {
                trait_name != EQUATABLE
                    && self.auto_derive_type_ready(
                        key,
                        trait_name,
                        type_params,
                        foreign_supports,
                    )
                    && self.auto_derive_type_ready(
                        value,
                        trait_name,
                        type_params,
                        foreign_supports,
                    )
            }
            Type::Tuple(fields) => fields
                .iter()
                .all(|(_, field)| {
                    self.auto_derive_type_ready(
                        field,
                        trait_name,
                        type_params,
                        foreign_supports,
                    )
                }),
            Type::Union(members) => members.iter().all(|member| {
                self.auto_derive_type_ready(
                    member,
                    trait_name,
                    type_params,
                    foreign_supports,
                )
            }),
            Type::Apply { name, args } => {
                foreign_supports(name, trait_name)
                    .unwrap_or_else(|| self.implements_trait(name, trait_name))
                    && args
                        .iter()
                        .all(|arg| {
                            self.auto_derive_type_ready(
                                arg,
                                trait_name,
                                type_params,
                                foreign_supports,
                            )
                        })
            }
            Type::Tagged { inner, .. } => {
                self.auto_derive_type_ready(inner, trait_name, type_params, foreign_supports)
            }
            Type::Named(name) if type_params.iter().any(|param| param.name == *name) => true,
            Type::Named(name) => foreign_supports(name, trait_name)
                .unwrap_or_else(|| self.implements_trait(name, trait_name)),
            Type::Int
            | Type::Float
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::IntN { .. }
            | Type::Float32 => true,
            Type::Shared(_) | Type::TraitObject(_) | Type::Fn { .. } => false,
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
            && trait_name != CLOSE
        {
            if trait_name == Syntax::TRAIT_COMPARABLE && type_name == Syntax::TYPE_FLOAT {
                return false;
            }
            if matches!(trait_name, Syntax::TRAIT_ADD | Syntax::TRAIT_SUB | Syntax::TRAIT_MUL | Syntax::TRAIT_DIV)
                && !matches!(type_name, Syntax::TYPE_INT | Syntax::TYPE_FLOAT)
            {
                return false;
            }
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
            EQUATABLE
                if self.auto_equatable.contains(type_name)
                    || self
                        .derives
                        .get(type_name)
                        .is_some_and(|derives| derives.contains(COMPARABLE)) =>
            {
                true
            }
            COMPARABLE | SERIALIZE | ENCODE | DECODE => self
                .derives
                .get(type_name)
                .is_some_and(|d| d.contains(trait_name)),
            // D-CLIFLAG1: `#[CLI]` is a derive-trait name like the others above,
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

    /// Whether a resolved type satisfies a trait bound. Keep structural types
    /// on the same path as ordinary trait-object assignment and inference.
    pub fn type_implements_trait(&self, ty: &Type, trait_name: &str) -> bool {
        match ty {
            Type::IntN { .. } => {
                Generics::is_builtin_trait(trait_name) && trait_name != CLOSE
            }
            Type::Float32 => {
                Generics::is_builtin_trait(trait_name)
                    && !matches!(trait_name, CLOSE | COMPARABLE)
            }
            Type::List(inner) | Type::Option(inner) | Type::FixedList { elem: inner, .. }
                if matches!(trait_name, EQUATABLE | COMPARABLE) =>
            {
                self.type_implements_trait(inner, trait_name)
            }
            Type::Result { ok, err } if matches!(trait_name, EQUATABLE | COMPARABLE) => {
                self.type_implements_trait(ok, trait_name)
                    && self.type_implements_trait(err, trait_name)
            }
            Type::Tuple(fields) if matches!(trait_name, EQUATABLE | COMPARABLE) => fields
                .iter()
                .all(|(_, field)| self.type_implements_trait(field, trait_name)),
            Type::Named(name) => self.implements_trait(name, trait_name),
            Type::Apply { name, args } => {
                if !self.implements_trait(name, trait_name) {
                    return false;
                }
                let Some(params) = self.struct_params.get(name) else {
                    return true;
                };
                params.len() == args.len()
                    && params.iter().zip(args).enumerate().all(|(index, (param, arg))| {
                        param
                            .bounds
                            .iter()
                            .all(|bound| self.type_implements_trait(arg, bound))
                            && (!matches!(trait_name, EQUATABLE | COMPARABLE)
                                || self.structural_params.get(name).is_some_and(|used| {
                                    !used.contains(&index)
                                        || self.type_implements_trait(arg, trait_name)
                                })
                                || (!self.structural_params.contains_key(name)
                                    && self.type_implements_trait(arg, trait_name)))
                    })
            }
            Type::TraitObject(bounds) => bounds.iter().any(|bound| bound == trait_name),
            Type::Tagged { inner, .. } => self.type_implements_trait(inner, trait_name),
            other => self.implements_trait(&other.name(), trait_name),
        }
    }

    pub fn infer_fn_subst(
        &self,
        sig: &FuncSig,
        arg_types: &[Type],
        type_params: &[TypeParam],
        expected_ret: Option<&Type>,
    ) -> Result<HashMap<String, Type>, String> {
        self.infer_subst(
            &sig.params,
            sig.return_type.as_ref(),
            arg_types,
            type_params,
            expected_ret,
        )
    }

    pub fn infer_fn_subst_without_bounds(
        &self,
        sig: &FuncSig,
        arg_types: &[Type],
        type_params: &[TypeParam],
        expected_ret: Option<&Type>,
    ) -> Result<HashMap<String, Type>, String> {
        self.infer_subst_inner(
            &sig.params,
            sig.return_type.as_ref(),
            arg_types,
            type_params,
            expected_ret,
            false,
        )
    }

    /// Ordinary one-way generic inference shared by functions and static
    /// constructors. Callers persist the resulting concrete arguments.
    pub fn infer_subst(
        &self,
        params: &[(AccessConvention, Type)],
        return_type: Option<&Type>,
        arg_types: &[Type],
        type_params: &[TypeParam],
        expected_ret: Option<&Type>,
    ) -> Result<HashMap<String, Type>, String> {
        self.infer_subst_inner(
            params,
            return_type,
            arg_types,
            type_params,
            expected_ret,
            true,
        )
    }

    fn infer_subst_inner(
        &self,
        params: &[(AccessConvention, Type)],
        return_type: Option<&Type>,
        arg_types: &[Type],
        type_params: &[TypeParam],
        expected_ret: Option<&Type>,
        check_bounds: bool,
    ) -> Result<HashMap<String, Type>, String> {
        if type_params.is_empty() {
            return Ok(HashMap::new());
        }
        // c148: build the declared-param name set so `unify_types` can recognize
        // multi-char type params (e.g. `Kind`) in addition to single-char ones.
        let tp_set: HashSet<String> = type_params.iter().map(|p| p.name.clone()).collect();
        let mut subst = HashMap::new();
        for (i, (_, pty)) in params.iter().enumerate() {
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
            if let Some(ret) = return_type {
                let inst_ret = substitute_type(ret, &subst);
                let _ = unify_types(&inst_ret, expected, &mut subst, &tp_set);
            }
        }
        for p in type_params {
            if !subst.contains_key(&p.name) {
                return Err(p.name.clone());
            }
            if check_bounds {
                for b in &p.bounds {
                    if !self.type_implements_trait(subst.get(&p.name).unwrap(), b) {
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
                    "can't declare `impl {} => {}` — neither type is defined in this program",
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
                    "duplicate error conversion: `impl {} => {}` is already declared",
                    from_ty, to_ty
                ),
                "there can be at most one declared way to convert a `Source` error into a `Target`"
                    .to_string(),
                "remove one of the two `impl … => …` blocks".to_string(),
                Some(span),
            ));
            let _ = prev; // the previous span could be added to the note in a future diagnostic upgrade
            return;
        }
        self.error_conversions.insert(key, span);
    }

    /// D-ARROW-CONTROL1: returns true if a declared `impl from_ty => to_ty` exists.
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
        // snapshot(self) => Snapshot
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
            return_view_provenance: Default::default(),
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
            return_view_provenance: Default::default(),
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
        // Keep sema's named-type admission tied to real prelude protocol
        // implementations. Compiler-known handles such as Range are types, but
        // do not become printable merely by being known to the compiler.
        for ty in [
            "ArgsSpec",
            "AriaRole",
            "BigInt",
            "BitSet",
            "BrowserError",
            "ByteBuffer",
            "Clock",
            "Closed",
            "CountMinSketch",
            "DBError",
            "DBValue",
            "DataError",
            "DataErrorKind",
            "DataTree",
            "Date",
            "DateTime",
            "Decimal",
            "DecodeError",
            "DirEntry",
            "DNSSrv",
            "Duration",
            "EncodingError",
            "EnvError",
            "EventResult",
            "F32x4",
            "F64x2",
            "Field",
            "FileLock",
            "GameAssets",
            "GameBackend",
            "GameFrame",
            "GameImage",
            "GameInputMap",
            "GameInputSnapshot",
            "GameReplay",
            "GameScene",
            "GameSound",
            "HTTPError",
            "HTTPHeaderName",
            "HTTPHeaderValue",
            "HTTPMethod",
            "HTTPRequest",
            "HTTPResponse",
            "HTTPRouter",
            "HTTPStatus",
            "HTTPVersion",
            "HyperLogLog",
            "Id",
            "Instant",
            "IOError",
            "IPAddr",
            "JSON",
            "JSONError",
            "Key",
            "LocalDate",
            "LocalTime",
            "Lru",
            "Mat3",
            "Mat4",
            "Measurement",
            "MIME",
            "NetError",
            "ParsedArgs",
            "Path",
            "Period",
            "Point",
            "Pool",
            "ProcessChild",
            "ProcessResult",
            "ProcessSpec",
            "Quat",
            "RangeError",
            "Rect",
            "ReflectField",
            "ReflectValue",
            "Regex",
            "RegexFlags",
            "RegexMatch",
            "ReservoirSampler",
            "Rng",
            "Size",
            "SocketAddr",
            "Solver",
            "Stat",
            "Stopwatch",
            "TcpListener",
            "TcpStream",
            "TDigest",
            "TempDir",
            "TempFile",
            "TextError",
            "TLSStream",
            "UDPPacket",
            "UdpSocket",
            "UnixListener",
            "UnixStream",
            "URL",
            "UTF8Error",
            "Value",
            "Vec2",
            "Vec3",
            "Vec4",
            "WalkEntry",
            "WatchEvent",
            "WsError",
            "Zone",
            "ZonedDateTime",
        ] {
            self.auto_printable.insert(ty.to_string());
        }
        for ty in [
            "BitSet",
            "ByteBuffer",
            "Clock",
            "Decimal",
            "GameImage",
            "GameSound",
            "Id",
            "IOError",
            "Lru",
            "Mat3",
            "Mat4",
            "Quat",
            "Vec2",
            "Vec3",
            "Vec4",
        ] {
            self.auto_debug.insert(ty.to_string());
        }
        // D-ENCSTREAM-SURFACE1=A: EncodingError Display is the exact stream error
        // projection law; Format/Kind/Cause/Error compare by value.
        self.trait_impls
            .insert(("EncodingError".to_string(), DISPLAY.to_string()));
        // D-DATAFLOW1=A: DataError Display is the typed analytics/stream error law.
        self.trait_impls
            .insert(("DataError".to_string(), DISPLAY.to_string()));
        for ty in [
            "EncodingFormat",
            "EncodingErrorKind",
            "EncodingCause",
            "EncodingError",
            "EncodingLimits",
            "DataError",
            "DataErrorKind",
            "DataLimits",
        ] {
            self.auto_equatable.insert(ty.to_string());
            self.auto_debug.insert(ty.to_string());
        }
    }

    /// D-SHAPE-RESOURCE2=A: one nominal consuming cleanup protocol. The
    /// ambient `close(^value)` call dispatches only through this trait.
    pub fn register_synthetic_close(&mut self) {
        self.register_synthetic_trait_method(
            crate::Syntax::TRAIT_CLOSE,
            crate::Syntax::RESOURCE_CLOSE,
            None,
            AccessConvention::Move,
        );
        for ty in [
            "FileReader", "FileWriter", "FileLock", "TcpStream", "UnixStream",
            "TLSStream", "DBConnection", "Arena", "Bump", "Pool", "Fixed",
        ] {
            self.trait_impls
                .insert((ty.to_string(), crate::Syntax::TRAIT_CLOSE.to_string()));
        }
    }

    /// D-OPDEF1=A: operator symbols are sugar over ordinary hook traits.
    pub fn register_synthetic_operators(&mut self) {
        for (trait_name, method, ret) in [
            (Syntax::TRAIT_ADD, "add", Type::Named(String::new())),
            (Syntax::TRAIT_SUB, "sub", Type::Named(String::new())),
            (Syntax::TRAIT_MUL, "mul", Type::Named(String::new())),
            (Syntax::TRAIT_DIV, "div", Type::Named(String::new())),
            (Syntax::TRAIT_EQUATABLE, "equal", Type::Bool),
            (
                Syntax::TRAIT_COMPARABLE,
                "compare",
                Type::Named(Syntax::TYPE_ORDERING.to_string()),
            ),
        ] {
            self.register_synthetic_binary_trait(trait_name, method, ret);
        }
    }

    /// D-NETIO-CONTRACT2=B: register one nominal byte-stream contract and the
    /// compiler-owned stream implementations. Runtime methods live on the same
    /// opaque handles; this metadata is the sema half of those implementations.
    pub fn register_synthetic_io(&mut self) {
        let dummy = Span { start: 0, end: 0 };
        let io_error = Type::Named(Syntax::TYPE_IO_ERROR.to_string());
        let bytes = Type::List(Box::new(Type::IntN { signed: false, bits: 8 }));
        let write_self = crate::AST::Param {
            name: Syntax::KW_SELF.to_string(),
            name_span: dummy,
            ty: Type::Named(String::new()),
            ty_span: dummy,
            convention: AccessConvention::Write,
            default: None,
            variadic: false,
            variadic_bound_list: None,
        };
        if !self.traits.contains_key(Syntax::TRAIT_IO_READER) {
            let mut methods = HashMap::new();
            methods.insert("read".to_string(), TraitMethodSig {
                name: "read".to_string(), name_span: dummy,
                params: vec![write_self.clone(), crate::AST::Param {
                    name: "limit".to_string(), name_span: dummy, ty: Type::Int,
                    ty_span: dummy, convention: AccessConvention::Move,
                    default: None, variadic: false, variadic_bound_list: None,
                }],
                return_type: Some(Type::Result { ok: Box::new(bytes.clone()), err: Box::new(io_error.clone()) }),
                span: dummy, default_body: None, is_pure: false, declared_effects: None,
                return_view_provenance: Default::default(),
            });
            self.local_traits.insert(Syntax::TRAIT_IO_READER.to_string());
            self.traits.insert(Syntax::TRAIT_IO_READER.to_string(), TraitInfo { methods, assoc_types: Vec::new(), span: dummy });
        }
        if !self.traits.contains_key(Syntax::TRAIT_IO_WRITER) {
            let bytes_param = crate::AST::Param {
                name: "bytes".to_string(), name_span: dummy, ty: bytes,
                ty_span: dummy, convention: AccessConvention::Read,
                default: None, variadic: false, variadic_bound_list: None,
            };
            let mut methods = HashMap::new();
            methods.insert("write".to_string(), TraitMethodSig {
                name: "write".to_string(), name_span: dummy,
                params: vec![write_self.clone(), bytes_param.clone()],
                return_type: Some(Type::Result { ok: Box::new(Type::Int), err: Box::new(io_error.clone()) }),
                span: dummy, default_body: None, is_pure: false, declared_effects: None,
                return_view_provenance: Default::default(),
            });
            methods.insert("write_all".to_string(), TraitMethodSig {
                name: "write_all".to_string(), name_span: dummy,
                params: vec![write_self, bytes_param],
                return_type: Some(Type::Result { ok: Box::new(Type::Named("Unit".to_string())), err: Box::new(io_error) }),
                span: dummy, default_body: None, is_pure: false, declared_effects: None,
                return_view_provenance: Default::default(),
            });
            self.local_traits.insert(Syntax::TRAIT_IO_WRITER.to_string());
            self.traits.insert(Syntax::TRAIT_IO_WRITER.to_string(), TraitInfo { methods, assoc_types: Vec::new(), span: dummy });
        }
        self.trait_impls.insert(("TcpStream".to_string(), Syntax::TRAIT_IO_READER.to_string()));
        self.trait_impls.insert(("TcpStream".to_string(), Syntax::TRAIT_IO_WRITER.to_string()));
        self.trait_impls.insert(("UnixStream".to_string(), Syntax::TRAIT_IO_READER.to_string()));
        self.trait_impls.insert(("UnixStream".to_string(), Syntax::TRAIT_IO_WRITER.to_string()));
        self.trait_impls.insert(("TLSStream".to_string(), Syntax::TRAIT_IO_READER.to_string()));
        self.trait_impls.insert(("TLSStream".to_string(), Syntax::TRAIT_IO_WRITER.to_string()));
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
                return_view_provenance: Default::default(),
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
                return_view_provenance: Default::default(),
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
                return_view_provenance: Default::default(),
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
                return_view_provenance: Default::default(),
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

    /// D-SERDE2 (card #131 S1-bridge): validate a hand `impl T.Encode`/`impl T.Decode`
    /// against the codec's fixed Jet-facing shape, so a wrong shape is a sema error before
    /// codegen bridges it to `jet_encode`/`jet_decode`.
    ///
    ///   `Encode`:  `fn encode(self) => Data`         (one `self` param, returns `Data`)
    ///   `Decode`:  `fn decode(tree: Data) => T ? DecodeError`
    ///              (static — no `self`; one `Data` param; returns the owning type or
    ///               `DecodeError`)
    fn check_serde_impl_methods(
        &self,
        type_name: &str,
        trait_name: &str,
        methods: &[Func],
        span: Span,
        diags: &mut Vec<Diagnostic>,
    ) {
        let verb = if trait_name == ENCODE { "encode" } else { "decode" };
        let is_data = |ty: &Type| matches!(ty, Type::Named(n) if Syntax::is_data_type_name(n));
        let mut saw_verb = false;
        for m in methods {
            if m.name != verb {
                // Only `encode`/`decode` belong in the codec impl (the trait owns exactly
                // one method); anything else can't be bridged.
                diags.push(e0907(trait_name, &m.name, m.name_span));
                continue;
            }
            saw_verb = true;
            let has_self = m.params.first().is_some_and(|p| p.name == Syntax::KW_SELF);
            let non_self: Vec<&crate::AST::Param> =
                m.params.iter().filter(|p| p.name != Syntax::KW_SELF).collect();
            let ok = if trait_name == ENCODE {
                // `encode(self) => Data`: exactly `self`, no other params, returns a Data.
                has_self
                    && non_self.is_empty()
                    && m.return_type.as_ref().is_some_and(is_data)
            } else {
                // `decode(tree: Data) => T ? DecodeError`: static, one `Data` param,
                // returns the owning type (or `Self`) or `DecodeError`.
                let ret_ok = matches!(
                    &m.return_type,
                    Some(Type::Result { ok, err })
                        if (matches!(ok.as_ref(), Type::Named(n) if n == type_name || n == "Self")
                            || matches!(ok.as_ref(), Type::Apply { name, .. } if name == type_name))
                            && matches!(err.as_ref(), Type::Named(n) if n == "DecodeError")
                );
                !has_self && non_self.len() == 1 && is_data(&non_self[0].ty) && ret_ok
            };
            if !ok {
                diags.push(e0907(trait_name, &m.name, m.name_span));
            }
        }
        if !saw_verb {
            diags.push(e0906(trait_name, &[verb.to_string()], span));
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
            return_view_provenance: Default::default(),
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

    fn register_synthetic_binary_trait(&mut self, trait_name: &str, method: &str, ret: Type) {
        if self.traits.contains_key(trait_name) { return; }
        let dummy = Span { start: 0, end: 0 };
        let param = |name: &str| crate::AST::Param {
            name: name.to_string(), name_span: dummy, ty: Type::Named(String::new()),
            ty_span: dummy, convention: AccessConvention::Read, default: None,
            variadic: false, variadic_bound_list: None,
        };
        let sig = TraitMethodSig {
            name: method.to_string(), name_span: dummy,
            params: vec![param(Syntax::KW_SELF), param("rhs")],
            return_type: Some(ret), span: dummy, default_body: None, is_pure: false,
            declared_effects: None, return_view_provenance: Default::default(),
        };
        self.local_traits.insert(trait_name.to_string());
        self.traits.insert(trait_name.to_string(), TraitInfo {
            methods: [(method.to_string(), sig)].into_iter().collect(),
            assoc_types: Vec::new(), span: dummy,
        });
    }
}

pub fn auto_derive_requested(
    markers: &[crate::AST::Marker],
    trait_name: &str,
    package_default: bool,
) -> bool {
    markers
        .iter()
        .rev()
        .find(|marker| marker.name == trait_name)
        .map_or(package_default, |marker| !marker.negated)
}

pub fn struct_auto_derive_ok(s: &StructDef) -> bool {
    !s.fields.is_empty() && s.fields.iter().all(|f| field_auto_ok(&f.ty, &s.name))
}

pub fn enum_auto_derive_ok(e: &EnumDef) -> bool {
    use crate::AST::VariantPayload;
    e.variants.iter().all(|v| match &v.payload {
        VariantPayload::Unit => true,
        VariantPayload::Single(t, _) => field_auto_ok(t, &e.name),
        VariantPayload::Named(fs) => fs.iter().all(|f| field_auto_ok(&f.ty, &e.name)),
    })
}

fn field_auto_ok(ty: &Type, owner: &str) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::IntN { .. }
        | Type::Float32 => true,
        Type::List(inner)
        | Type::Option(inner)
        | Type::Tagged { inner, .. }
        | Type::FixedList { elem: inner, .. } => field_auto_ok(inner, owner),
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            field_auto_ok(key, owner) && field_auto_ok(value, owner)
        }
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, field)| field_auto_ok(field, owner)),
        Type::Union(members) => members.iter().all(|member| field_auto_ok(member, owner)),
        Type::Named(n) => n != owner,
        Type::Apply { .. } => true,
        Type::Shared(_) | Type::TraitObject(_) | Type::Fn { .. } => false,
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
        Type::Apply { name, args } if name == "View" && args.len() == 1 => {
            if matches!(&args[0], Type::Named(inner) if inner == "str") {
                "&str".to_string()
            } else {
                format!("&[{}]", rust_type_name_assoc(&args[0], assoc))
            }
        }
        Type::Apply { name, args } if name == "ViewMut" && args.len() == 1 => {
            format!("&mut [{}]", rust_type_name_assoc(&args[0], assoc))
        }
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
            t.iter()
                .map(|n| format!("user_{n}"))
                .collect::<Vec<_>>()
                .join(" + ")
        ),
        Type::Option(inner) => format!("Option<{}>", rust_type_name_assoc(inner, assoc)),
        Type::Map { key, value, .. } => format!(
            "std::collections::BTreeMap<{}, {}>",
            rust_type_name_assoc(key, assoc),
            rust_type_name_assoc(value, assoc)
        ),
        Type::Fn { .. } => "Box<dyn Fn()>".to_string(),
        _ => "()".to_string(),
    }
}

fn add_view_lifetime(rust: String) -> String {
    if let Some(rest) = rust.strip_prefix("&mut ") {
        format!("&'__jet_view mut {rest}")
    } else if let Some(rest) = rust.strip_prefix('&') {
        format!("&'__jet_view {rest}")
    } else {
        rust
    }
}

pub fn emit_trait_def(
    t: &TraitDef,
    out: &mut String,
    render_view_return: impl Fn(&Type, &HashSet<String>) -> String,
) {
    out.push_str(&format!("pub trait user_{} {{\n", t.name));
    // D-LIB2: declare each associated type; method sigs below render uses of it
    // as `Self::Name`, and each impl emits `type Name = <concrete>;`.
    let assoc: HashSet<String> = t.assoc_types.iter().map(|(n, _)| n.clone()).collect();
    for (name, _) in &t.assoc_types {
        out.push_str(&format!("    type {name};\n"));
    }
    for m in &t.methods {
        let view_provenance = m.return_view_provenance.get();
        let has_view_return = view_provenance.is_some_and(|map| !map.is_empty());
        let borrows_receiver = view_provenance.is_some_and(|map| {
            map.values()
                .any(|p| matches!(p.source, crate::AST::ViewSource::Receiver))
        });
        let ret = m
            .return_type
            .as_ref()
            .map(|ty| {
                if has_view_return {
                    render_view_return(ty, &assoc)
                } else {
                    rust_type_name_assoc(ty, &assoc)
                }
            })
            .unwrap_or_else(|| "()".to_string());
        let mut param_index = 0usize;
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
                        AccessConvention::Write
                            if borrows_receiver =>
                        {
                            "&'__jet_view mut self".to_string()
                        }
                        AccessConvention::Read
                            if borrows_receiver =>
                        {
                            "&'__jet_view self".to_string()
                        }
                        AccessConvention::Write => "&mut self".to_string(),
                        AccessConvention::Move => "self".to_string(),
                        AccessConvention::Read => "&self".to_string(),
                    }
                } else {
                    // Match the convention applied by emit_trait_method / rust_param_type.
                    let base = rust_type_name_assoc(&p.ty, &assoc);
                    let mut rust_ty = match p.convention {
                        AccessConvention::Read if p.ty.is_scalar() => {
                            base
                        }
                        AccessConvention::Read => format!("&{}", base),
                        AccessConvention::Write => format!("&mut {}", base),
                        AccessConvention::Move => base,
                    };
                    if view_provenance.is_some_and(|map| map.values().any(|p| {
                        matches!(p.source, crate::AST::ViewSource::Parameter(index) if index == param_index)
                    })) {
                        rust_ty = add_view_lifetime(rust_ty);
                    }
                    param_index += 1;
                    format!("_{}: {}", p.name, rust_ty)
                }
            })
            .collect();
        out.push_str(&format!(
            "    fn {}{}({}) -> {};\n",
            m.name,
            if has_view_return { "<'__jet_view>" } else { "" },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sized_scalars_only_satisfy_builtin_traits() {
        let traits = TraitRegistry::default();
        for ty in [Type::IntN { signed: false, bits: 8 }, Type::Float32] {
            assert!(traits.type_implements_trait(&ty, PRINTABLE));
            assert!(!traits.type_implements_trait(&ty, "UserTrait"));
        }
        assert!(!traits.type_implements_trait(&Type::Float32, COMPARABLE));
        assert!(!traits.implements_trait("Float", COMPARABLE));
    }
}
