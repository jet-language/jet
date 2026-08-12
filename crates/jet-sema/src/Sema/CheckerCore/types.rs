use crate::AST::Type;
use crate::Generics::substitute_type;
use crate::Sema::Checker;
use std::collections::HashMap;
impl<'a> Checker<'a> {
        pub(crate) fn resolve_type(&self, ty: Type) -> Type {
            match ty {
                // D-ENC-DYN1=A+: `JSON`/`TOML`/`YAML`/`CSV` are type aliases over the one
                // dynamic `Data` value — canonicalize every alias to `Data` so they unify.
                Type::Named(n) if crate::Syntax::is_data_type_name(&n) => {
                    Type::Named(crate::Syntax::TYPE_DATA.to_string())
                }
                // D-BOUND-HEAD1=A: `URL` is the canonical source spelling;
                // keep the existing `Url` nominal in semantic and generated
                // value types.
                Type::Named(n) if n == crate::Syntax::TYPE_URL => {
                    Type::Named("Url".to_string())
                }
                // D-LANGNS-NAME1=A: `core.lang` publishes compiler vocabulary as
                // ordinary generated enum declarations. Membership is decided by
                // the rule table, not a fixed leaf list, so it can't join the
                // generic Core-export table below.
                Type::Named(n)
                    if n.split_once('.').is_some_and(|(alias, leaf)| {
                        self.core_imports.get(alias).is_some_and(|module| {
                            module == "core.lang"
                                && crate::Policy::rule_arg_declaration(leaf).is_some()
                        })
                    }) => Type::Named(n.split_once('.').unwrap().1.to_string()),
                // Every other qualified Core import (crypto, encoding, email,
                // env, ...) resolves through one table of module -> exported
                // leaves (`jet_foundation::CoreModuleExports`). Adding a Core
                // module's exported types needs a table row, not a match arm.
                Type::Named(n)
                    if n.split_once('.').is_some_and(|(alias, leaf)| {
                        self.core_imports.get(alias).is_some_and(|module| {
                            jet_foundation::CoreModuleExports::core_leaf_kind(module, leaf)
                                .is_some()
                        })
                    }) =>
                {
                    let (alias, leaf) = n.split_once('.').unwrap();
                    let module = self.core_imports.get(alias).unwrap();
                    match jet_foundation::CoreModuleExports::core_leaf_kind(module, leaf) {
                        Some(jet_foundation::CoreModuleExports::CoreLeafKind::CryptoNominal) => {
                            crate::Sema::Diagnostics::core_crypto_nominal(Type::Named(
                                leaf.to_string(),
                            ))
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
                    }) => {
                        let (_, leaf) = n.rsplit_once('.').unwrap();
                        if self.modules.is_some_and(|modules| {
                            self.imports
                                .get(n.rsplit_once('.').unwrap().0)
                                .is_some_and(|&idx| modules[idx].trait_reg.is_trait_name(leaf))
                        }) {
                            Type::TraitObject(vec![leaf.to_string()])
                        } else {
                            // File-module qualification is nominal identity.
                            // A local type may deliberately use the same leaf.
                            Type::Named(n)
                        }
                    }
                Type::Named(n) if self.trait_reg.is_trait_name(&n) && !self.registry.contains(&n) => {
                    Type::TraitObject(vec![n])
                }
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
                        name,
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
                Type::FixedList { elem, len, len_symbol } => Type::FixedList {
                    elem: Box::new(self.resolve_type(*elem)),
                    len,
                    len_symbol,
                },
Type::Fn { params, ret, effect_bound, param_contract, return_view_provenance } => Type::Fn {
                    param_contract: param_contract.clone(),
                    params: params.into_iter().map(|ty| self.resolve_type(ty)).collect(),
                    ret: ret.map(|ty| Box::new(self.resolve_type(*ty))),
                    effect_bound,
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
                Type::Named(name) => Type::Named(name),
                Type::TraitObject(names) => Type::TraitObject(names),
                Type::IntN { signed, bits } => Type::IntN { signed, bits },
                Type::Float32 => Type::Float32,
                Type::Quantity { base, dimension } => Type::Quantity {
                    base: Box::new(self.resolve_type(*base)),
                    dimension,
                },
                Type::ComputeDim(value) => Type::ComputeDim(value),
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
            let (import_ns, lookup_name) = self.struct_type_name_parts(type_name);
            let params = self
                .struct_owner_module(lookup_name, import_ns)
                .and_then(|owner_mod| {
                    if owner_mod == self.module_idx {
                        self.trait_reg
                            .struct_params
                            .get(lookup_name)
                            .or_else(|| self.trait_reg.enum_params.get(lookup_name))
                    } else {
                        self.modules.and_then(|modules| {
                            modules
                                .get(owner_mod)
                                .and_then(|module| {
                                    module
                                        .trait_reg
                                        .struct_params
                                        .get(lookup_name)
                                        .or_else(|| module.trait_reg.enum_params.get(lookup_name))
                                })
                        })
                    }
                })
                .cloned()
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
    
}
