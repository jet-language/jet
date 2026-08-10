//! One semantic name ledger and one Rust-name projection.

use crate::Diagnostics::Span;
use std::collections::HashMap;

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

/// A checked source reference and its resolved declaration origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameReference {
    pub module_path: String,
    pub kind: String,
    pub def_span: Span,
    pub semantic_identity: Option<String>,
}

/// Shared name facts. Loader seeds import edges; sema adds declarations,
/// aliases, visibility, paths, and checked reference origins.
#[derive(Debug, Clone, Default)]
pub struct NameLedger {
    imports: HashMap<(usize, Span), usize>,
    modules: HashMap<usize, NameModule>,
    declarations: HashMap<(usize, String), NameDeclaration>,
    aliases: HashMap<(usize, String), NameAlias>,
    references: HashMap<(String, usize, usize), NameReference>,
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
        self.modules.insert(module, NameModule { alias, path, package });
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

    pub fn declaration_path(&self, module: usize, name: &str) -> Option<&str> {
        self.declaration(module, name).map(|declaration| declaration.path.as_str())
    }

    pub fn semantic_identity(&self, module: usize, name: &str) -> Option<String> {
        self.module_alias(module)
            .map(|alias| format!("{alias}::{name}"))
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
        self.aliases.insert(
            (module, name.clone()),
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

    /// Return an import binding only when a declaration has not replaced it.
    /// D-NAME-TREE1 makes a declaration at the same name win over an alias.
    pub fn effective_alias(&self, module: usize, name: &str) -> Option<&NameAlias> {
        self.declaration(module, name)
            .is_none()
            .then(|| self.alias(module, name))
            .flatten()
    }

    pub fn visible(&self, from_module: usize, target_module: usize, name: &str) -> bool {
        let visibility = self
            .declaration(target_module, name)
            .map(|declaration| declaration.visibility)
            .or_else(|| self.effective_alias(target_module, name).map(|alias| alias.visibility));
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
        self.references.insert((source_module, start, end), reference);
    }

    pub fn reference(&self, module_path: &str, start: usize, end: usize) -> Option<&NameReference> {
        self.references.get(&(module_path.to_string(), start, end))
    }

    pub fn references(&self) -> &HashMap<(String, usize, usize), NameReference> {
        &self.references
    }

    pub fn merge_references(&mut self, other: &Self) {
        self.references.extend(other.references.clone());
    }

    /// Copy lookup facts for an incremental body check without copying prior
    /// body references into the cache entry.
    pub fn body_snapshot(&self) -> Self {
        let mut snapshot = self.clone();
        snapshot.references.clear();
        snapshot
    }

    pub fn clear_sema_facts(&mut self) {
        self.modules.clear();
        self.declarations.clear();
        self.aliases.clear();
        self.references.clear();
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

/// Rust identifier for an inline-module member identity.
pub fn member_name(module: &str, name: &str) -> String {
    format!("{module}__{name}")
}

pub use mangle_path as mangle_variant;
pub use mangle_path as user_type_rust;

pub fn user_trait_rust(name: &str) -> String {
    mangle(name)
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
        ledger.set_module(0, "app".to_string(), "app.jet".to_string(), "pkg".to_string());
        ledger.set_module(1, "lib".to_string(), "lib.jet".to_string(), "pkg".to_string());
        ledger.set_module(2, "dep".to_string(), "dep.jet".to_string(), "dep".to_string());
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
        ledger.set_module(0, "app".to_string(), "app.jet".to_string(), "pkg".to_string());
        ledger.set_module(1, "lib".to_string(), "lib.jet".to_string(), "pkg".to_string());
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
        assert_eq!(ledger.alias(0, "Thing").map(|alias| alias.target.as_str()), Some("lib.Thing"));

        // D-NAME-TREE1=A: a declaration attaches at the name and replaces an
        // alias projection at that same point in the tree.
        ledger.declare(
            0,
            "Thing".to_string(),
            "app.Thing".to_string(),
            "type".to_string(),
            span(),
            NameVisibility::Private,
        );
        assert!(!ledger.exported(0, "Thing"));
        assert!(ledger.effective_alias(0, "Thing").is_none());
    }

    #[test]
    fn rust_names_use_one_projection() {
        assert_eq!(mangle("run"), "__jet_run");
        assert_eq!(mangle("$value"), "__jet_ct_value");
        assert_eq!(mangle_path("Fire.Burn"), "__jet_Fire__Burn");
        assert_eq!(member_name("math", "double"), "math__double");
    }
}
