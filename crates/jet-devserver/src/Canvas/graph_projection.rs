use std::collections::BTreeMap;
use std::path::Path;

use jet_driver::AST::{self, Expr, Item, Stmt};
use jet_driver::Diagnostics::Span;
use jet_semindex::{SemIndex, SemIndexEffectFacts, SourceSpan, SymbolKind};

use super::graph_helpers::{
    assignment_title, binding_type, call_has_effects, call_ret, effect_badges, expr_title,
    expr_type, graph_id, insert_offset, lvalue_type, pure_leaf, snippet,
    span_through_closing_parens, starts_uppercase, text_matches, wire_ident_refs,
};
use super::graph_json::{
    add_arm_pin, add_execution_overlay, add_inline, add_node, add_pin, add_region,
    add_source_comment_regions, add_wire, add_wire_with_span, graph_to_json, meta_attr_json,
    node_catalog, set_pin_append, set_pin_source_span,
};
use super::schema_api::{
    CanvasCallableExport, GRAPH_SCHEMA_VERSION, GraphBuilder, GraphEditAnchor, InlineExpr,
    NodeQueryRef, NodeRec, PinRec, Projection, source_revision,
};
use super::validation_json::{json_str, span_json};

pub(super) fn project_checked(
    path: &Path,
    source_id: &str,
    src: &str,
    bundle: &AST::ProgramBundle,
    facts: &SemIndexEffectFacts,
    package_facts: Option<jet_driver::Package::PackageFacts>,
    workspace_overlay_policy: Option<jet_env_model::Overlay::OverlayPolicy>,
    runtime_events: Option<&str>,
) -> Projection {
    let mut index = jet_semindex::from_checked(bundle, facts);
    if let Some(package) = package_facts {
        index.attach_package_facts(package);
    }
    if let Some(policy) = workspace_overlay_policy {
        index.attach_workspace_overlay_policy(policy);
    }
    let mut graph_json = Vec::new();
    let mut inline_spans = Vec::new();
    let mut anchors = Vec::new();
    let mut node_refs = Vec::new();
    let callable_exports = canvas_callable_exports(bundle, facts);
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        collect_item_graphs(
            path,
            src,
            &index,
            &facts.name_ledger,
            module_idx,
            bundle.entry,
            &module.display,
            &module.source,
            &module.items,
            None,
            &mut graph_json,
            &mut inline_spans,
            &mut anchors,
            &mut node_refs,
        );
    }
    let fmt = jet_driver::Formatter::format_source(src).unwrap_or_else(|_| src.to_string());
    let blueprint = canvas_blueprint_facts_json(path, src, bundle, &index, runtime_events);
    let enum_catalog = enum_catalog_json(bundle);
    let json = format!(
        "{{\"protocol\":\"jet.canvas.graph\",\"schema_version\":{},\"source_id\":{},\"revision\":{},\"fmt_fingerprint\":{},\"source_text\":{},\"node_descriptors\":{},\"graphs\":[{}],\"diagnostics\":[],\"facts\":{{\"semindex_schema_version\":{},\"handles\":[\"definitions\",\"references\",\"calls\",\"effects\",\"members\",\"outputs\"],\"enum_variants\":{},\"blueprint\":{}}}}}",
        GRAPH_SCHEMA_VERSION,
        json_str(source_id),
        json_str(&source_revision(src)),
        json_str(&source_revision(&fmt)),
        json_str(src),
        node_catalog::catalog_json(),
        graph_json.join(","),
        index.schema_version(),
        enum_catalog,
        blueprint
    );
    Projection {
        json,
        inline_exprs: inline_spans,
        graph_anchors: anchors,
        node_refs,
        callable_exports,
    }
}

/// Project the checked name ledger's callable surface for the current entry.
///
/// Entry-local private functions are callable from Canvas because the editor
/// is editing that module. Imported functions must satisfy the same visibility
/// law as source code (`pub` or package-visible within the package). Matching
/// by module path and declaration span keeps this projection tied to the
/// checked AST instead of reconstructing names from SemIndex strings.
fn canvas_callable_exports(
    bundle: &AST::ProgramBundle,
    facts: &SemIndexEffectFacts,
) -> Vec<CanvasCallableExport> {
    facts
        .name_ledger
        .declarations()
        .filter(|declaration| declaration.kind == "function")
        .filter(|declaration| {
            bundle
                .modules
                .get(declaration.module)
                .is_some_and(|module| !is_foreign_module(&module.display))
        })
        .filter(|declaration| {
            facts
                .name_ledger
                .visible(bundle.entry, declaration.module, &declaration.name)
        })
        .filter_map(|declaration| {
            bundle
                .modules
                .get(declaration.module)
                .map(|module| CanvasCallableExport {
                    module_path: module.display.clone(),
                    span: declaration.span.into(),
                    callee: canvas_callable_callee(bundle, declaration),
                })
        })
        .collect()
}

fn is_foreign_module(display: &str) -> bool {
    display
        .split_once('.')
        .and_then(|(root, _)| AST::ForeignLanguage::from_root(root))
        .is_some()
}

fn canvas_callable_callee(
    bundle: &AST::ProgramBundle,
    declaration: &jet_foundation::Names::NameDeclaration,
) -> String {
    let leaf = declaration
        .name
        .rsplit_once('.')
        .map_or(declaration.name.as_str(), |(_, leaf)| leaf);
    if declaration.module == bundle.entry {
        return declaration.name.clone();
    }

    if let Some(alias) = bundle.name_ledger.aliases().find(|alias| {
        alias.module == bundle.entry
            && alias.target_module == Some(declaration.module)
            && alias
                .target
                .rsplit_once('.')
                .map_or(alias.target.as_str(), |(_, target_leaf)| target_leaf)
                == leaf
    }) {
        return alias.name.clone();
    }

    let module_alias = bundle
        .modules
        .get(declaration.module)
        .map(|module| module.alias.as_str())
        .unwrap_or(leaf);
    let prefix = bundle
        .name_ledger
        .aliases()
        .find(|alias| {
            alias.module == bundle.entry
                && alias.target_module == Some(declaration.module)
                && alias.target == module_alias
        })
        .map(|alias| alias.name.as_str())
        .unwrap_or(module_alias);
    // `declaration.name` is the checked relative path inside the imported
    // module. Keep it intact: reducing it to `leaf` changes
    // `h.tools.square` into the different callee `h.square`.
    format!("{prefix}.{}", declaration.name)
}

fn enum_catalog_json(bundle: &AST::ProgramBundle) -> String {
    let mut catalog = BTreeMap::<String, Vec<String>>::new();
    for module in &bundle.modules {
        collect_enum_variants(&module.items, &mut catalog);
    }
    let entries = catalog
        .into_iter()
        .map(|(name, variants)| {
            let variants = variants
                .into_iter()
                .map(|variant| {
                    format!(
                        "{{\"name\":{},\"source\":{}}}",
                        json_str(&variant),
                        json_str(&format!("{name}.{variant}"))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{}:[{}]", json_str(&name), variants)
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{entries}}}")
}

fn collect_enum_variants(items: &[Item], catalog: &mut BTreeMap<String, Vec<String>>) {
    for item in items {
        match item {
            Item::Enum(def) => {
                let variants = def
                    .variants
                    .iter()
                    .filter(|variant| matches!(variant.payload, AST::VariantPayload::Unit))
                    .map(|variant| variant.name.clone())
                    .collect::<Vec<_>>();
                catalog.entry(def.name.clone()).or_insert(variants);
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_enum_variants(body, catalog);
                }
            }
            _ => {}
        }
    }
}

fn collect_item_graphs(
    entry_path: &Path,
    entry_src: &str,
    index: &SemIndex,
    ledger: &jet_foundation::Names::NameLedger,
    module_idx: usize,
    entry_module_idx: usize,
    module_display: &str,
    module_src: &str,
    items: &[Item],
    owner: Option<&str>,
    out: &mut Vec<String>,
    inline_spans: &mut Vec<InlineExpr>,
    anchors: &mut Vec<GraphEditAnchor>,
    node_refs: &mut Vec<NodeQueryRef>,
) {
    for item in items {
        match item {
            Item::Func(f) => {
                let graph = project_func(index, module_display, module_src, f);
                let ledger_name = owner
                    .map(|owner| jet_foundation::Names::member_name(owner, &f.name))
                    .unwrap_or_else(|| f.name.clone());
                let visibility = super::graph_json::ledger_function_visibility(
                    ledger,
                    module_idx,
                    &ledger_name,
                );
                inline_spans.extend(graph.inline_exprs.iter().map(|i| InlineExpr {
                    id: i.id.clone(),
                    span: i.span,
                }));
                if module_idx == entry_module_idx {
                    anchors.push(GraphEditAnchor {
                        graph_id: graph.graph_id.clone(),
                        insert_offset: insert_offset(entry_src, f),
                        fallible: function_is_fallible(f),
                    });
                }
                collect_node_refs(&graph, node_refs);
                out.push(graph_to_json(&graph, f, module_src, visibility));
            }
            Item::Struct(s) => {
                for method in &s.methods {
                    let graph = project_func(index, module_display, module_src, method);
                    let visibility = super::graph_json::ledger_function_visibility(
                        ledger,
                        module_idx,
                        &format!("{}.{}", s.name, method.name),
                    );
                    inline_spans.extend(graph.inline_exprs.iter().map(|i| InlineExpr {
                        id: i.id.clone(),
                        span: i.span,
                    }));
                    if module_idx == entry_module_idx {
                        anchors.push(GraphEditAnchor {
                            graph_id: graph.graph_id.clone(),
                            insert_offset: insert_offset(entry_src, method),
                            fallible: function_is_fallible(method),
                        });
                    }
                    collect_node_refs(&graph, node_refs);
                    out.push(graph_to_json(&graph, method, module_src, visibility));
                }
            }
            Item::Impl(i) => {
                for method in &i.methods {
                    let graph = project_func(index, module_display, module_src, method);
                    let visibility = super::graph_json::ledger_function_visibility(
                        ledger,
                        module_idx,
                        &format!("{}.{}", i.type_name, method.name),
                    );
                    inline_spans.extend(graph.inline_exprs.iter().map(|e| InlineExpr {
                        id: e.id.clone(),
                        span: e.span,
                    }));
                    if module_idx == entry_module_idx {
                        anchors.push(GraphEditAnchor {
                            graph_id: graph.graph_id.clone(),
                            insert_offset: insert_offset(entry_src, method),
                            fallible: function_is_fallible(method),
                        });
                    }
                    collect_node_refs(&graph, node_refs);
                    out.push(graph_to_json(&graph, method, module_src, visibility));
                }
            }
            Item::CodeModule(m) => {
                if let Some(body) = &m.body {
                    collect_item_graphs(
                        entry_path,
                        entry_src,
                        index,
                        ledger,
                        module_idx,
                        entry_module_idx,
                        module_display,
                        module_src,
                        body,
                        Some(&m.name),
                        out,
                        inline_spans,
                        anchors,
                        node_refs,
                    );
                }
            }
            _ => {}
        }
    }
    let _ = entry_path;
}

fn function_is_fallible(f: &AST::Func) -> bool {
    matches!(f.return_type.as_ref(), Some(AST::Type::Result { .. }))
}

fn canvas_blueprint_facts_json(
    path: &Path,
    src: &str,
    bundle: &AST::ProgramBundle,
    index: &SemIndex,
    runtime_events: Option<&str>,
) -> String {
    let mut interfaces = Vec::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        let items = module.items.iter().collect::<Vec<_>>();
        collect_interface_facts(
            &items,
            &mut interfaces,
            &[],
            &module.source,
            module_idx,
            &module.alias,
            &bundle.name_ledger,
        );
    }
    let task_flows = task_flow_facts(src).join(",");
    let outputs = index.outputs().iter().map(output_fact_json).collect::<Vec<_>>().join(",");
    let package_facts = index
        .package_facts()
        .map(jet_semindex::package_facts_json)
        .unwrap_or_else(|| "null".to_string());
    let workspace_overlays = index
        .workspace_overlay_policy()
        .map(jet_semindex::workspace_overlay_policy_json)
        .unwrap_or_else(|| "null".to_string());
    let event_dispatchers = event_dispatcher_facts(path, src, bundle, index).join(",");
    format!(
        "{{\"runtime_events\":{},\"event_dispatchers\":[{}],\"interfaces\":[{}],\"task_flows\":[{}],\"outputs\":[{}],\"package_facts\":{},\"workspace_overlays\":{},\"source_truth\":\"ordinary_jet_source\"}}",
        runtime_events.unwrap_or("null"),
        event_dispatchers,
        interfaces.join(","),
        task_flows,
        outputs,
        package_facts,
        workspace_overlays,
    )
}

fn output_fact_json(output: &jet_semindex::OutputFact) -> String {
    let effects = output.entry.effects.iter().map(|effect| json_str(effect)).collect::<Vec<_>>().join(",");
    let params = output.entry.params.iter().map(|param| json_str(param)).collect::<Vec<_>>().join(",");
    format!(
        "{{\"binding\":{},\"kind\":{},\"name\":{},\"entry\":{{\"identity\":{},\"name\":{},\"module_path\":{},\"definition_span\":{},\"reference_span\":{},\"params\":[{}],\"return_type\":{},\"authority\":{},\"effects\":[{}]}},\"fact_source\":\"semindex_resolved_output\"}}",
        json_str(&output.binding), json_str(&output.kind), json_str(&output.name),
        json_str(&output.entry.identity), json_str(&output.entry.name),
        json_str(&output.entry.module_path), span_json(output.entry.definition_span),
        span_json(output.entry.reference_span), params,
        output.entry.return_type.as_deref().map(json_str).unwrap_or_else(|| "null".to_string()),
        json_str(&output.entry.authority), effects,
    )
}

fn collect_interface_facts(
    items: &[&Item],
    out: &mut Vec<String>,
    scope: &[String],
    src: &str,
    module_idx: usize,
    module_alias: &str,
    name_ledger: &jet_foundation::Names::NameLedger,
) {
    for item in items {
        match item {
            Item::Trait(t) => {
                let trait_path = source_item_path(
                    name_ledger,
                    module_idx,
                    module_alias,
                    &t.name,
                    t.name_span,
                    scope,
                );
                out.push(trait_fact_json(src, t, &trait_path));
            }
            Item::Impl(i) if i.trait_name.is_some() && !i.is_generated_serde => {
                let type_path = source_item_path(
                    name_ledger,
                    module_idx,
                    module_alias,
                    &i.type_name,
                    i.type_span,
                    scope,
                );
                let trait_name = i.trait_name.as_deref().unwrap_or("");
                let trait_path = source_item_path(
                    name_ledger,
                    module_idx,
                    module_alias,
                    trait_name,
                    i.trait_span.unwrap_or(i.type_span),
                    scope,
                );
                out.push(impl_fact_json(i, &type_path, &trait_path));
            }
            Item::Struct(s) => {
                for block in s
                    .trait_impls
                    .iter()
                    .filter(|block| !block.compiler_generated)
                {
                    let type_path = source_item_path(
                        name_ledger,
                        module_idx,
                        module_alias,
                        &s.name,
                        s.name_span,
                        scope,
                    );
                    let trait_path = source_item_path(
                        name_ledger,
                        module_idx,
                        module_alias,
                        &block.trait_name,
                        block.trait_span,
                        scope,
                    );
                    out.push(inline_trait_impl_fact_json(
                        &type_path,
                        &trait_path,
                        block,
                    ));
                }
            }
            Item::Enum(e) => {
                for block in e
                    .trait_impls
                    .iter()
                    .filter(|block| !block.compiler_generated)
                {
                    let type_path = source_item_path(
                        name_ledger,
                        module_idx,
                        module_alias,
                        &e.name,
                        e.name_span,
                        scope,
                    );
                    let trait_path = source_item_path(
                        name_ledger,
                        module_idx,
                        module_alias,
                        &block.trait_name,
                        block.trait_span,
                        scope,
                    );
                    out.push(inline_trait_impl_fact_json(
                        &type_path,
                        &trait_path,
                        block,
                    ));
                }
            }
            Item::CodeModule(m) => {
                if let Some(body) = &m.body {
                    let nested = body.iter().collect::<Vec<_>>();
                    let mut nested_scope = scope.to_vec();
                    nested_scope.push(m.name.clone());
                    collect_interface_facts(
                        &nested,
                        out,
                        &nested_scope,
                        src,
                        module_idx,
                        module_alias,
                        name_ledger,
                    );
                }
            }
            _ => {}
        }
    }
}

fn source_item_path(
    name_ledger: &jet_foundation::Names::NameLedger,
    module_idx: usize,
    module_alias: &str,
    internal_name: &str,
    span: Span,
    fallback_scope: &[String],
) -> String {
    for candidate in [
        format!("{module_alias}.{internal_name}"),
        if fallback_scope.is_empty() {
            format!("{module_alias}.{internal_name}")
        } else {
            format!("{module_alias}.{}.{}", fallback_scope.join("."), internal_name)
        },
    ] {
        if let Some(path) = name_ledger.display_path(module_idx, &candidate, Some(module_idx)) {
            return strip_module_alias(&path, module_alias);
        }
    }
    if let Some(path) = name_ledger.canonical_path_at(module_idx, span.start, span.end) {
        return strip_module_alias(&path, module_alias);
    }
    let fallback = if fallback_scope.is_empty() {
        internal_name.to_string()
    } else if internal_name.contains('.') {
        internal_name.to_string()
    } else {
        format!("{}.{}", fallback_scope.join("."), internal_name)
    };
    strip_module_alias(&fallback, module_alias)
}

fn strip_module_alias(path: &str, module_alias: &str) -> String {
    path.strip_prefix(module_alias)
        .and_then(|rest| rest.strip_prefix('.'))
        .unwrap_or(path)
        .to_string()
}

fn trait_fact_json(src: &str, t: &AST::TraitDef, display_path: &str) -> String {
    let (scope, trait_name) = display_path
        .rsplit_once('.')
        .map_or(("", display_path), |(scope, name)| (scope, name));
    let associated_types = t
        .assoc_types
        .iter()
        .map(|(name, _)| json_str(name))
        .collect::<Vec<_>>()
        .join(",");
    let methods = t
        .methods
        .iter()
        .map(|m| {
            let effects = m
                .declared_effects
                .as_ref()
                .map(|effects| {
                    effects
                        .iter()
                        .map(|(name, _)| json_str(name))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            format!(
                "{{\"name\":{},\"signature\":{},\"required\":{},\"default\":{},\"pure\":{},\"effects\":[{}],\"source_span\":{}}}",
                json_str(&m.name),
                json_str(&trait_method_signature(src, m)),
                if m.default_body.is_none() { "true" } else { "false" },
                if m.default_body.is_some() { "true" } else { "false" },
                if m.is_pure { "true" } else { "false" },
                effects,
                span_json(m.span.into())
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"trait_interface\",\"trait\":{},\"scope\":{},\"associated_types\":[{}],\"methods\":[{}],\"source_span\":{},\"authoring\":[\"create_trait_impl\",\"jump_trait_to_impls\",\"palette_trait_methods\"]}}",
        json_str(trait_name),
        json_str(scope),
        associated_types,
        methods,
        span_json(t.name_span.into())
    )
}

fn impl_fact_json(i: &AST::ImplDef, type_path: &str, trait_path: &str) -> String {
    let methods = i
        .methods
        .iter()
        .map(|m| json_str(&m.name))
        .collect::<Vec<_>>()
        .join(",");
    let associated_types = i
        .assoc_type_impls
        .iter()
        .map(|(name, _, _)| json_str(name))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"trait_impl\",\"type\":{},\"trait\":{},\"scope\":{},\"methods\":[{}],\"associated_types\":[{}],\"delegation_field\":{},\"source_span\":{},\"diagnostic_affordance\":\"surface_missing_trait_members\"}}",
        json_str(type_path),
        json_str(trait_path.rsplit_once('.').map_or(trait_path, |(_, name)| name)),
        json_str(trait_path.rsplit_once('.').map_or("", |(scope, _)| scope)),
        methods,
        associated_types,
        i.delegation_field
            .as_deref()
            .map(json_str)
            .unwrap_or_else(|| "null".to_string()),
        span_json(i.type_span.into())
    )
}

fn inline_trait_impl_fact_json(
    type_path: &str,
    trait_path: &str,
    block: &AST::TraitImplBlock,
) -> String {
    let methods = block
        .methods
        .iter()
        .map(|m| json_str(&m.name))
        .collect::<Vec<_>>()
        .join(",");
    let associated_types = block
        .assoc_type_impls
        .iter()
        .map(|(name, _, _)| json_str(name))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"trait_impl\",\"type\":{},\"trait\":{},\"scope\":{},\"methods\":[{}],\"associated_types\":[{}],\"delegation_field\":null,\"source_span\":{},\"diagnostic_affordance\":\"surface_missing_trait_members\"}}",
        json_str(type_path),
        json_str(trait_path.rsplit_once('.').map_or(trait_path, |(_, name)| name)),
        json_str(trait_path.rsplit_once('.').map_or("", |(scope, _)| scope)),
        methods,
        associated_types,
        span_json(block.trait_span.into())
    )
}

pub(super) fn trait_method_signature(src: &str, m: &AST::TraitMethodSig) -> String {
    let params = m
        .params
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            let mut parts = Vec::new();
            if p.zone == AST::ParamZone::LabelOnly
                && !m.params[..i]
                    .iter()
                    .any(|previous| previous.zone == AST::ParamZone::LabelOnly)
            {
                parts.push("*".to_string());
            }
            parts.push(trait_param_signature(src, p));
            if p.zone == AST::ParamZone::PositionalOnly
                && m.params
                    .get(i + 1)
                    .is_none_or(|next| next.zone != AST::ParamZone::PositionalOnly)
            {
                parts.push("/".to_string());
            }
            parts
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = m
        .return_type
        .as_ref()
        .map(|t| format!(" {}", t.name()))
        .unwrap_or_default();
    let provenance = trait_return_view_from(m, &m.params);
    let effects = if let Some(row) = &m.declared_effects {
        format!(
            " -[{}]>",
            row.iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else if m.is_pure {
        " -[]>".to_string()
    } else {
        String::new()
    };
    format!("fn {}({}){}{}{}", m.name, params, ret, provenance, effects)
}

fn event_dispatcher_facts(
    path: &Path,
    source: &str,
    bundle: &AST::ProgramBundle,
    index: &SemIndex,
) -> Vec<String> {
    let path_text = path.to_string_lossy();
    let Some(module) = bundle
        .modules
        .iter()
        .find(|module| module.display == path_text.as_ref())
        .or_else(|| bundle.modules.iter().find(|module| module.source == source))
    else {
        return Vec::new();
    };
    let src = module.source.as_str();
    index
        .call_edges()
        .iter()
        .filter(|call| call.module_path == module.display)
        .filter_map(|call| {
            let aliases = event_module_aliases(src);
            let call_source_span = event_call_span(src, call.call_span).unwrap_or(call.call_span);
            let source = src
                .get(call_source_span.start..call_source_span.end)
                .unwrap_or(&call.callee);
            let constructor = event_constructor_kind(
                src,
                call.call_span,
                call.callee.as_str(),
                source,
                &aliases,
            );
            let receiver = event_receiver(src, index, call.call_span, &module.display);
            let kind = constructor.or_else(|| {
                let receiver_type = receiver.as_ref().map(|(_, ty, _)| ty.as_str())?;
                event_method_kind(receiver_type, call.callee.as_str())
            })?;
            let scope = event_scope_argument(index, call_source_span, &module.display);
            let receiver_type = constructor_type(call.callee.as_str(), source)
                .or_else(|| receiver.as_ref().map(|(_, ty, _)| ty.clone()));
            let receiver_name = receiver.as_ref().map(|(name, _, _)| name.as_str());
            let receiver_span = receiver.as_ref().map(|(_, _, span)| span_json(*span));
            let scope_name = scope.as_ref().map(|(name, _)| name.as_str());
            let scope_span = scope.as_ref().map(|(_, span)| span_json(*span));
            Some(format!(
                "{{\"kind\":{},\"source\":{},\"source_span\":{},\"receiver\":{},\"receiver_source_span\":{},\"receiver_type\":{},\"scope\":{},\"scope_source_span\":{},\"fact_source\":\"semindex_checked_call\",\"lifetime\":\"EventScope-owned\",\"observables\":[\"listener_count\",\"blocked_count\",\"queued_count\",\"running_count\",\"DispatchReport.trace\"],\"semantics\":\"core.event_source_truth\"}}",
                json_str(kind),
                json_str(source),
                span_json(call_source_span),
                receiver_name.map(json_str).unwrap_or_else(|| "null".to_string()),
                receiver_span.unwrap_or_else(|| "null".to_string()),
                receiver_type
                    .as_deref()
                    .map(json_str)
                    .unwrap_or_else(|| "null".to_string()),
                scope_name.map(json_str).unwrap_or_else(|| "null".to_string()),
                scope_span.unwrap_or_else(|| "null".to_string()),
            ))
        })
        .collect()
}

fn event_module_aliases(src: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    for line in src.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        let Some(rest) = line.strip_prefix("use core.") else {
            continue;
        };
        let full = format!("core.{rest}");
        let (module, alias) = full
            .split_once(" as ")
            .map(|(module, alias)| (module.trim(), alias.trim()))
            .unwrap_or_else(|| {
                let module = full.trim();
                (module, module.rsplit('.').next().unwrap_or(""))
            });
        if module == "core.event" && !alias.is_empty() {
            aliases.push(alias.to_string());
        }
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

fn event_constructor_kind(
    src: &str,
    call_span: SourceSpan,
    callee: &str,
    _source: &str,
    aliases: &[String],
) -> Option<&'static str> {
    let before = src.get(..call_span.start.min(src.len()))?.trim_end();
    if !aliases.iter().any(|alias| before.ends_with(&format!("{alias}."))) {
        return None;
    }
    match callee {
        "new" => Some("event_stream_create"),
        "with_policy" => Some("event_stream_create_with_policy"),
        "async_result" => Some("async_event_create"),
        "hook" => Some("hook_create"),
        "decision_hook" => Some("decision_hook_create"),
        "scope" => Some("event_scope_create"),
        "policy_sync" => Some("event_policy_sync"),
        _ => None,
    }
}

fn constructor_type(callee: &str, source: &str) -> Option<String> {
    let type_args = source
        .find('<')
        .and_then(|start| source[start + 1..].find('>').map(|end| &source[start + 1..start + 1 + end]))
        .map(str::trim)
        .filter(|args| !args.is_empty());
    match callee {
        "new" | "with_policy" => type_args.map(|args| format!("Event<{args}>")),
        "async_result" => type_args.map(|args| format!("AsyncEvent<{args}>")),
        "hook" => type_args.map(|args| format!("Hook<{args}>")),
        "decision_hook" => type_args.map(|args| format!("DecisionHook<{args}>")),
        "scope" => Some("EventScope".to_string()),
        "policy_sync" => Some("EventPolicy".to_string()),
        _ => None,
    }
}

fn event_call_span(src: &str, callee_span: SourceSpan) -> Option<SourceSpan> {
    let tail = src.get(callee_span.end..)?;
    let open_offset = tail.find('(')?;
    let open = callee_span.end + open_offset;
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, byte) in bytes.iter().enumerate().skip(open) {
        let ch = *byte as char;
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        if ch == '"' {
            quoted = true;
            continue;
        }
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(SourceSpan {
                    start: callee_span.start,
                    end: offset + 1,
                });
            }
        }
    }
    None
}

fn event_receiver(
    src: &str,
    index: &SemIndex,
    call_span: SourceSpan,
    module_path: &str,
) -> Option<(String, String, SourceSpan)> {
    let reference = index
        .references()
        .iter()
        .filter(|reference| reference.module_path == module_path)
        .filter(|reference| reference.span.end <= call_span.start)
        .filter(|reference| {
            src.get(reference.span.end..call_span.start)
                .is_some_and(|between| between.trim() == ".")
        })
        .max_by_key(|reference| reference.span.end)?;
    let target = reference.target.as_ref()?;
    let definition = index.definitions().iter().find(|definition| {
        definition.module_path == target.module_path && definition.def_span == target.def_span
    })?;
    let ty = match &definition.kind {
        SymbolKind::Local { ty: Some(ty), .. } | SymbolKind::Param { ty } => ty.clone(),
        _ => return None,
    };
    Some((reference.name.clone(), ty, reference.span))
}

fn event_scope_argument(
    index: &SemIndex,
    call_span: SourceSpan,
    module_path: &str,
) -> Option<(String, SourceSpan)> {
    index
        .references()
        .iter()
        .filter(|reference| reference.module_path == module_path)
        .filter(|reference| {
            reference.span.start >= call_span.start && reference.span.end <= call_span.end
        })
        .filter_map(|reference| {
            let target = reference.target.as_ref()?;
            let definition = index.definitions().iter().find(|definition| {
                definition.module_path == target.module_path && definition.def_span == target.def_span
            })?;
            matches!(&definition.kind, SymbolKind::Local { ty: Some(ty), .. } if ty == "EventScope")
                .then(|| (reference.name.clone(), reference.span))
        })
        .min_by_key(|(_, span)| span.start)
}

fn event_method_kind(receiver_type: &str, method: &str) -> Option<&'static str> {
    let kind = match (receiver_type, method) {
        (ty, "on") if is_event_handler_type(ty) => "event_subscribe",
        (ty, "once") if is_event_handler_type(ty) => "event_subscribe_once",
        (ty, "on_priority") if is_event_handler_type(ty) => "event_subscribe_priority",
        (ty, "emit") if ty.starts_with("Event<") => "event_emit",
        (ty, "emit_async") if ty.starts_with("AsyncEvent<") => "event_emit_async",
        (ty, "close") if ty.starts_with("AsyncEvent<") => "event_close",
        (ty, "listener_count") if is_event_handler_type(ty) => "event_listener_count",
        (ty, "queued_count") if ty.starts_with("AsyncEvent<") => "event_queued_count",
        (ty, "running_count") if ty.starts_with("AsyncEvent<") => "event_running_count",
        (ty, "blocked_count") if ty.starts_with("AsyncEvent<") => "event_pending_count",
        (ty, "run") if ty.starts_with("Hook<") => "hook_run",
        (ty, "run") if ty.starts_with("DecisionHook<") => "decision_hook_run",
        ("EventScope", "cancel") => "event_scope_cancel",
        ("EventScope", "active_count") => "event_scope_active_count",
        ("Subscription", "unsubscribe") => "event_unsubscribe",
        ("Subscription", "is_active") => "event_subscription_active",
        ("EventTrace", "summary") => "event_trace_summary",
        ("EventTrace", "delivered" | "queued" | "dropped") => "event_trace_count",
        (ty, "accepted") if ty.starts_with("DispatchReport<") => "event_report_accepted",
        (ty, "delivered_handlers") if ty.starts_with("DispatchReport<") => "event_report_delivered",
        (ty, "state") if ty.starts_with("DispatchReport<") => "event_report_state",
        (ty, "failures") if ty.starts_with("DispatchReport<") => "event_report_failures",
        (ty, "trace") if ty.starts_with("DispatchReport<") => "event_report_trace",
        _ => return None,
    };
    Some(kind)
}

fn is_event_handler_type(ty: &str) -> bool {
    ty.starts_with("Event<")
        || ty.starts_with("AsyncEvent<")
        || ty.starts_with("Hook<")
        || ty.starts_with("DecisionHook<")
}

fn trait_param_signature(src: &str, p: &AST::Param) -> String {
    let is_self = p.name == "self" && p.ty.name().is_empty();
    let mut out = String::new();
    if p.root {
        out.push_str("#Root ");
    }
    if is_self {
        out.push_str(p.convention.sigil());
        out.push_str(&p.name);
        return out;
    }
    if let Some((label, _)) = &p.public_label {
        out.push_str(label);
        out.push(' ');
    }
    out.push_str(&p.name);
    out.push_str(": ");
    out.push_str(p.convention.sigil());
    if p.variadic {
        out.push_str("...");
    }
    if let Some(bounds) = &p.variadic_bound_list {
        out.push('[');
        out.push_str(&bounds.join(", "));
        out.push(']');
    } else {
        out.push_str(&p.ty.name());
    }
    if let Some(names) = &p.declared_view_from_names {
        if !names.is_empty() {
            out.push_str(" from ");
            out.push_str(&names.join(" | "));
        }
    }
    if let Some(default) = &p.default {
        out.push('{');
        out.push_str(&snippet(src, default.span()));
        out.push('}');
    }
    out
}

fn trait_return_view_from(
    m: &AST::TraitMethodSig,
    params: &[AST::Param],
) -> String {
    let Some(map) = &m.declared_return_view_provenance else {
        return String::new();
    };
    if map.is_empty() {
        return String::new();
    }
    let source_union = |provenance: &AST::ViewProvenance| {
        provenance
            .sources
            .iter()
            .map(|path| {
                let mut source = match &path.source {
                    AST::ViewSource::Receiver => "self".to_string(),
                    AST::ViewSource::Parameter(index) => params
                        .iter()
                        .filter(|param| param.name != "self")
                        .nth(*index)
                        .map(|param| param.name.clone())
                        .unwrap_or_else(|| format!("param{index}")),
                    AST::ViewSource::Static { module_path, name } => {
                        if module_path.is_empty() {
                            format!("static.{name}")
                        } else {
                            format!("static.{module_path}.{name}")
                        }
                    }
                };
                for projection in &path.projections {
                    if let AST::ViewSourceProjection::Field(field) = projection {
                        source.push('.');
                        source.push_str(field);
                    }
                }
                source
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let body = if map.len() == 1 {
        let (path, provenance) = map.iter().next().expect("non-empty view map");
        if path.is_empty() {
            source_union(provenance)
        } else {
            format!("{}: {}", path.join("."), source_union(provenance))
        }
    } else {
        map.iter()
            .map(|(path, provenance)| {
                format!("{}: {}", path.join("."), source_union(provenance))
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    if map.len() == 1 && map.iter().next().is_some_and(|(path, _)| path.is_empty()) {
        format!(" from {body}")
    } else {
        format!(" from ({body})")
    }
}

fn task_flow_facts(src: &str) -> Vec<String> {
    let mut facts = Vec::new();
    // D-CONC-SPAWN1=D: `task.group g { … }` / bare `task …` /
    // `task.all|race|any { … }` are the canonical task surface.
    // "task " (trailing space) catches the bare spawn keyword without
    // matching the qualified `task.group`/`task.all`/`task.race`/`task.any`
    // forms, which are always `task` immediately followed by `.`.
    for (needle, kind) in [
        ("task.group", "structured_task_scope"),
        ("task ", "task_spawn"),
        ("task.all", "task_all"),
        ("task.race", "task_race"),
        ("task.any", "task_any"),
        (".join(", "join_task"),
        ("channel", "channel_create"),
        (".send(", "channel_send"),
        (".receive(", "channel_receive"),
        ("#Context", "deadline_context"),
    ] {
        for span in text_matches(src, needle) {
            facts.push(format!(
                "{{\"kind\":{},\"source\":{},\"source_span\":{},\"rail\":\"async\",\"semantics\":\"core.tasks_source_truth\"}}",
                json_str(kind),
                json_str(needle),
                span_json(span)
            ));
        }
    }
    facts
}

fn project_func(
    index: &SemIndex,
    module_display: &str,
    module_src: &str,
    f: &AST::Func,
) -> GraphBuilder {
    let graph_id = graph_id(module_display, f);
    let mut g = GraphBuilder {
        graph_id: graph_id.clone(),
        ..GraphBuilder::default()
    };
    let entry_id = format!("{graph_id}:entry");
    let entry_descriptor = node_catalog::descriptor_for_id("entry");
    g.nodes.push(NodeRec {
        id: entry_id.clone(),
        kind: entry_descriptor.kind.to_string(),
        archetype: entry_descriptor.archetype.to_string(),
        title: f.name.clone(),
        span: f.name_span.into(),
        x: 40,
        y: 40,
        badges: effect_badges(index, &f.name)
            .into_iter()
            .map(str::to_string)
            .collect(),
        affordances: vec![
            "source_jump".to_string(),
            "insert_call".to_string(),
            "rename_function".to_string(),
            "edit_function_signature".to_string(),
            "create_function".to_string(),
        ],
        meta_json: meta_attr_json(f.meta.as_ref()),
    });
    for (i, p) in f.params.iter().enumerate() {
        let pin_id = format!("{entry_id}:out:{}", p.name);
        let ty = p.ty.name();
        g.local_pins.insert(p.name.clone(), pin_id.clone());
        g.local_types.insert(p.name.clone(), ty.clone());
        g.pins.push(PinRec {
            id: pin_id,
            node_id: entry_id.clone(),
            name: p.name.clone(),
            direction: "output".to_string(),
            ty,
            role: None,
            pattern_source: None,
            ability: p.convention.sigil().to_string(),
            fallible: false,
            effect_grant_need: None,
            span: p.name_span.into(),
            pattern_source_span: None,
            append_op: None,
            element_index: None,
        });
        let _ = i;
    }
    for def in index.definitions() {
        match &def.kind {
            SymbolKind::Local { ty, .. } => {
                if let Some(t) = ty {
                    g.local_types.insert(def.name.clone(), t.clone());
                }
            }
            SymbolKind::Param { ty } => {
                g.local_types.insert(def.name.clone(), ty.clone());
            }
            _ => {}
        }
    }
    project_stmt_block(&mut g, index, module_src, &f.body, 0, 220, 170);
    add_source_comment_regions(&mut g, module_src, f);
    add_execution_overlay(&mut g, module_src, &f.body);
    g
}

fn collect_node_refs(graph: &GraphBuilder, out: &mut Vec<NodeQueryRef>) {
    for node in &graph.nodes {
        out.push(NodeQueryRef {
            graph_id: graph.graph_id.clone(),
            node_id: node.id.clone(),
            kind: node.kind.clone(),
            title: node.title.clone(),
            span: node.span,
        });
    }
    for inline in &graph.inline_exprs {
        out.push(NodeQueryRef {
            graph_id: graph.graph_id.clone(),
            node_id: inline.node_id.clone(),
            kind: format!("inline:{}", inline.role),
            title: inline.source.clone(),
            span: inline.span,
        });
    }
}

fn project_stmt_block(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    stmts: &[Stmt],
    base: usize,
    x: i32,
    y: i32,
) {
    let mut cursor_y = y;
    for (i, stmt) in stmts.iter().enumerate() {
        project_stmt(g, index, src, stmt, base + i + 1, x, cursor_y);
        cursor_y += stmt_row_step(stmt);
    }
}

/// Vertical spacing before the next sibling statement. The default slot
/// (130px) has room for a couple of data-provider rows below a binding; a
/// list initializer with more items renders taller than that, so
/// widen the gap or its node body collides with the next statement's own
/// data-provider node (multi-input node body height grows with item count,
/// e.g. after `append_multi_input`).
fn stmt_row_step(stmt: &Stmt) -> i32 {
    let items = match stmt {
        Stmt::Val(b) => multi_input_item_count(&b.init),
        Stmt::Assign { value, .. } => multi_input_item_count(value),
        _ => 0,
    };
    130 + (items.saturating_sub(2) as i32) * 55
}

fn multi_input_item_count(expr: &Expr) -> usize {
    match expr {
        Expr::ListLit(items, _) => items.len(),
        _ => 0,
    }
}

fn project_stmt(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    stmt: &Stmt,
    ordinal: usize,
    x: i32,
    y: i32,
) {
    match stmt {
        Stmt::Val(b) => {
            let node_id = format!("{}:stmt:{ordinal}:binding", g.graph_id);
            let ty = binding_type(g, &b.name, b);
            add_node(
                g,
                &node_id,
                "binding",
                &b.name,
                b.name_span.into(),
                x,
                y,
                vec!["local"],
                vec!["rename_binding", "edit_inline_expr", "source_jump"],
            );
            if let Some(node) = g.nodes.iter_mut().find(|node| node.id == node_id) {
                node.meta_json = meta_attr_json(b.meta.as_ref());
            }
            let input_pin = add_pin(g, &node_id, "value", "input", &ty, "", false);
            let output_pin = add_pin(g, &node_id, &b.name, "output", &ty, "", false);
            g.local_pins.insert(b.name.clone(), output_pin);
            g.local_types.insert(b.name.clone(), ty);
            let init_span = if b.pattern.is_none() {
                binding_init_span(src, b)
            } else {
                b.init.span()
            };
            connect_expr_to_input_with_span(
                g,
                index,
                src,
                &b.init,
                init_span,
                ordinal,
                "init",
                &node_id,
                &input_pin,
                x - 220,
                y,
            );
        }
        Stmt::Assign {
            target, op, value, ..
        } => {
            let node_id = format!("{}:stmt:{ordinal}:assign", g.graph_id);
            let title = assignment_title(target, *op);
            let ty = lvalue_type(g, target);
            add_node(
                g,
                &node_id,
                "assignment",
                &title,
                target.span().into(),
                x,
                y,
                vec!["write"],
                vec!["edit_inline_expr", "source_jump"],
            );
            let input = add_pin(g, &node_id, "value", "input", &ty, "", false);
            let output = add_pin(g, &node_id, "target", "output", &ty, "&", false);
            connect_expr_to_input(
                g,
                index,
                src,
                value,
                ordinal,
                "value",
                &node_id,
                &input,
                x - 220,
                y,
            );
            if let AST::LValue::Local { name, .. } = target {
                g.local_pins.insert(name.clone(), output);
                g.local_types.insert(name.clone(), ty);
            }
        }
        Stmt::Expr(e) => {
            let _ = project_expr_node(g, index, src, e, ordinal, x, y, true);
        }
        Stmt::DeferClose { close, span } => {
            let node_id = format!("{}:stmt:{ordinal}:defer_close", g.graph_id);
            add_node(
                g,
                &node_id,
                "defer_close",
                "defer close",
                (*span).into(),
                x,
                y,
                vec!["cleanup"],
                vec!["source_jump"],
            );
            add_inline(g, &node_id, ordinal, "close", src, close.span());
        }
        Stmt::Return(expr, span) => {
            let node_id = format!("{}:stmt:{ordinal}:return", g.graph_id);
            add_node(
                g,
                &node_id,
                "return",
                "return",
                (*span).into(),
                x,
                y,
                vec!["exit"],
                vec!["source_jump"],
            );
            if let Some(e) = expr {
                let ty = expr_type(g, index, e);
                let input = add_pin(g, &node_id, "value", "input", &ty, "", false);
                connect_expr_to_input(
                    g,
                    index,
                    src,
                    e,
                    ordinal,
                    "value",
                    &node_id,
                    &input,
                    x - 220,
                    y,
                );
            }
        }
        Stmt::While {
            cond, body, span, ..
        } => {
            let node_id = format!("{}:stmt:{ordinal}:loop", g.graph_id);
            add_node(
                g,
                &node_id,
                "loop",
                "loop",
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["edit_inline_expr", "source_jump"],
            );
            add_inline(g, &node_id, ordinal, "cond", src, cond.span());
            project_stmt_block(g, index, src, body, ordinal * 100 + 15, x + 230, y + 70);
        }
        Stmt::Loop { body, span, .. } => {
            let node_id = format!("{}:stmt:{ordinal}:loop", g.graph_id);
            add_node(
                g,
                &node_id,
                "loop",
                "loop",
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["source_jump"],
            );
            project_stmt_block(g, index, src, body, ordinal * 100 + 20, x + 230, y + 70);
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            span,
            ..
        } => {
            let node_id = format!("{}:stmt:{ordinal}:counted_loop", g.graph_id);
            add_node(
                g,
                &node_id,
                "loop",
                "counted loop",
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["add_pattern_arm", "edit_inline_expr", "source_jump"],
            );
            let initializer_span = Span::new(init.name_span.start, init.init.span().end);
            let init_pin = add_pin(g, &node_id, "initializer", "input", "Value", "", false);
            connect_expr_to_input_with_span(g, index, src, &init.init, initializer_span, ordinal * 10, "initializer", &node_id, &init_pin, x - 220, y);
            let cond_pin = add_pin(g, &node_id, "condition", "input", "Bool", "", false);
            connect_expr_to_input(g, index, src, cond, ordinal * 10 + 1, "condition", &node_id, &cond_pin, x - 220, y + 30);
            add_inline(g, &node_id, ordinal, "cond", src, cond.span());
            if let Some(step) = step {
                let afterthought = match step.as_ref() {
                    Stmt::Assign { target, value, .. } => {
                        Some((value, Span::new(target.span().start, expr_source_end(value))))
                    }
                    Stmt::Expr(value) => Some((value, value.span())),
                    _ => None,
                };
                if let Some((afterthought, afterthought_span)) = afterthought {
                    let pin = add_pin(g, &node_id, "afterthought", "input", "Value", "", false);
                    connect_expr_to_input_with_span(g, index, src, afterthought, afterthought_span, ordinal * 10 + 2, "afterthought", &node_id, &pin, x - 220, y + 60);
                }
            }
            project_stmt_block(g, index, src, body, ordinal * 100 + 30, x + 230, y + 200);
        }
        Stmt::For {
            var,
            var2,
            kind,
            body,
            span,
            ..
        } => {
            let node_id = format!("{}:stmt:{ordinal}:for", g.graph_id);
            add_node(
                g,
                &node_id,
                "loop",
                &format!("loop {var}"),
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["source_jump"],
            );
            let iter_ty = match kind {
                AST::ForKind::Range { .. } => "Int",
                AST::ForKind::In { .. } => "iterable",
            };
            let output = add_pin(g, &node_id, var, "output", iter_ty, "", false);
            g.local_pins.insert(var.clone(), output);
            g.local_types.insert(var.clone(), iter_ty.to_string());
            if let Some((var2, _)) = var2 {
                let output = add_pin(g, &node_id, var2, "output", iter_ty, "", false);
                g.local_pins.insert(var2.clone(), output);
                g.local_types.insert(var2.clone(), iter_ty.to_string());
            }
            match kind {
                AST::ForKind::Range { start, end, step, exclusive: _ } => {
                    let start_pin = add_pin(g, &node_id, "range_start", "input", "Int", "", false);
                    connect_expr_to_input(g, index, src, start, ordinal * 10, "range_start", &node_id, &start_pin, x - 220, y);
                    let end_pin = add_pin(g, &node_id, "range_end", "input", "Int", "", false);
                    connect_expr_to_input(g, index, src, end, ordinal * 10 + 1, "range_end", &node_id, &end_pin, x - 220, y + 30);
                    if let Some(step) = step {
                        let stride_pin = add_pin(g, &node_id, "stride", "input", "Int", "", false);
                        connect_expr_to_input(g, index, src, step, ordinal * 10 + 2, "stride", &node_id, &stride_pin, x - 220, y + 60);
                    }
                }
                AST::ForKind::In { collection, step } => {
                    let source_pin = add_pin(g, &node_id, "source", "input", "Iterable", "", false);
                    connect_expr_to_input(g, index, src, collection, ordinal * 10, "source", &node_id, &source_pin, x - 220, y);
                    if let Some(step) = step {
                        let stride_pin = add_pin(g, &node_id, "stride", "input", "Int", "", false);
                        connect_expr_to_input(g, index, src, step, ordinal * 10 + 1, "stride", &node_id, &stride_pin, x - 220, y + 30);
                    }
                }
            }
            project_stmt_block(g, index, src, body, ordinal * 100 + 40, x + 230, y + 70);
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            span,
        }
        | Stmt::ComptimeSwitch {
            subject,
            arms,
            else_body,
            span,
        } => {
            let subjectless = AST::is_subjectless_guard(subject, *span);
            if subjectless && switch_was_classic_if(src, arms, *span) {
                let Some(arm) = arms.first() else {
                    return;
                };
                let node_id = format!("{}:stmt:{ordinal}:branch", g.graph_id);
                let mut affordances = vec!["edit_inline_expr", "source_jump"];
                let is_pattern_test = matches!(arm.cond, Expr::PatternTest { .. });
                if is_pattern_test {
                    affordances.push("add_pattern_arm");
                }
                add_node(
                    g,
                    &node_id,
                    "branch",
                    if is_pattern_test { "if ==" } else { "if" },
                    (*span).into(),
                    x,
                    y,
                    vec!["control"],
                    affordances,
                );
                let cond = add_pin(g, &node_id, "cond", "input", "Bool", "", false);
                if is_pattern_test {
                    add_arm_pin(
                        g,
                        &node_id,
                        "arm1",
                        &pattern_pin_label(src, &arm.cond),
                        pattern_arm_edit_span(src, &arm.cond),
                    );
                }
                connect_expr_to_input(
                    g,
                    index,
                    src,
                    &arm.cond,
                    ordinal,
                    "cond",
                    &node_id,
                    &cond,
                    x - 220,
                    y,
                );
                project_stmt_block(
                    g,
                    index,
                    src,
                    &arm.body,
                    ordinal * 100 + 10,
                    x + 230,
                    y + 70,
                );
                project_classic_switch_else(
                    g,
                    index,
                    src,
                    else_body.as_deref(),
                    ordinal,
                    x + 460,
                    y + 70,
                );
                return;
            }
            let node_id = format!("{}:stmt:{ordinal}:dispatch", g.graph_id);
            add_node(
                g,
                &node_id,
                "dispatch",
                if subjectless { "if guards" } else { "if ==" },
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["add_pattern_arm", "edit_inline_expr", "source_jump"],
            );
            if !subjectless {
                let ty = expr_type(g, index, subject);
                let subject_pin = add_pin(g, &node_id, "subject", "input", &ty, "", false);
                connect_expr_to_input(
                    g,
                    index,
                    src,
                    subject,
                    ordinal,
                    "subject",
                    &node_id,
                    &subject_pin,
                    x - 220,
                    y,
                );
            }
            for (i, arm) in arms.iter().enumerate() {
                add_arm_pin(
                    g,
                    &node_id,
                    &format!("arm{}", i + 1),
                    &dispatch_arm_pattern_label(src, &arm.cond),
                    dispatch_arm_pattern_span(src, &arm.cond),
                );
                add_inline(
                    g,
                    &node_id,
                    ordinal,
                    &format!("arm{}", i + 1),
                    src,
                    arm.cond.span(),
                );
                project_stmt_block(
                    g,
                    index,
                    src,
                    &arm.body,
                    ordinal * 100 + 50 + i * 20,
                    x + 230 + i as i32 * 180,
                    y + 100,
                );
            }
            if let Some(body) = else_body {
                project_stmt_block(g, index, src, body, ordinal * 100 + 90, x + 460, y + 230);
            }
        }
        Stmt::Break(span)
        | Stmt::Continue(span)
        | Stmt::BreakLabel(_, span)
        | Stmt::ContinueLabel(_, span) => {
            let node_id = format!("{}:stmt:{ordinal}:flow", g.graph_id);
            let title = snippet(src, *span);
            add_node(
                g,
                &node_id,
                "flow",
                &title,
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["source_jump"],
            );
        }
        Stmt::BreakValue(value, span) | Stmt::BreakLabelValue(_, _, value, span) => {
            let node_id = format!("{}:stmt:{ordinal}:flow", g.graph_id);
            add_node(
                g,
                &node_id,
                "flow",
                "break",
                (*span).into(),
                x,
                y,
                vec!["control"],
                vec!["source_jump"],
            );
            let ty = expr_type(g, index, value);
            let input = add_pin(g, &node_id, "value", "input", &ty, "", false);
            connect_expr_to_input(
                g,
                index,
                src,
                value,
                ordinal,
                "value",
                &node_id,
                &input,
                x - 220,
                y,
            );
        }
        Stmt::Unsafe {
            span, audit, body, ..
        } => {
            g.regions.push(format!(
                "{{\"region_id\":{},\"kind\":\"unsafe\",\"title\":{},\"source_span\":{}}}",
                json_str(&format!("{}:region:{ordinal}:unsafe", g.graph_id)),
                json_str(audit.as_deref().unwrap_or("#Unsafe")),
                span_json((*span).into())
            ));
            project_stmt_block(g, index, src, body, ordinal * 100 + 95, x + 230, y + 70);
        }
        Stmt::Impure {
            reason, body, span, ..
        } => {
            add_region(
                g,
                ordinal,
                "impure",
                reason.as_deref().unwrap_or("#Impure"),
                *span,
            );
            project_stmt_block(g, index, src, body, ordinal * 100 + 100, x + 230, y + 70);
        }
        Stmt::Reactive { body, span } => {
            add_region(g, ordinal, "reactive", "#Reactive", *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 110, x + 230, y + 70);
        }
        Stmt::Shield { body, span } => {
            add_region(g, ordinal, "shield", "#Shield", *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 115, x + 230, y + 70);
        }
        Stmt::Switched { marker, body, span } => {
            let node_id = format!("{}:stmt:{ordinal}:switched", g.graph_id);
            let badge = format!("#{}", marker.name);
            add_node(
                g,
                &node_id,
                "switched",
                &badge,
                (*span).into(),
                x,
                y,
                vec![&badge],
                vec!["toggle_switch_state", "source_jump"],
            );
            project_stmt_block(g, index, src, body, ordinal * 100 + 125, x + 230, y + 70);
        }
        Stmt::Region {
            name, body, span, ..
        } => {
            add_region(g, ordinal, "region", name, *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 130, x + 230, y + 70);
        }
        Stmt::Policy { declarations, body, span } => {
            let label = declarations.iter().map(|d| d.key.name()).collect::<Vec<_>>().join(", ");
            add_region(g, ordinal, "policy", &format!("#Policy({label})"), *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 135, x + 230, y + 70);
        }
        Stmt::TaskGroup {
            name, body, span, ..
        } => {
            add_region(g, ordinal, "task.group", name, *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 140, x + 230, y + 70);
        }
        Stmt::Layout {
            name, body, span, ..
        } => {
            add_region(g, ordinal, "layout", name, *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 150, x + 230, y + 70);
        }
        Stmt::Caps {
            caps, body, span, ..
        } => {
            let title = caps
                .iter()
                .map(|(cap, _)| cap.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            add_region(g, ordinal, "caps", &title, *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 160, x + 230, y + 70);
        }
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            span,
            ..
        } => {
            add_region(g, ordinal, "comptime_if", "@if", *span);
            let node_id = format!("{}:stmt:{ordinal}:comptime_if", g.graph_id);
            add_node(
                g,
                &node_id,
                "branch",
                "@if",
                (*span).into(),
                x,
                y,
                vec!["comptime"],
                vec!["edit_inline_expr", "source_jump"],
            );
            add_inline(g, &node_id, ordinal, "cond", src, cond.span());
            project_stmt_block(
                g,
                index,
                src,
                then_body,
                ordinal * 100 + 170,
                x + 230,
                y + 70,
            );
            if let Some(body) = else_body {
                project_stmt_block(g, index, src, body, ordinal * 100 + 180, x + 460, y + 70);
            }
        }
        Stmt::ComptimeBlock { body, span } => {
            add_region(g, ordinal, "comptime", "comptime", *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 190, x + 230, y + 70);
        }
        Stmt::ContextBlock { body, span, .. }
        | Stmt::Live { body, span }
        | Stmt::AssumeDet { body, span, .. }
        | Stmt::Transact { body, span, .. } => {
            add_region(g, ordinal, "scope", "scope", *span);
            project_stmt_block(g, index, src, body, ordinal * 100 + 200, x + 230, y + 70);
        }
        Stmt::Yield(expr, span) => {
            let node_id = format!("{}:stmt:{ordinal}:yield", g.graph_id);
            add_node(
                g,
                &node_id,
                "yield",
                "yield",
                (*span).into(),
                x,
                y,
                vec!["stream"],
                vec!["source_jump"],
            );
            let ty = expr_type(g, index, expr);
            let input = add_pin(g, &node_id, "value", "input", &ty, "", false);
            connect_expr_to_input(
                g,
                index,
                src,
                expr,
                ordinal,
                "value",
                &node_id,
                &input,
                x - 220,
                y,
            );
        }
        Stmt::ScopeMember {
            name,
            args,
            body,
            span,
            ..
        } => {
            let node_id = format!("{}:stmt:{ordinal}:scope_member:{name}", g.graph_id);
            add_node(
                g,
                &node_id,
                "function_exec",
                &format!(".{name}"),
                (*span).into(),
                x,
                y,
                vec!["scope"],
                vec!["source_jump"],
            );
            for (i, arg) in args.iter().enumerate() {
                add_inline(
                    g,
                    &node_id,
                    ordinal,
                    &format!("arg{}", i + 1),
                    src,
                    arg.span(),
                );
            }
            project_stmt_block(g, index, src, body, ordinal * 100 + 210, x + 230, y + 70);
        }
    }
}

fn switch_was_classic_if(src: &str, arms: &[AST::SwitchArm], span: Span) -> bool {
    let Some(first) = arms.first() else {
        return false;
    };
    AST::uses_classic_if_spelling(src, span, first.cond.span())
}

fn pattern_pin_label(src: &str, expr: &Expr) -> String {
    let raw = snippet(src, expr.span());
    let balanced = balance_closing_parens(raw.trim());
    if matches!(expr, Expr::PatternTest { .. }) {
        if let Some(pos) = balanced.find("==") {
            return balanced[pos..].trim().to_string();
        }
        return format!("== {}", balanced.trim());
    }
    balanced
}

/// Label for a dispatch-form (`if subject == { ... }`) arm pattern.
/// Dispatch arms never carry their own `==`, and the leading dot on
/// enum-variant patterns is source spelling rather than arm identity.
fn dispatch_arm_pattern_label(src: &str, expr: &Expr) -> String {
    let (raw, _) = dispatch_arm_pattern_fragment(src, expr);
    let balanced = balance_closing_parens(raw.trim());
    balanced.strip_prefix('.').unwrap_or(&balanced).to_string()
}

fn dispatch_arm_pattern_span(src: &str, expr: &Expr) -> SourceSpan {
    let span: SourceSpan = expr.span().into();
    let (pattern, offset) = dispatch_arm_pattern_fragment(src, expr);
    SourceSpan {
        start: span.start + offset,
        end: span.start + offset + pattern.trim_end().len(),
    }
}

fn dispatch_arm_pattern_fragment(src: &str, expr: &Expr) -> (String, usize) {
    let span: SourceSpan = expr.span().into();
    let source = src.get(span.start..span.end).unwrap_or("");
    let trimmed = source.trim_start();
    let mut offset = source.len() - trimmed.len();
    let mut fragment = trimmed;

    let dispatch_open = fragment.find("==").and_then(|pos| {
        let after_cmp = &fragment[pos + 2..];
        let after_space = after_cmp.trim_start();
        after_space.starts_with('{').then(|| (pos + 2, after_cmp.len() - after_space.len()))
    });
    if let Some((comparator, whitespace)) = dispatch_open {
        offset += comparator + whitespace;
        fragment = &fragment[comparator + whitespace..];
    } else {
        let after_space = fragment.trim_start();
        offset += fragment.len() - after_space.len();
        fragment = after_space;
    }
    if let Some(after_open) = fragment.strip_prefix('{') {
        offset += 1;
        fragment = after_open.trim_start();
        offset += after_open.len() - fragment.len();
    }

    if let Some(arrow) = fragment.rfind("->") {
        let after_arrow = &fragment[arrow + 2..];
        if let Some(close) = after_arrow.rfind('}') {
            let after_body = &after_arrow[close + 1..];
            offset += arrow + 2 + close + 1;
            fragment = after_body.trim_start();
            offset += after_body.len() - fragment.len();
        }
    }

    (fragment.trim_end().to_string(), offset)
}

fn pattern_arm_edit_span(src: &str, expr: &Expr) -> SourceSpan {
    match expr {
        Expr::PatternTest { pattern, .. } => span_through_closing_parens(src, pattern.span()),
        _ => expr.span().into(),
    }
}

fn balance_closing_parens(s: &str) -> String {
    let mut out = s.to_string();
    let opens = s.chars().filter(|c| *c == '(').count();
    let closes = s.chars().filter(|c| *c == ')').count();
    for _ in closes..opens {
        out.push(')');
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn project_classic_switch_else(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    else_body: Option<&[Stmt]>,
    ordinal: usize,
    x: i32,
    y: i32,
) {
    match else_body {
        Some([stmt @ Stmt::Switch {
            subject,
            arms,
            span,
            ..
        }]) if AST::is_subjectless_guard(subject, *span)
            && switch_was_classic_if(src, arms, *span) =>
        {
            project_stmt(g, index, src, stmt, ordinal * 100 + 60, x, y);
        }
        Some(body) => {
            project_stmt_block(g, index, src, body, ordinal * 100 + 70, x, y);
        }
        None => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn connect_expr_to_input(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    expr: &Expr,
    ordinal: usize,
    role: &str,
    owner_node_id: &str,
    input_pin: &str,
    x: i32,
    y: i32,
) {
    connect_expr_to_input_with_span(
        g,
        index,
        src,
        expr,
        expr.span(),
        ordinal,
        role,
        owner_node_id,
        input_pin,
        x,
        y,
    );
}

fn expr_source_end(expr: &Expr) -> usize {
    match expr {
        // Sema expands compound assignment with the target span on the new
        // binary node; the original RHS child retains the source end.
        Expr::Binary(_, left, right, _) => expr_source_end(left).max(expr_source_end(right)),
        _ => expr.span().end,
    }
}

fn binding_init_span(src: &str, binding: &AST::Binding) -> Span {
    let end = expr_source_end(&binding.init);
    let mut start = binding
        .sigil_span
        .map(|span| span.end)
        .unwrap_or_else(|| binding.init.span().start);
    while start < end && src.as_bytes()[start].is_ascii_whitespace() {
        start += 1;
    }
    Span::new(start, end)
}

#[allow(clippy::too_many_arguments)]
fn connect_expr_to_input_with_span(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    expr: &Expr,
    inline_span: Span,
    ordinal: usize,
    role: &str,
    owner_node_id: &str,
    input_pin: &str,
    x: i32,
    y: i32,
) {
    let provider_y = data_provider_y(y, ordinal);
    if let Some(out) = project_value_node(g, index, src, expr, ordinal, role, x, provider_y) {
        add_inline(g, owner_node_id, ordinal, role, src, inline_span);
        add_wire_with_span(g, &out, input_pin, "data", Some(expr.span().into()));
    } else if pure_leaf(expr) {
        add_inline(g, owner_node_id, ordinal, role, src, inline_span);
        wire_ident_refs(g, expr, input_pin);
    } else if let Some(out) = project_expr_node(g, index, src, expr, ordinal, x, provider_y, false) {
        if details_composite_expr(expr)
            || matches!(expr, Expr::EnumLit { .. })
            || matches!(expr, Expr::MethodCall { method, .. } if starts_uppercase(method))
        {
            add_inline(g, owner_node_id, ordinal, role, src, inline_span);
        }
        add_wire_with_span(g, &out, input_pin, "data", Some(expr.span().into()));
    }
}

fn details_composite_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::ListLit(_, _)
            | Expr::MapLit(_, _)
            | Expr::StructLit { .. }
            | Expr::TypedLit { .. }
            | Expr::TupleLit(_, _, _)
            | Expr::Present(_, _)
            | Expr::Ok(_, _)
            | Expr::Err(_, _)
    )
}

fn data_provider_y(y: i32, ordinal: usize) -> i32 {
    y + 96 + ((ordinal % 2) as i32 * 18)
}

fn project_value_node(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    expr: &Expr,
    ordinal: usize,
    role: &str,
    x: i32,
    y: i32,
) -> Option<String> {
    if let Expr::Place(inner, _, _) = expr {
        if let Expr::Ident(name, _) = inner.as_ref() {
            if let Some(pin) = g.local_pins.get(name).cloned() {
                return Some(pin);
            }
        }
        return project_value_node(g, index, src, inner, ordinal, role, x, y);
    }
    if let Expr::Ident(name, span) = expr {
        let source_span: SourceSpan = (*span).into();
        let key = getter_key(index, name, source_span);
        if let Some(pin) = g.getter_pins.get(&key).cloned() {
            return Some(pin);
        }
        let ty = expr_type(g, index, expr);
        let base_node_id = format!("{}:value:get:{}", g.graph_id, canvas_ident_fragment(name));
        let node_id = if g
            .nodes
            .iter()
            .any(|node| node.id == base_node_id && node.span != source_span)
        {
            format!("{base_node_id}:{}-{}", span.start, span.end)
        } else {
            base_node_id
        };
        add_node(
            g,
            &node_id,
            "variable_get",
            name,
            source_span,
            x,
            y,
            vec!["read"],
            vec!["edit_inline_expr", "source_jump"],
        );
        let pin = add_pin(g, &node_id, name, "output", &ty, "", false);
        g.getter_pins.insert(key, pin.clone());
        return Some(pin);
    }
    let (title, badges) = match expr {
        Expr::Int(_, _, _, _)
        | Expr::Float(_, _, _, _)
        | Expr::Bool(_, _)
        | Expr::Str(_, _)
        | Expr::Char(_, _)
        | Expr::Absent(_)
        | Expr::Todo { .. } => (snippet(src, expr.span()), vec!["const"]),
        _ => return None,
    };
    let span: SourceSpan = expr.span().into();
    let node_id = format!(
        "{}:value:{ordinal}:{role}:{}-{}",
        g.graph_id, span.start, span.end
    );
    add_node(
        g,
        &node_id,
        "constant",
        &title,
        span,
        x,
        y,
        badges,
        vec!["edit_inline_expr", "source_jump"],
    );
    Some(add_pin(
        g,
        &node_id,
        "value",
        "output",
        &expr_type(g, index, expr),
        "",
        false,
    ))
}

fn getter_key(index: &SemIndex, name: &str, span: SourceSpan) -> String {
    let Some(reference) = index
        .references()
        .iter()
        .find(|reference| reference.span == span && reference.name == name)
    else {
        return format!("{name}@{}:{}", span.start, span.end);
    };
    let Some(target) = reference.target.as_ref() else {
        return format!("{name}@{}:{}", span.start, span.end);
    };
    format!(
        "{name}@{}:{}:{}",
        target.module_path, target.def_span.start, target.def_span.end
    )
}

fn canvas_ident_fragment(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

fn project_expr_node(
    g: &mut GraphBuilder,
    index: &SemIndex,
    src: &str,
    expr: &Expr,
    ordinal: usize,
    x: i32,
    y: i32,
    exec_context: bool,
) -> Option<String> {
    match expr {
        Expr::Call(c) => {
            let node_id = format!("{}:expr:{ordinal}:call:{}", g.graph_id, c.name);
            let archetype = if exec_context || call_has_effects(index, &c.name) {
                "function_exec"
            } else {
                "function_pure"
            };
            add_node(
                g,
                &node_id,
                archetype,
                &c.name,
                c.name_span.into(),
                x,
                y,
                effect_badges(index, &c.name),
                vec!["insert_call", "source_jump"],
            );
            for (i, arg) in c.args.iter().enumerate() {
                let ty = expr_type(g, index, &arg.expr);
                let input = add_pin(
                    g,
                    &node_id,
                    &format!("arg{}", i + 1),
                    "input",
                    &ty,
                    arg.convention.sigil(),
                    false,
                );
                connect_expr_to_input(
                    g,
                    index,
                    src,
                    &arg.expr,
                    ordinal * 1000 + i + 1,
                    &format!("arg{}", i + 1),
                    &node_id,
                    &input,
                    x - 220,
                    y + i as i32 * 74,
                );
            }
            let ret = call_ret(index, &c.name).unwrap_or_else(|| "Void".to_string());
            Some(add_pin(
                g,
                &node_id,
                "result",
                "output",
                &ret,
                "",
                ret.ends_with('?'),
            ))
        }
        Expr::MethodCall {
            receiver,
            method,
            method_span,
            args,
            recv_type,
            resolved_ret,
            ..
        } => {
            let node_id = format!("{}:expr:{ordinal}:method:{method}", g.graph_id);
            let variant_like = starts_uppercase(method);
            let archetype = if variant_like {
                "function_pure"
            } else if exec_context || call_has_effects(index, method) {
                "function_exec"
            } else {
                "function_pure"
            };
            // D-CONC-SPAWN1=D: `task …`/`task.all { … }`/etc. desugar onto the
            // compiler-private `INTERNAL_TASK_RECEIVER` (parser-only, never a
            // real receiver a user typed) — but inside an active `task.group`,
            // sema rewrites that receiver in place to the group's own name
            // (`infer_task_surface_method`, CheckerTaskGroup.rs), so by the
            // time this checked AST reaches canvas the receiver identity is
            // gone. `recv_type` survives that rewrite: sema always tags the
            // dispatch with `INTERNAL_TASK_SURFACE_TYPE` (detached) or
            // `INTERNAL_TASK_GROUP_SURFACE_TYPE` (lexical group), never
            // `TYPE_TASKGROUP` (a real `TaskGroup` value, e.g. a retired wait-builder call,
            // which keeps its ordinary `.method` title). Show the surface
            // spelling the author actually wrote instead of the internal
            // dispatch method name (`spawn`) or the hidden receiver.
            let is_task_surface = matches!(
                recv_type.as_deref(),
                Some(jet_driver::Syntax::INTERNAL_TASK_SURFACE_TYPE)
                    | Some(jet_driver::Syntax::INTERNAL_TASK_GROUP_SURFACE_TYPE)
            );
            let title = if is_task_surface {
                if method == "spawn" {
                    "task".to_string()
                } else {
                    format!("task.{method}")
                }
            } else if variant_like {
                method.clone()
            } else {
                format!(".{method}")
            };
            add_node(
                g,
                &node_id,
                if variant_like { "variant" } else { archetype },
                &title,
                (*method_span).into(),
                x,
                y,
                Vec::new(),
                vec!["insert_call", "source_jump"],
            );
            let recv_ty = expr_type(g, index, receiver);
            let recv_pin = add_pin(g, &node_id, "self", "input", &recv_ty, "", false);
            connect_expr_to_input(
                g,
                index,
                src,
                receiver,
                ordinal * 1000 + 1,
                "self",
                &node_id,
                &recv_pin,
                x - 220,
                y,
            );
            for (i, arg) in args.iter().enumerate() {
                let ty = expr_type(g, index, &arg.expr);
                let input = add_pin(
                    g,
                    &node_id,
                    &format!("arg{}", i + 1),
                    "input",
                    &ty,
                    arg.convention.sigil(),
                    false,
                );
                connect_expr_to_input(
                    g,
                    index,
                    src,
                    &arg.expr,
                    ordinal * 1000 + i + 2,
                    &format!("arg{}", i + 1),
                    &node_id,
                    &input,
                    x - 220,
                    y + (i as i32 + 1) * 74,
                );
            }
            let ret = resolved_ret
                .as_ref()
                .map(AST::Type::name)
                .unwrap_or_else(|| "unknown".to_string());
            Some(add_pin(
                g,
                &node_id,
                "result",
                "output",
                &ret,
                "",
                ret.ends_with('?'),
            ))
        }
        Expr::Try(inner, span, _, _) => {
            let node_id = format!("{}:expr:{ordinal}:fallible", g.graph_id);
            add_node(
                g,
                &node_id,
                "fallible",
                "?",
                (*span).into(),
                x,
                y,
                vec!["fallible"],
                vec!["add_fallback_rail", "source_jump"],
            );
            let input = add_pin(
                g,
                &node_id,
                "value",
                "input",
                &expr_type(g, index, inner),
                "",
                true,
            );
            if let Some(out) = project_expr_node(g, index, src, inner, ordinal, x - 180, y, false) {
                add_wire(g, &out, &input, "fallible");
            }
            Some(add_pin(g, &node_id, "ok", "output", "unknown", "", false))
        }
        Expr::OrFallback { value, .. } => {
            project_expr_node(g, index, src, value, ordinal, x, y, exec_context)
        }
        Expr::ListLit(items, span) => {
            let node_id = format!("{}:expr:{ordinal}:list", g.graph_id);
            add_node(
                g,
                &node_id,
                "expression",
                "list",
                (*span).into(),
                x,
                y,
                vec!["multi-input"],
                vec!["append_multi_input", "edit_inline_expr", "source_jump"],
            );
            for (i, item) in items.iter().enumerate() {
                let ty = expr_type(g, index, item);
                let input = add_pin(
                    g,
                    &node_id,
                    &format!("item{}", i + 1),
                    "input",
                    &ty,
                    "",
                    false,
                );
                set_pin_source_span(g, &input, item.span().into());
                set_pin_append(g, &input, "remove_multi_input_element", i);
                connect_expr_to_input(
                    g,
                    index,
                    src,
                    item,
                    ordinal * 1000 + i + 1,
                    &format!("item{}", i + 1),
                    &node_id,
                    &input,
                    x - 220,
                    y + i as i32 * 74,
                );
            }
            Some(add_pin(
                g,
                &node_id,
                "value",
                "output",
                &expr_type(g, index, expr),
                "",
                false,
            ))
        }
        _ => {
            let node_id = format!("{}:expr:{ordinal}:expr", g.graph_id);
            let title = expr_title(expr);
            add_node(
                g,
                &node_id,
                "expression",
                title,
                expr.span().into(),
                x,
                y,
                vec!["expression"],
                vec!["edit_inline_expr", "source_jump"],
            );
            add_inline(g, &node_id, ordinal, "value", src, expr.span());
            Some(add_pin(
                g,
                &node_id,
                "value",
                "output",
                &expr_type(g, index, expr),
                "",
                false,
            ))
        }
    }
}
