//! One semantic name ledger and one Rust-name projection.

use crate::Diagnostics::Span;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Stable package scope used by every module identity.
///
/// A package rooted inside the loaded project uses its project-relative
/// directory as scope. This keeps semantic identities stable when the same
/// checkout moves, while distinct nested packages remain disjoint. A package
/// outside the project root retains its normalized parent path because that
/// path is the only available identity at this layer.
pub fn package_scope_for(path: &Path, project_root: &Path) -> String {
    let norm_path = normalize_path(path);
    let norm_root = normalize_path(project_root);
    let scope = if norm_path.starts_with(&norm_root) {
        norm_path
            .strip_prefix(&norm_root)
            .ok()
            .and_then(|relative| relative.parent())
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        norm_path.parent().map(normalize_path).unwrap_or(norm_path)
    };
    scope.to_string_lossy().into_owned()
}

/// Lexically fold `.` and `..` components without touching the filesystem.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn name_leaf(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(_, leaf)| leaf)
}

/// Visibility recorded for a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameVisibility {
    Private,
    Package,
    Public,
}

impl NameVisibility {
    pub fn from_flags(is_pub: bool, is_package_pub: bool) -> Self {
        if is_pub && !is_package_pub {
            Self::Public
        } else if is_package_pub {
            Self::Package
        } else {
            Self::Private
        }
    }

    pub fn is_exported(self) -> bool {
        !matches!(self, Self::Private)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameModule {
    pub alias: String,
    pub path: String,
    pub package: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameDeclaration {
    pub module: usize,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub span: Span,
    pub visibility: NameVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameAlias {
    pub module: usize,
    pub name: String,
    pub target: String,
    pub target_module: Option<usize>,
    pub span: Span,
    pub visibility: NameVisibility,
}

/// D-STRUCT-PLANE1=A: the three structure subjects share one fact shape. The
/// semantic owner supplies the status/detail; the gate is optional because a
/// fact that tightens silently has no ledger entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub enum StructureFactKind {
    Liveness,
    Lifecycle,
    ImportEdge,
}

impl StructureFactKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Liveness => "liveness",
            Self::Lifecycle => "lifecycle",
            Self::ImportEdge => "import-edge",
        }
    }
}

/// One checked structure observation. This is compiler data only; it is never
/// an AST value and has no codegen projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureFact {
    pub kind: StructureFactKind,
    pub subject: String,
    pub source: String,
    pub span: Span,
    pub status: String,
    pub detail: String,
    pub gate: Option<String>,
}

impl StructureFact {
    pub fn new(
        kind: StructureFactKind,
        subject: impl Into<String>,
        source: impl Into<String>,
        span: Span,
        status: impl Into<String>,
        detail: impl Into<String>,
        gate: Option<String>,
    ) -> Self {
        Self {
            kind,
            subject: subject.into(),
            source: source.into(),
            span,
            status: status.into(),
            detail: detail.into(),
            gate,
        }
    }
}

/// A checked source reference and its resolved declaration origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameReference {
    pub module_path: String,
    pub kind: String,
    pub def_span: Span,
    pub semantic_identity: Option<String>,
}

/// Shared name facts. Loader seeds module/import identities and checked
/// structure observations; sema adds declarations, aliases, visibility,
/// paths, and checked reference origins.
///
/// The lookup tables are shared copy-on-write: registration fills them, and
/// every function body then reads them through its own `body_snapshot`.
/// Sharing makes that snapshot O(1); copying the tables per body made body
/// checking quadratic in the size of a unit.
#[derive(Debug, Clone, Default)]
pub struct NameLedger {
    tables: Arc<NameTables>,
    /// Import uses keyed by the defining alias span. Source spellings and
    /// target module text must not decide liveness.
    alias_uses: HashSet<(usize, Span)>,
    references: HashMap<(String, usize, usize), NameReference>,
    reference_sites: HashMap<String, Vec<(String, usize, usize)>>,
    structure_facts: Vec<StructureFact>,
}

/// The lookup half of [`NameLedger`], written during loading and
/// registration and read by body checks.
#[derive(Debug, Clone, Default)]
struct NameTables {
    imports: HashMap<(usize, Span), usize>,
    modules: HashMap<usize, NameModule>,
    declarations: HashMap<(usize, String), NameDeclaration>,
    module_declarations: HashMap<usize, Vec<(usize, String)>>,
    declaration_names: HashMap<String, Vec<(usize, String)>>,
    /// Source-facing paths for compiler-owned declarations. Generated
    /// generic-instance names remain semantic keys, but diagnostics and
    /// tooling project them back to the instance member path.
    display_paths: HashMap<(usize, String), String>,
    aliases: HashMap<(usize, String), NameAlias>,
    module_aliases: HashMap<usize, Vec<(usize, String)>>,
    alias_names: HashMap<String, Vec<(usize, String)>>,
    /// Loader-owned roots, such as package manifest Output references, must
    /// survive sema's declaration/reference refresh.
    loader_alias_uses: HashSet<(usize, Span)>,
    /// D-MOD-CYCLE1=A: the package namespace each package member file belongs
    /// to (its package root). Files of one package see each other's
    /// top-level names without imports. Loose files have no entry.
    namespaces: HashMap<usize, String>,
    namespace_members: HashMap<String, Vec<usize>>,
}

impl NameLedger {
    fn tables_mut(&mut self) -> &mut NameTables {
        Arc::make_mut(&mut self.tables)
    }

    pub fn with_imports(imports: HashMap<(usize, Span), usize>) -> Self {
        Self {
            tables: Arc::new(NameTables {
                imports,
                ..NameTables::default()
            }),
            ..Self::default()
        }
    }

    pub fn import_target(&self, module: usize, span: Span) -> Option<usize> {
        self.tables.imports.get(&(module, span)).copied()
    }

    pub fn record_import_target(&mut self, module: usize, span: Span, target: usize) {
        self.tables_mut().imports.insert((module, span), target);
    }

    pub fn set_module(&mut self, module: usize, alias: String, path: String, package: String) {
        self.tables_mut().modules.insert(
            module,
            NameModule {
                alias,
                path,
                package,
            },
        );
    }

    pub fn module(&self, module: usize) -> Option<&NameModule> {
        self.tables.modules.get(&module)
    }

    /// D-MOD-CYCLE1=A: record that `module` is a member file of the package
    /// whose root is `package_root`. The loader writes this before sema.
    pub fn set_module_namespace(&mut self, module: usize, package_root: String) {
        let tables = self.tables_mut();
        if let Some(previous) = tables.namespaces.insert(module, package_root.clone()) {
            if let Some(members) = tables.namespace_members.get_mut(&previous) {
                members.retain(|&member| member != module);
            }
        }
        let members = tables.namespace_members.entry(package_root).or_default();
        let position = members.binary_search(&module).unwrap_or_else(|position| position);
        members.insert(position, module);
    }

    /// D-MOD-CYCLE1=A: true when `from` and `to` are the same file or member
    /// files of one package, so `from` sees every top-level name of `to`
    /// (private ones included) without an import.
    pub fn same_namespace(&self, from: usize, to: usize) -> bool {
        from == to
            || self
                .tables
                .namespaces
                .get(&from)
                .is_some_and(|root| self.tables.namespaces.get(&to) == Some(root))
    }

    /// D-MOD-CYCLE1=A: every other member file of `module`'s package, in
    /// module order. Empty for a loose file.
    pub fn namespace_siblings(&self, module: usize) -> impl Iterator<Item = usize> + '_ {
        self.tables.namespaces.get(&module)
            .and_then(|root| self.tables.namespace_members.get(root))
            .into_iter().flatten().copied().filter(move |&other| other != module)
    }

    /// D-MOD-CYCLE1=A: the package root `module` is a member file of, as the
    /// loader recorded it. `None` for a loose file, a one-file package, Core,
    /// and synthetic modules.
    pub fn module_namespace(&self, module: usize) -> Option<&str> {
        self.tables.namespaces.get(&module).map(String::as_str)
    }

    /// Every loader-resolved file import as `(importing module, target
    /// module)`, sorted and deduplicated.
    pub fn import_edges(&self) -> Vec<(usize, usize)> {
        let mut edges = self
            .tables
            .imports
            .iter()
            .map(|((module, _), target)| (*module, *target))
            .collect::<Vec<_>>();
        edges.sort_unstable();
        edges.dedup();
        edges
    }

    pub fn module_path(&self, module: usize) -> Option<&str> {
        self.module(module).map(|module| module.path.as_str())
    }

    pub fn module_alias(&self, module: usize) -> Option<&str> {
        self.module(module).map(|module| module.alias.as_str())
    }

    /// One stable namespace identity for a loaded source module.
    ///
    /// The loader alias is a Rust-name projection and can be renamed or
    /// disambiguated when two packages contain the same leaf.  It is not a
    /// nominal identity.  Package scope plus the stable source path is the
    /// semantic namespace instead.
    pub fn module_identity(&self, module: usize) -> Option<String> {
        self.module(module)
            .map(|module| format!("{}::{}", module.package, module.path))
    }

    pub fn declare(
        &mut self,
        module: usize,
        name: String,
        path: String,
        kind: String,
        span: Span,
        visibility: NameVisibility,
    ) {
        let key = (module, name.clone());
        let tables = self.tables_mut();
        let fresh = !tables.declarations.contains_key(&key);
        tables.declarations.insert(
            key,
            NameDeclaration {
                module,
                name: name.clone(),
                path,
                kind,
                span,
                visibility,
            },
        );
        if fresh {
            tables.module_declarations.entry(module).or_default().push((module, name.clone()));
            tables
                .declaration_names
                .entry(name_leaf(&name).to_string())
                .or_default()
                .push((module, name));
        }
    }

    pub fn declaration(&self, module: usize, name: &str) -> Option<&NameDeclaration> {
        self.tables.declarations.get(&(module, name.to_string()))
    }

    pub fn declarations(&self) -> impl Iterator<Item = &NameDeclaration> {
        self.tables.declarations.values()
    }

    pub fn module_declarations(&self, module: usize) -> impl Iterator<Item = &NameDeclaration> {
        self.tables.module_declarations.get(&module).into_iter().flatten()
            .filter_map(|key| self.tables.declarations.get(key))
    }

    pub fn declaration_path(&self, module: usize, name: &str) -> Option<&str> {
        self.declaration(module, name)
            .map(|declaration| declaration.path.as_str())
    }

    fn display_declaration_path(&self, module: usize, name: &str) -> Option<String> {
        let declaration = self.declaration(module, name)?;
        self.tables
            .display_paths
            .get(&(module, declaration.path.clone()))
            .cloned()
            .or_else(|| Some(declaration.path.clone()))
    }

    /// Record the source-facing path for one compiler-owned declaration path.
    /// The semantic declaration key stays unchanged for registration and
    /// codegen; only presentation consumers use this projection.
    pub fn record_display_path(
        &mut self,
        module: usize,
        internal_path: impl Into<String>,
        display_path: impl Into<String>,
    ) {
        self.tables_mut()
            .display_paths
            .insert((module, internal_path.into()), display_path.into());
    }

    /// Resolve the one declaration that owns a source span.
    ///
    /// A span identifies a declaration only while it belongs to one. Every
    /// compiler-synthesized member — the auto-derived `encode`/`decode`/
    /// `compare` bodies, and each parameter and local inside them — reuses
    /// its type's declaration span, so one span in one module can carry
    /// several ledger declarations. `key` is the ledger key of the symbol
    /// being projected: the declaration recorded under exactly that key owns
    /// the span; a lone candidate is the inline-module bridge this lookup
    /// exists for; several candidates that are none of them are not evidence
    /// at all. Picking one by iteration would hand presentation output a
    /// different answer in every process.
    fn declaration_at(
        &self,
        module: usize,
        start: usize,
        end: usize,
        key: Option<&str>,
    ) -> Option<&NameDeclaration> {
        if let Some(declaration) = key.and_then(|key| self.declaration(module, key)) {
            if declaration.span.start == start && declaration.span.end == end {
                return Some(declaration);
            }
        }
        let mut only = None;
        for declaration in self.module_declarations(module) {
            if declaration.module != module
                || declaration.span.start != start
                || declaration.span.end != end
            {
                continue;
            }
            if only.is_some() {
                return None;
            }
            only = Some(declaration);
        }
        only
    }

    /// Canonical identity for a nominal declared in one loaded module.
    ///
    /// Every semantic nominal crossing a module boundary uses this form.  It
    /// contains package scope and source path, so equal leaf names in sibling
    /// modules or different packages cannot compare equal merely because a
    /// loader alias happens to match.
    pub fn nominal_identity(&self, module: usize, name: &str) -> Option<String> {
        self.module_identity(module)
            .map(|module| format!("{module}::{name}"))
    }

    /// Return the owner module for a canonical nominal identity.
    pub fn nominal_module(&self, identity: &str) -> Option<usize> {
        let (namespace, _) = identity.rsplit_once("::")?;
        self.tables.modules.iter().find_map(|(module, facts)| {
            (format!("{}::{}", facts.package, facts.path) == namespace).then_some(*module)
        })
    }

    /// Resolve a declaration without reconstructing its semantic key. Source
    /// indexes already carry the declaration span, which is the bridge for
    /// inline-module names stored under generated keys. A span shared by
    /// several declarations names none of them, so it resolves to nothing.
    pub fn canonical_path_at(&self, module: usize, start: usize, end: usize) -> Option<String> {
        self.declaration_at(module, start, end, None)
            .map(|declaration| declaration.path.clone())
    }

    /// Resolve one semantic name to the canonical path recorded by sema.
    /// User-facing diagnostics and tooling use `display_path`, which applies
    /// compiler-owned presentation projections without changing this key.
    pub fn canonical_path(&self, module: usize, name: &str) -> Option<String> {
        if let Some(path) = self.declaration_path(module, name) {
            return Some(path.to_string());
        }
        let alias = self.effective_alias(module, name)?;
        if let Some(target_module) = alias.target_module {
            let target_name = alias
                .target
                .rsplit_once('.')
                .map(|(_, leaf)| leaf)
                .unwrap_or(alias.target.as_str());
            if let Some(path) = self.declaration_path(target_module, target_name) {
                return Some(path.to_string());
            }
        }
        alias.target.contains('.').then(|| alias.target.clone())
    }

    /// Pick the one user-facing spelling for a resolved name. A leaf is safe
    /// only when the visible declarations with that leaf collapse to one
    /// canonical path; otherwise the resolved declaration keeps its full
    /// path. The caller supplies the resolved module so an ambiguous lookup
    /// never guesses from HashMap iteration order.
    pub fn display_path(
        &self,
        from_module: usize,
        name: &str,
        resolved_module: Option<usize>,
    ) -> Option<String> {
        let leaf = name.rsplit_once('.').map_or(name, |(_, leaf)| leaf);
        let mut paths = BTreeSet::new();
        if let Some(declarations) = self.tables.declaration_names.get(leaf) {
            for (module, name) in declarations {
                if self.visible(from_module, *module, leaf) {
                    if let Some(path) = self.display_declaration_path(*module, name) {
                        paths.insert(path);
                    }
                }
            }
        }
        if let Some(aliases) = self.tables.alias_names.get(leaf) {
            for (module, name) in aliases {
                let Some(alias) = self.tables.aliases.get(&(*module, name.clone())) else {
                    continue;
                };
                if !self.visible(from_module, *module, leaf) {
                    continue;
                }
                let path = alias
                    .target_module
                    .and_then(|module| {
                        let target_leaf = alias
                            .target
                            .rsplit_once('.')
                            .map_or(alias.target.as_str(), |(_, leaf)| leaf);
                        self.display_declaration_path(module, target_leaf)
                    })
                    .or_else(|| alias.target.contains('.').then(|| alias.target.clone()));
                if let Some(path) = path {
                    paths.insert(path);
                }
            }
        }

        let resolved_path = resolved_module
            .and_then(|module| self.display_declaration_path(module, leaf))
            .or_else(|| self.display_declaration_path(from_module, leaf))
            .or_else(|| self.canonical_path(from_module, name));
        let resolved_path = resolved_path.or_else(|| paths.iter().next().cloned())?;
        let has_display_projection = self
            .tables
            .declaration_names
            .get(leaf)
            .into_iter()
            .flatten()
            .any(|(module, name)| {
                self.visible(from_module, *module, leaf)
                    && self.declaration(*module, name).is_some_and(|declaration| {
                        self.tables
                            .display_paths
                            .contains_key(&(*module, declaration.path.clone()))
                    })
            });
        if paths.len() <= 1
            && !name.contains('.')
            && !name.starts_with(crate::Syntax::GENERATED_NAME_PREFIX)
            && !has_display_projection
        {
            Some(leaf.to_string())
        } else {
            Some(resolved_path)
        }
    }

    /// Project one indexed definition through the same unique/ambiguous rule
    /// as diagnostics. Inline-module declarations use generated ledger keys,
    /// so their recorded canonical path is the only source-facing fallback.
    /// Members first project their owner, then append the member leaf.
    ///
    /// The span is evidence, not a precondition: a definition whose span
    /// names no declaration of its own — a parameter or local, or anything
    /// sharing a compiler-synthesized member's span — still gets the ordinary
    /// name projection instead of nothing.
    pub fn display_path_at(
        &self,
        from_module: usize,
        start: usize,
        end: usize,
        name: &str,
        owner: Option<&str>,
        resolved_module: Option<usize>,
    ) -> Option<String> {
        let member = owner.map(|owner| format!("{owner}.{name}"));
        let key = member.as_deref().unwrap_or(name);
        let declaration = self.declaration_at(from_module, start, end, Some(key));
        let canonical = declaration.map(|declaration| {
            self.display_declaration_path(from_module, &declaration.name)
                .unwrap_or_else(|| declaration.path.clone())
        });
        if let Some(owner) = owner {
            return self
                .display_path(from_module, owner, resolved_module)
                .map(|owner| format!("{owner}.{name}"))
                .or(canonical);
        }
        if declaration.is_some_and(|declaration| {
            declaration
                .name
                .starts_with(crate::Syntax::GENERATED_NAME_PREFIX)
        }) {
            return canonical;
        }
        self.display_path(from_module, name, resolved_module)
            .or(canonical)
    }

    /// Return every source-name key in one module with its canonical path.
    /// Nominal declarations also publish their semantic identity as a key.
    /// Codegen uses that second key only to project canonical semantic types
    /// back to their source-facing path; the identity itself remains the
    /// package/path-qualified key used for uniqueness.
    pub fn canonical_paths(&self, module: usize) -> Vec<(String, String)> {
        let mut paths = BTreeSet::new();
        for declaration in self.module_declarations(module) {
            let name = &declaration.name;
            let path = declaration.path.clone();
            paths.insert((name.clone(), path.clone()));
            if declaration.kind == "type" {
                if let Some(identity) = self.nominal_identity(module, name) {
                    paths.insert((identity, path.clone()));
                }
            }
            // Qualified source names and nominal identities are also keys.
            if path.as_str() != name.as_str() {
                paths.insert((path.clone(), path));
            }
        }
        for alias in self.module_aliases(module) {
            if let Some(path) = self.canonical_path(module, &alias.name) {
                paths.insert((alias.name.clone(), path));
            }
            // A module import is also a source qualifier (`dep.Thing`).
            if let Some(target_module) = alias.target_module {
                if !alias.target.contains('.') {
                    if let Some(target_alias) = self.module_alias(target_module) {
                        let prefix = format!("{target_alias}.");
                        for declaration in self.module_declarations(target_module) {
                            if let Some(suffix) = declaration.path.strip_prefix(prefix.as_str()) {
                                paths.insert((format!("{}.{}", alias.name, suffix), declaration.path.clone()));
                            }
                        }
                    }
                }
            }
        }
        paths.into_iter().collect()
    }

    /// Lookup one key of `canonical_paths` without enumerating the package's
    /// import projections. Comptime needs only keys actually used by types.
    pub fn canonical_source_path(&self, module: usize, key: &str) -> Option<String> {
        let mut path = self.canonical_path(module, key);
        for declaration in self.module_declarations(module) {
            if declaration.path == key
                || (declaration.kind == "type" && self.nominal_identity(module, &declaration.name).as_deref() == Some(key))
            {
                if path.as_ref().is_none_or(|path| path < &declaration.path) {
                    path = Some(declaration.path.clone());
                }
            }
        }
        for (position, _) in key.match_indices('.') {
            let Some(alias) = self.alias(module, &key[..position]) else { continue };
            if alias.target.contains('.') { continue }
            let Some(target) = alias.target_module else { continue };
            let Some(target_alias) = self.module_alias(target) else { continue };
            let candidate = format!("{target_alias}.{}", &key[position + 1..]);
            if self.module_declarations(target).any(|declaration| declaration.path == candidate)
                && path.as_ref().is_none_or(|path| path < &candidate)
            {
                path = Some(candidate);
            }
        }
        path
    }

    pub fn semantic_identity(&self, module: usize, name: &str) -> Option<String> {
        self.nominal_identity(module, name)
    }

    pub fn record_alias(
        &mut self,
        module: usize,
        name: String,
        target: String,
        target_module: Option<usize>,
        span: Span,
        visibility: NameVisibility,
    ) {
        let key = (module, name.clone());
        let tables = self.tables_mut();
        let fresh = !tables.aliases.contains_key(&key);
        tables.aliases.insert(
            key,
            NameAlias {
                module,
                name: name.clone(),
                target,
                target_module,
                span,
                visibility,
            },
        );
        if fresh {
            tables.module_aliases.entry(module).or_default().push((module, name.clone()));
            tables
                .alias_names
                .entry(name_leaf(&name).to_string())
                .or_default()
                .push((module, name));
        }
    }

    pub fn record_alias_use(&mut self, module: usize, span: Span) {
        self.alias_uses.insert((module, span));
    }

    pub fn record_loader_alias_use(&mut self, module: usize, span: Span) {
        let key = (module, span);
        self.tables_mut().loader_alias_uses.insert(key);
        self.alias_uses.insert(key);
    }

    pub fn alias_used(&self, module: usize, alias: &NameAlias) -> bool {
        self.alias_uses.contains(&(module, alias.span))
    }

    /// Import-alias uses recorded so far, loader-owned ones included.
    pub fn alias_uses(&self) -> &HashSet<(usize, Span)> {
        &self.alias_uses
    }

    /// True when the loader recorded this alias use before any body check.
    pub fn loader_alias_use(&self, module: usize, span: Span) -> bool {
        self.tables.loader_alias_uses.contains(&(module, span))
    }

    /// Stable identity for one import binding. The defining span is part of
    /// the identity so equal alias spellings in different scopes cannot join.
    pub fn alias_identity(&self, module: usize, span: Span) -> Option<String> {
        self.module_aliases(module)
            .any(|alias| alias.span == span)
            .then(|| {
                format!(
                    "import:{}::{}..{}",
                    self.module_path(module).unwrap_or_default(),
                    span.start,
                    span.end
                )
            })
    }

    pub fn alias(&self, module: usize, name: &str) -> Option<&NameAlias> {
        self.tables.aliases.get(&(module, name.to_string()))
    }

    pub fn aliases(&self) -> impl Iterator<Item = &NameAlias> {
        self.tables.aliases.values()
    }

    pub fn module_aliases(&self, module: usize) -> impl Iterator<Item = &NameAlias> {
        self.tables.module_aliases.get(&module).into_iter().flatten()
            .filter_map(|key| self.tables.aliases.get(key))
    }

    pub fn effective_alias(&self, module: usize, name: &str) -> Option<&NameAlias> {
        self.alias(module, name)
    }

    pub fn visible(&self, from_module: usize, target_module: usize, name: &str) -> bool {
        let visibility = self
            .declaration(target_module, name)
            .map(|declaration| declaration.visibility)
            .or_else(|| {
                self.effective_alias(target_module, name)
                    .map(|alias| alias.visibility)
            })
            .or_else(|| {
                // D-MOD-CYCLE1=A: a method of a type may be declared by an
                // `impl` in another file of the type's package.
                self.tables.declaration_names.get(name_leaf(name))
                    .into_iter().flatten()
                    .filter(|(module, candidate)| candidate == name && self.same_namespace(target_module, *module))
                    .filter_map(|(module, candidate)| self.declaration(*module, candidate))
                    .min_by_key(|declaration| declaration.module)
                    .map(|declaration| declaration.visibility)
            });
        let Some(visibility) = visibility else {
            return false;
        };
        match visibility {
            NameVisibility::Public => true,
            NameVisibility::Package => {
                self.same_namespace(from_module, target_module)
                    || self
                        .module(from_module)
                        .zip(self.module(target_module))
                        .is_some_and(|(from, target)| from.package == target.package)
            }
            // D-MOD-CYCLE1=A: files of one package share one namespace.
            NameVisibility::Private => self.same_namespace(from_module, target_module),
        }
    }

    pub fn exported(&self, module: usize, name: &str) -> bool {
        if let Some(declaration) = self.declaration(module, name) {
            declaration.visibility.is_exported()
        } else {
            self.effective_alias(module, name)
                .is_some_and(|alias| alias.visibility.is_exported())
        }
    }

    pub fn public(&self, module: usize, name: &str) -> bool {
        if let Some(declaration) = self.declaration(module, name) {
            declaration.visibility == NameVisibility::Public
        } else {
            self.effective_alias(module, name)
                .is_some_and(|alias| alias.visibility == NameVisibility::Public)
        }
    }

    pub fn record_reference(
        &mut self,
        source_module: String,
        start: usize,
        end: usize,
        reference: NameReference,
    ) {
        let site = (source_module, start, end);
        if !self.references.contains_key(&site) {
            self.reference_sites.entry(site.0.clone()).or_default().push(site.clone());
        }
        self.references.insert(site, reference);
    }

    pub fn reference(&self, module_path: &str, start: usize, end: usize) -> Option<&NameReference> {
        self.references.get(&(module_path.to_string(), start, end))
    }

    pub fn references(&self) -> &HashMap<(String, usize, usize), NameReference> {
        &self.references
    }

    pub fn module_references(&self, path: &str) -> impl Iterator<Item = (&(String, usize, usize), &NameReference)> {
        self.reference_sites.get(path).into_iter().flatten()
            .filter_map(|site| self.references.get(site).map(|reference| (site, reference)))
    }

    /// Record one structure fact in the same ledger as names and references.
    /// Repeated writers of the same observation coalesce here, so no later
    /// reader has to reconcile two structure tables.
    pub fn record_structure_fact(&mut self, fact: StructureFact) {
        if !self.structure_facts.contains(&fact) {
            self.structure_facts.push(fact);
        }
    }

    pub fn structure_facts(&self) -> &[StructureFact] {
        &self.structure_facts
    }

    pub fn merge_structure_facts(&mut self, other: &Self) {
        for fact in &other.structure_facts {
            self.record_structure_fact(fact.clone());
        }
    }

    pub fn merge_references(&mut self, other: &Self) {
        self.alias_uses.extend(other.alias_uses.iter().copied());
        for ((source, start, end), reference) in &other.references {
            self.record_reference(source.clone(), *start, *end, reference.clone());
        }
    }

    /// Share lookup facts with an incremental body check without copying
    /// prior body references into the cache entry. The tables are shared, not
    /// copied, so a snapshot per body costs O(1) instead of O(unit).
    pub fn body_snapshot(&self) -> Self {
        Self {
            tables: Arc::clone(&self.tables),
            alias_uses: self.tables.loader_alias_uses.clone(),
            references: HashMap::new(),
            reference_sites: HashMap::new(),
            structure_facts: Vec::new(),
        }
    }

    pub fn clear_sema_facts(&mut self) {
        let tables = self.tables_mut();
        tables.modules.clear();
        tables.declarations.clear();
        tables.declaration_names.clear();
        tables.module_declarations.clear();
        tables.display_paths.clear();
        tables.aliases.clear();
        tables.alias_names.clear();
        tables.module_aliases.clear();
        self.alias_uses = self.tables.loader_alias_uses.clone();
        self.references.clear();
        self.reference_sites.clear();
        // The loader owns import-edge observations and sema must not erase
        // them when it refreshes its declaration/reference facts. Liveness
        // and lifecycle rows are sema-owned and are rebuilt below.
        self.structure_facts
            .retain(|fact| fact.kind == StructureFactKind::ImportEdge);
    }
}

/// Rust identifier for one Jet name.
pub fn mangle(name: &str) -> String {
    let name = match name.strip_prefix(crate::Syntax::COMPTIME_MARK) {
        Some(rest) => format!("ct_{rest}"),
        None => name.to_string(),
    };
    crate::Syntax::generated_name(&name)
}

/// Rust identifier for a canonical Jet path or module identity.
pub fn mangle_path(path: &str) -> String {
    crate::Syntax::generated_path(path)
}

/// Mangle a compiler-generated stem into the machine-name lane.
///
/// Generated locals use a reserved dunder suffix after the single `__jet_`
/// prefix. This keeps a generated local distinct from the canonical mangle of
/// a source identifier with the same visible stem.
pub fn mangle_generated(name: &str) -> String {
    let name = name
        .strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(name);
    let name = name.strip_prefix("__").unwrap_or(name);
    crate::Syntax::generated_name(&format!("__{name}"))
}

/// Rust identifier for an inline-module member identity.
pub fn member_name(module: &str, name: &str) -> String {
    let module = module
        .strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(module);
    let decoded = crate::Syntax::decode_generated_path_suffix(module);
    let module = decoded.as_deref().unwrap_or(module);
    mangle_path(&format!("{module}.{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Span {
        Span::new(3, 7)
    }

    #[test]
    fn ledger_projects_paths_and_visibility() {
        let mut ledger = NameLedger::default();
        ledger.set_module(
            0,
            "app".to_string(),
            "app.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.set_module(
            1,
            "lib".to_string(),
            "lib.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.set_module(
            2,
            "dep".to_string(),
            "dep.jet".to_string(),
            "dep".to_string(),
        );
        ledger.declare(
            1,
            "Thing".to_string(),
            "lib.Thing".to_string(),
            "type".to_string(),
            span(),
            NameVisibility::Package,
        );
        assert_eq!(ledger.declaration_path(1, "Thing"), Some("lib.Thing"));
        assert!(ledger.visible(0, 1, "Thing"));
        assert!(!ledger.visible(2, 1, "Thing"));
    }

    #[test]
    fn ledger_projects_alias_visibility() {
        let mut ledger = NameLedger::default();
        ledger.set_module(
            0,
            "app".to_string(),
            "app.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.set_module(
            1,
            "lib".to_string(),
            "lib.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.record_alias(
            0,
            "Thing".to_string(),
            "lib.Thing".to_string(),
            Some(1),
            span(),
            NameVisibility::Public,
        );
        assert!(ledger.exported(0, "Thing"));
        assert!(ledger.public(0, "Thing"));
        assert_eq!(
            ledger.alias(0, "Thing").map(|alias| alias.target.as_str()),
            Some("lib.Thing")
        );

        // Declaration visibility governs the public surface; the alias keeps
        // the import target available to consumers.
        ledger.declare(
            0,
            "Thing".to_string(),
            "app.Thing".to_string(),
            "type".to_string(),
            span(),
            NameVisibility::Private,
        );
        assert!(!ledger.exported(0, "Thing"));
        assert!(ledger.effective_alias(0, "Thing").is_some());
    }

    #[test]
    fn file_module_declaration_keeps_visibility_and_alias_target() {
        let mut ledger = NameLedger::default();
        ledger.set_module(
            0,
            "app".to_string(),
            "app.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.set_module(
            1,
            "lib".to_string(),
            "lib.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.set_module(
            2,
            "dep".to_string(),
            "dep.jet".to_string(),
            "dep".to_string(),
        );

        ledger.declare(
            1,
            "helper".to_string(),
            "lib.helper".to_string(),
            "file_module".to_string(),
            span(),
            NameVisibility::Package,
        );
        assert!(ledger.exported(1, "helper"));
        assert!(!ledger.public(1, "helper"));
        assert!(ledger.visible(0, 1, "helper"));
        assert!(!ledger.visible(2, 1, "helper"));

        ledger.record_alias(
            1,
            "helper".to_string(),
            "helper".to_string(),
            Some(2),
            span(),
            NameVisibility::Package,
        );
        let alias = ledger.alias(1, "helper").expect("synthetic file alias");
        assert_eq!(alias.target_module, Some(2));
        assert!(ledger.effective_alias(1, "helper").is_some());
    }

    #[test]
    fn alias_record_replaces_target_even_when_visibility_weakens() {
        let mut ledger = NameLedger::default();
        ledger.record_alias(
            0,
            "helper".to_string(),
            "first".to_string(),
            Some(1),
            span(),
            NameVisibility::Public,
        );
        ledger.record_alias(
            0,
            "helper".to_string(),
            "second".to_string(),
            Some(2),
            span(),
            NameVisibility::Private,
        );
        let alias = ledger.alias(0, "helper").expect("latest alias");
        assert_eq!(alias.target, "second");
        assert_eq!(alias.target_module, Some(2));
        assert_eq!(alias.visibility, NameVisibility::Private);
    }

    #[test]
    fn rust_names_use_one_projection() {
        assert_eq!(mangle("run"), "__jet_run");
        assert_eq!(mangle("$value"), "__jet_ct_value");
        assert_eq!(mangle_path("Fire.Burn"), "__jet_Fire_dBurn");
        assert_eq!(member_name("math", "double"), "__jet_math_ddouble");
        assert_eq!(
            member_name(&member_name("outer", "inner"), "helper"),
            "__jet_outer_dinner_dhelper"
        );
    }

    #[test]
    fn generated_names_use_a_collision_safe_lane() {
        assert_eq!(mangle_generated("value"), "__jet___value");
        assert_eq!(mangle_generated("__jet_value"), "__jet___value");
        assert_eq!(mangle_generated("__jet___value"), "__jet___value");
    }

    #[test]
    fn canonical_path_follows_declarations_and_aliases() {
        let mut ledger = NameLedger::default();
        ledger.set_module(
            0,
            "app".to_string(),
            "app.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.set_module(
            1,
            "lib".to_string(),
            "lib.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.declare(
            1,
            "Thing".to_string(),
            "lib.Thing".to_string(),
            "type".to_string(),
            span(),
            NameVisibility::Public,
        );
        ledger.record_alias(
            0,
            "Alias".to_string(),
            "lib.Thing".to_string(),
            Some(1),
            span(),
            NameVisibility::Public,
        );
        assert_eq!(
            ledger.canonical_path(1, "Thing"),
            Some("lib.Thing".to_string())
        );
        assert!(ledger
            .canonical_paths(1)
            .contains(&(
                "pkg::lib.jet::Thing".to_string(),
                "lib.Thing".to_string()
            )));
        assert_eq!(
            ledger.canonical_path_at(1, 3, 7),
            Some("lib.Thing".to_string())
        );
        ledger.declare(
            0,
            member_name("Inner", "helper"),
            "app.Inner.helper".to_string(),
            "function".to_string(),
            span(),
            NameVisibility::Private,
        );
        assert_eq!(
            ledger.display_path(0, &member_name("Inner", "helper"), Some(0)),
            Some("app.Inner.helper".to_string())
        );
        assert_eq!(
            ledger.canonical_path(0, "Alias"),
            Some("lib.Thing".to_string())
        );
        assert_eq!(
            ledger.display_path(0, "Alias", Some(1)),
            Some("Alias".to_string())
        );

        ledger.record_alias(
            0,
            "lib".to_string(),
            "lib".to_string(),
            Some(1),
            span(),
            NameVisibility::Public,
        );
        assert!(ledger
            .canonical_paths(0)
            .contains(&("lib.Thing".to_string(), "lib.Thing".to_string())));
    }

    #[test]
    fn display_path_qualifies_ambiguous_visible_leaves() {
        let mut ledger = NameLedger::default();
        ledger.set_module(
            0,
            "app".to_string(),
            "app.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.set_module(
            1,
            "one".to_string(),
            "one.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.set_module(
            2,
            "two".to_string(),
            "two.jet".to_string(),
            "pkg".to_string(),
        );
        for (module, path) in [(1, "one.Thing"), (2, "two.Thing")] {
            ledger.declare(
                module,
                "Thing".to_string(),
                path.to_string(),
                "type".to_string(),
                span(),
                NameVisibility::Public,
            );
        }
        assert_eq!(
            ledger.display_path(0, "Thing", Some(2)),
            Some("two.Thing".to_string())
        );
    }

    #[test]
    fn display_path_at_projects_unique_and_ambiguous_definitions() {
        let mut ledger = NameLedger::default();
        ledger.set_module(
            0,
            "app".to_string(),
            "app.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.set_module(
            1,
            "one".to_string(),
            "one.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.set_module(
            2,
            "two".to_string(),
            "two.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.declare(
            0,
            "Point".to_string(),
            "app.Point".to_string(),
            "type".to_string(),
            Span::new(10, 15),
            NameVisibility::Public,
        );
        assert_eq!(
            ledger.display_path_at(0, 10, 15, "Point", None, Some(0)),
            Some("Point".to_string())
        );
        for (module, path) in [(1, "one.Point"), (2, "two.Point")] {
            ledger.declare(
                module,
                "Point".to_string(),
                path.to_string(),
                "type".to_string(),
                span(),
                NameVisibility::Public,
            );
        }
        assert_eq!(
            ledger.display_path_at(0, 3, 7, "Point", None, Some(2)),
            Some("two.Point".to_string())
        );
    }

    #[test]
    fn display_path_at_projects_members_from_their_owner() {
        let mut ledger = NameLedger::default();
        ledger.set_module(
            0,
            "app".to_string(),
            "app.jet".to_string(),
            "pkg".to_string(),
        );
        ledger.declare(
            0,
            "Point".to_string(),
            "app.Point".to_string(),
            "type".to_string(),
            Span::new(10, 15),
            NameVisibility::Public,
        );
        ledger.declare(
            0,
            "Point.x".to_string(),
            "app.Point.x".to_string(),
            "field".to_string(),
            Span::new(20, 21),
            NameVisibility::Public,
        );
        assert_eq!(
            ledger.display_path_at(0, 20, 21, "x", Some("Point"), Some(0)),
            Some("Point.x".to_string())
        );
    }

    #[test]
    fn module_indices_follow_replacement_snapshot_merge_and_refresh() {
        let mut ledger = NameLedger::default();
        for module in [2, 0, 1] {
            ledger.set_module(module, format!("m{module}"), format!("m{module}.jet"), "pkg".into());
            ledger.set_module_namespace(module, "pkg".into());
        }
        assert_eq!(ledger.namespace_siblings(1).collect::<Vec<_>>(), vec![0, 2]);
        ledger.set_module_namespace(2, "other".into());
        assert_eq!(ledger.namespace_siblings(1).collect::<Vec<_>>(), vec![0]);
        for path in ["m0.Old", "m0.New"] {
            ledger.declare(0, "T".into(), path.into(), "type".into(), span(), NameVisibility::Private);
        }
        ledger.record_alias(1, "dep".into(), "m0".into(), Some(0), span(), NameVisibility::Private);
        ledger.record_alias(1, "dep".into(), "m0".into(), Some(0), span(), NameVisibility::Public);
        assert_eq!(ledger.module_declarations(0).count(), 1);
        assert_eq!(ledger.module_aliases(1).count(), 1);
        for (key, path) in ledger.canonical_paths(1) {
            assert_eq!(ledger.canonical_source_path(1, &key), Some(path));
        }
        assert!(ledger.visible(1, 1, "T"));
        let snapshot = ledger.body_snapshot();
        ledger.declare(0, "Later".into(), "m0.Later".into(), "type".into(), span(), NameVisibility::Private);
        assert_eq!(snapshot.module_declarations(0).count(), 1);
        let mut body = ledger.body_snapshot();
        let reference = NameReference {
            module_path: "m0.jet".into(), kind: "type".into(), def_span: span(), semantic_identity: None,
        };
        body.record_reference("m1.jet".into(), 10, 12, reference.clone());
        body.record_reference("m1.jet".into(), 10, 12, reference);
        ledger.merge_references(&body);
        ledger.merge_references(&body);
        assert_eq!(ledger.module_references("m1.jet").count(), 1);
        assert_eq!(ledger.module_references("m0.jet").count(), 0);
        ledger.clear_sema_facts();
        assert_eq!(ledger.module_declarations(0).count(), 0);
        assert_eq!(ledger.module_aliases(1).count(), 0);
        assert_eq!(ledger.module_references("m1.jet").count(), 0);
        assert_eq!(ledger.namespace_siblings(1).collect::<Vec<_>>(), vec![0]);
    }
}
