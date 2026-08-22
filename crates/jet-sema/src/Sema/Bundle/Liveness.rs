//! D-STRUCT-LIVE1=A: structure liveness is one sema projection over the name
//! ledger. The pass does not inspect generated code or ask an execution tier
//! to discover reachability.

use crate::AST::{Func, ImportKind, Item, ProgramBundle};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use jet_foundation::App::AppGraph;
use jet_foundation::Names::{NameAlias, NameDeclaration, NameLedger, NameVisibility, StructureFact,
    StructureFactKind};

struct LivenessCandidate {
    code: &'static str,
    name: String,
    source: String,
    span: Span,
    detail: &'static str,
    edit: crate::Diagnostics::TextEdit,
}

impl LivenessCandidate {
    fn diagnostic(&self) -> Diagnostic {
        Diagnostic::from_row(self.code, &[("name", self.name.as_str())], Some(self.span))
            .with_edit(self.edit.clone())
    }

    fn fact(&self) -> StructureFact {
        StructureFact::new(
            StructureFactKind::Liveness,
            self.name.clone(),
            self.source.clone(),
            self.span,
            "unreachable",
            self.detail,
            Some("_name".to_string()),
        )
    }
}

pub(super) fn check_liveness(
    bundle: &ProgramBundle,
    ledger: &mut NameLedger,
    app_graph: Option<&AppGraph>,
) -> Vec<Diagnostic> {
    let mut candidates = Vec::new();
    collect_unused_imports(bundle, ledger, &mut candidates);
    collect_unused_private_functions(bundle, ledger, app_graph, &mut candidates);
    collect_unreachable_exports(bundle, ledger, app_graph, &mut candidates);

    candidates.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.span.start.cmp(&b.span.start))
            .then(a.code.cmp(b.code))
            .then(a.name.cmp(&b.name))
    });

    candidates
        .into_iter()
        .map(|candidate| {
            ledger.record_structure_fact(candidate.fact());
            candidate.diagnostic()
        })
        .collect()
}

fn collect_unused_imports(
    bundle: &ProgramBundle,
    ledger: &NameLedger,
    candidates: &mut Vec<LivenessCandidate>,
) {
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for import in &module.imports {
            // A quoted Jet module has no `ImportBinding` row, but an explicit
            // alias still introduces a local namespace in the NameLedger.
            // C headers stay outside liveness because importing a bridge can
            // have link-time meaning even when no Jet name is read.
            if matches!(&import.kind, ImportKind::File(path, _) if !path.ends_with(".h"))
                && !import.is_pub
                && !import.is_package_pub
                && !ignored_name(&import.alias)
            {
                if let Some(alias) = ledger.alias(module_idx, &import.alias) {
                    if alias.span.start < alias.span.end
                        && !alias_used(ledger, &module.display, alias)
                    {
                        let name = import.alias.clone();
                        let explicit_alias = import.alias_span.start > import.span.start;
                        let span = if explicit_alias {
                            import.alias_span
                        } else {
                            import.span
                        };
                        let edit = if explicit_alias {
                            rename_edit(span, &name)
                        } else {
                            crate::Diagnostics::TextEdit {
                                span: import.span,
                                new_text: String::new(),
                            }
                        };
                        candidates.push(LivenessCandidate {
                            code: "L0103",
                            name: name.clone(),
                            source: module.display.clone(),
                            span,
                            detail: "import is never read",
                            edit,
                        });
                    }
                }
            }
            for binding in import.walk_bindings() {
                // A public import is an export surface, not a private local
                // binding. Its reachability is checked by the export pass.
                if binding.is_pub || binding.is_package_pub || ignored_name(&binding.local) {
                    continue;
                }
                let Some(alias) = ledger.alias(module_idx, &binding.local) else {
                    continue;
                };
                if alias.span.start >= alias.span.end || alias_used(ledger, &module.display, alias) {
                    continue;
                }
                let span = import_binding_span(&module.source, &binding);
                if span.start >= span.end {
                    continue;
                }
                let name = binding.local;
                candidates.push(LivenessCandidate {
                    code: "L0103",
                    name: name.clone(),
                    source: module.display.clone(),
                    span,
                    detail: "import is never read",
                    edit: rename_edit(span, &name),
                });
            }
        }
    }
}

fn collect_unused_private_functions(
    bundle: &ProgramBundle,
    ledger: &NameLedger,
    app_graph: Option<&AppGraph>,
    candidates: &mut Vec<LivenessCandidate>,
) {
    for declaration in ledger.declarations() {
        if declaration.visibility != NameVisibility::Private
            || declaration.kind != "function"
            || ignored_name(&declaration.name)
            || declaration.span.start >= declaration.span.end
        {
            continue;
        }
        if function_is_root(bundle, declaration, app_graph)
            || private_function_used(bundle, ledger, declaration)
        {
            continue;
        }
        let Some(module) = bundle.modules.get(declaration.module) else {
            continue;
        };
        candidates.push(LivenessCandidate {
            code: "L0104",
            name: display_name(declaration),
            source: module.display.clone(),
            span: declaration.span,
            detail: "private function is never reached",
            edit: rename_edit(declaration.span, &display_name(declaration)),
        });
    }
}

fn collect_unreachable_exports(
    bundle: &ProgramBundle,
    ledger: &NameLedger,
    app_graph: Option<&AppGraph>,
    candidates: &mut Vec<LivenessCandidate>,
) {
    for declaration in ledger.declarations() {
        if declaration.visibility != NameVisibility::Package
            || !export_kind(&declaration.kind)
            || ignored_name(&declaration.name)
            || declaration.name.contains('.')
            || declaration.span.start >= declaration.span.end
        {
            continue;
        }
        if function_is_root(bundle, declaration, app_graph)
            || declaration_reached_from_package(bundle, ledger, declaration)
        {
            continue;
        }
        let Some(module) = bundle.modules.get(declaration.module) else {
            continue;
        };
        candidates.push(LivenessCandidate {
            code: "L0105",
            name: display_name(declaration),
            source: module.display.clone(),
            span: declaration.span,
            detail: "package export is unreachable",
            edit: rename_edit(declaration.span, &display_name(declaration)),
        });
    }

    // `pub(package) use …` has an alias declaration rather than a top-level
    // item declaration. Only aliases represented by source import bindings
    // enter this export verdict; compiler-prelude aliases remain invisible.
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for import in &module.imports {
            for binding in import.walk_bindings() {
                if !binding.is_package_pub || ignored_name(&binding.local) {
                    continue;
                }
                let Some(alias) = ledger.alias(module_idx, &binding.local) else {
                    continue;
                };
                if alias.span.start >= alias.span.end
                    || alias_reached_from_package(bundle, ledger, alias)
                {
                    continue;
                }
                let span = import_binding_span(&module.source, &binding);
                if span.start >= span.end {
                    continue;
                }
                let name = binding.local;
                candidates.push(LivenessCandidate {
                    code: "L0105",
                    name: name.clone(),
                    source: module.display.clone(),
                    span,
                    detail: "package re-export is unreachable",
                    edit: rename_edit(span, &name),
                });
            }
        }
    }
}

fn ignored_name(name: &str) -> bool {
    name.is_empty()
        || name.starts_with('_')
        || name.starts_with(Syntax::GENERATED_NAME_PREFIX)
}

fn rename_edit(span: Span, name: &str) -> crate::Diagnostics::TextEdit {
    crate::Diagnostics::TextEdit {
        span,
        new_text: format!("_{name}"),
    }
}

/// Import aliases have two parser-owned spans: the source path and, for the
/// simple module form, the explicit alias. Member-list aliases retain the
/// original member span only. Recover the local identifier from the bounded
/// import source so a fix never replaces the imported target path.
fn import_binding_span(
    source: &str,
    binding: &crate::AST::ImportBinding<'_>,
) -> Span {
    let original = binding.item_span.unwrap_or(binding.module_alias_span);
    if let Some(alias) = binding.alias {
        if let Some((start, end)) = identifier_after_as(
            source,
            original.end,
            binding.import_span.end,
            alias,
        ) {
            return Span::new(start, end);
        }
    }
    last_identifier_span(source, original).unwrap_or(original)
}

fn identifier_after_as(
    source: &str,
    start: usize,
    end: usize,
    alias: &str,
) -> Option<(usize, usize)> {
    let source = source.get(start..end)?;
    let mut cursor = 0;
    while let Some((token, _token_start, token_end)) = next_identifier(source, cursor) {
        cursor = token_end;
        if token != "as" {
            continue;
        }
        let Some((candidate, candidate_start, candidate_end)) = next_identifier(source, cursor)
        else {
            return None;
        };
        if candidate == alias {
            return Some((start + candidate_start, start + candidate_end));
        }
        cursor = candidate_end;
    }
    None
}

fn last_identifier_span(source: &str, span: Span) -> Option<Span> {
    let source = source.get(span.start..span.end)?;
    let mut cursor = 0;
    let mut last = None;
    while let Some((_, token_start, token_end)) = next_identifier(source, cursor) {
        last = Some(Span::new(span.start + token_start, span.start + token_end));
        cursor = token_end;
    }
    last
}

fn next_identifier(source: &str, mut cursor: usize) -> Option<(&str, usize, usize)> {
    while cursor < source.len() {
        let ch = source[cursor..].chars().next()?;
        if ch == '_' || ch.is_alphabetic() {
            let start = cursor;
            cursor += ch.len_utf8();
            while cursor < source.len() {
                let ch = source[cursor..].chars().next()?;
                if ch == '_' || ch.is_alphanumeric() {
                    cursor += ch.len_utf8();
                } else {
                    break;
                }
            }
            return Some((&source[start..cursor], start, cursor));
        }
        cursor += ch.len_utf8();
    }
    None
}

fn export_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function"
            | "type"
            | "const"
            | "trait"
            | "protocol"
            | "state"
            | "tag"
            | "module"
            | "file_module"
            | "extern"
            | "checked_text_head"
    )
}

fn display_name(declaration: &NameDeclaration) -> String {
    declaration
        .name
        .rsplit_once('.')
        .map(|(_, leaf)| leaf.to_string())
        .unwrap_or_else(|| declaration.name.clone())
}

fn alias_used(ledger: &NameLedger, source: &str, alias: &NameAlias) -> bool {
    ledger.references().iter().any(|((module, _, _), reference)| {
        module == source
            && reference.kind == "import_alias"
            && reference.def_span == alias.span
    })
}

fn private_function_used(
    bundle: &ProgramBundle,
    ledger: &NameLedger,
    declaration: &NameDeclaration,
) -> bool {
    let Some(target_path) = bundle
        .modules
        .get(declaration.module)
        .map(|module| module.display.as_str())
    else {
        return false;
    };
    let function_span = find_function_by_name_span(
        &bundle.modules[declaration.module].items,
        declaration.span,
    )
    .map(|function| function.span);
    ledger.references().iter().any(|((source, start, end), reference)| {
        source == target_path
            && reference.module_path == target_path
            && reference.kind == "function"
            && reference.def_span == declaration.span
            && !function_span.is_some_and(|span| {
                *start >= span.start && *end <= span.end
            })
    })
}

fn declaration_reached_from_package(
    bundle: &ProgramBundle,
    ledger: &NameLedger,
    declaration: &NameDeclaration,
) -> bool {
    let Some(target_module) = bundle.modules.get(declaration.module) else {
        return false;
    };
    ledger.references().iter().any(|((source, _, _), reference)| {
        let Some(source_idx) = bundle
            .modules
            .iter()
            .position(|module| module.display == *source)
        else {
            return false;
        };
        source_idx != declaration.module
            && ledger
                .module(source_idx)
                .zip(ledger.module(declaration.module))
                .is_some_and(|(source, target)| source.package == target.package)
            && reference.module_path == target_module.display
            && reference.def_span == declaration.span
    })
}

fn alias_reached_from_package(
    bundle: &ProgramBundle,
    ledger: &NameLedger,
    alias: &NameAlias,
) -> bool {
    let Some(owner) = ledger.module(alias.module) else {
        return false;
    };
    ledger.aliases().any(|consumer| {
        consumer.module != alias.module
            && consumer.target_module == Some(alias.module)
            && consumer.target.rsplit_once('.').map_or(false, |(_, leaf)| leaf == alias.name)
            && ledger
                .module(consumer.module)
                .is_some_and(|module| module.package == owner.package)
            && bundle
                .modules
                .get(consumer.module)
                .is_some_and(|module| module.display != owner.path)
    })
}

fn function_is_root(
    bundle: &ProgramBundle,
    declaration: &NameDeclaration,
    app_graph: Option<&AppGraph>,
) -> bool {
    let Some(function) = find_function_by_name_span(
        &bundle.modules[declaration.module].items,
        declaration.span,
    ) else {
        return false;
    };
    if function.is_job
        || matches!(
            function.web_marker,
            Some(jet_foundation::WebPartition::WebPartitionMarker::WasmExport)
        )
    {
        return true;
    }
    if declaration.module == bundle.entry && function.name == "run" {
        return true;
    }
    let Some(graph) = app_graph else { return false };
    if declaration.module != bundle.entry {
        return false;
    }
    let handler = |name: &str| name == function.name;
    graph.routes.iter().any(|route| handler(&route.handler))
        || graph.actions.iter().any(|action| handler(&action.handler))
        || graph.mounts.iter().any(|mount| handler(&mount.handler))
}

fn find_function_by_name_span(items: &[Item], span: Span) -> Option<&Func> {
    for item in items {
        match item {
            Item::Func(function) if function.name_span == span => return Some(function),
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    if let Some(function) = find_function_by_name_span(body, span) {
                        return Some(function);
                    }
                }
            }
            _ => {}
        }
    }
    None
}
