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
        self.reference_anchors.insert(
            (self.module_path.to_string(), span.start, span.end),
            DefinitionAnchorFact {
                module_path: module_path.to_string(),
                kind: kind.to_string(),
                def_span,
            },
        );
    }

    pub(crate) fn record_local_reference(&mut self, span: Span, info: &LocalInfo) {
        let kind = if info.param_conv.is_some() { "param" } else { "local" };
        let module_path = self.module_path.to_string();
        self.record_reference_anchor(span, &module_path, kind, info.def_span);
    }

    pub(crate) fn record_function_reference(&mut self, module_idx: usize, name: &str, span: Span) {
        let target = self.modules.and_then(|modules| modules.get(module_idx)).and_then(|module| {
            module.func_spans.get(name).copied().map(|def_span| (module.module_path.clone(), def_span))
        });
        if let Some((module_path, def_span)) = target {
            self.record_reference_anchor(span, &module_path, "function", def_span);
        }
    }

    pub(crate) fn record_current_function_reference(&mut self, name: &str, span: Span) {
        self.record_function_reference(self.module_idx, name, span);
    }

    pub(crate) fn record_const_reference(&mut self, name: &str, span: Span) {
        let target = self.modules.and_then(|modules| modules.get(self.module_idx)).and_then(|module| {
            module.const_spans.get(name).copied().map(|def_span| (module.module_path.clone(), def_span))
        });
        if let Some((module_path, def_span)) = target {
            self.record_reference_anchor(span, &module_path, "const", def_span);
        }
    }

    pub(crate) fn record_import_alias_reference(&mut self, alias: &str, span: Span) {
        let def_span = self.modules.and_then(|modules| modules.get(self.module_idx))
            .and_then(|module| module.import_spans.get(alias)).copied();
        if let Some(def_span) = def_span {
            let module_path = self.module_path.to_string();
            self.record_reference_anchor(span, &module_path, "import_alias", def_span);
        }
    }

    pub(crate) fn record_method_reference(&mut self, type_name: &str, method: &str, span: Span) {
        let Some(owner) = self.struct_owner_module(type_name, None) else { return };
        let target = if owner == self.module_idx {
            self.registry.method(type_name, method).map(|sig| (self.module_path.to_string(), sig.name_span))
        } else {
            self.modules.and_then(|modules| modules.get(owner)).and_then(|module| {
                module.registry.method(type_name, method).map(|sig| (module.module_path.clone(), sig.name_span))
            })
        };
        if let Some((module_path, def_span)) = target {
            self.record_reference_anchor(span, &module_path, "function", def_span);
        }
    }

    pub(crate) fn record_field_reference(&mut self, owner: usize, type_name: &str, member: &str, span: Span) {
        let fields = self.struct_fields_of(owner, type_name).and_then(|fields| {
            fields.iter().find(|(name, ..)| name == member).map(|(_, def_span, ..)| *def_span)
        });
        let module_path = if owner == self.module_idx {
            Some(self.module_path.to_string())
        } else {
            self.modules.and_then(|modules| modules.get(owner)).map(|module| module.module_path.clone())
        };
        if let (Some(def_span), Some(module_path)) = (fields, module_path) {
            self.record_reference_anchor(span, &module_path, "field", def_span);
        }
    }
}
