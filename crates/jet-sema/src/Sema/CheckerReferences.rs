//! Reference identities emitted only after checker lookup succeeds.

use super::*;

impl<'a> Checker<'a> {
    fn unused_name_is_intentional(name: &str) -> bool {
        name.is_empty()
            || name.starts_with('_')
            || name.starts_with("@_")
            || name == crate::Syntax::KW_SELF
            || name == "result"
            || name == crate::Syntax::AMBIENT_ERR
            || name == crate::Syntax::KW_IT
    }

    pub(crate) fn note_unused_binding(&mut self, name: &str, span: Span, parameter: bool) {
        if Self::unused_name_is_intentional(name)
            || span.start >= span.end
            || self
                .unused_bindings
                .iter()
                .any(|binding| binding.span == span)
        {
            return;
        }
        self.unused_bindings.push(UnusedBinding {
            name: name.to_string(),
            span,
            parameter,
        });
    }

    pub(crate) fn mark_local_write(&mut self, name: &str) {
        if let Some(def_span) = self.lookup(name).map(|info| info.def_span) {
            self.unused_binding_refs.insert(def_span);
        }
    }

    pub(crate) fn mark_local_name_reference(&mut self, name: &str) {
        if let Some(def_span) = self.lookup(name).map(|info| info.def_span) {
            self.unused_binding_refs.insert(def_span);
        }
    }

    pub(crate) fn mark_default_parameter_references(&mut self, params: &[crate::AST::Param]) {
        for param in params {
            let Some(default) = &param.default else {
                continue;
            };
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
            self.name_ledger
                .record_structure_fact(jet_foundation::Names::StructureFact::new(
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
                ));
            let code = if binding.parameter { "L0102" } else { "L0101" };
            let name = binding.name.as_str();
            let diagnostic = Diagnostic::from_row(code, &[("name", name)], Some(binding.span))
                .with_edit(self.unused_binding_edit(&binding, name));
            self.diags.push(diagnostic);
        }
    }

    fn unused_binding_edit(
        &self,
        binding: &UnusedBinding,
        name: &str,
    ) -> crate::Diagnostics::TextEdit {
        if !binding.parameter {
            if let Some(span) = self.side_effect_free_binding_span(binding.span) {
                return crate::Diagnostics::TextEdit {
                    span,
                    new_text: String::new(),
                };
            }
        }
        crate::Diagnostics::TextEdit {
            span: binding.span,
            new_text: if let Some(rest) = name.strip_prefix('@') {
                format!("@_{rest}")
            } else {
                format!("_{name}")
            },
        }
    }

    fn side_effect_free_binding_span(&self, name_span: Span) -> Option<Span> {
        let function = find_function_by_span(&self.items, self.current_function_span)?;
        let binding = binding_by_span(&function.body, name_span)?;
        if binding.pattern.is_some()
            || binding.is_comptime
            || binding.ct.is_some()
            || binding.uninit
            || binding.meta.is_some()
            || !binding.markers.is_empty()
            || binding.arena_view
            || binding.string_view
            || binding.gc_promotion.is_some()
            || binding.gc_transferred
            || !literal_without_effects(&binding.init)
        {
            return None;
        }
        Some(Span::new(binding.name_span.start, binding.init.span().end))
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
        let kind = if info.param_conv.is_some() {
            "param"
        } else {
            "local"
        };
        let module_path = self.module_path.to_string();
        self.record_reference_anchor(span, &module_path, kind, info.def_span);
    }

    pub(crate) fn record_function_reference(&mut self, module_idx: usize, name: &str, span: Span) {
        let target = self
            .name_ledger
            .declaration(module_idx, name)
            .map(|declaration| {
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
        let target = self
            .name_ledger
            .declaration(self.module_idx, name)
            .map(|declaration| {
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

    pub(crate) fn record_import_alias_use(&mut self, alias: &str) -> Option<Span> {
        if self.lookup(alias).is_some() {
            return None;
        }
        record_import_alias_use_in_ledger(self.name_ledger, self.module_idx, alias)
    }

    pub(crate) fn record_import_alias_reference(&mut self, alias: &str, span: Span) {
        let def_span = self.record_import_alias_use(alias);
        if let Some(def_span) = def_span {
            let module_path = self.module_path.to_string();
            let identity = self.name_ledger.alias_identity(self.module_idx, def_span);
            self.record_reference_anchor_with_identity(
                span,
                &module_path,
                "import_alias",
                def_span,
                identity,
            );
        }
    }

    pub(crate) fn record_method_reference(&mut self, type_name: &str, method: &str, span: Span) {
        let (import_ns, leaf) = Self::split_type_name(type_name);
        let Some(owner) = self.struct_owner_module(leaf, import_ns) else {
            return;
        };
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
                    self.name_ledger
                        .semantic_identity(declaration.module, &name)
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

    pub(crate) fn record_field_reference(
        &mut self,
        owner: usize,
        type_name: &str,
        member: &str,
        span: Span,
    ) {
        let (_, leaf) = Self::split_type_name(type_name);
        let name = format!("{leaf}.{member}");
        let target = self
            .name_ledger
            .declaration(owner, &name)
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

fn record_import_alias_use_in_ledger(
    ledger: &mut jet_foundation::Names::NameLedger,
    module_idx: usize,
    alias: &str,
) -> Option<Span> {
    let def_span = ledger
        .effective_alias(module_idx, alias)
        .map(|alias| alias.span);
    if let Some(def_span) = def_span {
        ledger.record_alias_use(module_idx, def_span);
    }
    def_span
}
pub(crate) fn record_comptime_import_alias_uses(
    ledger: &mut jet_foundation::Names::NameLedger,
    module_idx: usize,
    expression: &Expr,
    imports: &HashMap<String, String>,
    globals: &HashMap<String, crate::Comptime::CtValue>,
) {
    let global_aliases: HashSet<String> = imports
        .keys()
        .filter(|alias| globals.contains_key(*alias))
        .cloned()
        .collect();
    let mut shadowed = HashSet::new();
    expression.for_each_expr(|node| match node {
        Expr::Lambda(lambda) => {
            let mut lambda_aliases = HashSet::new();
            for parameter in &lambda.params {
                if imports.contains_key(&parameter.name) {
                    lambda_aliases.insert(parameter.name.clone());
                }
            }
            for (name, _) in &lambda.take_names {
                if imports.contains_key(name) {
                    lambda_aliases.insert(name.clone());
                }
            }
            match &lambda.body {
                crate::AST::LambdaBody::Expr(body) => {
                    mark_shadowed_expr(body, &lambda_aliases, &mut shadowed);
                }
                crate::AST::LambdaBody::Block(body) => {
                    mark_shadowed_stmts(body, &lambda_aliases, &mut shadowed);
                    collect_shadowed_stmts(body, imports, &mut shadowed);
                }
            }
        }
        Expr::If {
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            let then_aliases = collect_shadowed_stmts(then_body, imports, &mut shadowed);
            mark_shadowed_expr(then_value, &then_aliases, &mut shadowed);
            let else_aliases = collect_shadowed_stmts(else_body, imports, &mut shadowed);
            mark_shadowed_expr(else_value, &else_aliases, &mut shadowed);
        }
        Expr::OrFallback {
            fallback: crate::AST::OrFallback::Block { body, value, .. },
            ..
        } => {
            let mut fallback_aliases = HashSet::new();
            if imports.contains_key(crate::Syntax::AMBIENT_ERR) {
                fallback_aliases.insert(crate::Syntax::AMBIENT_ERR.to_string());
            }
            mark_shadowed_stmts(body, &fallback_aliases, &mut shadowed);
            fallback_aliases.extend(collect_shadowed_stmts(body, imports, &mut shadowed));
            if let Some(value) = value {
                mark_shadowed_expr(value, &fallback_aliases, &mut shadowed);
            }
        }
        _ => {}
    });

    expression.for_each_expr(|node| {
        let alias = match node {
            Expr::Call(call) => call.name.split_once('.').map(|(alias, _)| alias),
            Expr::Field(base, ..)
            | Expr::Index { base, .. }
            | Expr::Slice { base, .. }
            | Expr::MethodCall { receiver: base, .. }
            | Expr::OptField { base, .. } => import_alias_root(base),
            Expr::CallValue { callee, .. } => import_alias_root(callee),
            Expr::PtrFromAddr { alias, .. } => Some(alias.as_str()),
            Expr::StructLit {
                import_ns: Some(alias),
                ..
            } => Some(alias.as_str()),
            _ => None,
        };
        let Some(alias) = alias else {
            return;
        };
        if !imports.contains_key(alias)
            || global_aliases.contains(alias)
            || shadowed.contains(&(node.span(), alias.to_string()))
        {
            return;
        }
        record_import_alias_use_in_ledger(ledger, module_idx, alias);
    });
}

fn mark_shadowed_expr(
    expression: &Expr,
    aliases: &HashSet<String>,
    shadowed: &mut HashSet<(Span, String)>,
) {
    if aliases.is_empty() {
        return;
    }
    expression.for_each_expr(|node| {
        for alias in aliases {
            shadowed.insert((node.span(), alias.clone()));
        }
    });
}

fn mark_shadowed_stmt(
    statement: &Stmt,
    aliases: &HashSet<String>,
    shadowed: &mut HashSet<(Span, String)>,
) {
    if aliases.is_empty() {
        return;
    }
    statement.for_each_expr(|node| {
        for alias in aliases {
            shadowed.insert((node.span(), alias.clone()));
        }
    });
}

fn mark_shadowed_stmts(
    statements: &[Stmt],
    aliases: &HashSet<String>,
    shadowed: &mut HashSet<(Span, String)>,
) {
    for statement in statements {
        mark_shadowed_stmt(statement, aliases, shadowed);
    }
}

fn binding_import_aliases(
    binding: &crate::AST::Binding,
    imports: &HashMap<String, String>,
) -> HashSet<String> {
    let mut aliases = HashSet::new();
    if !binding.name.is_empty() && imports.contains_key(&binding.name) {
        aliases.insert(binding.name.clone());
    }
    if let Some(pattern) = &binding.pattern {
        for name in pattern.names() {
            if imports.contains_key(&name.name) {
                aliases.insert(name.name.clone());
            }
        }
    }
    aliases
}

fn collect_shadowed_stmts(
    statements: &[Stmt],
    imports: &HashMap<String, String>,
    shadowed: &mut HashSet<(Span, String)>,
) -> HashSet<String> {
    let mut bound = HashSet::new();
    for statement in statements {
        mark_shadowed_stmt(statement, &bound, shadowed);
        collect_shadowed_stmt_scopes(statement, imports, shadowed);
        if let Stmt::Val(binding) = statement {
            bound.extend(binding_import_aliases(binding, imports));
        }
    }
    bound
}

fn collect_shadowed_stmt_scopes(
    statement: &Stmt,
    imports: &HashMap<String, String>,
    shadowed: &mut HashSet<(Span, String)>,
) {
    match statement {
        Stmt::While { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::AuthorityScope { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::Layout { body, .. } => {
            collect_shadowed_stmts(body, imports, shadowed);
        }
        Stmt::For {
            var, var2, body, ..
        } => {
            let mut aliases = HashSet::new();
            if imports.contains_key(var) {
                aliases.insert(var.clone());
            }
            if let Some((var2, _)) = var2 {
                if imports.contains_key(var2) {
                    aliases.insert(var2.clone());
                }
            }
            mark_shadowed_stmts(body, &aliases, shadowed);
            collect_shadowed_stmts(body, imports, shadowed);
        }
        Stmt::Switch {
            arms, else_body, ..
        }
        | Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => {
            for arm in arms {
                collect_shadowed_stmts(&arm.body, imports, shadowed);
            }
            if let Some(body) = else_body {
                collect_shadowed_stmts(body, imports, shadowed);
            }
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            let aliases = binding_import_aliases(init, imports);
            mark_shadowed_expr(cond, &aliases, shadowed);
            if let Some(step) = step {
                mark_shadowed_stmt(step, &aliases, shadowed);
                collect_shadowed_stmt_scopes(step, imports, shadowed);
            }
            mark_shadowed_stmts(body, &aliases, shadowed);
            collect_shadowed_stmts(body, imports, shadowed);
        }
        Stmt::Unsafe { body, .. } | Stmt::Impure { body, .. } => {
            collect_shadowed_stmts(body, imports, shadowed);
        }
        Stmt::TaskGroup { body, .. } => {
            collect_shadowed_stmts(body, imports, shadowed);
        }
        Stmt::ContextBlock { body, .. } => {
            collect_shadowed_stmts(body, imports, shadowed);
        }
        Stmt::AssumeDet { body, .. } => {
            collect_shadowed_stmts(body, imports, shadowed);
        }
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            collect_shadowed_stmts(then_body, imports, shadowed);
            if let Some(body) = else_body {
                collect_shadowed_stmts(body, imports, shadowed);
            }
        }
        Stmt::ScopeMember { body, .. } => {
            collect_shadowed_stmts(body, imports, shadowed);
        }
        _ => {}
    }
}

fn import_alias_root(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(name, _) => Some(name),
        Expr::Paren(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _, _) => import_alias_root(inner),
        Expr::OrFallback { value, .. } => import_alias_root(value),
        Expr::Index { base, .. }
        | Expr::Slice { base, .. }
        | Expr::Field(base, ..)
        | Expr::OptField { base, .. } => import_alias_root(base),
        Expr::MethodCall { receiver, .. } => import_alias_root(receiver),
        Expr::CallValue { callee, .. } => import_alias_root(callee),
        Expr::PtrFromAddr { alias, .. } => Some(alias.as_str()),
        Expr::StructLit {
            import_ns: Some(alias),
            ..
        } => Some(alias.as_str()),
        _ => None,
    }
}

fn find_function_by_span<'a>(
    items: &'a [crate::AST::Item],
    span: Span,
) -> Option<&'a crate::AST::Func> {
    for item in items {
        let function = match item {
            crate::AST::Item::Func(function) if function.span == span => Some(function),
            crate::AST::Item::Struct(definition) => definition
                .methods
                .iter()
                .find(|function| function.span == span)
                .or_else(|| {
                    definition
                        .trait_impls
                        .iter()
                        .flat_map(|implementation| implementation.methods.iter())
                        .find(|function| function.span == span)
                }),
            crate::AST::Item::Enum(definition) => definition
                .methods
                .iter()
                .find(|function| function.span == span)
                .or_else(|| {
                    definition
                        .trait_impls
                        .iter()
                        .flat_map(|implementation| implementation.methods.iter())
                        .find(|function| function.span == span)
                }),
            crate::AST::Item::Impl(implementation) => implementation
                .methods
                .iter()
                .find(|function| function.span == span),
            crate::AST::Item::CodeModule(module) => module
                .body
                .as_deref()
                .and_then(|body| find_function_by_span(body, span)),
            _ => None,
        };
        if function.is_some() {
            return function;
        }
    }
    None
}

fn binding_by_span<'a>(body: &'a [Stmt], span: Span) -> Option<&'a crate::AST::Binding> {
    for statement in body {
        let binding = match statement {
            Stmt::Val(binding) => Some(binding),
            _ => None,
        };
        if binding.is_some_and(|binding| binding.name_span == span) {
            return binding;
        }
        for nested in crate::Sema::UnsafeObligations::nested_bodies(statement) {
            if let Some(binding) = binding_by_span(nested, span) {
                return Some(binding);
            }
        }
    }
    None
}

fn literal_without_effects(expr: &Expr) -> bool {
    match expr.without_parens() {
        Expr::Str(parts, _) => parts.iter().all(|part| matches!(part, StrPart::Lit(_))),
        Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Char(..) | Expr::Absent(..) => {
            true
        }
        _ => false,
    }
}
