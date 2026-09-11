use super::{Func, Item, Marker, Type};
use crate::Diagnostics::Span;
use crate::Syntax;
use std::collections::BTreeMap;

/// D-DX-PLUGIN1=D: typed projection of the existing `#DevPanel` marker.
///
/// The parser retains the original marker in `Func::markers`; this projection
/// carries only the source facts needed by sema and tooling.  In particular,
/// it does not add another function flag or a second marker vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DevPanelMarker {
    pub span: Span,
    pub name_span: Span,
    pub negated: bool,
    pub arg_count: usize,
}

impl DevPanelMarker {
    pub const fn is_valid(self) -> bool {
        !self.negated && self.arg_count == 0
    }
}

impl Marker {
    /// Return the typed `#DevPanel` projection while preserving malformed
    /// applications for the sema diagnostic at the original marker span.
    pub fn dev_panel_marker(&self) -> Option<DevPanelMarker> {
        (self.name == Syntax::MARKER_DEV_PANEL).then_some(DevPanelMarker {
            span: self.span,
            name_span: self.name_span,
            negated: self.negated,
            arg_count: self.args.len(),
        })
    }
}

impl Func {
    /// Return the first source marker named `#DevPanel` in declaration order.
    /// Duplicate applications remain visible through `Func::markers` so sema
    /// can issue its registered duplicate diagnostic rather than silently
    /// coalescing source.
    pub fn dev_panel_marker(&self) -> Option<DevPanelMarker> {
        self.markers.iter().find_map(Marker::dev_panel_marker)
    }
}

/// One state field fed by a typed devtools publication site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevtoolsStateField {
    pub name: String,
    pub ty: Type,
    pub span: Span,
    pub has_default: bool,
}

impl DevtoolsStateField {
    pub fn new(name: impl Into<String>, ty: Type, span: Span, has_default: bool) -> Self {
        Self {
            name: name.into(),
            ty,
            span,
            has_default,
        }
    }

    /// An optional field or a field with an absence default is renderable
    /// before its first event, as required by the panel state contract.
    pub fn is_ready_before_publication(&self) -> bool {
        self.has_default || matches!(self.ty, Type::Option(_))
    }
}

/// A package-owned panel discovered from one marked, typed function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevtoolsPanel {
    pub package: String,
    /// Stable source-module identity, not the loader's Rust alias.
    pub module: String,
    pub function: String,
    pub state_type: Type,
    pub state_fields: Vec<DevtoolsStateField>,
    pub span: Span,
}

impl DevtoolsPanel {
    pub fn new(
        package: impl Into<String>,
        module: impl Into<String>,
        function: impl Into<String>,
        state_type: Type,
        state_fields: Vec<DevtoolsStateField>,
        span: Span,
    ) -> Self {
        Self {
            package: package.into(),
            module: module.into(),
            function: function.into(),
            state_type,
            state_fields,
            span,
        }
    }

    /// Stable panel identity used by static discovery and every publication.
    pub fn identity(&self) -> String {
        format!("{}::{}::{}", self.package, self.module, self.function)
    }

    pub fn state_field(&self, name: &str) -> Option<&DevtoolsStateField> {
        self.state_fields.iter().find(|field| field.name == name)
    }
}

/// One explicit `core.devtools.publish` site. Identity is resolved to the
/// marked panel before this fact is registered; protocol serialization belongs
/// to the shared Prelude protocol seam.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevtoolsFactPublication {
    pub package: String,
    pub module: String,
    pub panel: String,
    pub field: String,
    pub value_type: Type,
    pub span: Span,
}

impl DevtoolsFactPublication {
    pub fn new(
        package: impl Into<String>,
        module: impl Into<String>,
        panel: impl Into<String>,
        field: impl Into<String>,
        value_type: Type,
        span: Span,
    ) -> Self {
        Self {
            package: package.into(),
            module: module.into(),
            panel: panel.into(),
            field: field.into(),
            value_type,
            span,
        }
    }
}

/// One registry for panel declarations and their typed publication sites.
/// Consumers (sema, inspect, codegen) project their own views from this table;
/// none maintains a parallel `(package, module, panel, field)` list.
#[derive(Clone, Debug, Default)]
pub struct DevtoolsRegistry {
    panels: BTreeMap<String, DevtoolsPanel>,
    publications: Vec<DevtoolsFactPublication>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DevtoolsRegistryError {
    DuplicatePanel {
        identity: String,
        first: Span,
        duplicate: Span,
    },
    UnknownStateField {
        package: String,
        module: String,
        panel: String,
        field: String,
        span: Span,
    },
    StateFieldTypeMismatch {
        package: String,
        module: String,
        panel: String,
        field: String,
        expected: Type,
        actual: Type,
        span: Span,
    },
}

impl DevtoolsRegistry {
    pub fn register_panel(&mut self, panel: DevtoolsPanel) -> Result<(), DevtoolsRegistryError> {
        let identity = panel.identity();
        if let Some(first) = self.panels.get(&identity) {
            return Err(DevtoolsRegistryError::DuplicatePanel {
                identity,
                first: first.span,
                duplicate: panel.span,
            });
        }
        self.panels.insert(identity, panel);
        Ok(())
    }

    /// Validate one publication without changing the registry. Inference uses
    /// this read-only path; successful publication facts are collected by a
    /// separate deterministic late pass.
    pub fn validate_publication(
        &self,
        publication: &DevtoolsFactPublication,
    ) -> Result<(), DevtoolsRegistryError> {
        let Some(panel) = self.panel(&publication.panel) else {
            return Err(DevtoolsRegistryError::UnknownStateField {
                package: publication.package.clone(),
                module: publication.module.clone(),
                panel: publication.panel.clone(),
                field: publication.field.clone(),
                span: publication.span,
            });
        };
        if panel.package != publication.package
            || panel.module != publication.module
            || panel.identity() != publication.panel
        {
            return Err(DevtoolsRegistryError::UnknownStateField {
                package: publication.package.clone(),
                module: publication.module.clone(),
                panel: publication.panel.clone(),
                field: publication.field.clone(),
                span: publication.span,
            });
        }
        let Some(field) = panel.state_field(&publication.field) else {
            return Err(DevtoolsRegistryError::UnknownStateField {
                package: publication.package.clone(),
                module: publication.module.clone(),
                panel: publication.panel.clone(),
                field: publication.field.clone(),
                span: publication.span,
            });
        };
        if field.ty != publication.value_type {
            return Err(DevtoolsRegistryError::StateFieldTypeMismatch {
                package: publication.package.clone(),
                module: publication.module.clone(),
                panel: publication.panel.clone(),
                field: publication.field.clone(),
                expected: field.ty.clone(),
                actual: publication.value_type.clone(),
                span: publication.span,
            });
        }
        Ok(())
    }

    pub fn register_publication(
        &mut self,
        publication: DevtoolsFactPublication,
    ) -> Result<(), DevtoolsRegistryError> {
        self.validate_publication(&publication)?;
        self.publications.push(publication);
        Ok(())
    }

    pub fn panel(&self, identity: &str) -> Option<&DevtoolsPanel> {
        self.panels.get(identity)
    }

    pub fn panels(&self) -> impl Iterator<Item = &DevtoolsPanel> {
        self.panels.values()
    }

    pub fn publications(&self) -> &[DevtoolsFactPublication] {
        &self.publications
    }

    /// Resolve a field to the panel in the caller's source module.
    pub fn panel_for_field(
        &self,
        package: &str,
        module: &str,
        field: &str,
    ) -> Option<&DevtoolsPanel> {
        self.panels.values().find(|panel| {
            panel.package == package && panel.module == module && panel.state_field(field).is_some()
        })
    }

    /// Package-scoped fallback for a publication written in a helper module.
    /// Ambiguous same-named fields are rejected instead of guessing a panel.
    pub fn unique_panel_for_field(&self, package: &str, field: &str) -> Option<&DevtoolsPanel> {
        let mut matches = self
            .panels
            .values()
            .filter(|panel| panel.package == package && panel.state_field(field).is_some());
        let panel = matches.next()?;
        matches.next().is_none().then_some(panel)
    }

    /// Return the state field for one fully bound panel identity.
    pub fn state_field(
        &self,
        package: &str,
        module: &str,
        panel: &str,
        field: &str,
    ) -> Option<&DevtoolsStateField> {
        let candidate = self.panel(panel)?;
        (candidate.package == package && candidate.module == module)
            .then(|| candidate.state_field(field))
            .flatten()
    }

    /// Collect the package's ordinary items for sema callers that have not yet
    /// built a separate inspect index. This deliberately returns source items,
    /// not a second registry or synthesized panel list.
    pub fn panel_items<'a>(items: &'a [Item]) -> impl Iterator<Item = &'a Func> {
        items.iter().filter_map(|item| match item {
            Item::Func(function) if function.dev_panel_marker().is_some() => Some(function),
            _ => None,
        })
    }
}
