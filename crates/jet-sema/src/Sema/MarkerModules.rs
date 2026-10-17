//! D-MARKER-MODULE1=A: library marker identity is (declaring module, declaration).
use crate::AST::{Item, Marker, MarkerDecl, ProgramBundle};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Policy::MarkerVocabulary;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

pub struct ResolvedMarkerDeclaration<'a> {
    pub module: usize,
    pub declaration: &'a MarkerDecl,
    pub exported: bool,
}

pub enum MarkerResolution<'a> {
    Found(ResolvedMarkerDeclaration<'a>),
    Unknown,
    Private,
    Ambiguous,
}

fn import_alias(import: &crate::AST::ImportDecl) -> &str {
    if !import.alias.is_empty() { return &import.alias; }
    match &import.kind {
        crate::AST::ImportKind::File(path, _) => path.rsplit('/').next().unwrap_or("module"),
        crate::AST::ImportKind::Module(name, _) => name,
        crate::AST::ImportKind::Unqualified { module_alias, .. } => module_alias,
    }
}

fn import_target(bundle: &ProgramBundle, module: usize, import: &crate::AST::ImportDecl) -> Option<usize> {
    bundle.name_ledger.import_target(module, import.span).or_else(|| {
        let core_name = match &import.kind {
            crate::AST::ImportKind::Module(name, _) => Some(name.as_str()),
            crate::AST::ImportKind::Unqualified { module_alias, .. } if module_alias.starts_with("core.") => Some(module_alias.as_str()),
            _ => None,
        };
        if let Some(source) = core_name.and_then(jet_foundation::CoreModuleExports::core_source_module) {
            return bundle.modules.iter().position(|candidate| candidate.alias == source.alias);
        }
        let crate::AST::ImportKind::Unqualified { module_alias, .. } = &import.kind else { return None };
        bundle.modules[module].imports.iter().filter(|candidate| import_alias(candidate) == module_alias)
            .find_map(|candidate| bundle.name_ledger.import_target(module, candidate.span))
    })
}

fn declarations<'a>(items: &'a [Item], prefix: &str, exported: bool, rows: &mut Vec<(String, &'a MarkerDecl, bool)>) {
    for item in items {
        match item {
            Item::MarkerDecl(declaration) => rows.push((format!("{prefix}{}", declaration.name), declaration, exported && declaration.is_pub)),
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    declarations(body, &format!("{prefix}{}.", module.name), exported && module.is_pub, rows);
                }
            }
            _ => {}
        }
    }
}

fn declaration_rows(items: &[Item]) -> Vec<(String, &MarkerDecl, bool)> {
    let mut rows = Vec::new();
    declarations(items, "", true, &mut rows);
    rows
}

fn find_named<'a>(items: &'a [Item], name: &str, exported: bool, found: &mut Option<(&'a MarkerDecl, bool)>, ambiguous: &mut bool) {
    for item in items {
        match item {
            Item::MarkerDecl(declaration) if declaration.name == name => {
                if found.is_some() { *ambiguous = true; }
                else { *found = Some((declaration, exported && declaration.is_pub)); }
            }
            Item::CodeModule(namespace) => {
                if let Some(tail) = name.strip_prefix(&namespace.name).and_then(|tail| tail.strip_prefix('.')) {
                    if let Some(body) = &namespace.body {
                        find_named(body, tail, exported && namespace.is_pub, found, ambiguous);
                    }
                }
            }
            _ => {}
        }
    }
}

fn resolved_items<'a>(items: &'a [Item], module: usize, name: &str, exported: bool, require_export: bool) -> MarkerResolution<'a> {
    let mut found = None;
    let mut ambiguous = false;
    find_named(items, name, exported, &mut found, &mut ambiguous);
    if ambiguous { return MarkerResolution::Ambiguous; }
    match found {
        Some((declaration, exported)) if exported || !require_export => MarkerResolution::Found(ResolvedMarkerDeclaration { module, declaration, exported }),
        Some(_) => MarkerResolution::Private,
        None => MarkerResolution::Unknown,
    }
}

fn resolve_in_scope<'a>(bundle: &'a ProgramBundle, module: usize, items: &'a [Item], imports: &[crate::AST::ImportDecl], name: &str, exported: bool) -> MarkerResolution<'a> {
    let mut target = None;
    let mut ambiguous = false;
    let mut prefix_length = 0;
    let mut tail = name;
    for import in imports {
        let alias = import_alias(import);
        let Some(candidate_tail) = name.strip_prefix(alias).and_then(|rest| rest.strip_prefix('.')) else { continue };
        let Some(candidate) = import_target(bundle, module, import) else { continue };
        if alias.len() > prefix_length {
            target = Some(candidate); ambiguous = false; prefix_length = alias.len(); tail = candidate_tail;
        } else if alias.len() == prefix_length && target != Some(candidate) { ambiguous = true; }
    }
    if ambiguous { return MarkerResolution::Ambiguous; }
    if let Some(target) = target {
        return resolved_items(&bundle.modules[target].items, target, tail, true, true);
    }
    let mut nested = None;
    for item in items {
        let Item::CodeModule(namespace) = item else { continue };
        if let Some(tail) = name.strip_prefix(&namespace.name).and_then(|tail| tail.strip_prefix('.')) {
            if let Some(body) = &namespace.body {
                let resolution = resolve_in_scope(bundle, module, body, &namespace.imports, tail, exported && namespace.is_pub);
                if !matches!(resolution, MarkerResolution::Unknown) {
                    if nested.is_some() { return MarkerResolution::Ambiguous; }
                    nested = Some(resolution);
                }
            }
        }
    }
    if let Some(resolution) = nested { return resolution; }
    resolved_items(items, module, name, exported, false)
}

pub fn resolve_marker_declaration<'a>(bundle: &'a ProgramBundle, module: usize, name: &str) -> MarkerResolution<'a> {
    let source = &bundle.modules[module];
    resolve_in_scope(bundle, module, &source.items, &source.imports, name, true)
}

fn scope_claims_name(items: &[Item], imports: &[crate::AST::ImportDecl], name: &str) -> bool {
    imports.iter().any(|import| name.strip_prefix(import_alias(import)).is_some_and(|tail| tail.starts_with('.')))
        || items.iter().any(|item| match item {
            Item::MarkerDecl(declaration) => declaration.name == name,
            Item::CodeModule(namespace) => name.strip_prefix(&namespace.name).is_some_and(|tail| tail.starts_with('.')),
            _ => false,
        })
}

fn resolve_at_span<'a>(bundle: &'a ProgramBundle, module: usize, items: &'a [Item], imports: &[crate::AST::ImportDecl], name: &str, span: Span, exported: bool) -> Option<MarkerResolution<'a>> {
    for item in items {
        let Item::CodeModule(namespace) = item else { continue };
        if namespace.span.start <= span.start && span.end <= namespace.span.end {
            if let Some(body) = &namespace.body {
                if let Some(resolution) = resolve_at_span(bundle, module, body, &namespace.imports, name, span, exported && namespace.is_pub) { return Some(resolution); }
            }
        }
    }
    let resolution = resolve_in_scope(bundle, module, items, imports, name, exported);
    if !matches!(resolution, MarkerResolution::Unknown) || scope_claims_name(items, imports, name) { Some(resolution) } else { None }
}

fn find_identity(items: &[Item], span: Span, exported: bool) -> Option<(&MarkerDecl, bool)> {
    for item in items {
        match item {
            Item::MarkerDecl(declaration) if declaration.name_span == span => return Some((declaration, exported && declaration.is_pub)),
            Item::CodeModule(namespace) => {
                if let Some(found) = namespace.body.as_ref().and_then(|body| find_identity(body, span, exported && namespace.is_pub)) { return Some(found); }
            }
            _ => {}
        }
    }
    None
}

/// Group member identities are resolved in the declaration's lexical scope,
/// never against the imports or shadowing declarations at the use site.
pub fn resolve_marker_application<'a>(bundle: &'a ProgramBundle, module: usize, marker: &Marker) -> MarkerResolution<'a> {
    if let Some((declaring_module, span)) = marker.declaration {
        let Some(source) = bundle.modules.get(declaring_module) else { return MarkerResolution::Unknown };
        return match find_identity(&source.items, span, true) {
            Some((declaration, exported)) => MarkerResolution::Found(ResolvedMarkerDeclaration { module: declaring_module, declaration, exported }),
            None => MarkerResolution::Unknown,
        };
    }
    let source = &bundle.modules[module];
    resolve_at_span(bundle, module, &source.items, &source.imports, &marker.name, marker.name_span, true).unwrap_or(MarkerResolution::Unknown)
}

fn import_span_at(items: &[Item], imports: &[crate::AST::ImportDecl], name: &str, span: Span) -> Option<Span> {
    for item in items {
        let Item::CodeModule(namespace) = item else { continue };
        if namespace.span.start <= span.start && span.end <= namespace.span.end {
            if let Some(body) = &namespace.body {
                if let Some(found) = import_span_at(body, &namespace.imports, name, span) { return Some(found); }
                if scope_claims_name(body, &namespace.imports, name) { return None; }
            }
        }
    }
    if let Some(import) = imports.iter().filter(|import| name.strip_prefix(import_alias(import)).is_some_and(|tail| tail.starts_with('.')))
        .max_by_key(|import| import_alias(import).len()) { return Some(import.alias_span); }
    for item in items {
        let Item::CodeModule(namespace) = item else { continue };
        if let Some(tail) = name.strip_prefix(&namespace.name).and_then(|tail| tail.strip_prefix('.')) {
            if let Some(body) = &namespace.body {
                if let Some(found) = import_span_at(body, &namespace.imports, tail, span) { return Some(found); }
            }
        }
    }
    None
}

fn written_marker_name_matches(written: &str, name: &str) -> bool {
    let mut written = written.chars().peekable();
    let mut name = name.chars();
    while let Some(character) = written.next() {
        if character.is_whitespace() { continue; }
        if character == '/' {
            match written.next() {
                Some('/') => { for character in written.by_ref() { if character == '\n' { break; } } }
                Some('*') => {
                    let mut depth = 1usize;
                    while let Some(character) = written.next() {
                        if character == '/' && written.peek() == Some(&'*') { written.next(); depth += 1; }
                        else if character == '*' && written.peek() == Some(&'/') {
                            written.next(); depth -= 1;
                            if depth == 0 { break; }
                        }
                    }
                    if depth != 0 { return false; }
                }
                _ => return false,
            }
            continue;
        }
        if name.next() != Some(character) { return false; }
    }
    name.next().is_none()
}

/// Only the written application reads the caller's imports. Expanded members
/// keep the library's spelling, which must not credit an unrelated caller alias.
pub(crate) fn marker_import_span(bundle: &ProgramBundle, module: usize, marker: &Marker) -> Option<Span> {
    let source = &bundle.modules[module];
    let written = source.source.get(marker.name_span.start..marker.name_span.end)?;
    let written = written.strip_prefix('#').unwrap_or(written);
    if !written_marker_name_matches(written, &marker.name) { return None; }
    import_span_at(&source.items, &source.imports, &marker.name, marker.name_span)
}

pub fn declaration_by_span(items: &[Item], span: Span) -> Option<&MarkerDecl> {
    for item in items {
        match item {
            Item::MarkerDecl(declaration) if declaration.name_span == span => return Some(declaration),
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    if let Some(declaration) = declaration_by_span(body, span) { return Some(declaration); }
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn bind_marker_applications(bundle: &mut ProgramBundle) {
    for index in 0..bundle.modules.len() {
        let mut bindings = MarkerBindings::new();
        let module = &bundle.modules[index];
        for marker in super::CheckerMarkers::markers_in(&module.items).chain(module.rule_facts.iter().map(|fact| &fact.marker)) {
            if crate::Policy::applied_rule(&marker.name).is_some() { continue; }
            if let MarkerResolution::Found(resolved) = resolve_marker_application(bundle, index, marker) {
                bindings.entry((marker.name_span.start, marker.name_span.end)).or_default()
                    .insert(marker.name.clone(), MarkerBinding { declaration: (resolved.module, resolved.declaration.name_span), authored_name: None });
            }
        }
        let module = &mut bundle.modules[index];
        bind_items(&mut module.items, &bindings);
        for fact in &mut module.rule_facts { bind_markers(std::slice::from_mut(&mut fact.marker), &bindings); }
    }
}

pub(crate) fn bound_marker_declarations(bundle: &ProgramBundle) -> Arc<BTreeMap<(usize, usize, usize), MarkerDecl>> {
    Arc::new(bundle.modules.iter().enumerate().flat_map(|(module, source)| {
        declaration_rows(&source.items).into_iter().map(move |(_, declaration, _)| {
            ((module, declaration.name_span.start, declaration.name_span.end), declaration.clone())
        })
    }).collect())
}

fn scoped_imports<'a>(items: &'a [Item], imports: &'a [crate::AST::ImportDecl], prefix: &str, rows: &mut Vec<(String, &'a crate::AST::ImportDecl)>) {
    for import in imports { rows.push((format!("{prefix}{}", import_alias(import)), import)); }
    for item in items {
        if let Item::CodeModule(namespace) = item {
            if let Some(body) = &namespace.body {
                scoped_imports(body, &namespace.imports, &format!("{prefix}{}.", namespace.name), rows);
            }
        }
    }
}

pub(crate) fn module_marker_vocabulary(bundle: &ProgramBundle, module: usize, derives: impl IntoIterator<Item = String>) -> MarkerVocabulary {
    let mut bindings = Vec::new();
    let mut rows: Vec<_> = declaration_rows(&bundle.modules[module].items).into_iter().map(|(name, declaration, _)| {
        bindings.push((name.clone(), module, declaration.name_span));
        let mut row = declaration.clone(); row.name = name; row
    }).collect();
    let mut imports = Vec::new();
    scoped_imports(&bundle.modules[module].items, &bundle.modules[module].imports, "", &mut imports);
    for (alias, import) in imports {
        let Some(target) = import_target(bundle, module, import) else { continue };
        for (name, declaration, exported) in declaration_rows(&bundle.modules[target].items) {
            if !exported { continue; }
            let qualified = format!("{alias}.{name}");
            if matches!(resolve_marker_declaration(bundle, module, &qualified), MarkerResolution::Found(_)) {
                bindings.push((qualified.clone(), target, declaration.name_span));
                let mut declaration = declaration.clone(); declaration.name = qualified; rows.push(declaration);
            }
        }
    }
    let mut vocabulary = MarkerVocabulary::with_derives_and_declarations(derives, rows);
    for (name, module, span) in bindings { vocabulary.bind_declaration(name, module, span); }
    vocabulary
}

fn has_marker_leaf(items: &[Item], name: &str) -> bool {
    items.iter().any(|item| match item {
        Item::MarkerDecl(declaration) => declaration.name == name,
        Item::CodeModule(namespace) => namespace.body.as_deref().is_some_and(|body| has_marker_leaf(body, name)),
        _ => false,
    })
}

fn imported_marker_at(bundle: &ProgramBundle, module: usize, items: &[Item], imports: &[crate::AST::ImportDecl], marker: &Marker) -> bool {
    if imports.iter().filter_map(|import| import_target(bundle, module, import))
        .any(|target| has_marker_leaf(&bundle.modules[target].items, &marker.name)) { return true; }
    items.iter().any(|item| match item {
        Item::CodeModule(namespace) if namespace.span.start <= marker.name_span.start && marker.name_span.end <= namespace.span.end =>
            namespace.body.as_deref().is_some_and(|body| imported_marker_at(bundle, module, body, &namespace.imports, marker)),
        _ => false,
    })
}

pub(crate) fn check_marker_namespaces(bundle: &ProgramBundle) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let diagnostic_start = diagnostics.len();
        let vocabulary = module_marker_vocabulary(bundle, module_index, std::iter::empty());
        let mut seen = HashSet::new();
        for marker in super::CheckerMarkers::markers_in(&module.items).chain(module.rule_facts.iter().map(|fact| &fact.marker)) {
            if !seen.insert((marker.name_span, marker.name.as_str(), marker.declaration)) { continue; }
            let diagnostic = if marker.name.contains('.') || marker.declaration.is_some() {
                match resolve_marker_application(bundle, module_index, marker) {
                    MarkerResolution::Found(_) => None,
                    MarkerResolution::Unknown => Some(crate::Policy::marker_unknown_error(&marker.name, &vocabulary.names(), marker.name_span)),
                    MarkerResolution::Private => Some(Diagnostic::error("E0605", format!("marker `#{}` is private to its declaring module", marker.name),
                        "a library marker is public unless it is marked `priv`".into(),
                        "remove `priv` from the marker, or remove this use".into(), Some(marker.name_span))),
                    MarkerResolution::Ambiguous => Some(Diagnostic::from_row("E0086", &[], Some(marker.name_span))),
                }
            } else if crate::Policy::applied_rule(&marker.name).is_none() && !vocabulary.knows(&marker.name)
                && matches!(resolve_marker_application(bundle, module_index, marker), MarkerResolution::Unknown)
                && imported_marker_at(bundle, module_index, &module.items, &module.imports, marker) {
                let candidates: Vec<_> = vocabulary.names().into_iter().filter(|name| name.ends_with(&format!(".{}", marker.name))).collect();
                let fix = if candidates.len() == 1 { format!("write `#{}` through its import's local name", candidates[0]) }
                    else { "write the marker through the local name of its declaring module's import".into() };
                Some(Diagnostic::error("E0085", format!("library marker `#{}` must be qualified", marker.name),
                    "markers from another module must name their import; a module's own markers may stay bare".into(), fix, Some(marker.name_span)))
            } else { None };
            if let Some(diagnostic) = diagnostic { diagnostics.push(diagnostic); }
        }
        let mut names = HashSet::new();
        for (path, declaration, _) in declaration_rows(&module.items) {
            if !names.insert(path) {
                diagnostics.push(Diagnostic::error("E0105", format!("marker `{}` is declared twice in this module", declaration.name),
                    "one (declaring module, marker name) key must have one declaration".into(), "rename or remove one declaration".into(), Some(declaration.name_span)));
            }
            let Some(members) = &declaration.group else { continue };
            for member in members {
                let row = crate::Policy::applied_rule(&member.name);
                let resolution = if row.is_none() { Some(resolve_marker_application(bundle, module_index, member)) } else { None };
                let private = declaration.is_pub && matches!(&resolution, Some(MarkerResolution::Found(resolved)) if !resolved.exported)
                    || declaration.is_pub && matches!(&resolution, Some(MarkerResolution::Private));
                if private {
                    diagnostics.push(Diagnostic::error("E0605", format!("public marker group `{}` contains private marker `#{}`", declaration.name, member.name),
                        "a public group cannot publish a module-private marker through expansion".into(),
                        "make the member public by removing `priv`, or make the group private".into(), Some(member.span)));
                    continue;
                }
                let forbidden = crate::Policy::marker_group_member_forbidden(&member.name, member.negated)
                    || matches!(&resolution, Some(MarkerResolution::Found(resolved)) if resolved.declaration.group.is_some())
                    || matches!(&resolution, Some(MarkerResolution::Unknown | MarkerResolution::Private | MarkerResolution::Ambiguous));
                if forbidden { diagnostics.push(Diagnostic::from_row("E0400", &[], Some(member.span))); }
            }
        }
        if module_index != bundle.entry && diagnostics.len() > diagnostic_start {
            let origin = Arc::new(crate::Diagnostics::DiagnosticOrigin::new(module.display.clone(), module.path.to_string_lossy().into_owned(), module.source.clone()));
            for diagnostic in &mut diagnostics[diagnostic_start..] { diagnostic.set_origin(origin.clone()); }
        }
    }
    diagnostics
}

struct MarkerBinding {
    declaration: (usize, Span),
    authored_name: Option<String>,
}
type MarkerBindings = HashMap<(usize, usize), HashMap<String, MarkerBinding>>;
fn bind_markers(markers: &mut [Marker], bindings: &MarkerBindings) {
    for marker in markers {
        if let Some(binding) = bindings.get(&(marker.name_span.start, marker.name_span.end)).and_then(|rows| rows.get(marker.name.as_str())) {
            marker.declaration = Some(binding.declaration);
            if let Some(name) = &binding.authored_name { marker.name.clone_from(name); }
        }
    }
}
fn bind_derive_names(derives: &mut [(String, Span)], bindings: &MarkerBindings) {
    for (name, span) in derives {
        if let Some(authored) = bindings.get(&(span.start, span.end)).and_then(|rows| rows.get(name.as_str())).and_then(|binding| binding.authored_name.as_ref()) {
            name.clone_from(authored);
        }
    }
}

fn bind_items(items: &mut [Item], bindings: &MarkerBindings) {
    for item in items {
        match item {
            Item::Struct(definition) => {
                bind_markers(&mut definition.type_markers, bindings);
                bind_derive_names(&mut definition.derives, bindings);
                for field in &mut definition.fields { bind_markers(&mut field.serde_markers, bindings); }
                for method in &mut definition.methods { bind_markers(&mut method.markers, bindings); }
                for implementation in &mut definition.trait_impls { for method in &mut implementation.methods { bind_markers(&mut method.markers, bindings); } }
            }
            Item::Enum(definition) => {
                bind_markers(&mut definition.type_markers, bindings);
                bind_derive_names(&mut definition.derives, bindings);
                for variant in &mut definition.variants { bind_markers(&mut variant.serde_markers, bindings); }
                for method in &mut definition.methods { bind_markers(&mut method.markers, bindings); }
                for implementation in &mut definition.trait_impls { for method in &mut implementation.methods { bind_markers(&mut method.markers, bindings); } }
            }
            Item::Distinct(definition) => {
                bind_markers(&mut definition.type_markers, bindings);
                bind_derive_names(&mut definition.derives, bindings);
            }
            Item::Func(function) => bind_markers(&mut function.markers, bindings),
            Item::Impl(implementation) => { for method in &mut implementation.methods { bind_markers(&mut method.markers, bindings); } }
            Item::CodeModule(module) => { if let Some(body) = &mut module.body { bind_items(body, bindings); } }
            Item::GenericModule(module) => bind_items(&mut module.body, bindings),
            _ => {}
        }
    }
}

fn item_end(item: &Item) -> usize {
    match item {
        Item::Func(row) => row.span.end,
        Item::Struct(row) => row.span.end,
        Item::Enum(row) => row.span.end,
        Item::Distinct(row) => row.span.end,
        Item::TypeAlias(row) => row.span.end,
        Item::UnitFamily(row) => row.span.end,
        Item::Trait(row) => row.span.end,
        Item::Tag(row) => row.span.end,
        Item::EffectDecl(row) => row.span.end,
        Item::Impl(row) => row.span.end,
        Item::Const(row) => row.span.end,
        Item::Test(row) => row.span.end,
        Item::ExternRust(row) => row.span.end,
        Item::Module(row) => row.span.end,
        Item::CModule(row) => row.span.end,
        Item::CodeModule(row) => row.span.end,
        Item::ErrorConv(row) => row.body_span.end,
        Item::Migration(row) => row.span.end,
        Item::ProtocolDecl(row) => row.span.end,
        Item::UserDerive(row) => row.span.end,
        Item::TemplateLoop(row) => row.span.end,
        Item::GenericModule(row) => row.span.end,
        Item::ModuleAlias(row) => row.span.end,
        Item::MarkerDecl(row) => row.span.end,
        Item::FactDecl(row) => row.span.end,
    }
}

fn merge_replayed_imports(imports: &mut [crate::AST::ImportDecl], replayed: &[crate::AST::ImportDecl]) {
    for replayed in replayed {
        if let Some(existing) = imports.iter_mut().find(|import| import.span == replayed.span && import.alias_span == replayed.alias_span) {
            existing.is_pub = replayed.is_pub;
            existing.is_package_pub = replayed.is_package_pub;
        }
    }
}

/// Replace parser-owned payloads in place. The loader may have retained only
/// selected Core items, synthesized siblings, or assembled namespace imports.
/// None of those loader facts may be recreated or discarded by parser replay.
fn merge_replayed_items(items: &mut [Item], replayed: Vec<Item>) {
    for mut replayed in replayed {
        let end = item_end(&replayed);
        let Some(existing) = items.iter_mut().find(|item| item_end(item) == end && std::mem::discriminant(&**item) == std::mem::discriminant(&replayed)) else { continue };
        match (existing, &mut replayed) {
            (Item::CodeModule(existing), Item::CodeModule(replayed)) => {
                existing.is_pub = replayed.is_pub;
                existing.is_package_pub = replayed.is_package_pub;
                existing.web_target = replayed.web_target;
                merge_replayed_imports(&mut existing.imports, &replayed.imports);
                if let (Some(body), Some(replayed)) = (&mut existing.body, replayed.body.take()) { merge_replayed_items(body, replayed); }
            }
            (Item::GenericModule(existing), Item::GenericModule(replayed)) => {
                existing.is_pub = replayed.is_pub;
                existing.is_package_pub = replayed.is_package_pub;
                merge_replayed_imports(&mut existing.imports, &replayed.imports);
                merge_replayed_items(&mut existing.body, std::mem::take(&mut replayed.body));
            }
            (Item::Struct(existing), Item::Struct(replayed)) => {
                replayed.auto_derive_default = existing.auto_derive_default;
                std::mem::swap(existing, replayed);
            }
            (Item::Enum(existing), Item::Enum(replayed)) => {
                replayed.auto_derive_default = existing.auto_derive_default;
                std::mem::swap(existing, replayed);
            }
            (Item::Func(existing), _) if existing.compiler_generated => {}
            (existing, _) => *existing = replayed,
        }
    }
}

fn visit_marker_imports(bundle: &ProgramBundle, module: usize, items: &[Item], imports: &[crate::AST::ImportDecl], visited: &mut [bool], order: &mut Vec<usize>) {
    for import in imports {
        if let Some(target) = import_target(bundle, module, import) { visit_marker_module(bundle, target, visited, order); }
    }
    for item in items {
        match item {
            Item::CodeModule(namespace) => {
                if let Some(body) = &namespace.body { visit_marker_imports(bundle, module, body, &namespace.imports, visited, order); }
            }
            Item::GenericModule(namespace) => visit_marker_imports(bundle, module, &namespace.body, &namespace.imports, visited, order),
            _ => {}
        }
    }
}

fn visit_marker_module(bundle: &ProgramBundle, module: usize, visited: &mut [bool], order: &mut Vec<usize>) {
    // Visiting edges are already covered by their owner; loader-owned cyclic
    // package edges do not recursively replay a source module.
    if visited[module] { return; }
    visited[module] = true;
    let source = &bundle.modules[module];
    visit_marker_imports(bundle, module, &source.items, &source.imports, visited, order);
    order.push(module);
}

/// Replay fixed groups through ordinary parser applicators. Member identities
/// bind lexically; arguments, sites and duplicates are checked at the use site.
pub(crate) fn expand_marker_groups(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    let mut raw_groups = HashMap::new();
    for (index, module) in bundle.modules.iter().enumerate() {
        // Qualified fixed members are import reads in the declaring scope,
        // even when expansion later restores a member's local spelling.
        for (_, declaration, _) in declaration_rows(&module.items) {
            if let Some(members) = &declaration.group {
                for member in members {
                    if let Some(span) = import_span_at(&module.items, &module.imports, &member.name, member.name_span) {
                        bundle.name_ledger.record_loader_alias_use(index, span);
                    }
                }
            }
        }
        if declaration_rows(&module.items).iter().any(|(_, declaration, _)| declaration.group.is_some()) {
            let (tokens, _) = crate::Lexer::lex_generated(&module.source);
            raw_groups.insert(index, crate::Parser::named_marker_groups(&crate::Lexer::without_comments(&tokens)));
        }
    }
    let mut diagnostics = Vec::new();
    let mut order = Vec::with_capacity(bundle.modules.len());
    let mut visited = vec![false; bundle.modules.len()];
    for index in 0..bundle.modules.len() { visit_marker_module(bundle, index, &mut visited, &mut order); }
    for index in order {
        let vocabulary = module_marker_vocabulary(bundle, index, std::iter::empty());
        let mut groups = Vec::new();
        let mut group_bindings = HashMap::<String, Vec<(String, String, usize, Span)>>::new();
        for declaration in vocabulary.declarations().filter(|declaration| declaration.group.is_some()) {
            let MarkerResolution::Found(resolved) = resolve_marker_declaration(bundle, index, &declaration.name) else { continue };
            let Some(raw) = raw_groups.get(&resolved.module).and_then(|rows| rows.iter().find(|row| row.name_span == resolved.declaration.name_span)) else { continue };
            let mut bindings = Vec::new();
            let mut spellings = HashMap::new();
            for member in resolved.declaration.group.as_ref().unwrap() {
                if crate::Policy::applied_rule(&member.name).is_some() { continue; }
                if let MarkerResolution::Found(member_declaration) = resolve_marker_application(bundle, resolved.module, member) {
                    // Reserved transport heads cannot collide with authored
                    // groups. Binding restores the member's written spelling.
                    let spelling = format!("__marker.{}.{}", declaration.name, member.name);
                    bindings.push((spelling.clone(), member.name.clone(), member_declaration.module, member_declaration.declaration.name_span));
                    spellings.insert(member.name.clone(), spelling);
                }
            }
            group_bindings.insert(declaration.name.clone(), bindings);
            let mut raw = raw.clone();
            raw.name = declaration.name.clone();
            crate::Parser::map_marker_group_members(&mut raw, |name| spellings.get(name).cloned());
            groups.push(raw);
        }
        if groups.is_empty() { continue; }
        let source = &bundle.modules[index].source;
        let (tokens, errors) = crate::Lexer::lex_generated(source);
        if !errors.is_empty() { diagnostics.extend(errors); continue; }
        let tokens = crate::Lexer::without_comments(&tokens);
        let (expanded, uses) = crate::Parser::expand_named_marker_groups_with_uses(&tokens, &groups);
        if uses.is_empty() { continue; }
        let diagnostic_start = diagnostics.len();
        let mut bindings = MarkerBindings::new();
        let mut imports = Vec::new();
        scoped_imports(&bundle.modules[index].items, &bundle.modules[index].imports, "", &mut imports);
        let alias_uses: Vec<_> = uses.iter().filter_map(|application| {
            imports.iter().filter(|(alias, _)| application.name.strip_prefix(alias.as_str()).is_some_and(|tail| tail.starts_with('.')))
                .max_by_key(|(alias, _)| alias.len()).map(|(_, import)| import.alias_span)
        }).collect();
        for application in &uses {
            if let Some(members) = group_bindings.get(&application.name) {
                for (name, authored_name, module, span) in members {
                    bindings.entry((application.span.start, application.span.end)).or_default().insert(name.clone(), MarkerBinding {
                        declaration: (*module, *span), authored_name: Some(authored_name.clone()),
                    });
                }
            }
        }
        match crate::Parser::parse_for_check_with_source(&expanded, source) {
            Ok((mut program, teaching)) => {
                diagnostics.extend(teaching.into_iter().filter(|diagnostic| diagnostic.severity == crate::Diagnostics::Severity::Error));
                bind_items(&mut program.items, &bindings);
                for fact in &mut program.rule_facts { bind_markers(std::slice::from_mut(&mut fact.marker), &bindings); }
                let module = &mut bundle.modules[index];
                merge_replayed_items(&mut module.items, program.items);
                merge_replayed_imports(&mut module.imports, &program.imports);
                let expanded_use = |span: Span| uses.iter().any(|application| application.span.start <= span.start && span.end <= application.span.end);
                module.rule_facts.retain(|fact| fact.marker.name_span.start == fact.marker.name_span.end || !expanded_use(fact.marker.name_span));
                module.rule_facts.extend(program.rule_facts.into_iter().filter(|fact| expanded_use(fact.marker.name_span)));
                for span in program.block_spans {
                    if !module.block_spans.contains(&span) { module.block_spans.push(span); }
                }
                module.policy_declarations.retain(|declaration| declaration.span.start == declaration.span.end || !expanded_use(declaration.span));
                module.policy_declarations.extend(program.policy_declarations.into_iter().filter(|declaration| expanded_use(declaration.span)).map(|mut declaration| {
                    declaration.source = module.display.clone(); declaration
                }));
                module.web_target_ceiling = program.web_target_ceiling;
                module.no_prelude = program.no_prelude;
                module.default_target = program.default_target;
                module.html_path = program.html_path;
            }
            Err(errors) => diagnostics.extend(errors),
        }
        for span in alias_uses { bundle.name_ledger.record_loader_alias_use(index, span); }
        if index != bundle.entry && diagnostics.len() > diagnostic_start {
            let module = &bundle.modules[index];
            let origin = Arc::new(crate::Diagnostics::DiagnosticOrigin::new(module.display.clone(), module.path.to_string_lossy().into_owned(), module.source.clone()));
            for diagnostic in &mut diagnostics[diagnostic_start..] { diagnostic.set_origin(origin.clone()); }
        }
        // Local groups were already checked in the initial parse. The replay
        // replaces those exact reports; distinct repeats at one span remain.
        bundle.parse_teaching.retain(|previous| !diagnostics[diagnostic_start..].iter().any(|replayed| {
            previous.origin == replayed.origin && previous.severity == replayed.severity
                && previous.code == replayed.code && previous.span == replayed.span
                && previous.what == replayed.what && previous.why == replayed.why
                && previous.fix == replayed.fix && previous.edit == replayed.edit
        }));
    }
    diagnostics
}
