//! D-WEBAPP1=D / D-WEBAUTHOR1=D: extract the `fn run` App builder into one typed
//! application graph and diagnose undeclared dynamic edges, stray convention
//! files, and builder/file collisions.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{Expr, Func, Item, ProgramBundle, Stmt, StrPart, Type};
use crate::Traits::TraitRegistry;
use jet_foundation::App::{
    stable_island_identity, stable_pending_boundary_id, stable_route_identity, AppAction,
    AppCacheFact, AppFormFact, AppFormFieldFact, AppGraph, AppHydrationTrigger, AppIslandFact,
    AppMount, AppQueryFact, AppRenderFactMode, AppRenderFacts, AppRenderMode, AppResumeCapture,
    AppResumePayload, AppRoute, AppRouteBoundaries, AppRouteField, AppRouteLoader, AppRoutesFrom,
    AppStoreFact,
};
use crate::Sema::Effects::EffectSet;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Walk the entry module for the App-returning `fn run` and record the static
/// builder graph.
pub fn extract_app_graph(bundle: &ProgramBundle) -> (Option<AppGraph>, Vec<Diagnostic>) {
    let registries = TraitRegistry::bundle_auto_derives(bundle, &bundle.name_ledger);
    let registry = registries
        .get(bundle.entry)
        .cloned()
        .unwrap_or_default();
    extract_app_graph_inner(bundle, None, &registry)
}

pub fn extract_app_graph_with_effects_and_registry(
    bundle: &ProgramBundle,
    solved: &HashMap<String, EffectSet>,
    trait_reg: &TraitRegistry,
) -> (Option<AppGraph>, Vec<Diagnostic>) {
    extract_app_graph_inner(bundle, Some(solved), trait_reg)
}

fn extract_app_graph_inner(
    bundle: &ProgramBundle,
    solved: Option<&HashMap<String, EffectSet>>,
    trait_reg: &TraitRegistry,
) -> (Option<AppGraph>, Vec<Diagnostic>) {
    let Some(module) = bundle.modules.get(bundle.entry) else {
        return (None, Vec::new());
    };
    let Some(run_fn) = crate::AST::app_entry_run_fn(&module.items) else {
        return (None, Vec::new());
    };

    let mut graph = AppGraph {
        entry_file: module.display.clone(),
        hydration: "dev-overlay".to_string(),
        shared_tir: true,
        ..AppGraph::default()
    };
    let mut known_fns: HashMap<String, Func> = HashMap::new();
    for candidate in &bundle.modules {
        for function in collect_fn_defs(&candidate.items) {
            known_fns.entry(function.name.clone()).or_insert(function);
        }
    }
    let serializable_types = collect_serializable_types(bundle);
    let mut diags = Vec::new();
    let mut current_render = AppRenderMode::Csr;
    let mut next_override: Option<AppRenderFactMode> = None;
    let mut seen_paths: HashMap<String, (String, Span)> = HashMap::new();

    for stmt in &run_fn.body {
        if let Some(expr) = stmt_expr(stmt) {
            walk_builder(
                expr,
                &mut graph,
                &mut diags,
                &mut current_render,
                &mut next_override,
                &mut seen_paths,
                &known_fns,
                "builder",
            );
        }
    }
    // D-DX-SERVERFN1=A: actions and progressive forms share one checked wire
    // signature. Validate the named handler after graph discovery so route
    // extraction does not invent a second serializer/type registry.
    for action in &graph.actions {
        if matches!(action.kind.as_str(), "action" | "form" | "data" | "loader") {
            if let Some(function) = known_fns.get(&action.handler) {
                diags.extend(crate::Sema::CheckerCoreLib::validate_server_function_boundary(
                    function,
                    trait_reg,
                ));
            }
        }
    }

    // D-WEBAUTHOR1=D: expand each `.routes(from:)` root exhaustively.
    let roots: Vec<AppRoutesFrom> = graph.routes_from.clone();
    for root in roots {
        expand_routes_from(
            bundle,
            &root,
            &mut graph,
            &mut diags,
            &mut seen_paths,
            current_render,
            next_override,
        );
    }
    derive_route_contracts(bundle, &mut graph, &mut diags);
    graph.routes.sort_by(|left, right| {
        route_precedence(&right.path)
            .cmp(&route_precedence(&left.path))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.handler.cmp(&right.handler))
    });

    derive_render_facts(
        &mut graph,
        &module.items,
        module.alias.as_str(),
        solved,
        &serializable_types,
        &mut diags,
    );

    collect_query_facts(bundle, &mut graph);
    collect_store_facts(bundle, &mut graph);
    collect_form_facts(bundle, &mut graph);
    (Some(graph), diags)
}

fn collect_query_facts(bundle: &ProgramBundle, graph: &mut AppGraph) {
    let mut facts = BTreeSet::new();
    for module in &bundle.modules {
        for function in collect_fn_defs(&module.items) {
            for statement in &function.body {
                statement.for_each_expr(|expr| {
                    let (kind, args) = match expr {
                        Expr::Call(call) => {
                            (query_call_kind(&call.name), call.args.as_slice())
                        }
                        Expr::MethodCall {
                            receiver,
                            method,
                            args,
                            ..
                        } if is_query_receiver(receiver) => {
                            (query_method_kind(method), args.as_slice())
                        }
                        _ => return,
                    };
                    let Some(kind) = kind else {
                        return;
                    };
                    let (key, footprint) = if kind == "live" {
                        let Some(key) = const_string_arg(args, 0) else {
                            return;
                        };
                        let Some(footprint) = const_string_arg(args, 1) else {
                            return;
                        };
                        (key, footprint)
                    } else {
                        let Some(source) = const_string_arg(args, 0) else {
                            return;
                        };
                        (source.clone(), format!("ext:{source}"))
                    };
                    if key.trim().is_empty() || footprint.trim().is_empty() {
                        return;
                    }
                    facts.insert((
                        key,
                        footprint,
                        function.name.clone(),
                        kind.to_string(),
                    ));
                });
            }
        }
    }
    graph.queries = facts
        .into_iter()
        .map(|(key, footprint, source, kind)| AppQueryFact {
            key,
            footprint,
            source,
            kind,
        })
        .collect();
}

fn query_call_kind(name: &str) -> Option<&'static str> {
    match name {
        "query.live" | "web.query.live" | "core.web.query.live" => Some("live"),
        "query.subscribe"
        | "web.query.subscribe"
        | "core.web.query.subscribe" => Some("subscribe"),
        _ => None,
    }
}

fn query_method_kind(name: &str) -> Option<&'static str> {
    match name {
        "live" => Some("live"),
        "subscribe" => Some("subscribe"),
        _ => None,
    }
}

fn is_query_receiver(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name, _) => matches!(
            name.as_str(),
            "query" | "web.query" | "core.web.query"
        ),
        Expr::Field(base, field, _) => {
            field == "query"
                && matches!(
                    base.as_ref(),
                    Expr::Ident(name, _) if matches!(name.as_str(), "web" | "core.web")
                )
        }
        Expr::Paren(inner, _) => is_query_receiver(inner),
        _ => false,
    }
}
fn collect_store_facts(bundle: &ProgramBundle, graph: &mut AppGraph) {
    let mut facts = BTreeSet::new();
    for module in &bundle.modules {
        for function in collect_fn_defs(&module.items) {
            for statement in &function.body {
                statement.for_each_expr(|expr| {
                    let (kind, args) = match expr {
                        Expr::Call(call) => {
                            (store_call_kind(&call.name), call.args.as_slice())
                        }
                        Expr::MethodCall {
                            receiver,
                            method,
                            args,
                            ..
                        } if is_store_receiver(receiver) => {
                            (store_method_kind(method), args.as_slice())
                        }
                        _ => return,
                    };
                    let Some(kind) = kind else {
                        return;
                    };
                    let Some(name) = const_string_arg(args, 0) else {
                        return;
                    };
                    if name.trim().is_empty() {
                        return;
                    }
                    let history_limit = if kind == "with_history" {
                        const_int_arg(args, 2)
                            .map_or_else(|| "<dynamic>".to_string(), |value| value.to_string())
                    } else {
                        "default".to_string()
                    };
                    facts.insert((name, function.name.clone(), kind.to_string(), history_limit));
                });
            }
        }
    }
    graph.stores = facts
        .into_iter()
        .map(
            |(name, source, kind, history_limit)| AppStoreFact {
                name,
                source,
                kind,
                history_limit,
            },
        )
        .collect();
}

fn store_call_kind(name: &str) -> Option<&'static str> {
    match name {
        "store" | "web.store" | "core.web.store" => Some("default"),
        "store.with_history" | "web.store.with_history" | "core.web.store.with_history" => {
            Some("with_history")
        }
        _ => None,
    }
}

fn store_method_kind(name: &str) -> Option<&'static str> {
    match name {
        "new" => Some("default"),
        "with_history" => Some("with_history"),
        _ => None,
    }
}

fn is_store_receiver(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name, _) => matches!(
            name.as_str(),
            "store" | "web.store" | "core.web.store"
        ),
        Expr::Field(base, field, _) => {
            field == "store"
                && matches!(
                    base.as_ref(),
                    Expr::Ident(name, _) if matches!(name.as_str(), "web" | "core.web")
                )
        }
        Expr::Paren(inner, _) => is_store_receiver(inner),
        _ => false,
    }
}
/// D-WEBFORM1=A: project each checked `web.form(Model, action: handler)` call
/// onto the App graph. The projection reads the same model fields and action
/// contract as the core-call elaborator; it never stores live values.
fn collect_form_facts(bundle: &ProgramBundle, graph: &mut AppGraph) {
    let mut facts = BTreeMap::new();
    for module in &bundle.modules {
        for function in collect_fn_defs(&module.items) {
            for statement in &function.body {
                statement.for_each_expr(|expr| {
                    let Some((model, action, span)) = form_call_parts(expr) else {
                        return;
                    };
                    let Some(record) = find_struct_bundle(bundle, &model) else {
                        return;
                    };
                    let fields = record
                        .fields
                        .iter()
                        .filter(|field| field.computed.is_none())
                        .map(|field| {
                            let base = match &field.ty {
                                Type::Option(inner) => inner.as_ref(),
                                ty => ty,
                            };
                            let (ty, control) = match base {
                                Type::Int | Type::IntN { .. } => {
                                    ("Int".to_string(), "Number".to_string())
                                }
                                Type::Float => ("Float".to_string(), "Number".to_string()),
                                Type::Bool => ("Bool".to_string(), "Checkbox".to_string()),
                                Type::String => ("String".to_string(), "Text".to_string()),
                                other => (other.name(), "Text".to_string()),
                            };
                            AppFormFieldFact {
                                name: field.name.clone(),
                                ty,
                                required: !matches!(field.ty, Type::Option(_))
                                    && field.default.is_none(),
                                default: field
                                    .default
                                    .as_deref()
                                    .and_then(|default| const_wire_text(Some(default))),
                                label: form_label(&field.name),
                                control,
                                wire_name: field.name.clone(),
                            }
                        })
                        .collect::<Vec<_>>();
                    let action_fn = find_func_bundle(bundle, &action);
                    let (
                        input_type,
                        output_type,
                        error_type,
                        endpoint,
                        method,
                        csrf,
                        effects,
                        middleware,
                    ) = app_action_contract(action_fn, &action);
                    let fact = AppFormFact {
                        name: model.clone(),
                        model: model.clone(),
                        action: action.clone(),
                        input_type,
                        output_type,
                        error_type,
                        endpoint,
                        method,
                        csrf,
                        effects,
                        fields,
                        middleware,
                        provenance: function.name.clone(),
                        span_start: span.start,
                        span_end: span.end,
                    };
                    facts
                        .entry((model, action, function.name.clone()))
                        .or_insert(fact);
                });
            }
        }
    }
    graph.forms = facts.into_values().collect();
}

fn form_call_parts(expr: &Expr) -> Option<(String, String, Span)> {
    let Expr::Call(call) = expr else {
        return None;
    };
    if !matches!(
        call.name.as_str(),
        "form" | "web.form" | "core.web.form"
    ) {
        return None;
    }
    if call.args.len() != 2 {
        return None;
    }
    let action_index = call
        .args
        .iter()
        .position(|arg| arg.label.as_ref().is_some_and(|(label, _)| label == "action"))
        .unwrap_or(1);
    let model_index = if action_index == 0 { 1 } else { 0 };
    let model = form_model_name(&call.args.get(model_index)?.expr)?;
    let action = const_string_expr(&call.args.get(action_index)?.expr)
        .or_else(|| handler_name(call.args.get(action_index)))?;
    (!model.is_empty() && !action.is_empty()).then_some((model, action, call.name_span))
}

fn form_model_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Paren(inner, _) => form_model_name(inner),
        Expr::OrFallback { value, .. } => form_model_name(value),
        Expr::MethodCall { method, args, .. } if method == "input" => {
            const_string_arg(args, 0)
        }
        Expr::Call(call) if call.name.ends_with(".input") || call.name == "input" => {
            const_string_arg(&call.args, 0)
        }
        _ => None,
    }
}

fn form_label(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            first.to_uppercase().collect::<String>() + chars.as_str()
        })
        .collect::<Vec<_>>()
        .join(" ")
}


fn collect_fn_defs(items: &[Item]) -> Vec<Func> {
    let mut functions = Vec::new();
    for item in items {
        match item {
            Item::Func(function) => functions.push(function.clone()),
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    functions.extend(collect_fn_defs(body));
                }
            }
            _ => {}
        }
    }
    functions
}

fn stmt_expr(stmt: &Stmt) -> Option<&Expr> {
    match stmt {
        Stmt::Expr(expr) => Some(expr),
        Stmt::Return(Some(expr), _) => Some(expr),
        Stmt::Val(binding) => Some(&binding.init),
        Stmt::Assign { value, .. } => Some(value),
        _ => None,
    }
}

fn walk_builder(
    expr: &Expr,
    graph: &mut AppGraph,
    diags: &mut Vec<Diagnostic>,
    current_render: &mut AppRenderMode,
    next_override: &mut Option<AppRenderFactMode>,
    seen_paths: &mut HashMap<String, (String, Span)>,
    known_fns: &HashMap<String, Func>,
    provenance: &str,
) {
    match expr {
        Expr::MethodCall {
            receiver,
            method,
            args,
            method_span,
            ..
        } => {
            walk_builder(
                receiver,
                graph,
                diags,
                current_render,
                next_override,
                seen_paths,
                known_fns,
                provenance,
            );
            apply_method(
                method,
                args,
                *method_span,
                graph,
                diags,
                current_render,
                next_override,
                seen_paths,
                known_fns,
                provenance,
            );
        }
        // Sema may materialize an owning value, preserve a read place, or lift
        // the return payload through `Ok` around the root builder expression.
        // These wrappers do not change the application graph, so keep walking
        // through them.
        Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Paren(inner, _)
        | Expr::Ok(inner, _) => {
            walk_builder(
                inner,
                graph,
                diags,
                current_render,
                next_override,
                seen_paths,
                known_fns,
                provenance,
            );
        }
        Expr::Call(call) => {
            if is_app_ctor_name(&call.name) && call.args.is_empty() {
                return;
            }
            for arg in &call.args {
                walk_builder(
                    &arg.expr,
                    graph,
                    diags,
                    current_render,
                    next_override,
                    seen_paths,
                    known_fns,
                    provenance,
                );
            }
        }
        Expr::Field(base, field, _) if field == "app" => {
            let _ = base;
        }
        _ => {}
    }
}

fn is_app_ctor_name(name: &str) -> bool {
    matches!(name, "app" | "web.app" | "core.web.app")
}
fn app_action_contract(
    function: Option<&Func>,
    name: &str,
) -> (String, String, String, String, String, String, Vec<String>, Vec<String>) {
    let endpoint = format!("/actions/{name}");
    let method = "POST".to_string();
    let csrf = "same-origin".to_string();
    let Some(function) = function else {
        return (
            String::new(),
            String::new(),
            String::new(),
            endpoint,
            method,
            csrf,
            Vec::new(),
            Vec::new(),
        );
    };
    let params = function
        .params
        .iter()
        .filter(|parameter| parameter.name != "self")
        .collect::<Vec<_>>();
    let input_type = match params.as_slice() {
        [] => "Unit".to_string(),
        [parameter] => parameter.ty.name(),
        _ => format!(
            "({})",
            params
                .iter()
                .map(|parameter| format!("{}:{}", parameter.name, parameter.ty.name()))
                .collect::<Vec<_>>()
                .join(",")
        ),
    };
    let (output_type, error_type) = match function.failure_contract() {
        crate::AST::FailureContract::Default { success, error }
        | crate::AST::FailureContract::Explicit { success, error } => {
            (success.name(), error.name())
        }
        crate::AST::FailureContract::Optional { success } => (success.name(), "Never".to_string()),
        crate::AST::FailureContract::DeclaredNever => ("Never".to_string(), "Never".to_string()),
        crate::AST::FailureContract::Converted { success, target, .. } => {
            (success.name(), target.name())
        }
        crate::AST::FailureContract::ProvenUnreachable { success } => {
            (success.name(), "Never".to_string())
        }
    };
    let effects = function
        .declared_effects
        .as_ref()
        .map(|effects| effects.iter().map(|(name, _)| name.clone()).collect())
        .unwrap_or_default();
    let middleware = jet_foundation::App::APP_SERVER_FUNCTION_MIDDLEWARE
        .iter()
        .map(|name| name.as_str().to_string())
        .collect();
    (
        input_type,
        output_type,
        error_type,
        endpoint,
        method,
        csrf,
        effects,
        middleware,
    )
}


fn apply_method(
    method: &str,
    args: &[crate::AST::CallArg],
    method_span: Span,
    graph: &mut AppGraph,
    diags: &mut Vec<Diagnostic>,
    current_render: &mut AppRenderMode,
    next_override: &mut Option<AppRenderFactMode>,
    seen_paths: &mut HashMap<String, (String, Span)>,
    known_fns: &HashMap<String, Func>,
    provenance: &str,
) {
    match method {
        "route" | "page" | "layout" => {
            let path = canonical_route_path(
                &const_string_arg(args, 0).unwrap_or_else(|| "/".to_string()),
            );
            let handler = handler_name(args.get(1)).unwrap_or_else(|| "<dynamic>".to_string());
            if handler == "<dynamic>" || !known_fns.contains_key(&handler) {
                diags.push(Diagnostic::error(
                    "E2810",
                    format!("route `{path}` is not a statically known handler"),
                    "D-WEBAPP1 keeps every route on the typed application graph; a runtime-built handler outside `.mount` is an unanalyzed edge".to_string(),
                    "pass a named function, or declare a typed `.mount(prefix, handler)` for dynamic subtrees".to_string(),
                    Some(method_span),
                ));
            }
            let route_override = next_override.take();
            let render_facts = route_override.map_or_else(AppRenderFacts::default, |mode| {
                AppRenderFacts {
                    mode,
                    reason: vec![format!("explicit render mode `{}`", mode.as_str())],
                    override_mode: Some(mode),
                    ..AppRenderFacts::default()
                }
            });
            record_route(
                graph,
                diags,
                seen_paths,
                AppRoute {
                    path: path.clone(),
                    handler: handler.clone(),
                    route_identity: stable_route_identity(&path, &handler),
                    path_params: Vec::new(),
                    search_params: Vec::new(),
                    search_codec: "query".to_string(),
                    loader: None,
                    boundaries: AppRouteBoundaries::default(),
                    precedence: route_precedence_name(&path),
                    render: *current_render,
                    render_facts,
                    provenance: provenance.to_string(),
                    span_start: method_span.start,
                    span_end: method_span.end,
                },
            );
        }
        "action" | "form" | "data" | "loader" => {
            let name = if method == "loader" {
                handler_name(args.get(0))
                    .or_else(|| const_string_arg(args, 0))
                    .unwrap_or_else(|| method.to_string())
            } else {
                const_string_arg(args, 0).unwrap_or_else(|| method.to_string())
            };
            let handler = handler_name(args.get(1)).unwrap_or_else(|| "<dynamic>".to_string());
            let preload = method == "loader" && labeled_bool_arg(args, "preload").unwrap_or(false);
            if handler == "<dynamic>" || !known_fns.contains_key(&handler) {
                diags.push(Diagnostic::error(
                    "E2810",
                    format!("`{method}` `{name}` is not a statically known handler"),
                    "D-WEBAPP1 records actions/forms/data on the typed application graph; a runtime value outside `.mount` is an unanalyzed edge".to_string(),
                    "pass a named function, or declare a typed `.mount(prefix, handler)` for dynamic registration".to_string(),
                    Some(method_span),
                ));
            }
            if method == "loader" {
                // The runtime graph attaches a loader to the route registered
                // immediately before it; the source must name that handler.
                let attached = graph
                    .routes
                    .last()
                    .is_some_and(|route| route.handler == name);
                if !attached {
                    diags.push(Diagnostic::error(
                        "E2810",
                        format!("`loader` for `{name}` does not follow that route registration"),
                        "a loader belongs to the route registered immediately before it in the App graph".to_string(),
                        format!("write `.loader({name}, …)` directly after `.route(path, {name})`"),
                        Some(method_span),
                    ));
                }
            }
            let (
                input_type,
                output_type,
                error_type,
                endpoint,
                method_name,
                csrf,
                effects,
                middleware,
            ) = app_action_contract(known_fns.get(&handler), &name);
            graph.actions.push(AppAction {
                name,
                handler,
                kind: method.to_string(),
                preload,
                input_type,
                output_type,
                error_type,
                endpoint,
                method: method_name,
                csrf,
                effects,
                middleware,
                provenance: provenance.to_string(),
                span_start: method_span.start,
                span_end: method_span.end,
            });
        }
        "pending" | "not_found" | "error" => {
            let handler = handler_name(args.first()).unwrap_or_else(|| "<dynamic>".to_string());
            if handler == "<dynamic>" || !known_fns.contains_key(&handler) {
                diags.push(Diagnostic::error(
                    "E2810",
                    format!("`{method}` boundary is not a statically known handler"),
                    "route boundaries are part of the checked App graph and cannot be selected at runtime".to_string(),
                    "pass a named boundary function after a route registration".to_string(),
                    Some(method_span),
                ));
            }
            if let Some(route) = graph.routes.last_mut() {
                match method {
                    "pending" => route.boundaries.pending = Some(handler),
                    "not_found" => route.boundaries.not_found = Some(handler),
                    "error" => route.boundaries.error = Some(handler),
                    _ => {}
                }
            } else {
                diags.push(Diagnostic::error(
                    "E2810",
                    format!("`{method}` boundary has no preceding route"),
                    "a boundary must attach to one route identity in the App graph".to_string(),
                    "register a route before declaring its boundary".to_string(),
                    Some(method_span),
                ));
            }
        }
        "mount" => {
            let prefix = const_string_arg(args, 0).unwrap_or_else(|| "/".to_string());
            let handler = handler_name(args.get(1)).unwrap_or_else(|| "<mount>".to_string());
            let mut effects = Vec::new();
            let mut security = Vec::new();
            if let Some(arg) = args.get(2) {
                if let Some(s) = const_string_expr(&arg.expr) {
                    effects.push(s);
                }
            }
            if let Some(arg) = args.get(3) {
                if let Some(s) = const_string_expr(&arg.expr) {
                    security.push(s);
                }
            }
            graph.mounts.push(AppMount {
                prefix,
                handler,
                effects,
                security,
                provenance: provenance.to_string(),
                span_start: method_span.start,
                span_end: method_span.end,
            });
        }
        "routes" => {
            let root = labeled_string_arg(args, "from")
                .or_else(|| const_string_arg(args, 0))
                .unwrap_or_default();
            if root.is_empty() {
                diags.push(Diagnostic::error(
                    "E2806",
                    "`.routes(from:)` needs a directory path".to_string(),
                    "file routing activates only through an explicit builder opt-in (D-WEBAUTHOR1)"
                        .to_string(),
                    "write `.routes(from: \"routes\")` with the convention root".to_string(),
                    Some(method_span),
                ));
            } else {
                graph.routes_from.push(AppRoutesFrom {
                    root,
                    span_start: method_span.start,
                    span_end: method_span.end,
                });
            }
        }
        "csr" => {
            *current_render = AppRenderMode::Csr;
            *next_override = Some(AppRenderFactMode::from_legacy(*current_render));
        }
        "ssr" => {
            *current_render = AppRenderMode::Ssr;
            *next_override = Some(AppRenderFactMode::from_legacy(*current_render));
        }
        "ssg" => {
            *current_render = AppRenderMode::Ssg;
            *next_override = Some(AppRenderFactMode::from_legacy(*current_render));
        }
        "stream" | "streaming" => {
            *current_render = AppRenderMode::Stream;
            *next_override = Some(AppRenderFactMode::from_legacy(*current_render));
        }
        "island" => {
            *current_render = AppRenderMode::Island;
            *next_override = Some(AppRenderFactMode::from_legacy(*current_render));
        }
        "render" => {
            if let Some(mode) = render_mode_arg(args) {
                let legacy = legacy_render_mode(mode);
                *current_render = legacy;
                *next_override = Some(mode);
                if let Some(route) = graph.routes.last_mut() {
                    route.render = legacy;
                    route.render_facts = AppRenderFacts {
                        mode,
                        reason: vec![format!("explicit render mode `{}`", mode.as_str())],
                        override_mode: Some(mode),
                        ..AppRenderFacts::default()
                    };
                }
            }
        }
        "security" => {
            if let Some(s) = const_string_arg(args, 0) {
                graph.policy.security.push(s);
            }
        }
        "assets" => {
            if let Some(s) = const_string_arg(args, 0) {
                graph.policy.assets.push(s);
            }
        }
        "split" | "code_split" => {
            if let Some(s) = const_string_arg(args, 0) {
                graph.policy.split.push(s);
            }
        }
        "cache" => {
            if let Some(policy) = const_string_arg(args, 1) {
                if let Some(route) = graph.routes.last_mut() {
                    if const_string_arg(args, 0).as_deref() == Some(route.path.as_str()) {
                        if let Some(cache) = AppCacheFact::parse(&policy) {
                            route.render_facts.cache = cache;
                        }
                    }
                }
                graph.policy.cache.push(policy);
            } else if let Some(policy) = const_string_arg(args, 0) {
                graph.policy.cache.push(policy);
            }
        }
        "a11y" => {
            if let Some(s) = const_string_arg(args, 0) {
                graph.policy.a11y.push(s);
            }
        }
        "adapter" => {
            if let Some(s) = const_string_arg(args, 0) {
                graph.policy.adapters.push(s);
            }
        }
        "hydration_release" => {
            graph.hydration = "release-keep-server".to_string();
        }
        "hydration_dev" => {
            graph.hydration = "dev-overlay".to_string();
        }
        _ => {}
    }
}

fn record_route(
    graph: &mut AppGraph,
    diags: &mut Vec<Diagnostic>,
    seen_paths: &mut HashMap<String, (String, Span)>,
    route: AppRoute,
) {
    let span = Span::new(route.span_start, route.span_end);
    if let Some((prev_prov, prev_span)) = seen_paths.get(&route.path) {
        diags.push(Diagnostic::error(
            "E2807",
            format!(
                "route `{}` is registered both by `{}` and `{}`",
                route.path, prev_prov, route.provenance
            ),
            "explicit builder entries and `.routes(from:)` conventions must not claim the same path (D-WEBAUTHOR1)".to_string(),
            "remove one registration, or rename the convention file".to_string(),
            Some(span),
        ));
        if *prev_span != span {
            diags.push(Diagnostic::error(
                "E2807",
                format!("earlier registration of route `{}`", route.path),
                "both spans are kept so provenance stays audible".to_string(),
                "delete or rename this registration".to_string(),
                Some(*prev_span),
            ));
        }
        return;
    }
    for previous in &graph.routes {
        if routes_overlap(&previous.path, &route.path)
            && route_precedence(&previous.path) == route_precedence(&route.path)
        {
            diags.push(Diagnostic::error(
                "E2807",
                format!(
                    "routes `{}` and `{}` are ambiguous",
                    previous.path, route.path
                ),
                "one checked route graph uses static-before-dynamic precedence; equally specific overlapping patterns have no deterministic owner".to_string(),
                "make one pattern more specific, or remove the duplicate dynamic shape".to_string(),
                Some(span),
            ));
            return;
        }
    }
    seen_paths.insert(route.path.clone(), (route.provenance.clone(), span));
    graph.routes.push(route);
}
fn canonical_route_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.len() > 2 && segment.starts_with('{') && segment.ends_with('}') {
                format!(":{}", &segment[1..segment.len() - 1])
            } else {
                segment.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn route_parts(path: &str) -> Vec<(u8, String)> {
    canonical_route_path(path)
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            if let Some(name) = segment.strip_prefix('*') {
                (2, name.to_string())
            } else if let Some(name) = segment.strip_prefix(':') {
                (1, name.to_string())
            } else {
                (0, segment.to_string())
            }
        })
        .collect()
}

fn routes_overlap(left: &str, right: &str) -> bool {
    let left = route_parts(left);
    let right = route_parts(right);
    let left_catch = left.last().is_some_and(|(kind, _)| *kind == 2);
    let right_catch = right.last().is_some_and(|(kind, _)| *kind == 2);
    if !left_catch && !right_catch && left.len() != right.len() {
        return false;
    }
    let shared = left.len().min(right.len());
    for index in 0..shared {
        let (left_kind, left_name) = &left[index];
        let (right_kind, right_name) = &right[index];
        if *left_kind == 2 || *right_kind == 2 {
            return true;
        }
        if *left_kind == 0 && *right_kind == 0 && left_name != right_name {
            return false;
        }
    }
    left.len() == right.len() || left_catch || right_catch
}

fn route_precedence(path: &str) -> (usize, usize) {
    let parts = route_parts(path);
    (
        parts.iter().filter(|(kind, _)| *kind == 0).count(),
        parts.len(),
    )
}

fn route_precedence_name(path: &str) -> String {
    if route_parts(path).iter().all(|(kind, _)| *kind == 0) {
        "static".to_string()
    } else {
        "static-before-dynamic".to_string()
    }
}

fn derive_route_contracts(
    bundle: &ProgramBundle,
    graph: &mut AppGraph,
    diags: &mut Vec<Diagnostic>,
) {
    let serializable_types = collect_serializable_types(bundle);
    for route_index in 0..graph.routes.len() {
        let (path, handler, span, provenance) = {
            let route = &graph.routes[route_index];
            (
                route.path.clone(),
                route.handler.clone(),
                Span::new(route.span_start, route.span_end),
                route.provenance.clone(),
            )
        };
        let handler_fn = find_route_func(bundle, &graph.routes[route_index], &handler);
        let loader_action = graph
            .actions
            .iter()
            .filter(|action| {
                matches!(action.kind.as_str(), "loader" | "data")
                    && action_matches(&path, &handler, action)
            })
            .last()
            .cloned();
        let loader_fn = loader_action
            .as_ref()
            .and_then(|action| find_func_bundle(bundle, &action.handler));
        let loader_data_type = loader_action.as_ref().map(|action| {
            loader_fn
                .and_then(|function| function.return_type.as_ref())
                .map(|ty| match ty {
                    Type::Result { ok, .. } => ok.show(),
                    ty => ty.show(),
                })
                .unwrap_or_else(|| {
                    if action.output_type.is_empty() {
                        "Unit".to_string()
                    } else {
                        action.output_type.clone()
                    }
                })
        });
        if let Some(action) = &loader_action {
            if let Some(loader_fn) = loader_fn {
                // A loader takes the same typed route inputs as its page;
                // its result must cross the shared wire.
                let _ = derive_route_inputs(
                    bundle,
                    loader_fn,
                    &path,
                    &serializable_types,
                    None,
                    true,
                    diags,
                    span,
                );
                let data_ty = match loader_fn.return_type.as_ref() {
                    Some(Type::Result { ok, .. }) => Some(ok.as_ref().clone()),
                    other => other.cloned(),
                };
                if !data_ty
                    .as_ref()
                    .is_some_and(|ty| is_resume_type_serializable(ty, &serializable_types))
                {
                    diags.push(Diagnostic::error(
                        "E2810",
                        format!(
                            "route `{path}` loader `{}` data `{}` is not Codable",
                            action.handler,
                            loader_data_type.as_deref().unwrap_or("Unit")
                        ),
                        "loader data is serialized once for the page, the router cache, and the browser".to_string(),
                        "return a scalar, collection, or `#[Codable]` value from the loader".to_string(),
                        Some(Span::new(action.span_start, action.span_end)),
                    ));
                }
            }
        }
        let (path_params, search_params, search_codec) = handler_fn
            .map(|function| {
                derive_route_inputs(
                    bundle,
                    function,
                    &path,
                    &serializable_types,
                    loader_data_type.clone(),
                    false,
                    diags,
                    span,
                )
            })
            .unwrap_or_else(|| (Vec::new(), Vec::new(), "query".to_string()));
        let loader = loader_action.map(|action| AppRouteLoader {
            handler: action.handler.clone(),
            data_type: loader_data_type.clone().unwrap_or_else(|| "Unit".to_string()),
            dependency: if action.name.is_empty() {
                action.handler.clone()
            } else {
                action.name.clone()
            },
            preload: action.preload,
            cache_identity: stable_route_identity(&path, &handler),
        });
        let route = &mut graph.routes[route_index];
        route.path = canonical_route_path(&path);
        route.route_identity = stable_route_identity(&route.path, &handler);
        route.path_params = path_params;
        route.search_params = search_params;
        route.search_codec = search_codec;
        route.loader = loader;
        route.precedence = route_precedence_name(&route.path);
        let _ = provenance;
    }
}

fn find_route_func<'a>(
    bundle: &'a ProgramBundle,
    route: &AppRoute,
    handler: &str,
) -> Option<&'a Func> {
    find_func_bundle(bundle, handler).or_else(|| {
        bundle
            .modules
            .iter()
            .find(|module| module.display.ends_with(&route.provenance))
            .and_then(|module| find_func(&module.items, "page"))
    })
}

fn find_func_bundle<'a>(bundle: &'a ProgramBundle, name: &str) -> Option<&'a Func> {
    bundle
        .modules
        .iter()
        .find_map(|module| find_func(&module.items, name))
}

fn derive_route_inputs(
    bundle: &ProgramBundle,
    function: &Func,
    path: &str,
    serializable_types: &HashSet<String>,
    loader_data_type: Option<String>,
    loader: bool,
    diags: &mut Vec<Diagnostic>,
    span: Span,
) -> (Vec<AppRouteField>, Vec<AppRouteField>, String) {
    let mut path_params = Vec::new();
    let mut used = HashSet::new();
    let route_names = route_parts(path)
        .into_iter()
        .filter(|(kind, _)| *kind != 0)
        .map(|(_, name)| name)
        .collect::<Vec<_>>();
    for (position, name) in route_names.iter().enumerate() {
        let selected = function
            .params
            .iter()
            .enumerate()
            .find(|(index, parameter)| parameter.name == *name && !used.contains(index));
        let Some((index, parameter)) = selected else {
            diags.push(Diagnostic::error(
                "E2810",
                format!("route `{path}` has no typed handler input for path parameter `{name}`"),
                "every dynamic segment must map to one handler parameter in the checked App graph".to_string(),
                format!("add `{name}: T` to the `{}` handler", function.name),
                Some(span),
            ));
            continue;
        };
        used.insert(index);
        if matches!(&parameter.ty, Type::Option(_)) {
            diags.push(Diagnostic::error(
                "E2810",
                format!("route `{path}` path parameter `{name}` cannot be optional"),
                "path segments are always present; only search fields may be omitted".to_string(),
                "make the path input required".to_string(),
                Some(parameter.ty_span),
            ));
        }
        path_params.push(route_field(
            name,
            &parameter.ty,
            parameter.default.as_deref(),
            route_codec(&parameter.ty),
            true,
        ));
        let _ = position;
    }

    // Remaining inputs: at most one search binding (a Codable record, or a
    // scalar field named `search`/`query`) and, for page handlers, one
    // `data` input carrying the route's loader data.  TIR serializes exactly
    // this binding for the runtime decoder, so the rule is by name and type,
    // never by position.
    let mut search_params = Vec::new();
    let mut search_codec = "query".to_string();
    let mut search_bound = false;
    for (index, parameter) in function.params.iter().enumerate() {
        if used.contains(&index) {
            continue;
        }
        if parameter.name == "data" {
            if loader_data_type.is_none() {
                diags.push(Diagnostic::error(
                    "E2810",
                    format!(
                        "route `{path}` handler `{}` reads `data` but declares no loader",
                        function.name
                    ),
                    "route data comes only from the route's `.loader(handler, loader)` registration".to_string(),
                    "declare a loader for this route or remove the `data` input".to_string(),
                    Some(parameter.ty_span),
                ));
            } else if loader_data_type.as_deref() != Some(parameter.ty.show().as_str()) {
                diags.push(Diagnostic::error(
                    "E2810",
                    format!(
                        "route `{path}` `data` input `{}` does not match loader data `{}`",
                        parameter.ty.show(),
                        loader_data_type.as_deref().unwrap_or("Unit")
                    ),
                    "the loader's success type is the route data type".to_string(),
                    "declare the `data` input with the loader's return type".to_string(),
                    Some(parameter.ty_span),
                ));
            }
            used.insert(index);
            continue;
        }
        let record = named_type(&parameter.ty)
            .and_then(|name| find_struct_bundle(bundle, name).map(|structure| (name, structure)));
        let is_search = record.is_some() || matches!(parameter.name.as_str(), "search" | "query");
        if !is_search {
            continue;
        }
        if search_bound {
            diags.push(Diagnostic::error(
                "E2810",
                format!(
                    "route `{path}` handler `{}` binds more than one search input",
                    function.name
                ),
                "the checked route graph accepts one search record or one scalar search field".to_string(),
                "merge the search inputs into one Codable record".to_string(),
                Some(parameter.ty_span),
            ));
            used.insert(index);
            continue;
        }
        search_bound = true;
        used.insert(index);
        match record {
            Some((name, structure)) => {
                if !serializable_types.contains(name) {
                    diags.push(Diagnostic::error(
                        "E2810",
                        format!(
                            "route `{path}` search input `{}` is not Codable",
                            parameter.name
                        ),
                        "search records use the shared JSON codec and must have a checked Codable shape".to_string(),
                        format!("derive `#[Codable]` for `{name}`"),
                        Some(parameter.ty_span),
                    ));
                }
                search_params = structure
                    .fields
                    .iter()
                    .filter(|field| field.is_pub && field.computed.is_none())
                    .map(|field| {
                        route_field(
                            &field.name,
                            &field.ty,
                            field.default.as_deref(),
                            route_codec(&field.ty),
                            !is_optional_type(&field.ty) && field.default.is_none(),
                        )
                    })
                    .collect();
                search_codec = "json".to_string();
            }
            None => {
                search_params = vec![route_field(
                    &parameter.name,
                    &parameter.ty,
                    parameter.default.as_deref(),
                    route_codec(&parameter.ty),
                    !is_optional_type(&parameter.ty) && parameter.default.is_none(),
                )];
            }
        }
    }
    for (index, parameter) in function.params.iter().enumerate() {
        if !used.contains(&index) {
            diags.push(Diagnostic::error(
                "E2810",
                format!(
                    "route `{path}` handler `{}` has unbound input `{}`",
                    function.name, parameter.name
                ),
                "the checked route graph binds path parameters by name, one search record or a `search`/`query` field, and the loader `data`".to_string(),
                "rename the input to its path segment, `search`, `query`, or `data`, or remove it".to_string(),
                Some(parameter.ty_span),
            ));
        }
    }
    if !matches!(
        function.return_type.as_ref(),
        Some(Type::Named(name)) if name == "WebPage"
    ) && !matches!(
        function.return_type.as_ref(),
        Some(Type::Result { ok, .. }) if matches!(ok.as_ref(), Type::Named(name) if name == "WebPage")
    ) && !loader {
        diags.push(Diagnostic::error(
            "E2810",
            format!(
                "route `{path}` handler `{}` must return `WebPage`",
                function.name
            ),
            "page handlers render one WebPage or fail with a declared failure domain".to_string(),
            "declare the handler as `WebPage` or `WebPage !E`".to_string(),
            Some(span),
        ));
    }
    (path_params, search_params, search_codec)
}

fn find_struct_bundle<'a>(
    bundle: &'a ProgramBundle,
    name: &str,
) -> Option<&'a crate::AST::StructDef> {
    bundle
        .modules
        .iter()
        .find_map(|module| find_struct(&module.items, name))
}

fn find_struct<'a>(items: &'a [Item], name: &str) -> Option<&'a crate::AST::StructDef> {
    for item in items {
        match item {
            Item::Struct(def) if def.name == name => return Some(def),
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    if let Some(def) = find_struct(body, name) {
                        return Some(def);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn named_type(ty: &Type) -> Option<&str> {
    match ty {
        Type::Named(name) => Some(name.as_str()),
        Type::Option(inner) => named_type(inner),
        _ => None,
    }
}

fn is_optional_type(ty: &Type) -> bool {
    matches!(ty, Type::Option(_))
}

fn route_codec(ty: &Type) -> String {
    match ty {
        Type::Option(inner) => route_codec(inner),
        Type::Int | Type::IntN { .. } => "int".to_string(),
        Type::Float => "float".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Named(name) if matches!(name.as_str(), "JSON" | "Data") => "json".to_string(),
        _ => "string".to_string(),
    }
}

fn const_wire_text(expr: Option<&Expr>) -> Option<String> {
    match expr? {
        Expr::Str(parts, _) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    StrPart::Lit(value) => text.push_str(value),
                    StrPart::Interp(..) => return None,
                }
            }
            Some(text)
        }
        Expr::Int(value, ..) => Some(value.to_string()),
        Expr::Float(value, ..) => Some(value.to_string()),
        Expr::Bool(value, _) => Some(value.to_string()),
        Expr::Char(value, _) => Some(value.to_string()),
        _ => None,
    }
}

fn route_field(
    name: &str,
    ty: &Type,
    default_expr: Option<&Expr>,
    codec: String,
    required: bool,
) -> AppRouteField {
    AppRouteField {
        name: name.to_string(),
        ty: ty.show(),
        codec,
        required,
        default: const_wire_text(default_expr),
    }
}

fn derive_render_facts(
    graph: &mut AppGraph,
    items: &[Item],
    module_alias: &str,
    solved: Option<&HashMap<String, EffectSet>>,
    serializable_types: &HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    for route_index in 0..graph.routes.len() {
        let (path, handler, override_mode, existing_cache) = {
            let route = &graph.routes[route_index];
            (
                route.path.clone(),
                route.handler.clone(),
                route.render_facts.override_mode,
                route.render_facts.cache,
            )
        };
        let handler_fn = find_func(items, &handler);
        let mut effect_names = BTreeSet::new();
        let mut capture_funcs = Vec::new();
        if let Some(function) = handler_fn {
            append_function_effects(function, module_alias, solved, &mut effect_names);
            capture_funcs.push(function);
        }

        let matching_actions: Vec<(String, String, String)> = graph
            .actions
            .iter()
            .filter(|action| action_matches(&path, &handler, action))
            .map(|action| {
                (
                    action.kind.clone(),
                    action.name.clone(),
                    action.handler.clone(),
                )
            })
            .collect();
        let mut loader = None;
        let mut form = None;
        for (kind, name, action_handler) in &matching_actions {
            if matches!(kind.as_str(), "data" | "loader") {
                loader = Some(action_handler.clone());
            }
            if matches!(kind.as_str(), "form" | "action") {
                form = Some(action_handler.clone());
            }
            if let Some(function) = find_func(items, action_handler) {
                append_function_effects(function, module_alias, solved, &mut effect_names);
                if matches!(kind.as_str(), "form" | "action") {
                    capture_funcs.push(function);
                }
            }
            let _ = name;
        }

        let browser_effect = effect_names.iter().any(|effect| {
            effect_root(effect) == "Browser"
        }) || handler_fn.is_some_and(|function| {
            matches!(
                function.web_marker,
                Some(jet_foundation::WebPartition::WebPartitionMarker::JS)
            )
        });
        let runtime_effect = effect_names.iter().any(|effect| is_runtime_effect(effect));
        let reactive = handler_fn.is_some_and(|function| function.is_reactive);
        let interactive = browser_effect || reactive || form.is_some();
        let derived_mode = if browser_effect {
            AppRenderFactMode::Client
        } else if interactive {
            AppRenderFactMode::ServerIsland
        } else if loader.is_some() {
            AppRenderFactMode::ServerStream
        } else if runtime_effect {
            AppRenderFactMode::Server
        } else {
            AppRenderFactMode::Static
        };
        let mode = override_mode.unwrap_or(derived_mode);
        let pending_boundary_id = matches!(
            mode,
            AppRenderFactMode::ServerStream | AppRenderFactMode::ServerIsland
        )
        .then(|| stable_pending_boundary_id(&path, &handler));

        let mut reason = Vec::new();
        if let Some(explicit) = override_mode {
            reason.push(format!("explicit render mode `{}`", explicit.as_str()));
        } else {
            if browser_effect {
                reason.push(format!("handler `{handler}` reaches Browser"));
            }
            if reactive {
                reason.push(format!("handler `{handler}` is reactive"));
            }
            if let Some(loader) = &loader {
                reason.push(format!("loader `{loader}` supplies runtime data"));
            }
            if let Some(form) = &form {
                reason.push(format!("form/action `{form}` adds interactivity"));
            }
            if runtime_effect && loader.is_none() {
                reason.push(format!(
                    "handler `{handler}` reaches runtime effect(s): {}",
                    effect_names.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if reason.is_empty() {
                reason.push(format!("handler `{handler}` has no runtime data or interactivity"));
            }
        }

        let cache = if existing_cache != AppCacheFact::Unspecified {
            existing_cache
        } else {
            graph
                .policy
                .cache
                .iter()
                .rev()
                .find_map(|policy| AppCacheFact::parse(policy))
                .unwrap_or(AppCacheFact::Unspecified)
        };
        let needs_island =
            interactive || matches!(mode, AppRenderFactMode::ServerIsland | AppRenderFactMode::Client);
        let island = if needs_island {
            let mut captures = BTreeMap::new();
            for function in capture_funcs {
                for param in &function.params {
                    let ty = param.ty.show();
                    let serializable =
                        is_resume_type_serializable(&param.ty, serializable_types);
                    captures.entry(param.name.clone()).or_insert_with(|| AppResumeCapture {
                        name: param.name.clone(),
                        ty,
                        serializable,
                    });
                }
            }
            let captures: Vec<AppResumeCapture> = captures.into_values().collect();
            let payload_serializable = captures.iter().all(|capture| capture.serializable);
            for capture in &captures {
                if !capture.serializable {
                    diags.push(Diagnostic::error(
                        "E_WEB_SERIALIZE_CAPTURE",
                        format!(
                            "route `{path}` island capture `{}` of type `{}` cannot be resumed",
                            capture.name, capture.ty
                        ),
                        "resumable islands serialize their compiler-derived payload before browser execution"
                            .to_string(),
                        "use a scalar, collection, or `#[Codable]` value, or move the resource behind a server action"
                            .to_string(),
                        Some(Span::new(
                            graph.routes[route_index].span_start,
                            graph.routes[route_index].span_end,
                        )),
                    ));
                }
            }
            let identity = stable_island_identity(&path, &handler);
            Some(AppIslandFact {
                identity,
                hydration_trigger: AppHydrationTrigger::Immediate,
                resume_payload: AppResumePayload {
                    captures,
                    serializable: payload_serializable,
                },
            })
        } else {
            None
        };

        let route = &mut graph.routes[route_index];
        route.render_facts = AppRenderFacts {
            mode,
            reason,
            effects: effect_names.into_iter().collect(),
            interactive: interactive || matches!(mode, AppRenderFactMode::ServerIsland | AppRenderFactMode::Client),
            loader,
            form,
            cache,
            override_mode,
            pending_boundary_id,
            island,
        };
    }
}

fn action_matches(path: &str, handler: &str, action: &AppAction) -> bool {
    action.name == path
        || action.name == handler
        || action.name == "*"
        || (path == "/" && action.name.is_empty())
}

fn append_function_effects(
    function: &Func,
    module_alias: &str,
    solved: Option<&HashMap<String, EffectSet>>,
    out: &mut BTreeSet<String>,
) {
    if let Some(solved) = solved {
        if let Some(effects) = lookup_effects(solved, module_alias, &function.name) {
            out.extend(effects.iter().cloned());
            return;
        }
    }
    if let Some(declared) = &function.declared_effects {
        out.extend(declared.iter().map(|(name, _)| name.clone()));
    }
}

fn lookup_effects<'a>(
    solved: &'a HashMap<String, EffectSet>,
    module_alias: &str,
    function: &str,
) -> Option<&'a EffectSet> {
    let qualified = format!("{module_alias}::{function}");
    solved
        .get(&qualified)
        .or_else(|| solved.get(function))
        .or_else(|| {
            solved.iter().find_map(|(key, effects)| {
                (key.rsplit("::").next() == Some(function)
                    || key.rsplit("__").next() == Some(function))
                .then_some(effects)
            })
        })
}

fn effect_root(effect: &str) -> &str {
    effect.split('.').next().unwrap_or(effect)
}

fn is_runtime_effect(effect: &str) -> bool {
    matches!(
        effect_root(effect),
        "DB" | "FS" | "Net" | "IO" | "Time" | "Rand" | "Env" | "Exec" | "Secret"
    ) || effect_root(effect) == "RequestContext"
}

fn find_func<'a>(items: &'a [Item], name: &str) -> Option<&'a Func> {
    for item in items {
        match item {
            Item::Func(function) if function.name == name => return Some(function),
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    if let Some(function) = find_func(body, name) {
                        return Some(function);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn collect_serializable_types(bundle: &ProgramBundle) -> HashSet<String> {
    fn collect(items: &[Item], out: &mut HashSet<String>) {
        for item in items {
            match item {
                Item::Struct(def) if has_serde_derive(&def.derives) => {
                    out.insert(def.name.clone());
                }
                Item::Enum(def) if has_serde_derive(&def.derives) => {
                    out.insert(def.name.clone());
                }
                Item::CodeModule(module) => {
                    if let Some(body) = &module.body {
                        collect(body, out);
                    }
                }
                _ => {}
            }
        }
    }

    let mut out = HashSet::new();
    for module in &bundle.modules {
        collect(&module.items, &mut out);
    }
    out
}

fn has_serde_derive(derives: &[(String, Span)]) -> bool {
    derives
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "Codable" | "Encode" | "Decode"))
}

fn is_resume_type_serializable(ty: &Type, named: &HashSet<String>) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::IntN { .. }
        | Type::Float32
        | Type::Measure(_) => true,
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::InlineRange { base: inner, .. }
        | Type::Tagged { inner, .. }
        | Type::Quantity { base: inner, .. } => is_resume_type_serializable(inner, named),
        Type::FixedList { elem, .. } => is_resume_type_serializable(elem, named),
        Type::Map { key, value, .. } => {
            matches!(key.as_ref(), Type::String)
                && is_resume_type_serializable(value, named)
        }
        Type::Result { ok, err } => {
            is_resume_type_serializable(ok, named) && is_resume_type_serializable(err, named)
        }
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, field)| is_resume_type_serializable(field, named)),
        Type::Union(members) => members
            .iter()
            .all(|member| is_resume_type_serializable(member, named)),
        Type::Named(name) => {
            matches!(
                name.as_str(),
                "Data" | "JSON" | "Date" | "DateTime" | "Decimal" | "Duration" | "Unit"
            ) || named.contains(name)
        }
        Type::Apply { name, args } => {
            named.contains(name)
                && args
                    .iter()
                    .all(|arg| is_resume_type_serializable(arg, named))
        }
        Type::Fn { .. } | Type::TraitObject(_) => false,
    }
}

fn expand_routes_from(
    bundle: &ProgramBundle,
    root: &AppRoutesFrom,
    graph: &mut AppGraph,
    diags: &mut Vec<Diagnostic>,
    seen_paths: &mut HashMap<String, (String, Span)>,
    render: AppRenderMode,
    render_override: Option<AppRenderFactMode>,
) {
    let dir = bundle.project_root.join(&root.root);
    if !dir.is_dir() {
        diags.push(Diagnostic::error(
            "E2806",
            format!("`.routes(from: \"{}\")` directory does not exist", root.root),
            "file routing expands only an explicit builder root; a missing directory cannot invent endpoints".to_string(),
            format!("create `{}/`, or remove the `.routes(from:)` line", root.root),
            Some(Span::new(root.span_start, root.span_end)),
        ));
        return;
    }

    let mut files = Vec::new();
    if let Err(err) = collect_jet_files(&dir, &dir, &mut files) {
        diags.push(Diagnostic::error(
            "E2806",
            format!("could not read `.routes(from: \"{}\")`: {err}", root.root),
            "convention expansion must see every file under the declared root".to_string(),
            "fix directory permissions, or remove the opt-in".to_string(),
            Some(Span::new(root.span_start, root.span_end)),
        ));
        return;
    }
    files.sort();

    for rel in &files {
        let file_name = Path::new(rel)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        // Leading `_` = explicitly non-routed (D-WEBAUTHOR1).
        if file_name.starts_with('_') {
            continue;
        }
        let file_path = dir.join(rel);
        if !convention_has_page(&file_path) {
            diags.push(Diagnostic::error(
                "E2806",
                format!("convention file `{provenance}` has no `fn page`", provenance = format!("{}/{}", root.root, rel)),
                "every file under a `.routes(from:)` root must declare `fn page` or start with `_` to opt out (D-WEBAUTHOR1)".to_string(),
                "add `fn page()`, rename the file with a leading `_`, or remove it from the routes directory".to_string(),
                Some(Span::new(root.span_start, root.span_end)),
            ));
            continue;
        }
        let path = convention_path(rel);
        let provenance = format!("{}/{}", root.root, rel);
        let handler = convention_handler(rel);
        let render_facts = render_override.map_or_else(AppRenderFacts::default, |mode| {
            AppRenderFacts {
                mode,
                reason: vec![format!("explicit render mode `{}`", mode.as_str())],
                override_mode: Some(mode),
                ..AppRenderFacts::default()
            }
        });
        record_route(
            graph,
            diags,
            seen_paths,
            AppRoute {
                path: path.clone(),
                handler: handler.clone(),
                route_identity: stable_route_identity(&path, &handler),
                path_params: Vec::new(),
                search_params: Vec::new(),
                search_codec: "query".to_string(),
                loader: None,
                boundaries: AppRouteBoundaries::default(),
                precedence: route_precedence_name(&path),
                render,
                render_facts,
                provenance,
                span_start: root.span_start,
                span_end: root.span_end,
            },
        );
    }
}

fn collect_jet_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_jet_files(root, &path, out)?;
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("jet") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(rel);
    }
    Ok(())
}

fn convention_path(rel: &str) -> String {
    let stem = Path::new(rel).with_extension("");
    let mut parts: Vec<&str> = stem.iter().filter_map(|s| s.to_str()).collect();
    if parts.last().copied() == Some("index") || parts.last().copied() == Some("page") {
        parts.pop();
    }
    if parts.is_empty() {
        return "/".to_string();
    }
    let mut path = String::new();
    for part in parts {
        path.push('/');
        if part.starts_with('[') && part.ends_with(']') {
            let name = &part[1..part.len() - 1];
            path.push(':');
            path.push_str(name);
        } else {
            path.push_str(part);
        }
    }
    path
}

fn convention_handler(rel: &str) -> String {
    rel.trim_end_matches(".jet")
        .replace(['/', '\\', '[', ']'], "_")
}

fn convention_has_page(path: &Path) -> bool {
    let Ok(src) = fs::read_to_string(path) else {
        return false;
    };
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("fn page") || trimmed.starts_with("pub fn page") {
            return true;
        }
    }
    false
}

fn const_string_arg(args: &[crate::AST::CallArg], index: usize) -> Option<String> {
    args.get(index).and_then(|a| const_string_expr(&a.expr))
}
fn const_int_arg(args: &[crate::AST::CallArg], index: usize) -> Option<i64> {
    args.get(index).and_then(|arg| const_int_expr(&arg.expr))
}

fn const_int_expr(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Int(value, ..) => Some(*value),
        Expr::Paren(inner, _) => const_int_expr(inner),
        Expr::Unary(crate::AST::UnOp::Neg, inner, _) => const_int_expr(inner)?.checked_neg(),
        _ => None,
    }
}

fn labeled_string_arg(args: &[crate::AST::CallArg], label: &str) -> Option<String> {
    args.iter()
        .find(|a| a.label.as_ref().is_some_and(|(name, _)| name == label))
        .and_then(|a| const_string_expr(&a.expr))
}

fn labeled_bool_arg(args: &[crate::AST::CallArg], label: &str) -> Option<bool> {
    args.iter()
        .find(|a| a.label.as_ref().is_some_and(|(name, _)| name == label))
        .and_then(|a| const_bool_expr(&a.expr))
        .or_else(|| {
            args.iter()
                .rev()
                .find(|a| a.label.is_none())
                .and_then(|a| const_bool_expr(&a.expr))
        })
}

fn const_bool_expr(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Bool(value, _) => Some(*value),
        _ => None,
    }
}

fn const_string_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Str(parts, _) => {
            let mut out = String::new();
            for part in parts {
                match part {
                    StrPart::Lit(text) => out.push_str(text),
                    StrPart::Interp(..) => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
}
fn render_mode_arg(args: &[crate::AST::CallArg]) -> Option<AppRenderFactMode> {
    let expr = &args.first()?.expr;
    let name = const_string_expr(expr).or_else(|| handler_name(args.first()))?;
    AppRenderFactMode::parse(&name)
}

fn legacy_render_mode(mode: AppRenderFactMode) -> AppRenderMode {
    match mode {
        AppRenderFactMode::Static => AppRenderMode::Ssg,
        AppRenderFactMode::Server => AppRenderMode::Ssr,
        AppRenderFactMode::ServerStream => AppRenderMode::Stream,
        AppRenderFactMode::ServerIsland => AppRenderMode::Island,
        AppRenderFactMode::Client => AppRenderMode::Csr,
    }
}

fn handler_name(arg: Option<&crate::AST::CallArg>) -> Option<String> {
    let expr = &arg?.expr;
    match expr {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Field(_, field, _) => Some(field.clone()),
        _ => None,
    }
}
