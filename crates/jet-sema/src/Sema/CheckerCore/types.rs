use crate::Generics::substitute_type;
use crate::Sema::{Checker, TypeRegistry};
use crate::AST::Type;
use std::collections::{HashMap, HashSet};
impl<'a> Checker<'a> {
    /// Builtin contracts yield only to the exact loaded Core declaration.
    pub(crate) fn source_owns_nominal(&self, name: &str) -> bool {
        let (namespace, leaf) = Self::split_type_name(name);
        let Some(owner) = self.struct_owner_module(leaf, namespace) else { return false };
        let registry = if owner == self.module_idx {
            self.registry
        } else {
            let Some(module) = self.modules.and_then(|modules| modules.get(owner)) else { return false };
            &module.registry
        };
        registry.contains(leaf) && registry.source_owns_type(leaf)
    }

    /// A loaded Core declaration owns its methods even when its carrier has
    /// not crossed the nominal ownership ledger.
    pub(crate) fn core_source_declares_method(&self, name: &str, method: &str) -> bool {
        let (namespace, leaf) = self.struct_type_name_parts(name);
        let Some(owner) = self.struct_owner_module(leaf, namespace) else { return false };
        let registry = if owner == self.module_idx {
            self.registry
        } else {
            let Some(module) = self.modules.and_then(|modules| modules.get(owner)) else { return false };
            &module.registry
        };
        registry.core_source_module.is_some() && registry.method(leaf, method).is_some()
    }

    /// Qualified native carriers are an internal Core vocabulary, not exports.
    pub(crate) fn core_private_native_type(&self, module: &str, leaf: &str) -> bool {
        self.module_is_core_source(self.module_idx)
            && jet_foundation::CoreModuleExports::core_source_module(module).is_some()
            && crate::Sema::CheckerCoreLib::core_type_known(leaf)
    }

    /// A raw projection uses the base registered by the declaring module, not
    /// a same-spelled type in the reader's registry.
    pub(crate) fn distinct_base_for_reader(&self, name: &str) -> Option<Type> {
        let (namespace, leaf) = Self::split_type_name(name);
        let owner = self.struct_owner_module(leaf, namespace)?;
        let registry = if owner == self.module_idx {
            self.registry
        } else {
            &self.modules?.get(owner)?.registry
        };
        let base = registry.distinct_base(leaf)?;
        Some(self.owner_type_for_reader(owner, leaf, base.clone()))
    }
    pub(crate) fn has_source_operator_left(&self, left: &Type, trait_name: &str) -> bool {
        let left_name = left.name();
        self.modules.is_some_and(|modules| modules.iter().any(|module|
            module.trait_reg.operator_impls.iter().any(|row|
                row.left == left_name && row.trait_name == trait_name
                && matches!(&row.rhs, Type::Named(name) | Type::Apply { name, .. }
                    if module.registry.source_owns_type(Self::split_type_name(name).1)))))
    }

    pub(crate) fn source_operator_row(
        &self, left: &Type, trait_name: &str, right: &Type,
    ) -> Option<(usize, &jet_foundation::Traits::OperatorImpl)> {
        let modules = self.modules?;
        for operand in [left, right] {
            let name = match operand {
                Type::Named(name) | Type::Apply { name, .. } => name,
                _ => continue,
            };
            if !self.source_owns_nominal(name) { continue; }
            let (namespace, leaf) = Self::split_type_name(name);
            let Some(owner) = self.struct_owner_module(leaf, namespace) else { continue; };
            let Some(module) = modules.get(owner) else { continue; };
            for row in &module.trait_reg.operator_impls {
                if row.trait_name != trait_name { continue; }
                let row_left = crate::Sema::Diagnostics::builtin_type_from_ident(&row.left)
                    .unwrap_or_else(|| Type::Named(row.left.clone()));
                let row_left = self.qualify_method_type(owner, &row_left, &HashSet::new());
                let row_right = self.qualify_method_type(owner, &row.rhs, &HashSet::new());
                if &row_left == left && &row_right == right {
                    return Some((owner, row));
                }
            }
        }
        None
    }

    /// D-TEXTHEAD-TYPE1=A: resolve a library-defined checked text through the
    /// ordinary nominal and trait registries. The returned name is canonical
    /// for imported types; the error comes from the ordinary trait impl.
    pub(crate) fn checked_text_type_name(&self, name: &str) -> Option<(String, Type)> {
        let (import_ns, leaf) = Self::split_type_name(name);
        let owner = self.struct_owner_module(leaf, import_ns)?;
        let (registry, trait_reg, items) = if owner == self.module_idx {
            (self.registry, self.trait_reg, self.items)
        } else {
            let module = self.modules?.get(owner)?;
            (&module.registry, &module.trait_reg, module.items.as_slice())
        };
        if registry.distinct_base(leaf) != Some(&Type::String)
            || !trait_reg.implements_trait(leaf, crate::Generics::CHECKED_TEXT)
        {
            return None;
        }
        let error = items.iter().find_map(|item| {
            let blocks = match item {
                crate::AST::Item::Impl(implementation)
                    if implementation.type_name == leaf
                        && implementation.trait_name.as_deref()
                            == Some(crate::Generics::CHECKED_TEXT) =>
                {
                    Some(implementation.assoc_type_impls.as_slice())
                }
                crate::AST::Item::Struct(definition) if definition.name == leaf => definition
                    .trait_impls
                    .iter()
                    .find(|block| block.trait_name == crate::Generics::CHECKED_TEXT)
                    .map(|block| block.assoc_type_impls.as_slice()),
                crate::AST::Item::Enum(definition) if definition.name == leaf => definition
                    .trait_impls
                    .iter()
                    .find(|block| block.trait_name == crate::Generics::CHECKED_TEXT)
                    .map(|block| block.assoc_type_impls.as_slice()),
                _ => None,
            }?;
            blocks
                .iter()
                .find(|(assoc, _, _)| assoc == "Error")
                .map(|(_, _, ty)| ty.clone())
        })?;
        let canonical = if owner == self.module_idx {
            leaf.to_string()
        } else {
            self.canonical_nominal_name(owner, leaf)
        };
        Some((canonical, error))
    }

    // Card #2185 owns the library-defined checked-text surface; this predicate
    // is the shared consumer guard for every nominal assignment boundary.
    pub(crate) fn is_checked_text_type_name(&self, name: &str) -> bool {
        self.checked_text_type_name(name).is_some()
    }

    pub(crate) fn type_alias_binding(
        &self, name: &str,
    ) -> Option<(usize, &'a [crate::AST::TypeParam], &'a Type)> {
        let registry: &'a TypeRegistry = self.registry;
        if let Some((params, target)) = registry.type_alias(name) {
            return Some((self.module_idx, params, target));
        }
        if registry.contains(name) {
            return None;
        }
        let (namespace, leaf) = Self::split_type_name(name);
        let owner = self.struct_owner_module(leaf, namespace)?;
        let modules = self.modules?;
        if let Some((params, target)) = modules.get(owner)?.registry.type_alias(leaf) {
            return Some((owner, params, target));
        }
        if namespace.is_none()
            && jet_foundation::CoreModuleExports::core_bare_source_module(leaf).is_some()
        {
            for declaration in self.name_ledger.declarations_named(leaf) {
                let alias_owner = declaration.module;
                let Some((params, target)) = modules[alias_owner].registry.type_alias(leaf) else { continue };
                if !params.is_empty() || !self.type_is_pub_in(alias_owner, leaf) {
                    continue;
                }
                if let Type::Named(target_identity) = self.bare_alias_target(alias_owner, target) {
                    let (namespace, target_leaf) = Self::split_type_name(&target_identity);
                    if target_leaf == leaf
                        && namespace.and_then(|namespace|
                            self.name_ledger.nominal_module(&format!("{namespace}::{target_leaf}"))) == Some(owner)
                    {
                        return Some((alias_owner, params, target));
                    }
                }
            }
        }
        None
    }

    pub(crate) fn bare_alias_target(&self, owner: usize, target: &Type) -> Type {
        if let Type::Named(name) = target {
            let (namespace, leaf) = Self::split_type_name(name);
            if let Some(namespace) = namespace.filter(|namespace| !namespace.contains("::")) {
                let module = self.modules.and_then(|modules| modules.get(owner));
                let (core_imports, imports) = if owner == self.module_idx {
                    (self.core_imports, self.imports)
                } else if let Some(module) = module {
                    (&module.core_imports, &module.imports)
                } else {
                    return target.clone();
                };
                let target_owner = core_imports.get(namespace)
                    .and_then(|module| self.core_source_owner_module(module, leaf))
                    .or_else(|| imports.get(namespace).copied());
                if let Some(target_owner) = target_owner {
                    return Type::Named(self.canonical_nominal_name(target_owner, leaf));
                }
            }
        }
        self.qualify_method_type(owner, target, &HashSet::new())
    }

    fn core_annotation_nominal_name(&self, module: &str, leaf: &str) -> String {
        self.core_source_owner_module(module, leaf)
            .map(|owner| self.canonical_nominal_name(owner, leaf))
            .unwrap_or_else(|| leaf.to_string())
    }

    fn bare_alias_core_annotation(&self, owner: usize, target: &Type) -> Option<Type> {
        let Type::Named(name) = target else { return None };
        let (namespace, leaf) = Self::split_type_name(name);
        let namespace = namespace?;
        let core_imports = if owner == self.module_idx {
            &self.core_imports
        } else {
            &self.modules?.get(owner)?.core_imports
        };
        let module = core_imports.get(namespace)?;
        if module == "core.crypto"
            || jet_foundation::CoreModuleExports::core_leaf_kind(module, leaf).is_none()
        {
            return None;
        }
        Some(Type::Named(self.core_annotation_nominal_name(module, leaf)))
    }

    fn imported_nominal_type(&self, name: &str) -> Option<Type> {
        let (import_ns, leaf) = Self::split_type_name(name);
        let owner = self.struct_owner_module(leaf, import_ns)?;
        if owner == self.module_idx {
            return Some(Type::Named(leaf.to_string()));
        }
        let module = self.modules?.get(owner)?;
        if module.trait_reg.is_trait_name(leaf) && !module.registry.contains(leaf) {
            Some(Type::TraitObject(vec![leaf.to_string()]))
        } else if module.registry.contains(leaf) {
            let canonical = self.canonical_nominal_name(owner, leaf);
            Some(Type::Named(canonical))
        } else {
            None
        }
    }

    /// A struct field keeps its declared spelling in the registry, so a bare
    /// trait name there is still `Named`. Lift every declared trait name to
    /// the S48 trait value, as `resolve_type` does for signatures, against the
    /// field's owning module: `config.adapter: Adapter?` then dispatches and
    /// coerces exactly like an `Adapter?` parameter.
    pub(crate) fn owner_trait_values(&self, owner: usize, ty: Type) -> Type {
        let (registry, trait_reg) = if owner == self.module_idx {
            (self.registry, self.trait_reg)
        } else {
            match self.modules.and_then(|modules| modules.get(owner)) {
                Some(module) => (&module.registry, &module.trait_reg),
                None => return ty,
            }
        };
        let traits = std::cell::RefCell::new(HashMap::new());
        ty.map_named_types(&|name| {
            if trait_reg.trait_info(name).is_some() && !registry.contains(name) {
                traits
                    .borrow_mut()
                    .insert(name.to_string(), Type::TraitObject(vec![name.to_string()]));
            }
            None
        });
        let traits = traits.into_inner();
        if traits.is_empty() {
            ty
        } else {
            substitute_type(&ty, &traits)
        }
    }

    /// A source-owned `core.crypto` struct under its owner's canonical
    /// identity, the one its signatures and methods carry. Enums keep their
    /// leaf spelling so contextual `.Variant` inference is unchanged.
    fn core_crypto_struct_type(&self, name: &str) -> Option<Type> {
        let (import_ns, leaf) = Self::split_type_name(name);
        let owner = self.struct_owner_module(leaf, import_ns)?;
        let module = self.modules?.get(owner)?;
        if module.registry.enum_variants(leaf).is_some() {
            return None;
        }
        self.imported_nominal_type(name)
    }

    pub(crate) fn imported_nominal_head(&self, name: &str) -> String {
        if name.contains("::") {
            return name.to_string();
        }
        if let Some((item, module)) = self.core_item_imports.get(name).zip(self.core_imports.get(name)) {
            if jet_foundation::CoreModuleExports::core_leaf_kind(module, item).is_some() {
                return self.core_source_owner_module(module, item)
                    .map(|owner| self.canonical_nominal_name(owner, item))
                    .unwrap_or_else(|| item.clone());
            }
        }
        let (import_ns, leaf) = Self::split_type_name(name);
        if let Some(module) = import_ns.and_then(|alias| self.core_imports.get(alias)) {
            if jet_foundation::CoreModuleExports::core_leaf_kind(module, leaf).is_some() {
                return self.core_source_owner_module(module, leaf)
                    .map(|owner| self.canonical_nominal_name(owner, leaf))
                    .unwrap_or_else(|| leaf.to_string());
            }
        }
        let Some(owner) = self.struct_owner_module(leaf, import_ns) else {
            return name.to_string();
        };
        if owner == self.module_idx {
            name.to_string()
        } else {
            self.canonical_nominal_name(owner, leaf)
        }
    }

    fn record_type_import_use(&mut self, ty: &Type) {
        let name = match ty {
            Type::Named(name) | Type::Apply { name, .. } => name,
            Type::TraitObject(names) => {
                for name in names {
                    self.record_type_import_name_use(name);
                }
                return;
            }
            _ => return,
        };
        self.record_type_import_name_use(name);
    }

    pub(crate) fn record_type_import_name_use(&mut self, name: &str) {
        let alias = name.split_once('.').map_or(name, |(alias, _)| alias);
        if self.lookup(alias).is_none()
            && (self.core_imports.contains_key(alias)
                || self.core_item_imports.contains_key(alias)
                || self.imports.contains_key(alias)
                || self
                    .name_ledger
                    .alias(self.module_idx, alias)
                    .is_some_and(|binding| binding.target_module.is_some()))
        {
            self.record_import_alias_use(alias);
        }
    }

    pub(crate) fn resolve_type(&mut self, ty: Type) -> Type {
        self.record_type_import_use(&ty);
        if let Type::Named(name) = &ty {
            if !self.type_param_scope.iter().any(|param| &param.name == name) {
                if let Some((owner, params, target)) = self.type_alias_binding(name) {
                    if params.is_empty() {
                        if let Some(target) = self.bare_alias_core_annotation(owner, target) {
                            return target;
                        }
                        let target = self.bare_alias_target(owner, target);
                        return self.resolve_type(target);
                    }
                }
            }
        }
        match ty {
            Type::Named(name) if self.type_param_scope.iter().any(|param| param.name == name) => {
                Type::Named(name)
            }
            // D-NUMOPS1: typed numeric heads such as `Float{…}` are
            // source spellings for primitive carriers, not nominal types.
            // Resolve them before ordinary nominal lookup so call seams
            // see `Type::Float` (and the matching fixed-width primitive).
            Type::Named(n) if crate::AST::numeric_type_from_name(&n).is_some() => {
                crate::AST::numeric_type_from_name(&n).expect("numeric head was checked")
            }
            // D-ENC-DYN1=A+: `JSON`/`TOML`/`YAML`/`CSV` are type aliases over the one
            // dynamic `Data` value — canonicalize every alias to `Data` so they unify.
            Type::Named(n) if crate::Syntax::is_data_type_name(&n) => {
                Type::Named(crate::Syntax::TYPE_DATA.to_string())
            }
            // D-BOUND-HEAD1=A: the shared typed-head descriptor owns the
            // source-to-nominal spelling for URL (`Url` remains internal).
            Type::Named(n)
                if crate::Syntax::typed_head_kind(&n)
                    .is_some_and(|kind| kind.internal_type_name() != n.as_str()) =>
            {
                Type::Named(
                    crate::Syntax::typed_head_kind(&n)
                        .expect("descriptor guard checked above")
                        .internal_type_name()
                        .to_string(),
                )
            }
            // ordinary generated enum declarations. Membership is decided by
            // the rule table, not a fixed leaf list, so it can't join the
            // generic Core-export table below.
            Type::Named(n)
                if self
                    .core_item_imports
                    .get(&n)
                    .zip(self.core_imports.get(&n))
                    .is_some_and(|(item, module)| {
                        crate::Sema::CheckerCoreLib::core_module_type_item(module, item)
                    }) =>
            {
                let module = self
                    .core_imports
                    .get(&n)
                    .expect("Core item imports have a module alias");
                if module == "core.crypto" {
                    // Source-owned Core crypto types use the owner's canonical
                    // identity, the one its signatures and methods carry.
                    self.core_crypto_struct_type(&n).unwrap_or_else(|| {
                        crate::Sema::Diagnostics::core_crypto_nominal(Type::Named(
                            self.core_item_imports.get(&n).cloned().unwrap_or(n),
                        ))
                    })
                } else {
                    let item = self.core_item_imports.get(&n).map_or(n.as_str(), String::as_str);
                    Type::Named(self.core_annotation_nominal_name(module, item))
                }
            }
            Type::Named(n)
                if n.rsplit_once('.').is_some_and(|(alias, leaf)| {
                    self.core_imports.get(alias).is_some_and(|module| {
                        module == "core.compiler.lang"
                            && crate::Policy::rule_arg_declaration(leaf).is_some()
                    })
                }) =>
            {
                Type::Named(n.rsplit_once('.').unwrap().1.to_string())
            }
            // Every other qualified Core import (crypto, encoding, email,
            // env, ...) resolves through one table of module -> exported
            // leaves (`jet_foundation::CoreModuleExports`). Adding a Core
            // module's exported types needs a table row, not a match arm.
            Type::Named(n)
                if n.rsplit_once('.').is_some_and(|(alias, leaf)| {
                    self.core_imports.get(alias).is_some_and(|module| {
                        jet_foundation::CoreModuleExports::core_leaf_kind(module, leaf).is_some()
                            || self.core_private_native_type(module, leaf)
                    })
                }) =>
            {
                let (alias, leaf) = n.rsplit_once('.').unwrap();
                let module = self.core_imports.get(alias).unwrap();
                if module == "core.crypto" {
                    self.core_crypto_struct_type(&n).unwrap_or_else(|| {
                        crate::Sema::Diagnostics::core_crypto_nominal(Type::Named(leaf.to_string()))
                    })
                } else {
                    match jet_foundation::CoreModuleExports::core_leaf_kind(module, leaf) {
                        Some(jet_foundation::CoreModuleExports::CoreLeafKind::Plain)
                        | Some(jet_foundation::CoreModuleExports::CoreLeafKind::Generic(_))
                        | Some(jet_foundation::CoreModuleExports::CoreLeafKind::Enum(_))
                        | Some(jet_foundation::CoreModuleExports::CoreLeafKind::CryptoNominal)
                        | None => Type::Named(
                            self.core_annotation_nominal_name(module, leaf),
                        ),
                    }
                }
            }
            Type::Named(n)
                if n.rsplit_once('.').is_some_and(|(alias, leaf)| {
                    self.imports.get(alias).is_some_and(|&idx| {
                        self.modules.is_some_and(|modules| {
                            modules[idx].registry.contains(leaf)
                                || modules[idx].trait_reg.is_trait_name(leaf)
                        })
                    })
                }) =>
            {
                let (_, leaf) = n.rsplit_once('.').unwrap();
                if self.modules.is_some_and(|modules| {
                    self.imports
                        .get(n.rsplit_once('.').unwrap().0)
                        .is_some_and(|&idx| {
                            !modules[idx].registry.contains(leaf)
                                && modules[idx].trait_reg.is_trait_name(leaf)
                        })
                }) {
                    Type::TraitObject(vec![leaf.to_string()])
                } else {
                    self.imported_nominal_type(&n)
                        .unwrap_or_else(|| Type::Named(n))
                }
            }
            Type::Named(n) if self.trait_reg.is_trait_name(&n) && !self.registry.contains(&n) => {
                Type::TraitObject(vec![n])
            }
            Type::Named(n) => self.imported_nominal_type(&n).unwrap_or(Type::Named(n)),
            Type::List(inner) => Type::List(Box::new(self.resolve_type(*inner))),
            Type::Shared(inner) => Type::Shared(Box::new(self.resolve_type(*inner))),
            Type::Apply { name, args } => {
                if let Some((owner, params, target)) = self.type_alias_binding(&name) {
                    let binders: HashSet<String> = params.iter().map(|param| param.name.clone()).collect();
                    let target = self.qualify_method_type(owner, target, &binders);
                    let subst: HashMap<String, Type> = params.iter().zip(args.iter().cloned())
                        .map(|(param, argument)| (param.name.clone(), argument)).collect();
                    return self.resolve_type(substitute_type(&target, &subst));
                }
                Type::Apply {
                    name: self.imported_nominal_head(&name),
                    args: args.into_iter().map(|a| self.resolve_type(a)).collect(),
                }
            }
            Type::Option(inner) => Type::Option(Box::new(self.resolve_type(*inner))),
            Type::Map {
                key,
                key_span,
                value,
            } => Type::Map {
                key: Box::new(self.resolve_type(*key)),
                key_span,
                value: Box::new(self.resolve_type(*value)),
            },
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(self.resolve_type(*ok)),
                err: Box::new(self.resolve_type(*err)),
            },
            Type::Tuple(fields) => Type::Tuple(
                fields
                    .into_iter()
                    .map(|(n, t)| (n, Box::new(self.resolve_type(*t))))
                    .collect(),
            ),
            Type::FixedList { elem, len } => Type::FixedList {
                elem: Box::new(self.resolve_type(*elem)),
                len,
            },
            Type::InlineRange { base, lo, hi } => Type::InlineRange {
                base: Box::new(self.resolve_type(*base)),
                lo,
                hi,
            },
            Type::Fn {
                params,
                ret,
                effect_bound,
                param_contract,
                call_metadata,
                return_view_provenance,
            } => Type::Fn {
                params: params.into_iter().map(|ty| self.resolve_type(ty)).collect(),
                ret: ret.map(|ty| Box::new(self.resolve_type(*ty))),
                effect_bound,
                param_contract,
                call_metadata,
                return_view_provenance,
            },
            Type::Tagged {
                marker:
                    crate::AST::TagMarker::Internal(crate::AST::InternalTag::CoreCryptoNominal),
                inner,
            } => Type::Tagged {
                marker: crate::AST::TagMarker::Internal(crate::AST::InternalTag::CoreCryptoNominal),
                inner,
            },
            Type::Tagged { marker, inner } => Type::Tagged {
                marker,
                inner: Box::new(self.resolve_type(*inner)),
            },
            Type::Union(members) => crate::AST::canonicalize_union(
                members.into_iter().map(|m| self.resolve_type(m)).collect(),
            ),
            Type::Int => Type::Int,
            Type::Float => Type::Float,
            Type::Bool => Type::Bool,
            Type::String => Type::String,
            Type::Char => Type::Char,
            Type::TraitObject(names) => Type::TraitObject(names),
            Type::IntN { signed, bits } => Type::IntN { signed, bits },
            Type::Float32 => Type::Float32,
            Type::Quantity { base, dimension } => Type::Quantity {
                base: Box::new(self.resolve_type(*base)),
                dimension,
            },
            Type::Measure(measure) => Type::Measure(measure),
        }
    }

    /// Resolve a callable's declared return before projecting its failure
    /// carrier. A transparent alias may itself name a `Result`; projecting
    /// first would add the implicit default carrier around that result.
    pub(crate) fn checked_return_types(
        &mut self,
        return_type: Option<Type>,
        raw_protocol_return: bool,
    ) -> (Type, Type) {
        let has_declared_return = return_type.is_some();
        let declared = return_type.map(|ty| self.resolve_type(ty));
        let source = declared.clone().unwrap_or_else(|| {
            if raw_protocol_return {
                Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string())
            } else {
                crate::AST::FailureContract::from_return_type(None).effective_type()
            }
        });
        let effective = if raw_protocol_return {
            source.clone()
        } else if has_declared_return {
            crate::AST::FailureContract::from_return_type(declared.as_ref()).effective_type()
        } else {
            source.clone()
        };
        (source, effective)
    }

    /// Publish the canonical declared types on the checked AST function so
    /// body checking, signatures, and TIR all project the same carriers.
    pub(crate) fn canonicalize_function_signature(
        &mut self,
        function: &mut crate::AST::Func,
    ) {
        for param in &mut function.params {
            if param.name != crate::Syntax::KW_SELF {
                let ty = std::mem::replace(&mut param.ty, Type::Int);
                param.ty = self.resolve_type(ty);
            }
        }
        function.return_type = function.return_type.take().map(|ty| self.resolve_type(ty));
    }

    /// D-FAILURE-FOUNDATION1=A: validate an explicit failure domain after
    /// resolving source/import spellings. The local registry remains the
    /// source of truth; this method only selects the owning registry for a
    /// public imported nominal so validation does not reject a valid imported
    /// `#Error` type.
    pub(crate) fn is_error_domain(&self, ty: &Type) -> bool {
        fn visit(checker: &Checker<'_>, ty: &Type) -> bool {
            if checker.registry.is_error_domain(ty) {
                return true;
            }
            match ty {
                Type::Union(members) => {
                    !members.is_empty() && members.iter().all(|member| visit(checker, member))
                }
                Type::Named(name) => {
                    let (_, leaf) = Checker::split_type_name(name);
                    checker
                        .imported_error_registry(name)
                        .is_some_and(|registry| {
                            registry.is_error_domain(&Type::Named(leaf.to_string()))
                        })
                }
                Type::Apply { name, args } => {
                    let (_, leaf) = Checker::split_type_name(name);
                    checker
                        .imported_error_registry(name)
                        .is_some_and(|registry| {
                            registry.is_error_domain(&Type::Apply {
                                name: leaf.to_string(),
                                args: args.clone(),
                            })
                        })
                }
                _ => false,
            }
        }

        visit(self, ty)
    }

    fn imported_error_registry(&self, name: &str) -> Option<&TypeRegistry> {
        let (import_ns, leaf) = Self::split_type_name(name);
        let owner = self.struct_owner_module(leaf, import_ns)?;
        if owner == self.module_idx {
            Some(self.registry)
        } else {
            self.modules?.get(owner).map(|module| &module.registry)
        }
    }

    pub(crate) fn type_param_has_bound(&self, ty: &Type, bound: &str) -> bool {
        match ty {
            Type::Named(n) => self
                .type_param_scope
                .iter()
                .find(|p| p.name == *n)
                .is_some_and(|p| p.bounds.iter().any(|b| b == bound)),
            _ => false,
        }
    }

    pub(crate) fn struct_subst(
        &self,
        type_name: &str,
        type_args: &[Type],
    ) -> HashMap<String, Type> {
        let (import_ns, leaf) = Self::split_type_name(type_name);
        if let Some(owner) = self.struct_owner_module(leaf, import_ns) {
            return self.struct_subst_for_owner(owner, leaf, type_args);
        }
        self.struct_subst_for_owner(self.module_idx, leaf, type_args)
    }

    /// The number of type parameters `owner_mod` declares for `type_name`.
    pub(crate) fn struct_type_param_count(&self, owner_mod: usize, type_name: &str) -> usize {
        let leaf = Self::split_type_name(type_name).1;
        if owner_mod == self.module_idx {
            self.trait_reg
                .struct_params
                .get(leaf)
                .or_else(|| self.trait_reg.enum_params.get(leaf))
                .map_or(0, |params| params.len())
        } else {
            self.modules
                .and_then(|modules| modules.get(owner_mod))
                .and_then(|module| {
                    module
                        .trait_reg
                        .struct_params
                        .get(leaf)
                        .or_else(|| module.trait_reg.enum_params.get(leaf))
                })
                .map_or(0, |params| params.len())
        }
    }

    pub(crate) fn struct_subst_for_owner(
        &self,
        owner_mod: usize,
        type_name: &str,
        type_args: &[Type],
    ) -> HashMap<String, Type> {
        let leaf = Self::split_type_name(type_name).1;
        let params = if owner_mod == self.module_idx {
            self.trait_reg
                .struct_params
                .get(leaf)
                .or_else(|| self.trait_reg.enum_params.get(leaf))
                .cloned()
        } else {
            self.modules.and_then(|modules| {
                modules.get(owner_mod).and_then(|module| {
                    module
                        .trait_reg
                        .struct_params
                        .get(leaf)
                        .or_else(|| module.trait_reg.enum_params.get(leaf))
                        .cloned()
                })
            })
        }
        .unwrap_or_default();
        if params.is_empty() {
            return HashMap::new();
        }
        if type_args.is_empty() {
            params
                .iter()
                .map(|p| (p.name.clone(), Type::Named(p.name.clone())))
                .collect()
        } else {
            params
                .iter()
                .zip(type_args.iter())
                .map(|(p, a)| (p.name.clone(), a.clone()))
                .collect()
        }
    }

    pub(crate) fn instantiate_type_for_owner(
        &self,
        owner_mod: usize,
        ty: &Type,
        subst: &HashMap<String, Type>,
    ) -> Type {
        // Resolve declared names in the field owner's import scope before
        // substitution; caller-supplied generic arguments retain their owner.
        let generic_names = subst.keys().cloned().collect();
        let ty = self.qualify_method_type(owner_mod, ty, &generic_names);
        if owner_mod == self.module_idx {
            self.trait_reg.instantiate_type(&ty, subst)
        } else {
            self.modules
                .and_then(|modules| modules.get(owner_mod))
                .map(|module| module.trait_reg.instantiate_type(&ty, subst))
                .unwrap_or_else(|| substitute_type(&ty, subst))
        }
    }
}
