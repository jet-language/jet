//! Consumer-neutral semantic symbol facts for docs, completion, hover, and help.
//!
//! Program symbols come from `SymbolDB`; language builtins live here once.
//! Consumers choose presentation only — never rebuild signatures or identity.

use std::collections::HashMap;

use jet_foundation::Syntax;
use jet_foundation::AST::ProgramBundle;
use jet_foundation::{AST, Collections};

use crate::Build::{function_parameter_parts, SymKind, SymbolDB};
use crate::Types::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticSymbolKind {
    Module,
    Function,
    Type,
    Member,
    Constant,
    Local,
    Parameter,
    Keyword,
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticProvenance {
    Source { module_path: String },
    Builtin { module: String },
    CommandRegistry,
    Session,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticLexicalScope {
    pub identity: String,
    pub structural_parent: usize,
    pub structural_slot: String,
    pub span: SourceSpan,
    pub depth: usize,
    pub declaration_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticVisibilityAnchor<'a> {
    pub module_path: &'a str,
    pub offset: Option<usize>,
    pub session_top_level: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticSymbol {
    pub identity: String,
    pub name: String,
    pub qualified_name: String,
    pub owner: Option<String>,
    pub module_path: String,
    pub kind: SemanticSymbolKind,
    pub signature: String,
    pub summary: String,
    pub examples: Vec<String>,
    pub provenance: SemanticProvenance,
    pub span: Option<SourceSpan>,
    pub lexical_scope: Option<SemanticLexicalScope>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticSymbolIndex {
    symbols: Vec<SemanticSymbol>,
}

impl SemanticSymbolIndex {
    pub fn new(symbols: Vec<SemanticSymbol>) -> Self {
        let mut index = Self { symbols };
        index.sort();
        index
    }

    pub fn language() -> Self {
        Self::new(language_symbols())
    }

    pub fn symbols(&self) -> &[SemanticSymbol] {
        &self.symbols
    }

    pub fn push(&mut self, symbol: SemanticSymbol) {
        if let Some(slot) = self
            .symbols
            .iter_mut()
            .find(|existing| existing.identity == symbol.identity)
        {
            *slot = symbol;
        } else {
            self.symbols.push(symbol);
        }
        self.sort();
    }

    pub fn extend(&mut self, symbols: impl IntoIterator<Item = SemanticSymbol>) {
        for symbol in symbols {
            self.push(symbol);
        }
    }

    pub fn lookup_identity(&self, identity: &str) -> Option<&SemanticSymbol> {
        self.symbols.iter().find(|symbol| symbol.identity == identity)
    }

    pub fn lookup_qualified(&self, name: &str) -> Option<&SemanticSymbol> {
        let mut matches = self
            .symbols
            .iter()
            .filter(|symbol| symbol.qualified_name == name);
        let symbol = matches.next()?;
        matches.next().is_none().then_some(symbol)
    }

    pub fn lookup(&self, name: &str) -> Vec<&SemanticSymbol> {
        self.symbols
            .iter()
            .filter(|symbol| symbol.name == name || symbol.qualified_name == name)
            .collect()
    }

    pub fn lookup_member(&self, owner: &str, name: &str) -> Vec<&SemanticSymbol> {
        self.symbols
            .iter()
            .filter(|symbol| symbol.owner.as_deref() == Some(owner) && symbol.name == name)
            .collect()
    }

    /// Resolve the symbol visible through an unqualified spelling, or an
    /// explicitly qualified symbol when `name` contains its owner.
    ///
    /// Identity lookup remains lossless; this method applies Jet's shadowing
    /// order only at a consumer's visible-name boundary.
    pub fn resolve_visible(&self, name: &str) -> Option<&SemanticSymbol> {
        self.resolve_visible_in(name, None)
    }

    pub fn resolve_visible_in(
        &self,
        name: &str,
        module_path: Option<&str>,
    ) -> Option<&SemanticSymbol> {
        self.resolve_with_anchor(name, module_path.map(|module_path| SemanticVisibilityAnchor {
            module_path,
            offset: None,
            session_top_level: false,
        }))
    }

    pub fn resolve_visible_at(
        &self,
        name: &str,
        anchor: SemanticVisibilityAnchor<'_>,
    ) -> Option<&SemanticSymbol> {
        self.resolve_with_anchor(name, Some(anchor))
    }

    fn resolve_with_anchor(
        &self,
        name: &str,
        anchor: Option<SemanticVisibilityAnchor<'_>>,
    ) -> Option<&SemanticSymbol> {
        let qualified = name.contains('.');
        self.symbols
            .iter()
            .filter(|symbol| {
                if qualified {
                    symbol.qualified_name == name
                } else {
                    symbol.owner.is_none() && symbol.name == name
                }
            })
            .filter_map(|symbol| visibility_key(symbol, anchor).map(|key| (key, symbol)))
            .min_by_key(|(key, _)| *key)
            .map(|(_, symbol)| symbol)
    }

    pub fn complete(&self, prefix: &str, owner: Option<&str>) -> Vec<&SemanticSymbol> {
        self.symbols
            .iter()
            .filter(|symbol| symbol.owner.as_deref() == owner && symbol.name.starts_with(prefix))
            .collect()
    }

    /// Members of a Core module path (`core.math`, …) for REPL/LSP completion.
    ///
    /// Names come from the canonical Sema catalog (`core_module_items`) so
    /// completion, diagnostics, and codegen stay on one list.
    pub fn complete_core_module(module: &str, prefix: &str) -> Vec<SemanticSymbol> {
        core_module_member_symbols(module)
            .into_iter()
            .filter(|symbol| symbol.name.starts_with(prefix))
            .collect()
    }

    /// One Core module member by exact name, or `None` when absent.
    pub fn lookup_core_module_member(module: &str, name: &str) -> Option<SemanticSymbol> {
        core_module_member_symbols(module)
            .into_iter()
            .find(|symbol| symbol.name == name)
    }

    /// Complete visible spellings once while retaining every symbol by
    /// identity and every explicitly qualified owner path.
    pub fn complete_visible(&self, prefix: &str, owner: Option<&str>) -> Vec<&SemanticSymbol> {
        self.complete_visible_in(prefix, owner, None)
    }

    pub fn complete_visible_in(
        &self,
        prefix: &str,
        owner: Option<&str>,
        module_path: Option<&str>,
    ) -> Vec<&SemanticSymbol> {
        self.complete_with_anchor(
            prefix,
            owner,
            module_path.map(|module_path| SemanticVisibilityAnchor {
                module_path,
                offset: None,
                session_top_level: false,
            }),
        )
    }

    pub fn complete_visible_at(
        &self,
        prefix: &str,
        owner: Option<&str>,
        anchor: SemanticVisibilityAnchor<'_>,
    ) -> Vec<&SemanticSymbol> {
        self.complete_with_anchor(prefix, owner, Some(anchor))
    }

    fn complete_with_anchor(
        &self,
        prefix: &str,
        owner: Option<&str>,
        anchor: Option<SemanticVisibilityAnchor<'_>>,
    ) -> Vec<&SemanticSymbol> {
        let explicit_qualified = owner.is_none() && prefix.contains('.');
        let mut visible: Vec<(&str, (u8, usize, usize), &SemanticSymbol)> = Vec::new();
        for symbol in &self.symbols {
            if prefix.is_empty()
                && Syntax::classify_identifier(&symbol.name) == Syntax::IdentifierClass::SoftPublic
            {
                continue;
            }
            let matches = if explicit_qualified {
                symbol.qualified_name.starts_with(prefix)
            } else {
                symbol.owner.as_deref() == owner && symbol.name.starts_with(prefix)
            };
            if !matches {
                continue;
            }
            let Some(key) = visibility_key(symbol, anchor) else {
                continue;
            };
            let spelling = if explicit_qualified {
                symbol.qualified_name.as_str()
            } else {
                symbol.name.as_str()
            };
            if let Some((_, current_key, current)) =
                visible.iter_mut().find(|(name, _, _)| *name == spelling)
            {
                if key < *current_key {
                    *current_key = key;
                    *current = symbol;
                }
            } else {
                visible.push((spelling, key, symbol));
            }
        }
        visible.into_iter().map(|(_, _, symbol)| symbol).collect()
    }

    pub fn at(&self, module_path: &str, offset: usize) -> Option<&SemanticSymbol> {
        self.symbols.iter().find(|symbol| {
            symbol.module_path == module_path
                && symbol
                    .span
                    .is_some_and(|span| span.start <= offset && offset <= span.end)
        })
    }

    fn sort(&mut self) {
        self.symbols.sort_by(|a, b| {
            a.qualified_name
                .cmp(&b.qualified_name)
                .then(a.module_path.cmp(&b.module_path))
                .then(a.identity.cmp(&b.identity))
        });
    }
}

fn visibility_key(
    symbol: &SemanticSymbol,
    anchor: Option<SemanticVisibilityAnchor<'_>>,
) -> Option<(u8, usize, usize)> {
    if symbol.identity.starts_with("session:binding:") {
        return Some((0, 0, 0));
    }
    if let Some(scope) = &symbol.lexical_scope {
        let anchor = anchor?;
        if anchor.session_top_level || symbol.module_path != anchor.module_path {
            return None;
        }
        let offset = anchor.offset?;
        if offset < scope.declaration_offset || offset < scope.span.start || scope.span.end < offset {
            return None;
        }
        return Some((1, usize::MAX - scope.depth, usize::MAX - scope.declaration_offset));
    }
    if matches!(symbol.kind, SemanticSymbolKind::Local | SemanticSymbolKind::Parameter) {
        if let Some(anchor) = anchor {
            if anchor.session_top_level {
                return None;
            }
            return None;
        }
        return Some((1, 0, 0));
    }
    let module_path = anchor.map(|anchor| anchor.module_path);
    if !symbol.identity.starts_with("import:") {
        match symbol.provenance {
            SemanticProvenance::Session => return Some((2, 0, 0)),
            SemanticProvenance::Source { .. } => {
                return Some((if module_path.is_none_or(|path| symbol.module_path == path) {
                    2
                } else {
                    5
                }, 0, 0));
            }
            _ => {}
        }
    }
    if symbol.identity.starts_with("import:") {
        if module_path.is_some_and(|path| symbol.module_path != path) {
            return None;
        }
        return Some((3, 0, 0));
    }
    Some((4, 0, 0))
}

pub(crate) fn canonical_symbol_name(
    bundle: &ProgramBundle,
    module_path: &str,
    name: &str,
    owner: Option<&str>,
    span: Option<(usize, usize)>,
) -> String {
    let Some(module_idx) = bundle
        .modules
        .iter()
        .position(|module| module.display == module_path || module.alias == module_path)
    else {
        return owner.map_or_else(|| name.to_string(), |owner| format!("{owner}.{name}"));
    };
    let ledger = &bundle.name_ledger;
    if let Some((start, end)) = span {
        if let Some(path) = ledger.canonical_path_at(module_idx, start, end) {
            return path;
        }
    }
    match owner {
        Some(owner) => {
            let owner_path = ledger
                .display_path(module_idx, owner, Some(module_idx))
                .or_else(|| ledger.canonical_path(module_idx, owner))
                .unwrap_or_else(|| owner.to_string());
            format!("{owner_path}.{name}")
        }
        None => ledger
            .display_path(module_idx, name, Some(module_idx))
            .or_else(|| ledger.canonical_path(module_idx, name))
            .unwrap_or_else(|| name.to_string()),
    }
}

pub fn build_semantic_symbol_index(db: &SymbolDB, bundle: &ProgramBundle) -> SemanticSymbolIndex {
    let mut symbols = language_symbols();
    for module in &bundle.modules {
        collect_distinct_conversion_symbols(
            db,
            &module.items,
            &module.display,
            &format!("module:{}", module.display),
            None,
            &mut symbols,
        );
    }
    let members: HashMap<&str, &str> = db
        .members
        .iter()
        .map(|member| (member.identity.as_str(), member.owner.as_str()))
        .collect();
    let sources: HashMap<&str, &str> = bundle
        .modules
        .iter()
        .map(|module| (module.display.as_str(), module.source.as_str()))
        .collect();

    for def in &db.defs {
        let owner = members.get(def.identity.as_str()).map(|owner| (*owner).to_string()).or_else(|| {
            match &def.kind {
                SymKind::Field { parent, .. } | SymKind::EnumVariant { parent } => {
                    Some(parent.clone())
                }
                _ => None,
            }
        });
        let qualified_name = canonical_symbol_name(
            bundle,
            &def.module_path,
            &def.name,
            owner.as_deref(),
            Some((def.def_span.start, def.def_span.end)),
        );
        let (kind, signature) = semantic_shape(&def.name, &def.kind, owner.as_deref());
        let mut docs = sources
            .get(def.module_path.as_str())
            .map(|source| source_docs(source, def.def_span.start))
            .unwrap_or_default();
        if docs.0.is_empty() {
            if let SymKind::Local { mutable, .. } = &def.kind {
                docs.0 = if *mutable {
                    "mutable binding".to_string()
                } else {
                    "immutable binding".to_string()
                };
            }
        }
        let lexical_scope = if matches!(def.kind, SymKind::Local { .. } | SymKind::Param { .. }) {
            lexical_scope_for_def(
                db,
                &def.module_path,
                def.def_span.into(),
                matches!(def.kind, SymKind::Param { .. }),
            )
        } else {
            None
        };
        let identity = if lexical_scope.is_some() {
            format!("{}@{}", def.identity, def.def_span.start)
        } else {
            def.identity.clone()
        };
        symbols.push(SemanticSymbol {
            identity,
            name: def.name.clone(),
            qualified_name,
            owner,
            module_path: def.module_path.clone(),
            kind,
            signature,
            summary: docs.0,
            examples: docs.1,
            provenance: SemanticProvenance::Source {
                module_path: def.module_path.clone(),
            },
            span: Some(def.def_span.into()),
            lexical_scope,
        });
    }
    for module in &bundle.modules {
        collect_import_symbols(&mut symbols, bundle, module, &module.imports, None);
        collect_inline_import_symbols(db, bundle, module, &module.items, None, &mut symbols);
    }
    SemanticSymbolIndex::new(symbols)
}

fn collect_import_symbols(
    symbols: &mut Vec<SemanticSymbol>,
    bundle: &ProgramBundle,
    module: &jet_foundation::AST::LoadedModule,
    imports: &[jet_foundation::AST::ImportDecl],
    lexical_scope: Option<&SemanticLexicalScope>,
) {
    for import in imports {
        match &import.kind {
            jet_foundation::AST::ImportKind::Unqualified {
                module_alias,
                ..
            } => {
                let source_import = imports
                    .iter()
                    .chain(module.imports.iter())
                    .find(|candidate| {
                        !matches!(
                            &candidate.kind,
                            jet_foundation::AST::ImportKind::Unqualified { .. }
                        ) && candidate.import_alias() == *module_alias
                    });
                let imported_module = source_import.and_then(|source_import| {
                    match &source_import.kind {
                        jet_foundation::AST::ImportKind::File(path, _) => {
                            let relative = module
                                .path
                                .parent()
                                .unwrap_or_else(|| std::path::Path::new("."))
                                .join(format!("{path}.jet"));
                            bundle.modules.iter().find(|candidate| {
                                candidate.path == relative
                                    || candidate.path.file_stem() == relative.file_stem()
                            })
                        }
                        jet_foundation::AST::ImportKind::Module(name, _) => bundle
                            .modules
                            .iter()
                            .find(|candidate| candidate.alias == *name),
                        jet_foundation::AST::ImportKind::Unqualified { .. } => None,
                    }
                });
                let bindings = import.walk_bindings();
                let items_span = bindings
                    .first()
                    .and_then(|binding| binding.items_span)
                    .unwrap_or(import.span);
                for binding in &bindings {
                    let original = binding
                        .original
                        .expect("member walker returned a binding without a member");
                    let local_name = binding.local.clone();
                    let target = imported_module
                        .and_then(|imported| {
                            symbols.iter().find(|symbol| {
                                symbol.module_path == imported.display && symbol.name == original
                            })
                        })
                        .cloned();
                    let mut symbol = target.unwrap_or_else(|| SemanticSymbol {
                        identity: String::new(),
                        name: local_name.clone(),
                        qualified_name: local_name.clone(),
                        owner: None,
                        module_path: module.display.clone(),
                        kind: SemanticSymbolKind::Local,
                        signature: local_name.clone(),
                        summary: format!("Imported from {module_alias}."),
                        examples: Vec::new(),
                        provenance: SemanticProvenance::Source {
                            module_path: module.display.clone(),
                        },
                        span: Some(items_span.into()),
                        lexical_scope: lexical_scope.cloned(),
                    });
                    let suffix = format!("{module_alias}.{original}::{local_name}");
                    symbol.identity = import_identity(module, lexical_scope, &suffix);
                    symbol.name = local_name.clone();
                    symbol.qualified_name = local_name;
                    symbol.owner = None;
                    symbol.module_path = module.display.clone();
                    symbol.provenance = SemanticProvenance::Source {
                        module_path: module.display.clone(),
                    };
                    symbol.span = Some(items_span.into());
                    symbol.lexical_scope = lexical_scope.cloned();
                    symbols.push(symbol);
                }
            }
            jet_foundation::AST::ImportKind::File(_, _)
            | jet_foundation::AST::ImportKind::Module(_, _) => {
                let alias = import.import_alias();
                let module_idx = bundle
                    .modules
                    .iter()
                    .position(|candidate| candidate.display == module.display);
                let alias_name = module_idx
                    .and_then(|module_idx| {
                        bundle
                            .name_ledger
                            .effective_alias(module_idx, &alias)
                    })
                    .map(|name| name.name.clone())
                    .unwrap_or_else(|| alias.clone());
                symbols.push(SemanticSymbol {
                    identity: import_identity(module, lexical_scope, &alias_name),
                    name: alias_name.clone(),
                    qualified_name: alias_name.clone(),
                    owner: None,
                    module_path: module.display.clone(),
                    kind: SemanticSymbolKind::Module,
                    signature: format!("use {alias_name}"),
                    summary: "Imported module.".to_string(),
                    examples: Vec::new(),
                    provenance: SemanticProvenance::Source {
                        module_path: module.display.clone(),
                    },
                    span: Some(import.alias_span.into()),
                    lexical_scope: lexical_scope.cloned(),
                });
            }
        }
    }
}

fn import_identity(
    module: &jet_foundation::AST::LoadedModule,
    lexical_scope: Option<&SemanticLexicalScope>,
    suffix: &str,
) -> String {
    lexical_scope.map_or_else(
        || format!("import:{}::{suffix}", module.display),
        |scope| format!("import:{}::{}::{suffix}", module.display, scope.identity),
    )
}

fn collect_inline_import_symbols(
    db: &SymbolDB,
    bundle: &ProgramBundle,
    module: &jet_foundation::AST::LoadedModule,
    items: &[AST::Item],
    parent_scope: Option<&SemanticLexicalScope>,
    symbols: &mut Vec<SemanticSymbol>,
) {
    for item in items {
        let AST::Item::CodeModule(code_module) = item else {
            continue;
        };
        let scope = inline_module_scope(db, module.display.as_str(), code_module, parent_scope);
        collect_import_symbols(
            symbols,
            bundle,
            module,
            &code_module.imports,
            Some(&scope),
        );
        if let Some(body) = &code_module.body {
            collect_inline_import_symbols(db, bundle, module, body, Some(&scope), symbols);
        }
    }
}

fn inline_module_scope(
    db: &SymbolDB,
    module_path: &str,
    module: &AST::CodeModule,
    parent_scope: Option<&SemanticLexicalScope>,
) -> SemanticLexicalScope {
    let identity = db
        .defs
        .iter()
        .find(|def| {
            def.module_path == module_path
                && def.def_span == module.name_span
                && matches!(&def.kind, SymKind::Module)
        })
        .map(|def| def.identity.clone())
        .unwrap_or_else(|| {
            let parent_identity = parent_scope
                .map(|scope| scope.identity.clone())
                .unwrap_or_else(|| format!("module:{module_path}"));
            format!("module:{parent_identity}::{}", module.name)
        });
    let structural_parent = db
        .nodes
        .iter()
        .find(|node| {
            node.module_path == module_path
                && node.span == module.span.into()
                && node.class == "item"
        })
        .map_or(0, |node| node.id);
    SemanticLexicalScope {
        identity,
        structural_parent,
        structural_slot: "items".to_string(),
        span: module.span.into(),
        depth: parent_scope.map_or(1, |scope| scope.depth + 1),
        declaration_offset: module.name_span.start,
    }
}

fn collect_distinct_conversion_symbols(
    db: &SymbolDB,
    items: &[AST::Item],
    module_path: &str,
    scope_identity: &str,
    lexical_scope: Option<&SemanticLexicalScope>,
    symbols: &mut Vec<SemanticSymbol>,
) {
    for item in items {
        match item {
            AST::Item::Distinct(def) => symbols.extend(distinct_conversion_symbols(
                &def.name,
                &def.base,
                def.range.is_some(),
                module_path,
                scope_identity,
                def.name_span.into(),
                lexical_scope,
            )),
            AST::Item::UnitFamily(family) => {
                for def in family.distinct_defs() {
                    symbols.extend(distinct_conversion_symbols(
                        &def.name,
                        &def.base,
                        false,
                        module_path,
                        scope_identity,
                        def.name_span.into(),
                        lexical_scope,
                    ));
                }
            }
            AST::Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    let identity = db
                        .defs
                        .iter()
                        .find(|def| {
                            def.module_path == module_path
                                && def.def_span == module.name_span
                                && matches!(def.kind, SymKind::Module)
                        })
                        .map(|def| def.identity.clone())
                        .unwrap_or_else(|| format!("module:{scope_identity}::{}", module.name));
                    let structural_parent = db
                        .nodes
                        .iter()
                        .find(|node| {
                            node.module_path == module_path
                                && node.span == module.span.into()
                                && node.class == "item"
                        })
                        .map_or(0, |node| node.id);
                    let scope = SemanticLexicalScope {
                        identity: identity.clone(),
                        structural_parent,
                        structural_slot: "items".to_string(),
                        span: module.span.into(),
                        depth: lexical_scope.map_or(1, |scope| scope.depth + 1),
                        declaration_offset: module.name_span.start,
                    };
                    collect_distinct_conversion_symbols(
                        db,
                        body,
                        module_path,
                        &identity,
                        Some(&scope),
                        symbols,
                    );
                }
            }
            _ => {}
        }
    }
}

fn distinct_conversion_symbols(
    owner: &str,
    base: &AST::Type,
    ranged: bool,
    module_path: &str,
    scope_identity: &str,
    span: SourceSpan,
    lexical_scope: Option<&SemanticLexicalScope>,
) -> Vec<SemanticSymbol> {
    let methods: Vec<(String, String)> = if base.is_numeric() {
        Syntax::NUMERIC_CONVERSION_SOURCES
            .iter()
            .map(|(method, source)| (method.to_string(), source.to_string()))
            .collect()
    } else {
        vec![(Syntax::conversion_method_for_source(&base.name()), base.name())]
    };
    methods
        .into_iter()
        .map(|(method, source)| {
            let fallible = ranged
                || Collections::numeric_conversion_return(base, &method, 1)
                    .flatten()
                    .is_some_and(|ty| matches!(ty, AST::Type::Result { .. }));
            let qualified_name = format!("{owner}.{method}");
            SemanticSymbol {
                identity: format!("source:method:{scope_identity}:{qualified_name}"),
                name: method.to_string(),
                qualified_name,
                owner: Some(owner.to_string()),
                module_path: module_path.to_string(),
                kind: SemanticSymbolKind::Member,
                signature: format!(
                    "{owner}.{method}(value: {source}) -> {owner}{}",
                    if fallible { " ? String" } else { "" }
                ),
                summary: format!("Converts {source} to {owner}."),
                examples: Vec::new(),
                provenance: SemanticProvenance::Source {
                    module_path: module_path.to_string(),
                },
                span: Some(span),
                lexical_scope: lexical_scope.cloned(),
            }
        })
        .collect()
}

fn lexical_scope_for_def(
    db: &SymbolDB,
    module_path: &str,
    def_span: SourceSpan,
    is_param: bool,
) -> Option<SemanticLexicalScope> {
    let nodes = db
        .nodes
        .iter()
        .filter(|node| node.module_path == module_path)
        .collect::<Vec<_>>();
    let mut current = nodes
        .iter()
        .filter(|node| node.span.start <= def_span.start && def_span.end <= node.span.end)
        .min_by_key(|node| node.span.end.saturating_sub(node.span.start))
        .map(|node| node.id)?;
    let initial = current;
    let mut innermost = nodes
        .iter()
        .find(|node| node.parent == Some(initial) && is_lexical_slot(&node.slot))
        .map(|node| (initial, node.slot.clone()));
    let mut depth = usize::from(innermost.is_some());
    loop {
        let node = db.nodes.get(current)?;
        if is_lexical_slot(&node.slot) {
            depth += 1;
            if innermost.is_none() {
                innermost = Some((node.parent?, node.slot.clone()));
            }
        }
        let Some(parent) = node.parent else {
            break;
        };
        current = parent;
    }
    let (parent, slot) = innermost.or_else(|| {
        nodes
            .iter()
            .filter(|node| node.class == "item" && node.span.start <= def_span.start && def_span.end <= node.span.end)
            .min_by_key(|node| node.span.end.saturating_sub(node.span.start))
            .map(|node| (node.id, "body".to_string()))
    })?;
    let span = db
        .slot_boundaries
        .iter()
        .find(|boundary| {
            boundary.module_path == module_path
                && boundary.parent == parent
                && boundary.slot == slot
        })?
        .span;
    Some(SemanticLexicalScope {
        identity: format!("scope:{module_path}:{parent}:{slot}"),
        structural_parent: parent,
        structural_slot: slot,
        span,
        depth: depth.max(1),
        declaration_offset: if is_param { 0 } else { def_span.start },
    })
}

fn is_lexical_slot(slot: &str) -> bool {
    slot == "body" || slot.ends_with("_body") || slot.ends_with("_bodies")
}

fn semantic_shape(
    name: &str,
    kind: &SymKind,
    owner: Option<&str>,
) -> (SemanticSymbolKind, String) {
    match kind {
        SymKind::Module => (SemanticSymbolKind::Module, format!("module {name}")),
        SymKind::Function {
            params,
            param_contract,
            param_variadic,
            ret,
            effects,
            effect_via,
        } => {
            let params = function_parameter_parts(
                params,
                param_contract,
                param_variadic,
            )
            .join(", ");
            let prefix = owner.map_or_else(|| format!("fn {name}"), |owner| format!("{owner}.{name}"));
            let arrow = if let Some((param, _)) = effect_via {
                format!(" =[via {param}]=>")
            } else {
                effects.as_ref().map_or_else(
                    || ret.as_ref().map(|_| " =>".to_string()).unwrap_or_default(),
                    |row| format!(" =[{}]=>", row.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ")),
                )
            };
            let result = ret.as_ref().map(|ty| format!(" {}", ty.name())).unwrap_or_default();
            (SemanticSymbolKind::Function, format!("{prefix}({params}){arrow}{result}"))
        }
        SymKind::Struct { fields } => (
            SemanticSymbolKind::Type,
            format!(
                "struct {name} {{ {} }}",
                fields
                    .iter()
                    .map(|(name, ty)| format!("{name}: {}", ty.name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        SymKind::Enum { variants } => (
            SemanticSymbolKind::Type,
            format!("enum {name} {{ {} }}", variants.join(", ")),
        ),
        SymKind::Trait => (SemanticSymbolKind::Type, format!("trait {name}")),
        SymKind::Tag => (SemanticSymbolKind::Type, format!("tag {name}")),
        SymKind::Type => (SemanticSymbolKind::Type, format!("type {name}")),
        SymKind::Const => (SemanticSymbolKind::Constant, format!("const {name}")),
        SymKind::EnumVariant { parent } => (
            SemanticSymbolKind::Member,
            format!("{parent}.{name}"),
        ),
        SymKind::Field { ty, parent } => (
            SemanticSymbolKind::Member,
            format!("{parent}.{name}: {}", ty.name()),
        ),
        SymKind::Local { mutable: _, ty } => {
            let ty = ty.as_ref().map(|ty| ty.name()).unwrap_or_else(|| "value".to_string());
            (SemanticSymbolKind::Local, format!("{name}: {ty}"))
        }
        SymKind::Param { ty } => (
            SemanticSymbolKind::Parameter,
            format!("{name}: {}", ty.name()),
        ),
    }
}

fn source_docs(source: &str, def_start: usize) -> (String, Vec<String>) {
    let prefix = &source[..def_start.min(source.len())];
    let before_declaration = prefix.rsplit_once('\n').map_or("", |(before, _)| before);
    let mut lines = Vec::new();
    for line in before_declaration.lines().rev() {
        let line = line.trim();
        let Some(doc) = line.strip_prefix("///") else {
            break;
        };
        lines.push(doc.trim().to_string());
    }
    lines.reverse();
    let mut summary = String::new();
    let mut examples = Vec::new();
    for line in lines {
        if let Some(example) = line.strip_prefix("Example:") {
            examples.push(example.trim().to_string());
        } else if summary.is_empty() && !line.is_empty() {
            summary = line;
        }
    }
    (summary, examples)
}

fn core_module_member_symbols(module: &str) -> Vec<SemanticSymbol> {
    jet_sema::Sema::core_module_items(module)
        .into_iter()
        .map(|name| {
            let qualified_name = format!("{module}.{name}");
            let declaration = (module == "core.lang")
                .then(|| jet_foundation::Policy::rule_arg_declaration(&name))
                .flatten();
            SemanticSymbol {
                identity: format!("builtin:module:{qualified_name}"),
                name: name.clone(),
                qualified_name: qualified_name.clone(),
                owner: None,
                module_path: module.to_string(),
                kind: if declaration.is_some() {
                    SemanticSymbolKind::Type
                } else {
                    SemanticSymbolKind::Function
                },
                signature: declaration.map_or_else(
                    || qualified_name.clone(),
                    |declaration| {
                        format!(
                            "enum {} {{ {} }}",
                            qualified_name,
                            declaration.variants.join(", ")
                        )
                    },
                ),
                summary: declaration.map_or_else(String::new, |_| {
                    "Compiler vocabulary published as an ordinary enum.".to_string()
                }),
                examples: Vec::new(),
                provenance: SemanticProvenance::Builtin {
                    module: module.to_string(),
                },
                span: None,
                lexical_scope: None,
            }
        })
        .collect()
}

fn language_symbols() -> Vec<SemanticSymbol> {
    let mut symbols = Vec::new();
    for ty in Syntax::JET_TYPE_LIST {
        symbols.push(SemanticSymbol {
            identity: format!("builtin:type:{ty}"),
            name: (*ty).to_string(),
            qualified_name: (*ty).to_string(),
            owner: None,
            module_path: "core".to_string(),
            kind: SemanticSymbolKind::Type,
            signature: format!("type {ty}"),
            summary: "Built-in Jet type.".to_string(),
            examples: Vec::new(),
            provenance: SemanticProvenance::Builtin { module: "core".to_string() },
            span: None,
            lexical_scope: None,
        });
    }
    for keyword in Syntax::JET_KEYWORD_LIST {
        symbols.push(SemanticSymbol {
            identity: format!("builtin:keyword:{keyword}"),
            name: (*keyword).to_string(),
            qualified_name: (*keyword).to_string(),
            owner: None,
            module_path: "syntax".to_string(),
            kind: SemanticSymbolKind::Keyword,
            signature: (*keyword).to_string(),
            summary: "Jet keyword.".to_string(),
            examples: Vec::new(),
            provenance: SemanticProvenance::Builtin { module: "syntax".to_string() },
            span: None,
            lexical_scope: None,
        });
    }
    for &(qualified_name, signature, summary, example) in BUILTIN_METHODS {
        let (owner, name) = qualified_name.split_once('.').expect("qualified builtin method");
        symbols.push(SemanticSymbol {
            identity: format!("builtin:method:{qualified_name}"),
            name: name.to_string(),
            qualified_name: qualified_name.to_string(),
            owner: Some(owner.to_string()),
            module_path: "core.collections".to_string(),
            kind: SemanticSymbolKind::Member,
            signature: signature.to_string(),
            summary: summary.to_string(),
            examples: example.into_iter().map(str::to_string).collect(),
            provenance: SemanticProvenance::Builtin { module: "core.collections".to_string() },
            span: None,
            lexical_scope: None,
        });
    }
    for (owner, signature, summary, example) in [
        (
            "Int",
            "Int.parse(text: String) -> Int ? ParseError",
            "Parses text as an Int.",
            "Int.parse(text)?",
        ),
        (
            "Float",
            "Float.parse(text: String) -> Float ? ParseError",
            "Parses text as a Float.",
            "Float.parse(text)?",
        ),
    ] {
        let qualified_name = format!("{owner}.parse");
        symbols.push(SemanticSymbol {
            identity: format!("builtin:method:{qualified_name}"),
            name: "parse".to_string(),
            qualified_name,
            owner: Some(owner.to_string()),
            module_path: "core.numeric".to_string(),
            kind: SemanticSymbolKind::Member,
            signature: signature.to_string(),
            summary: summary.to_string(),
            examples: vec![example.to_string()],
            provenance: SemanticProvenance::Builtin {
                module: "core.numeric".to_string(),
            },
            span: None,
            lexical_scope: None,
        });
    }
    for &(method, source_name) in Syntax::NUMERIC_CONVERSION_SOURCES {
        for target_name in [
            "I8", "I16", "I32", "I64", "Int", "U8", "U16", "U32", "U64", "F32", "F64",
            "Float",
        ] {
            let target = AST::numeric_type_from_name(target_name)
                .expect("numeric catalog target names a numeric type");
            let ret = Collections::numeric_conversion_return(&target, method, 1)
                .flatten()
                .expect("numeric conversion catalog entry has a return type");
            let result = match ret {
                AST::Type::Result { .. } => format!("{target_name} ? String"),
                _ => target_name.to_string(),
            };
            let qualified_name = format!("{target_name}.{method}");
            symbols.push(SemanticSymbol {
                identity: format!("builtin:method:{qualified_name}"),
                name: method.to_string(),
                qualified_name,
                owner: Some(target_name.to_string()),
                module_path: "core.numeric".to_string(),
                kind: SemanticSymbolKind::Member,
                signature: format!("{target_name}.{method}(value: {source_name}) -> {result}"),
                summary: format!("Converts {source_name} to {target_name}."),
                examples: Vec::new(),
                provenance: SemanticProvenance::Builtin { module: "core.numeric".to_string() },
                span: None,
                lexical_scope: None,
            });
        }
    }
    symbols
}

const BUILTIN_METHODS: &[(&str, &str, &str, Option<&str>)] = &[
    ("Duration.milliseconds", "Duration.milliseconds(value: Int | Float) -> Duration ? RangeError", "Checked runtime duration in milliseconds.", None),
    ("Duration.seconds", "Duration.seconds(value: Int | Float) -> Duration ? RangeError", "Checked runtime duration in seconds.", None),
    ("Duration.minutes", "Duration.minutes(value: Int | Float) -> Duration ? RangeError", "Checked runtime duration in minutes.", None),
    ("Clock.system", "Clock.system() -> Clock #(Time)", "Explicit monotonic production clock.", None),
    ("Duration.hours", "Duration.hours(value: Int | Float) -> Duration ? RangeError", "Checked runtime duration in hours.", None),
    ("Duration.in", "Duration.in(unit: DurationUnit) -> Int ? RangeError", "Reads a checked whole duration unit.", Some("duration.in(.Milliseconds)?")),
    ("List.len", "List.len() => Int", "Number of items.", Some("items.len()")),
    ("List.is_empty", "List.is_empty() => Bool", "True when there are no items.", None),
    ("List.push", "List.push(item: T)", "Appends an item to the end.", None),
    ("List.pop", "List.pop() => T?", "Removes and returns the last item, if any.", None),
    ("List.get", "List.get(i: Int) => T?", "The item at index i, if in bounds.", None),
    ("List.first", "List.first() => T?", "The first item, if any.", None),
    ("List.last", "List.last() => T?", "The last item, if any.", None),
    ("List.contains", "List.contains(item: T) => Bool", "True when item appears in the list.", None),
    ("List.index_of", "List.index_of(item: T) => Int?", "Index of the first matching item, if any.", None),
    ("List.join", "List.join(sep: String) => String", "Joins string items with sep.", None),
    ("List.sum", "List.sum() => T", "Sum of all items.", None),
    ("List.product", "List.product() => T", "Product of all items.", None),
    ("List.min", "List.min() => T?", "The smallest item, if any.", None),
    ("List.max", "List.max() => T?", "The largest item, if any.", None),
    ("List.map", "List.map(f: fn(T) => R) => [R]", "Transforms each item with f.", None),
    ("List.filter", "List.filter(f: fn(T) => Bool) => List<T>", "Keeps items where f(item) is true.", Some("items.filter(fn (item: T) => Bool { return true })")),
    ("List.filter_map", "List.filter_map(f: fn(T) => V?) => [V]", "Maps then drops failures — keeps only successes.", None),
    ("List.each", "List.each(f: fn(T))", "Runs f once per item, for its side effects.", None),
    ("List.find", "List.find(f: fn(T) => Bool) => T?", "The first item where f(item) is true, if any.", None),
    ("List.any", "List.any(f: fn(T) => Bool) => Bool", "True if f is true for at least one item.", None),
    ("List.all", "List.all(f: fn(T) => Bool) => Bool", "True if f is true for every item.", None),
    ("List.sort_by", "List.sort_by(key: fn(T) => K)", "Sorts in place by the key f extracts.", None),
    ("List.reduce", "List.reduce(init: R, f: fn(R, T) => R) => R", "Folds items into one value, starting from init.", None),
    ("List.fold", "List.fold(init: R, f: fn(R, T) => R) => R", "Folds items into one value, starting from init.", None),
    ("List.reverse", "List.reverse()", "Reverses the list in place.", None),
    ("List.sort", "List.sort()", "Sorts the list in place.", None),
    ("List.clear", "List.clear()", "Removes every item.", None),
    ("List.insert", "List.insert(i: Int, item: T)", "Inserts item at index i.", None),
    ("List.remove", "List.remove(value: T, by: RemoveBy = .Val) => T?", "Removes the first equal value; `.Slot` selects positional removal.", None),
    ("List.count", "List.count(value: T) => Int", "Counts items equal to value.", None),
    ("List.extend", "List.extend(other: [T])", "Appends every item from other in order.", None),
    ("List.concat", "List.concat(other: [T]) => [T]", "Returns this list followed by other.", None),
    ("List.enumerate", "List.enumerate() => [(idx: Int, item: T)]", "Pairs each item with its index.", None),
    ("List.zip", "List.zip(other: [U]) => [(a: T, b: U)]", "Pairs items from two lists positionally.", None),
    ("Map.len", "Map.len() -> Int", "Number of entries.", None),
    ("Map.is_empty", "Map.is_empty() -> Bool", "True when there are no entries.", None),
    ("Map.get", "Map.get(key: K) -> V?", "Value for key, if present.", None),
    ("Map.insert", "Map.insert(key: K, value: V)", "Inserts or overwrites the value for key.", None),
    ("Map.remove", "Map.remove(key: K) -> V?", "Removes and returns the value for key, if present.", None),
    ("Map.contains_key", "Map.contains_key(key: K) -> Bool", "True when key has an entry.", None),
    ("Map.keys", "Map.keys() -> Iter<K>", "Lazily yields every key in map order.", None),
    ("Map.values", "Map.values() -> Iter<V>", "Lazily yields every value in map order.", None),
    ("Map.each", "Map.each(f: fn(K, V))", "Runs f once per entry.", None),
    ("String.len", "String.len() -> Int", "Number of characters.", None),
    ("String.is_empty", "String.is_empty() -> Bool", "True when the string is empty.", None),
    ("String.contains", "String.contains(s: String) -> Bool", "True when s appears in the string.", None),
    ("String.starts_with", "String.starts_with(s: String) -> Bool", "True when the string starts with s.", None),
    ("String.ends_with", "String.ends_with(s: String) -> Bool", "True when the string ends with s.", None),
    ("String.trim", "String.trim() -> String", "Removes leading/trailing whitespace.", None),
    ("String.to_upper", "String.to_upper() -> String", "Uppercased copy.", None),
    ("String.to_lower", "String.to_lower() -> String", "Lowercased copy.", None),
    ("String.split", "String.split(sep: String) -> [String]", "Splits on every occurrence of sep.", None),
    ("String.lines", "String.lines() -> [String]", "Splits into lines.", None),
    ("String.chars", "String.chars() -> [Char]", "Every character, in order.", None),
    ("String.replace", "String.replace(from: String, to: String) -> String", "Replaces every occurrence of from with to.", None),
    ("String.repeat", "String.repeat(n: Int) -> String", "Concatenates n copies of the string.", None),
];
