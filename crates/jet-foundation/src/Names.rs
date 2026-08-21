//! One semantic name ledger and one Rust-name projection.

use crate::Diagnostics::Span;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Stable package scope used by every module identity.
pub fn package_scope_for(path: &Path, project_root: &Path) -> String {
    let norm_path = normalize_path(path);
    let norm_root = normalize_path(project_root);
    let scope = if norm_path.starts_with(&norm_root) {
        norm_root
    } else {
        norm_path.parent().map(normalize_path).unwrap_or(norm_path)
    };
    scope.to_string_lossy().into_owned()
}

fn normalize_path(path: &Path) -> PathBuf {
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

/// Shared name facts. Loader seeds module and import identities; sema adds
/// declarations, aliases, visibility, paths, and checked reference origins.
#[derive(Debug, Clone, Default)]
pub struct NameLedger {
    imports: HashMap<(usize, Span), usize>,
    modules: HashMap<usize, NameModule>,
    declarations: HashMap<(usize, String), NameDeclaration>,
    /// Source-facing paths for compiler-owned declarations. Generated
    /// generic-instance names remain semantic keys, but diagnostics and
    /// tooling project them back to the instance member path.
    display_paths: HashMap<(usize, String), String>,
    aliases: HashMap<(usize, String), NameAlias>,
    references: HashMap<(String, usize, usize), NameReference>,
    structure_facts: Vec<StructureFact>,
}

impl NameLedger {
    pub fn with_imports(imports: HashMap<(usize, Span), usize>) -> Self {
        Self {
            imports,
            ..Self::default()
        }
    }

    pub fn import_target(&self, module: usize, span: Span) -> Option<usize> {
        self.imports.get(&(module, span)).copied()
    }

    pub fn record_import_target(&mut self, module: usize, span: Span, target: usize) {
        self.imports.insert((module, span), target);
    }

    pub fn set_module(&mut self, module: usize, alias: String, path: String, package: String) {
        self.modules.insert(
            module,
            NameModule {
                alias,
                path,
                package,
            },
        );
    }

    pub fn module(&self, module: usize) -> Option<&NameModule> {
        self.modules.get(&module)
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
        self.declarations.insert(
            (module, name.clone()),
            NameDeclaration {
                module,
                name,
                path,
                kind,
                span,
                visibility,
            },
        );
    }

    pub fn declaration(&self, module: usize, name: &str) -> Option<&NameDeclaration> {
        self.declarations.get(&(module, name.to_string()))
    }

    pub fn declarations(&self) -> impl Iterator<Item = &NameDeclaration> {
        self.declarations.values()
    }

    pub fn declaration_path(&self, module: usize, name: &str) -> Option<&str> {
        self.declaration(module, name)
            .map(|declaration| declaration.path.as_str())
    }

    fn display_declaration_path(&self, module: usize, name: &str) -> Option<String> {
        let declaration = self.declaration(module, name)?;
        self.display_paths
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
        self.display_paths
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
        for declaration in self.declarations.values() {
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
        self.modules.iter().find_map(|(module, facts)| {
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
        for declaration in self.declarations.values() {
            if declaration.name == leaf && self.visible(from_module, declaration.module, leaf) {
                if let Some(path) =
                    self.display_declaration_path(declaration.module, &declaration.name)
                {
                    paths.insert(path);
                }
            }
        }
        for alias in self.aliases.values() {
            if alias.name != leaf || !self.visible(from_module, alias.module, leaf) {
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

        let resolved_path = resolved_module
            .and_then(|module| self.display_declaration_path(module, leaf))
            .or_else(|| self.display_declaration_path(from_module, leaf))
            .or_else(|| self.canonical_path(from_module, name));
        let resolved_path = resolved_path.or_else(|| paths.iter().next().cloned())?;
        let has_display_projection = self.declarations.values().any(|declaration| {
            declaration.name == leaf
                && self.visible(from_module, declaration.module, leaf)
                && self
                    .display_paths
                    .contains_key(&(declaration.module, declaration.path.clone()))
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
    /// This is the projection boundary for codegen and runtime tooling: the
    /// consumers do not walk declarations or aliases themselves.
    pub fn canonical_paths(&self, module: usize) -> Vec<(String, String)> {
        let mut paths = BTreeSet::new();
        for ((owner, name), declaration) in &self.declarations {
            if *owner == module {
                let path = declaration.path.clone();
                paths.insert((name.clone(), path.clone()));
                // A qualified type name is itself a source key. Keep it in
                // the same projection so consumers never reconstruct it from
                // declaration storage.
                if path.as_str() != name.as_str() {
                    paths.insert((path.clone(), path));
                }
            }
        }
        for ((owner, name), alias) in &self.aliases {
            if *owner == module {
                if let Some(path) = self.canonical_path(module, name) {
                    paths.insert((name.clone(), path));
                }
                // A module import is also a source qualifier (`dep.Thing`).
                // Publish those keys here so codegen can resolve qualified
                // external types without rebuilding an import path.
                if let Some(target_module) = alias.target_module {
                    if !alias.target.contains('.') {
                        if let Some(target_alias) = self.module_alias(target_module) {
                            let prefix = format!("{target_alias}.");
                            for declaration in self.declarations.values() {
                                if declaration.module != target_module {
                                    continue;
                                }
                                let Some(suffix) = declaration.path.strip_prefix(prefix.as_str())
                                else {
                                    continue;
                                };
                                paths.insert((
                                    format!("{}.{}", alias.name, suffix),
                                    declaration.path.clone(),
                                ));
                            }
                        }
                    }
                }
            }
        }
        paths.into_iter().collect()
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
        self.aliases.insert(
            key,
            NameAlias {
                module,
                name,
                target,
                target_module,
                span,
                visibility,
            },
        );
    }

    pub fn alias(&self, module: usize, name: &str) -> Option<&NameAlias> {
        self.aliases.get(&(module, name.to_string()))
    }

    pub fn aliases(&self) -> impl Iterator<Item = &NameAlias> {
        self.aliases.values()
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
            });
        let Some(visibility) = visibility else {
            return false;
        };
        match visibility {
            NameVisibility::Public => true,
            NameVisibility::Package => self
                .module(from_module)
                .zip(self.module(target_module))
                .is_some_and(|(from, target)| from.package == target.package),
            NameVisibility::Private => from_module == target_module,
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
        self.references
            .insert((source_module, start, end), reference);
    }

    pub fn reference(&self, module_path: &str, start: usize, end: usize) -> Option<&NameReference> {
        self.references.get(&(module_path.to_string(), start, end))
    }

    pub fn references(&self) -> &HashMap<(String, usize, usize), NameReference> {
        &self.references
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
        self.references.extend(other.references.clone());
    }

    /// Copy lookup facts for an incremental body check without copying prior
    /// body references into the cache entry.
    pub fn body_snapshot(&self) -> Self {
        let mut snapshot = self.clone();
        snapshot.references.clear();
        snapshot.structure_facts.clear();
        snapshot
    }

    pub fn clear_sema_facts(&mut self) {
        self.modules.clear();
        self.declarations.clear();
        self.display_paths.clear();
        self.aliases.clear();
        self.references.clear();
        self.structure_facts.clear();
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

/// Rust identifier for a dotted Jet path.
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
        assert_eq!(mangle("@value"), "__jet_ct_value");
        assert_eq!(mangle_path("Fire.Burn"), "__jet_Fire__Burn");
        assert_eq!(member_name("math", "double"), "__jet_math__double");
        assert_eq!(
            member_name(&member_name("outer", "inner"), "helper"),
            "__jet_outer__inner__helper"
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
}
