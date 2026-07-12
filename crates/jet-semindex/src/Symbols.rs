//! Consumer-neutral semantic symbol facts for docs, completion, hover, and help.
//!
//! Program symbols come from `SymbolDB`; language builtins live here once.
//! Consumers choose presentation only — never rebuild signatures or identity.

use std::collections::HashMap;

use jet_foundation::Syntax;
use jet_foundation::AST::ProgramBundle;

use crate::Build::{SymKind, SymbolDB};
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

    pub fn complete(&self, prefix: &str, owner: Option<&str>) -> Vec<&SemanticSymbol> {
        self.symbols
            .iter()
            .filter(|symbol| symbol.owner.as_deref() == owner && symbol.name.starts_with(prefix))
            .collect()
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

pub fn build_semantic_symbol_index(db: &SymbolDB, bundle: &ProgramBundle) -> SemanticSymbolIndex {
    let mut symbols = language_symbols();
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
        let qualified_name = owner
            .as_ref()
            .map(|owner| format!("{owner}.{}", def.name))
            .unwrap_or_else(|| def.name.clone());
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
        symbols.push(SemanticSymbol {
            identity: def.identity.clone(),
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
        });
    }
    for module in &bundle.modules {
        for import in &module.imports {
            match &import.kind {
                jet_foundation::AST::ImportKind::Unqualified {
                    module_alias,
                    items,
                    items_span,
                    ..
                } => {
                    let source_import = module
                        .imports
                        .iter()
                        .find(|candidate| {
                            !matches!(candidate.kind, jet_foundation::AST::ImportKind::Unqualified { .. })
                                && candidate.import_alias() == *module_alias
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
                    for (original, alias) in items {
                        let local_name = alias.as_deref().unwrap_or(original);
                        let target = imported_module.and_then(|imported| {
                            symbols.iter().find(|symbol| {
                                symbol.module_path == imported.display && symbol.name == *original
                            })
                        });
                        let mut symbol = target.cloned().unwrap_or_else(|| SemanticSymbol {
                            identity: String::new(),
                            name: local_name.to_string(),
                            qualified_name: local_name.to_string(),
                            owner: None,
                            module_path: module.display.clone(),
                            kind: SemanticSymbolKind::Local,
                            signature: local_name.to_string(),
                            summary: format!("Imported from {module_alias}."),
                            examples: Vec::new(),
                            provenance: SemanticProvenance::Source {
                                module_path: module.display.clone(),
                            },
                            span: Some((*items_span).into()),
                        });
                        symbol.identity = format!(
                            "import:{}::{module_alias}.{original}::{local_name}",
                            module.display
                        );
                        symbol.name = local_name.to_string();
                        symbol.qualified_name = local_name.to_string();
                        symbol.owner = None;
                        symbol.module_path = module.display.clone();
                        symbol.provenance = SemanticProvenance::Source {
                            module_path: module.display.clone(),
                        };
                        symbol.span = Some((*items_span).into());
                        symbols.push(symbol);
                    }
                }
                jet_foundation::AST::ImportKind::File(_, _)
                | jet_foundation::AST::ImportKind::Module(_, _) => {
                    let alias = import.import_alias();
                    symbols.push(SemanticSymbol {
                        identity: format!("import:{}::{alias}", module.display),
                        name: alias.clone(),
                        qualified_name: alias.clone(),
                        owner: None,
                        module_path: module.display.clone(),
                        kind: SemanticSymbolKind::Module,
                        signature: format!("use {alias}"),
                        summary: "Imported module.".to_string(),
                        examples: Vec::new(),
                        provenance: SemanticProvenance::Source {
                            module_path: module.display.clone(),
                        },
                        span: Some(import.alias_span.into()),
                    });
                }
            }
        }
    }
    SemanticSymbolIndex::new(symbols)
}

fn semantic_shape(
    name: &str,
    kind: &SymKind,
    owner: Option<&str>,
) -> (SemanticSymbolKind, String) {
    match kind {
        SymKind::Module => (SemanticSymbolKind::Module, format!("module {name}")),
        SymKind::Function { params, ret } => {
            let params = params
                .iter()
                .map(|(name, ty)| format!("{name}: {}", ty.name()))
                .collect::<Vec<_>>()
                .join(", ");
            let prefix = owner.map_or_else(|| format!("fn {name}"), |owner| format!("{owner}.{name}"));
            let result = ret
                .as_ref()
                .map(|ty| format!(" -> {}", ty.name()))
                .unwrap_or_default();
            (SemanticSymbolKind::Function, format!("{prefix}({params}){result}"))
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
        });
    }
    symbols
}

const BUILTIN_METHODS: &[(&str, &str, &str, Option<&str>)] = &[
    ("List.len", "List.len() -> Int", "Number of items.", Some("items.len()")),
    ("List.is_empty", "List.is_empty() -> Bool", "True when there are no items.", None),
    ("List.push", "List.push(item: T)", "Appends an item to the end.", None),
    ("List.pop", "List.pop() -> T?", "Removes and returns the last item, if any.", None),
    ("List.get", "List.get(i: Int) -> T?", "The item at index i, if in bounds.", None),
    ("List.first", "List.first() -> T?", "The first item, if any.", None),
    ("List.last", "List.last() -> T?", "The last item, if any.", None),
    ("List.contains", "List.contains(item: T) -> Bool", "True when item appears in the list.", None),
    ("List.index_of", "List.index_of(item: T) -> Int?", "Index of the first matching item, if any.", None),
    ("List.join", "List.join(sep: String) -> String", "Joins string items with sep.", None),
    ("List.sum", "List.sum() -> T", "Sum of all items.", None),
    ("List.product", "List.product() -> T", "Product of all items.", None),
    ("List.min", "List.min() -> T?", "The smallest item, if any.", None),
    ("List.max", "List.max() -> T?", "The largest item, if any.", None),
    ("List.map", "List.map(f: fn(T) -> R) -> [R]", "Transforms each item with f.", None),
    ("List.filter", "List.filter(f: fn(T) -> Bool) -> List<T>", "Keeps items where f(item) is true.", Some("items.filter(fn (item: T) -> Bool { return true })")),
    ("List.filter_map", "List.filter_map(f: fn(T) -> V?) -> [V]", "Maps then drops failures — keeps only successes.", None),
    ("List.each", "List.each(f: fn(T))", "Runs f once per item, for its side effects.", None),
    ("List.find", "List.find(f: fn(T) -> Bool) -> T?", "The first item where f(item) is true, if any.", None),
    ("List.any", "List.any(f: fn(T) -> Bool) -> Bool", "True if f is true for at least one item.", None),
    ("List.all", "List.all(f: fn(T) -> Bool) -> Bool", "True if f is true for every item.", None),
    ("List.sort_by", "List.sort_by(key: fn(T) -> K)", "Sorts in place by the key f extracts.", None),
    ("List.reduce", "List.reduce(init: R, f: fn(R, T) -> R) -> R", "Folds items into one value, starting from init.", None),
    ("List.fold", "List.fold(init: R, f: fn(R, T) -> R) -> R", "Folds items into one value, starting from init.", None),
    ("List.reverse", "List.reverse()", "Reverses the list in place.", None),
    ("List.sort", "List.sort()", "Sorts the list in place.", None),
    ("List.clear", "List.clear()", "Removes every item.", None),
    ("List.insert", "List.insert(i: Int, item: T)", "Inserts item at index i.", None),
    ("List.remove", "List.remove(i: Int) -> T?", "Removes and returns the item at index i.", None),
    ("List.enumerate", "List.enumerate() -> [(idx: Int, item: T)]", "Pairs each item with its index.", None),
    ("List.zip", "List.zip(other: [U]) -> [(a: T, b: U)]", "Pairs items from two lists positionally.", None),
    ("Map.len", "Map.len() -> Int", "Number of entries.", None),
    ("Map.is_empty", "Map.is_empty() -> Bool", "True when there are no entries.", None),
    ("Map.get", "Map.get(key: K) -> V?", "Value for key, if present.", None),
    ("Map.insert", "Map.insert(key: K, value: V)", "Inserts or overwrites the value for key.", None),
    ("Map.remove", "Map.remove(key: K) -> V?", "Removes and returns the value for key, if present.", None),
    ("Map.contains_key", "Map.contains_key(key: K) -> Bool", "True when key has an entry.", None),
    ("Map.keys", "Map.keys() -> [K]", "Every key, in map order.", None),
    ("Map.values", "Map.values() -> [V]", "Every value, in map order.", None),
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
    ("String.to_int", "String.to_int() -> Int ? ParseError", "Parses the string as an Int.", None),
];
