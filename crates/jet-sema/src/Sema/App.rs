//! D-WEBAPP1=D / D-WEBAUTHOR1=D: extract the `fn run` App builder into one typed
//! application graph and diagnose undeclared dynamic edges, stray convention
//! files, and builder/file collisions.

use crate::AST::{Expr, Item, ProgramBundle, Stmt, StrPart};
use crate::Diagnostics::{Diagnostic, Span};
use jet_foundation::App::{
    AppAction, AppGraph, AppMount, AppRoute, AppRoutesFrom, AppRenderMode,
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Walk the entry module for the App-returning `fn run` and record the static
/// builder graph.
pub fn extract_app_graph(bundle: &ProgramBundle) -> (Option<AppGraph>, Vec<Diagnostic>) {
    let Some(module) = bundle.modules.get(bundle.entry) else {
        return (None, Vec::new());
    };
    let Some(run_fn) = find_app_run_fn(&module.items) else {
        return (None, Vec::new());
    };

    let mut graph = AppGraph {
        entry_file: module.display.clone(),
        hydration: "dev-overlay".to_string(),
        shared_tir: true,
        ..AppGraph::default()
    };
    let known_fns: HashMap<String, ()> = collect_fn_names(&module.items)
        .into_iter()
        .map(|n| (n, ()))
        .collect();
    let mut diags = Vec::new();
    let mut current_render = AppRenderMode::Csr;
    let mut seen_paths: HashMap<String, (String, Span)> = HashMap::new();

    for stmt in &run_fn.body {
        if let Some(expr) = stmt_expr(stmt) {
            walk_builder(
                expr,
                &mut graph,
                &mut diags,
                &mut current_render,
                &mut seen_paths,
                &known_fns,
                "builder",
            );
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
        );
    }

    (Some(graph), diags)
}

fn find_app_run_fn(items: &[Item]) -> Option<&crate::AST::Func> {
    for item in items {
        match item {
            Item::Func(f) if f.name == "run" && returns_app(f.return_type.as_ref()) => {
                return Some(f)
            }
            Item::CodeModule(cm) => {
                if let Some(body) = &cm.body {
                    if let Some(f) = find_app_run_fn(body) {
                        return Some(f);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn returns_app(ty: Option<&crate::AST::Type>) -> bool {
    match ty {
        Some(crate::AST::Type::Named(name)) => name == "App",
        Some(crate::AST::Type::Result { ok, .. }) => {
            matches!(ok.as_ref(), crate::AST::Type::Named(name) if name == "App")
        }
        _ => false,
    }
}

fn collect_fn_names(items: &[Item]) -> Vec<String> {
    let mut names = Vec::new();
    for item in items {
        match item {
            Item::Func(f) => names.push(f.name.clone()),
            Item::CodeModule(cm) => {
                if let Some(body) = &cm.body {
                    names.extend(collect_fn_names(body));
                }
            }
            _ => {}
        }
    }
    names
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
    seen_paths: &mut HashMap<String, (String, Span)>,
    known_fns: &HashMap<String, ()>,
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

fn apply_method(
    method: &str,
    args: &[crate::AST::CallArg],
    method_span: Span,
    graph: &mut AppGraph,
    diags: &mut Vec<Diagnostic>,
    current_render: &mut AppRenderMode,
    seen_paths: &mut HashMap<String, (String, Span)>,
    known_fns: &HashMap<String, ()>,
    provenance: &str,
) {
    match method {
        "route" | "page" | "layout" => {
            let path = const_string_arg(args, 0).unwrap_or_else(|| "/".to_string());
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
            record_route(
                graph,
                diags,
                seen_paths,
                AppRoute {
                    path,
                    handler,
                    render: *current_render,
                    provenance: provenance.to_string(),
                    span_start: method_span.start,
                    span_end: method_span.end,
                },
            );
        }
        "action" | "form" | "data" => {
            let name = const_string_arg(args, 0).unwrap_or_else(|| method.to_string());
            let handler = handler_name(args.get(1)).unwrap_or_else(|| "<dynamic>".to_string());
            if handler == "<dynamic>" || !known_fns.contains_key(&handler) {
                diags.push(Diagnostic::error(
                    "E2810",
                    format!("`{method}` `{name}` is not a statically known handler"),
                    "D-WEBAPP1 records actions/forms/data on the typed application graph; a runtime value outside `.mount` is an unanalyzed edge".to_string(),
                    "pass a named function, or declare a typed `.mount` for dynamic registration".to_string(),
                    Some(method_span),
                ));
            }
            graph.actions.push(AppAction {
                name,
                handler,
                kind: method.to_string(),
                provenance: provenance.to_string(),
                span_start: method_span.start,
                span_end: method_span.end,
            });
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
                    "file routing activates only through an explicit builder opt-in (D-WEBAUTHOR1)".to_string(),
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
        "csr" => *current_render = AppRenderMode::Csr,
        "ssr" => *current_render = AppRenderMode::Ssr,
        "ssg" => *current_render = AppRenderMode::Ssg,
        "stream" | "streaming" => *current_render = AppRenderMode::Stream,
        "island" => *current_render = AppRenderMode::Island,
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
            if let Some(s) = const_string_arg(args, 0) {
                graph.policy.cache.push(s);
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
    seen_paths.insert(route.path.clone(), (route.provenance.clone(), span));
    graph.routes.push(route);
}

fn expand_routes_from(
    bundle: &ProgramBundle,
    root: &AppRoutesFrom,
    graph: &mut AppGraph,
    diags: &mut Vec<Diagnostic>,
    seen_paths: &mut HashMap<String, (String, Span)>,
    render: AppRenderMode,
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
        record_route(
            graph,
            diags,
            seen_paths,
            AppRoute {
                path,
                handler,
                render,
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

fn labeled_string_arg(args: &[crate::AST::CallArg], label: &str) -> Option<String> {
    args.iter()
        .find(|a| a.label.as_ref().is_some_and(|(name, _)| name == label))
        .and_then(|a| const_string_expr(&a.expr))
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

fn handler_name(arg: Option<&crate::AST::CallArg>) -> Option<String> {
    let expr = &arg?.expr;
    match expr {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Field(_, field, _) => Some(field.clone()),
        _ => None,
    }
}
