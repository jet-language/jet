//! Reference identities emitted only after checker lookup succeeds.

use super::*;

fn is_side_effect_free_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Char(..) | Expr::UnitLit { .. } => {
            true
        }
        Expr::Str(parts, _) => parts
            .iter()
            .all(|part| matches!(part, crate::AST::StrPart::Lit(_))),
        Expr::Paren(inner, _) | Expr::Unary(_, inner, _) => is_side_effect_free_literal(inner),
        Expr::Binary(_, left, right, _) => {
            is_side_effect_free_literal(left) && is_side_effect_free_literal(right)
        }
        _ => false,
    }
}

impl<'a> Checker<'a> {
    fn unused_name_is_intentional(name: &str) -> bool {
        name.is_empty()
            || name.starts_with('_')
            || name == crate::Syntax::KW_SELF
            || name == "result"
            || name == crate::Syntax::AMBIENT_ERR
            || name == crate::Syntax::KW_IT
    }

    pub(crate) fn note_unused_binding(&mut self, name: &str, span: Span, parameter: bool) {
        if Self::unused_name_is_intentional(name)
            || span.start >= span.end
            || self.unused_bindings.iter().any(|binding| binding.span == span)
        {
            return;
        }
        self.unused_bindings.push(UnusedBinding {
            name: name.to_string(),
            span,
            parameter,
            fix: None,
        });
    }

    pub(crate) fn note_unused_binding_fix(
        &mut self,
        span: Span,
        init: &Expr,
        is_comptime: bool,
        has_metadata: bool,
    ) {
        if is_comptime || has_metadata || !is_side_effect_free_literal(init) {
            return;
        }
        let init_span = init.span();
        if init_span.start <= span.start {
            return;
        }
        if let Some(binding) = self.unused_bindings.iter_mut().find(|binding| binding.span == span) {
            binding.fix = Some(crate::Diagnostics::TextEdit {
                span: Span::new(span.start, init_span.start),
                new_text: String::new(),
            });
        }
    }

    pub(crate) fn mark_local_write(&mut self, name: &str) {
        if let Some(def_span) = self.lookup(name).map(|info| info.def_span) {
            self.unused_binding_refs.insert(def_span);
        }
    }

    fn mark_local_name_reference(&mut self, name: &str) {
        if let Some(def_span) = self.lookup(name).map(|info| info.def_span) {
            self.unused_binding_refs.insert(def_span);
        }
    }

    pub(crate) fn mark_default_parameter_references(&mut self, params: &[crate::AST::Param]) {
        for param in params {
            let Some(default) = &param.default else { continue };
            let mut copy = default.clone();
            copy.for_each_expr_mut(|node| match node {
                Expr::Ident(name, _) | Expr::ComptimeName { name, .. } => {
                    self.mark_local_name_reference(name);
                }
                Expr::Call(call) => self.mark_local_name_reference(&call.name),
                _ => {}
            });
        }
    }

    pub(crate) fn emit_unused_binding_lints(&mut self) {
        for binding in std::mem::take(&mut self.unused_bindings) {
            if self.unused_binding_refs.contains(&binding.span) {
                continue;
            }
            self.name_ledger.record_structure_fact(
                jet_foundation::Names::StructureFact::new(
                    jet_foundation::Names::StructureFactKind::Liveness,
                    binding.name.clone(),
                    self.module_path.to_string(),
                    binding.span,
                    "unused",
                    if binding.parameter {
                        "parameter is never read"
                    } else {
                        "binding is never read"
                    },
                    Some("_name".to_string()),
                ),
            );
            let code = if binding.parameter { "L0102" } else { "L0101" };
            let name = binding.name.as_str();
            let mut diagnostic = Diagnostic::from_row(code, &[("name", name)], Some(binding.span));
            if let Some(edit) = binding.fix {
                diagnostic = diagnostic.with_edit(edit);
            }
            self.diags.push(diagnostic);
        }
    }

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
        self.unused_binding_refs.insert(info.def_span);
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
            self.record_reference_anchor_with_identity(
                span,
                &module_path,
                "import_alias",
                def_span,
                Some(format!("import:{alias}")),
            );
        }
    }

    pub(crate) fn record_core_import_reference(&mut self, module: &str, span: Span) {
        let aliases: Vec<String> = self
            .core_imports
            .iter()
            .filter(|(_, imported)| {
                *imported == module
                    || module
                        .strip_prefix(imported.as_str())
                        .is_some_and(|rest| rest.starts_with('.'))
            })
            .map(|(alias, _)| alias.clone())
            .collect();
        for alias in aliases {
            self.record_import_alias_reference(&alias, span);
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
