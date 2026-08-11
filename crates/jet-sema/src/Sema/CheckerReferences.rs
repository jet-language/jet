//! Reference identities emitted only after checker lookup succeeds.

use super::*;

impl<'a> Checker<'a> {
    pub(crate) fn record_reference_anchor(
        &mut self,
        span: Span,
        module_path: &str,
        kind: &str,
        def_span: Span,
    ) {
        self.record_reference_anchor_with_identity(span, module_path, kind, def_span, None);
    }

    fn record_reference_anchor_with_identity(
        &mut self,
        span: Span,
        module_path: &str,
        kind: &str,
        def_span: Span,
        semantic_identity: Option<String>,
    ) {
        self.name_ledger.record_reference(
            self.module_path.to_string(),
            span.start,
            span.end,
            jet_foundation::Names::NameReference {
                module_path: module_path.to_string(),
                kind: kind.to_string(),
                def_span,
                semantic_identity,
            },
        );
    }

    pub(crate) fn record_semantic_reference(&mut self, span: Span, semantic_identity: String) {
        self.name_ledger.record_reference(
            self.module_path.to_string(),
            span.start,
            span.end,
            jet_foundation::Names::NameReference {
                module_path: self.module_path.to_string(),
                kind: "semantic".to_string(),
                def_span: span,
                semantic_identity: Some(semantic_identity),
            },
        );
    }

    pub(crate) fn record_local_reference(&mut self, span: Span, info: &LocalInfo) {
        let kind = if info.param_conv.is_some() { "param" } else { "local" };
        let module_path = self.module_path.to_string();
        self.record_reference_anchor(span, &module_path, kind, info.def_span);
    }

    pub(crate) fn record_function_reference(&mut self, module_idx: usize, name: &str, span: Span) {
        let target = self.name_ledger.declaration(module_idx, name).map(|declaration| {
            (
                self.name_ledger
                    .module_path(module_idx)
                    .unwrap_or(self.module_path)
                    .to_string(),
                declaration.span,
            )
        });
        if let Some((module_path, def_span)) = target {
            let identity = self.name_ledger.semantic_identity(module_idx, name);
            self.record_reference_anchor_with_identity(
                span,
                &module_path,
                "function",
                def_span,
                identity,
            );
        }
    }

    pub(crate) fn record_current_function_reference(&mut self, name: &str, span: Span) {
        self.record_function_reference(self.module_idx, name, span);
    }

    pub(crate) fn record_const_reference(&mut self, name: &str, span: Span) {
        let target = self.name_ledger.declaration(self.module_idx, name).map(|declaration| {
            (
                self.name_ledger
                    .module_path(self.module_idx)
                    .unwrap_or(self.module_path)
                    .to_string(),
                declaration.span,
            )
        });
        if let Some((module_path, def_span)) = target {
            let identity = self.name_ledger.semantic_identity(self.module_idx, name);
            self.record_reference_anchor_with_identity(
                span,
                &module_path,
                "const",
                def_span,
                identity,
            );
        }
    }

    pub(crate) fn record_import_alias_reference(&mut self, alias: &str, span: Span) {
        let def_span = self
            .name_ledger
            .effective_alias(self.module_idx, alias)
            .map(|alias| alias.span);
        if let Some(def_span) = def_span {
            let module_path = self.module_path.to_string();
            self.record_reference_anchor(span, &module_path, "import_alias", def_span);
        }
    }

    pub(crate) fn record_method_reference(&mut self, type_name: &str, method: &str, span: Span) {
        let (import_ns, leaf) = Self::split_type_name(type_name);
        let Some(owner) = self.struct_owner_module(leaf, import_ns) else { return };
        let name = format!("{leaf}.{method}");
        let target = self
            .name_ledger
            .declaration(owner, &name)
            .or_else(|| self.name_ledger.declaration(self.module_idx, &name))
            .map(|declaration| {
                (
                    self.name_ledger
                        .module_path(declaration.module)
                        .unwrap_or(self.module_path)
                        .to_string(),
                    declaration.span,
                )
            });
        if let Some((module_path, def_span)) = target {
            let identity = self
                .name_ledger
                .declaration(owner, &name)
                .or_else(|| self.name_ledger.declaration(self.module_idx, &name))
                .and_then(|declaration| {
                    self.name_ledger.semantic_identity(declaration.module, &name)
                });
            self.record_reference_anchor_with_identity(
                span,
                &module_path,
                "function",
                def_span,
                identity,
            );
        }
    }

    pub(crate) fn record_field_reference(&mut self, owner: usize, type_name: &str, member: &str, span: Span) {
        let (_, leaf) = Self::split_type_name(type_name);
        let name = format!("{leaf}.{member}");
        let target = self.name_ledger.declaration(owner, &name).map(|declaration| {
            (
                self.name_ledger
                    .module_path(declaration.module)
                    .unwrap_or(self.module_path)
                    .to_string(),
                declaration.span,
            )
        });
        if let Some((module_path, def_span)) = target {
            let identity = self.name_ledger.semantic_identity(owner, &name);
            self.record_reference_anchor_with_identity(
                span,
                &module_path,
                "field",
                def_span,
                identity,
            );
        }
    }
}
