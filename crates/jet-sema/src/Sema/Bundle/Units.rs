//! Unit-dimension resolution: injecting the standard `Prelude/Units.jet`
//! catalog into modules that mention it, then resolving every `#UnitFamily`
//! dimension claim (base or derived) to a stable, package-owned identity.
//! Split out of `Bundle.rs` to keep the module under the card #510 boundary.

use super::GenericModules;
use crate::AST::{Item, ProgramBundle};
use crate::Diagnostics::{Diagnostic, Span};
use std::collections::{HashMap, HashSet};

/// Load the standard dimension catalog from ordinary Jet source. Local names
/// shadow Prelude members; physical dimension behavior remains explicit opt-in.
pub(super) fn inject_units_prelude(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    const SOURCE: &str = include_str!("../../../../jet-codegen/src/Prelude/Units.jet");
    let (tokens, mut diagnostics) = crate::Lexer::lex_generated(SOURCE);
    let mut prelude = match crate::Parser::parse(&tokens) {
        Ok(program) => program
            .items
            .into_iter()
            .filter_map(|item| match item {
                Item::UnitFamily(family) => Some(family),
                _ => None,
            })
            .collect::<Vec<_>>(),
        Err(mut parse_diagnostics) => {
            diagnostics.append(&mut parse_diagnostics);
            return diagnostics;
        }
    };
    resolve_standard_unit_dimensions(&mut prelude);

    for module in &mut bundle.modules {
        if module.no_prelude {
            continue;
        }
        let occupied = module
            .items
            .iter()
            .flat_map(|item| match item {
                Item::UnitFamily(family) => family
                    .distinct_defs()
                    .into_iter()
                    .map(|definition| definition.name)
                    .collect::<Vec<_>>(),
                Item::Distinct(definition) => vec![definition.name.clone()],
                Item::Struct(definition) => vec![definition.name.clone()],
                Item::Enum(definition) => vec![definition.name.clone()],
                Item::TypeAlias(definition) => vec![definition.name.clone()],
                _ => Vec::new(),
            })
            .collect::<HashSet<_>>();
        let mut selected = prelude
            .iter()
            .filter(|family| {
                source_mentions_identifier(&module.source, &family.family)
                    || family
                        .members
                        .iter()
                        .any(|member| source_mentions_unit_member(&module.source, &member.name))
            })
            .map(|family| family.family.clone())
            .collect::<HashSet<_>>();
        loop {
            let mut added = false;
            for family in &prelude {
                if !selected.contains(&family.family) {
                    continue;
                }
                let Some(crate::AST::UnitDimensionDecl::Derived(expression)) = &family.dimension
                else {
                    continue;
                };
                for dependency in dimension_dependencies(expression) {
                    added |= selected.insert(dependency);
                }
            }
            if !added {
                break;
            }
        }
        for standard in &prelude {
            if module
                .items
                .iter()
                .any(|item| matches!(item, Item::UnitFamily(local) if local.family == standard.family))
            {
                continue;
            }
            let mut standard = standard.clone();
            let used_members = standard
                .members
                .iter()
                .filter(|member| source_mentions_unit_member(&module.source, &member.name))
                .map(|member| member.name.clone())
                .collect::<HashSet<_>>();
            if !selected.contains(&standard.family) {
                continue;
            }
            standard.members.retain(|member| {
                let is_base = standard
                    .base
                    .as_ref()
                    .is_some_and(|base| base.0 == member.name);
                (is_base || used_members.contains(&member.name))
                    && !occupied.contains(&crate::AST::UnitFamilyDef::type_name(&member.name))
            });
            module.items.push(Item::UnitFamily(standard));
        }
    }
    diagnostics
}

fn dimension_dependencies(expression: &crate::AST::Expr) -> Vec<String> {
    match expression {
        crate::AST::Expr::Ident(name, _) => vec![name.clone()],
        crate::AST::Expr::Binary(_, left, right, _) => {
            let mut dependencies = dimension_dependencies(left);
            dependencies.extend(dimension_dependencies(right));
            dependencies
        }
        _ => Vec::new(),
    }
}

fn source_mentions_identifier(source: &str, name: &str) -> bool {
    source.match_indices(name).any(|(start, _)| {
        let before = source[..start].chars().next_back();
        let after = source[start + name.len()..].chars().next();
        !before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            && !after.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
    })
}

fn source_mentions_unit_member(source: &str, member: &str) -> bool {
    source_mentions_unqualified_identifier(
        source,
        &crate::AST::UnitFamilyDef::type_name(member),
    ) || source.contains(&format!("from_{member}"))
        || source.match_indices(member).any(|(start, _)| {
            source[..start]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_digit())
        })
}

fn source_mentions_unqualified_identifier(source: &str, name: &str) -> bool {
    source.match_indices(name).any(|(start, _)| {
        let before = source[..start].chars().next_back();
        let after = source[start + name.len()..].chars().next();
        before != Some('.')
            && !before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            && !after.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
    })
}

/// Give the shared catalog one stable identity, independent of the package
/// whose module receives the ordinary Prelude declarations.
fn resolve_standard_unit_dimensions(prelude: &mut [crate::AST::UnitFamilyDef]) {
    let mut known = HashMap::<String, crate::AST::Dimension>::new();
    for family in prelude.iter() {
        if matches!(family.dimension, Some(crate::AST::UnitDimensionDecl::Base(_))) {
            known.insert(
                family.family.clone(),
                crate::AST::Dimension::base(format!("core.units::{}", family.family)),
            );
        }
    }
    loop {
        let mut progress = false;
        for family in prelude.iter() {
            if known.contains_key(&family.family) {
                continue;
            }
            let Some(crate::AST::UnitDimensionDecl::Derived(expression)) = &family.dimension else {
                continue;
            };
            let DimensionLookup::Found(dimension) = resolve_dimension_expression(
                expression,
                &|qualifier, name| {
                    if qualifier.is_none() {
                        known
                            .get(name)
                            .cloned()
                            .map_or(DimensionLookup::Missing, DimensionLookup::Found)
                    } else {
                        DimensionLookup::Missing
                    }
                },
            ) else {
                continue;
            };
            known.insert(family.family.clone(), dimension);
            progress = true;
        }
        if !progress {
            break;
        }
    }
    for family in prelude {
        family.resolved_dimension = known.get(&family.family).cloned();
        family.resolved_owner = Some("core.units".to_string());
    }
}

fn stable_unit_owner(bundle: &ProgramBundle, module: usize) -> (String, String) {
    let module = &bundle.modules[module];
    let (package_root, dependency_name) =
        GenericModules::owning_package(bundle, &module.path);
    let package = GenericModules::package_identity(bundle, package_root, dependency_name);
    let module_path = module
        .path
        .strip_prefix(package_root)
        .unwrap_or(&module.path)
        .to_string_lossy()
        .replace('\\', "/");
    (package.clone(), format!("{package}::{module_path}"))
}

/// Resolve open unit dimensions before registration. The declaration graph is
/// compile-time only; backends receive the normalized map already attached to
/// each family.
pub(super) fn resolve_unit_dimensions(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    #[derive(Clone)]
    struct Declaration {
        module: usize,
        item: usize,
        family: String,
        span: Span,
        is_pub: bool,
        claim: crate::AST::UnitDimensionDecl,
        preset: Option<crate::AST::Dimension>,
    }

    let declarations = bundle
        .modules
        .iter()
        .enumerate()
        .flat_map(|(module_index, module)| {
            module
            .items
            .iter()
            .enumerate()
            .filter_map(move |(item_index, item)| match item {
                Item::UnitFamily(family) => family.dimension.clone().map(|claim| Declaration {
                    module: module_index,
                    item: item_index,
                    family: family.family.clone(),
                    span: family.family_span,
                    is_pub: family.is_pub,
                    claim,
                    preset: family.resolved_dimension.clone(),
                }),
                _ => None,
            })
        })
        .collect::<Vec<_>>();

    let imported_modules = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(module_index, module)| {
            module
                .imports
                .iter()
                .filter_map(|import| bundle.import_targets.get(&(module_index, import.span)).copied())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let import_aliases = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(module_index, module)| {
            module
                .imports
                .iter()
                .filter_map(|import| {
                    bundle
                        .import_targets
                        .get(&(module_index, import.span))
                        .copied()
                        .map(|target| (import.import_alias(), target))
                })
                .collect::<HashMap<_, _>>()
        })
        .collect::<Vec<_>>();
    let mut known = HashMap::<(usize, String), crate::AST::Dimension>::new();
    for declaration in &declarations {
        if let Some(dimension) = declaration.preset.clone() {
            known.insert(
                (declaration.module, declaration.family.clone()),
                dimension,
            );
        } else if matches!(declaration.claim, crate::AST::UnitDimensionDecl::Base(_)) {
            let (_, module_identity) = stable_unit_owner(bundle, declaration.module);
            known.insert(
                (declaration.module, declaration.family.clone()),
                crate::AST::Dimension::base(format!(
                    "{module_identity}::{}",
                    declaration.family
                )),
            );
        }
    }

    let mut pending = declarations
        .iter()
        .filter(|declaration| {
            declaration.preset.is_none()
                && matches!(declaration.claim, crate::AST::UnitDimensionDecl::Derived(_))
        })
        .cloned()
        .collect::<Vec<_>>();
    loop {
        let mut progress = false;
        pending.retain(|declaration| {
            let crate::AST::UnitDimensionDecl::Derived(expression) = &declaration.claim else {
                unreachable!()
            };
            let visible = |qualifier: Option<&str>, name: &str| {
                if let Some(alias) = qualifier {
                    let Some(target) = import_aliases[declaration.module].get(alias) else {
                        return DimensionLookup::Missing;
                    };
                    let Some(candidate) = declarations.iter().find(|candidate| {
                        candidate.module == *target
                            && candidate.is_pub
                            && candidate.family == name
                    }) else {
                        return DimensionLookup::Missing;
                    };
                    return known
                        .get(&(candidate.module, candidate.family.clone()))
                        .cloned()
                        .map_or(DimensionLookup::Missing, DimensionLookup::Found);
                }
                if declarations.iter().any(|candidate| {
                    candidate.module == declaration.module && candidate.family == name
                }) {
                    return known
                        .get(&(declaration.module, name.to_string()))
                        .cloned()
                        .map_or(DimensionLookup::Missing, DimensionLookup::Found);
                }
                let candidates = imported_modules[declaration.module]
                    .iter()
                    .copied()
                    .filter(|target| {
                        declarations.iter().any(|candidate| {
                            candidate.module == *target
                                && candidate.is_pub
                                && candidate.family == name
                        })
                    })
                    .collect::<HashSet<_>>();
                if candidates.len() > 1 {
                    return DimensionLookup::Ambiguous(name.to_string());
                }
                let Some(target) = candidates.into_iter().next() else {
                    return DimensionLookup::Missing;
                };
                known
                    .get(&(target, name.to_string()))
                    .cloned()
                    .map_or(DimensionLookup::Missing, DimensionLookup::Found)
            };
            let dimension = match resolve_dimension_expression(expression, &visible) {
                DimensionLookup::Found(dimension) => dimension,
                DimensionLookup::Missing | DimensionLookup::Ambiguous(_) => return true,
            };
            known.insert(
                (declaration.module, declaration.family.clone()),
                dimension,
            );
            progress = true;
            false
        });
        if !progress {
            break;
        }
    }

    let mut diagnostics = Vec::new();
    for declaration in &declarations {
        let resolved = known
            .get(&(declaration.module, declaration.family.clone()))
            .cloned();
        if resolved.is_none() {
            let ambiguity = match &declaration.claim {
                crate::AST::UnitDimensionDecl::Derived(expression) => {
                    let visible = |qualifier: Option<&str>, name: &str| {
                        if qualifier.is_some() {
                            return DimensionLookup::Missing;
                        }
                        if declarations.iter().any(|candidate| {
                            candidate.module == declaration.module
                                && candidate.family == name
                        }) {
                            return DimensionLookup::Missing;
                        }
                        let matches = imported_modules[declaration.module]
                            .iter()
                            .copied()
                            .filter(|target| {
                                declarations.iter().any(|candidate| {
                                    candidate.module == *target
                                        && candidate.is_pub
                                        && candidate.family == name
                                })
                            })
                            .collect::<HashSet<_>>();
                        if matches.len() > 1 {
                            DimensionLookup::Ambiguous(name.to_string())
                        } else {
                            DimensionLookup::Missing
                        }
                    };
                    match resolve_dimension_expression(expression, &visible) {
                        DimensionLookup::Ambiguous(name) => Some(name),
                        _ => None,
                    }
                }
                crate::AST::UnitDimensionDecl::Base(_) => None,
            };
            diagnostics.push(if let Some(name) = ambiguity {
                Diagnostic::error(
                    "E0905",
                    format!("dimension name `{name}` is ambiguous"),
                    "more than one imported module exports that dimension".to_string(),
                    format!("qualify it with the intended module alias, such as `dep.{name}`"),
                    Some(declaration.span),
                )
            } else {
                Diagnostic::error(
                    "E0905",
                    format!("dimension `{}` cannot be resolved", declaration.family),
                    "derived dimensions can use visible declared dimensions and cannot form a cycle"
                        .to_string(),
                    "import or declare every base dimension and remove any dimension cycle".to_string(),
                    Some(declaration.span),
                )
            });
        }
        let owner = stable_unit_owner(bundle, declaration.module).0;
        if let Item::UnitFamily(definition) =
            &mut bundle.modules[declaration.module].items[declaration.item]
        {
            definition.resolved_dimension = resolved;
            if definition.resolved_owner.is_none() {
                definition.resolved_owner = Some(owner);
            }
        }
    }

    // D-DIMENSION-OPEN1=D: a family that never claimed a dimension still needs
    // its owning package recorded, because its unit facts carry the scale,
    // offset, and kind that same-family conversion depends on.
    let owners = (0..bundle.modules.len())
        .map(|module| stable_unit_owner(bundle, module).0)
        .collect::<Vec<_>>();
    for (module, owner) in owners.into_iter().enumerate() {
        for item in &mut bundle.modules[module].items {
            if let Item::UnitFamily(definition) = item {
                if definition.resolved_owner.is_none() {
                    definition.resolved_owner = Some(owner.clone());
                }
            }
        }
    }
    diagnostics
}

enum DimensionLookup {
    Found(crate::AST::Dimension),
    Missing,
    Ambiguous(String),
}

fn resolve_dimension_expression(
    expression: &crate::AST::Expr,
    visible: &impl Fn(Option<&str>, &str) -> DimensionLookup,
) -> DimensionLookup {
    match expression {
        crate::AST::Expr::Ident(name, _) => visible(None, name),
        crate::AST::Expr::Field(base, name, _) => match base.as_ref() {
            crate::AST::Expr::Ident(alias, _) => visible(Some(alias), name),
            _ => DimensionLookup::Missing,
        },
        crate::AST::Expr::Binary(
            op @ (crate::AST::BinOp::Mul | crate::AST::BinOp::Div),
            left,
            right,
            _,
        ) => {
            let left = resolve_dimension_expression(left, visible);
            let right = resolve_dimension_expression(right, visible);
            match (left, right) {
                (DimensionLookup::Ambiguous(name), _)
                | (_, DimensionLookup::Ambiguous(name)) => DimensionLookup::Ambiguous(name),
                (DimensionLookup::Found(left), DimensionLookup::Found(right)) => {
                    let dimension = if *op == crate::AST::BinOp::Mul {
                        left.multiply(&right)
                    } else {
                        left.divide(&right)
                    };
                    dimension.map_or(DimensionLookup::Missing, DimensionLookup::Found)
                }
                _ => DimensionLookup::Missing,
            }
        }
        _ => DimensionLookup::Missing,
    }
}
