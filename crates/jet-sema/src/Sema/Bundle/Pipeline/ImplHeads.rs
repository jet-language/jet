use super::*;

/// One trait named through a module alias in an `impl` head: the key the
/// impl now carries, and the owner module plus leaf name to register.
pub(super) struct QualifiedImplTrait {
    pub(super) key: String,
    pub(super) target: usize,
    pub(super) leaf: String,
}

/// D-IMPLDOT1=A splits a dotted `impl` head at its last segment, which cannot
/// see module aliases: `impl types.Span` parses as type `types` with trait
/// `Span`. Once imports are resolved, an alias segment binds to the segment
/// after it, so `impl types.Span` is an inherent impl of `types.Span` and
/// `impl Local.types.Named` implements `types.Named` for `Local`.
/// D-MOD-CYCLE1=A: a declaration from another file of the same package lives
/// in this file's namespace, so it is spelled by its leaf name; a declaration
/// from another package keeps its qualified spelling (the orphan rule then
/// judges the impl). Returns, per module, the traits named this way.
pub(super) fn normalize_qualified_impl_heads(
    bundle: &mut ProgramBundle,
) -> Vec<Vec<QualifiedImplTrait>> {
    let mut qualified_traits = Vec::with_capacity(bundle.modules.len());
    for idx in 0..bundle.modules.len() {
        let module = &bundle.modules[idx];
        let aliases: HashMap<String, usize> = module
            .imports
            .iter()
            .filter(|import| !matches!(import.kind, ImportKind::Unqualified { .. }))
            .filter_map(|import| {
                bundle
                    .name_ledger
                    .import_target(idx, import.span)
                    .map(|target| (import.import_alias(), target))
            })
            .collect();
        let mut traits = Vec::new();
        if aliases.is_empty() {
            qualified_traits.push(traits);
            continue;
        }
        let same_package: HashMap<usize, bool> = aliases
            .values()
            .map(|&target| (target, bundle.name_ledger.same_namespace(idx, target)))
            .collect();
        let module_identities: HashMap<usize, String> = aliases
            .values()
            .filter_map(|&target| {
                bundle
                    .name_ledger
                    .module_identity(target)
                    .map(|module| (target, module))
            })
            .collect();
        let mut used_aliases = Vec::new();
        for item in &mut bundle.modules[idx].items {
            let Item::Impl(implementation) = item else {
                continue;
            };
            let Some(trait_name) = implementation.trait_name.as_deref() else {
                continue;
            };
            let segments: Vec<String> = implementation
                .type_name
                .split('.')
                .chain(trait_name.split('.'))
                .map(str::to_string)
                .collect();
            // Each path is (owner module through an alias, spelling as
            // written, leaf name).
            let mut paths: Vec<(Option<usize>, String, String)> = Vec::new();
            let mut at = 0;
            while at < segments.len() {
                match aliases.get(&segments[at]) {
                    Some(&target) if at + 1 < segments.len() => {
                        paths.push((
                            Some(target),
                            format!("{}.{}", segments[at], segments[at + 1]),
                            segments[at + 1].clone(),
                        ));
                        at += 2;
                    }
                    _ => {
                        paths.push((None, segments[at].clone(), segments[at].clone()));
                        at += 1;
                    }
                }
            }
            if paths.len() > 2 || paths.iter().all(|(target, _, _)| target.is_none()) {
                continue;
            }
            for (target, written, _) in &paths {
                if target.is_some() {
                    if let Some((alias, _)) = written.split_once('.') {
                        used_aliases.push(alias.to_string());
                    }
                }
            }
            let resolved = |(target, written, leaf): &(Option<usize>, String, String)| match target {
                Some(target) if same_package[target] => leaf.clone(),
                _ => written.clone(),
            };
            let start = implementation.type_span.start;
            let end = implementation
                .trait_span
                .map_or(implementation.type_span.end, |span| span.end);
            let first_end = start + paths[0].1.len();
            let type_name = resolved(&paths[0]);
            if let Some(trait_path) = paths.get(1) {
                // A trait from another package is keyed by its canonical
                // nominal identity, the spelling MIR trait references use.
                let trait_key = match trait_path.0 {
                    Some(target) if !same_package[&target] => module_identities
                        .get(&target)
                        .map(|module| format!("{module}::{}", trait_path.2))
                        .unwrap_or_else(|| resolved(trait_path)),
                    _ => resolved(trait_path),
                };
                if let Some(target) = trait_path.0 {
                    traits.push(QualifiedImplTrait {
                        key: trait_key.clone(),
                        target,
                        leaf: trait_path.2.clone(),
                    });
                }
                implementation.type_name = type_name;
                implementation.type_span = crate::Diagnostics::Span::new(start, first_end);
                implementation.trait_name = Some(trait_key);
                implementation.trait_span =
                    Some(crate::Diagnostics::Span::new(end - trait_path.1.len(), end));
            } else {
                implementation.type_name = type_name;
                implementation.type_span = crate::Diagnostics::Span::new(start, end);
                implementation.trait_name = None;
                implementation.trait_span = None;
            }
        }
        // The rewritten head no longer spells the alias, so record the use
        // here; otherwise the import reads as unused (L0103).
        for alias in used_aliases {
            if let Some(span) = bundle
                .name_ledger
                .effective_alias(idx, &alias)
                .map(|alias| alias.span)
            {
                bundle.name_ledger.record_loader_alias_use(idx, span);
            }
        }
        qualified_traits.push(traits);
    }
    qualified_traits
}
