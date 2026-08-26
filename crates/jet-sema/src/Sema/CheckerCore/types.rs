use crate::Generics::substitute_type;
use crate::Sema::{Checker, TypeRegistry};
use crate::AST::Type;
use std::collections::HashMap;
impl<'a> Checker<'a> {
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
                            == Some(crate::Generics::CHECKED_TEXT) => {
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

    fn imported_nominal_type(&self, name: &str) -> Option<Type> {
        let (import_ns, leaf) = Self::split_type_name(name);
        let owner = self.struct_owner_module(leaf, import_ns)?;
        if owner == self.module_idx {
            return None;
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

    pub(crate) fn imported_nominal_head(&self, name: &str) -> String {
        if name.contains("::") {
            return name.to_string();
        }
        let (import_ns, leaf) = Self::split_type_name(name);
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

    fn record_type_import_name_use(&mut self, name: &str) {
        let alias = name.split_once('.').map_or(name, |(alias, _)| alias);
        if self.lookup(alias).is_none()
            && (self.core_imports.contains_key(alias)
                || self.core_item_imports.contains_key(alias)
                || self.imports.contains_key(alias))
        {
            self.record_import_alias_use(alias);
        }
    }

    pub(crate) fn resolve_type(&mut self, ty: Type) -> Type {
        self.record_type_import_use(&ty);
        match ty {
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
            // D-LANGNS-NAME1=A: `core.compiler.lang` publishes compiler vocabulary as
            // ordinary generated enum declarations. Membership is decided by
            // the rule table, not a fixed leaf list, so it can't join the
            // generic Core-export table below.
            Type::Named(n)
                if n.split_once('.').is_some_and(|(alias, leaf)| {
                    self.core_imports.get(alias).is_some_and(|module| {
                        module == "core.compiler.lang"
                            && crate::Policy::rule_arg_declaration(leaf).is_some()
                    })
                }) =>
            {
                Type::Named(n.split_once('.').unwrap().1.to_string())
            }
            // Every other qualified Core import (crypto, encoding, email,
            // env, ...) resolves through one table of module -> exported
            // leaves (`jet_foundation::CoreModuleExports`). Adding a Core
            // module's exported types needs a table row, not a match arm.
            Type::Named(n)
                if n.split_once('.').is_some_and(|(alias, leaf)| {
                    self.core_imports.get(alias).is_some_and(|module| {
                        jet_foundation::CoreModuleExports::core_leaf_kind(module, leaf).is_some()
                    })
                }) =>
            {
                let (alias, leaf) = n.split_once('.').unwrap();
                let module = self.core_imports.get(alias).unwrap();
                match jet_foundation::CoreModuleExports::core_leaf_kind(module, leaf) {
                    Some(jet_foundation::CoreModuleExports::CoreLeafKind::CryptoNominal) => {
                        crate::Sema::Diagnostics::core_crypto_nominal(Type::Named(leaf.to_string()))
                    }
                    Some(jet_foundation::CoreModuleExports::CoreLeafKind::Plain) | None => {
                        Type::Named(leaf.to_string())
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
                        .is_some_and(|&idx| modules[idx].trait_reg.is_trait_name(leaf))
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
                if self.registry.is_type_alias(&name) {
                    if let Some((params, target)) = self.registry.type_alias(&name) {
                        let subst: std::collections::HashMap<String, Type> = params
                            .iter()
                            .zip(args.iter().cloned())
                            .map(|(p, a)| (p.name.clone(), a))
                            .collect();
                        let expanded = substitute_type(target, &subst);
                        return self.resolve_type(expanded);
                    }
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
        if owner_mod == self.module_idx {
            self.trait_reg.instantiate_type(ty, subst)
        } else {
            self.modules
                .and_then(|modules| modules.get(owner_mod))
                .map(|module| module.trait_reg.instantiate_type(ty, subst))
                .unwrap_or_else(|| substitute_type(ty, subst))
        }
    }
}
