//! D-WEBBACKEND1 / D-WEBKIND1 / D-DOMGEN1 (c123 M2): WASM + JS web backend emission.

use super::{
    build_cx_items, bundle_extern_funcs, populate_cx_from_bundle, register_foreign_enum_variants,
    update_cloneability_with_foreign_types, mangle, Cx, TIR,
};
use crate::Diagnostics::Span;
use crate::Sema::CompileMode;
use crate::Syntax;
use crate::AST::{AccessConvention, FfiLink, Func, Item, ProgramBundle, Type};
use jet_foundation::WebPartition::{WebBucket, WebPartitionMarker};

/// Generated web backend artifacts (WASM Rust, JS loader/app, DOM shim, manifest).
#[derive(Debug, Clone)]
pub struct WebArtifacts {
    pub manifest_json: String,
    pub wasm_rust: String,
    pub js_app: String,
    pub dom_runtime: String,
    /// D-DOMGEN1=A (Phase 7 extension): a minimal host page that loads
    /// `app.js` as an ES module and runs `jet_main()` — so `jet build --target
    /// web` produces something openable in a browser, not just source files.
    /// Generic on purpose: it doesn't know about any app-specific exported
    /// function beyond `jet_main`. An example that wants real interactivity
    /// (a button calling an exported function) ships its own companion HTML
    /// alongside the `.jet` source instead of relying on this default.
    pub index_html: String,
    /// D-HTMLPAIR1 (ratified 2026-07-01, c134): the entry file's `@Html("path.html")`
    /// marker, if any — relative to the `.jet` source's own directory.
    pub explicit_html_path: Option<String>,
    /// D-SHAPE-CLI-CARRIER1=A: canonical record embedded as a Wasm custom
    /// section by the artifact writer before publication/signing.
    pub command_record: Vec<u8>,
}

const DOM_RUNTIME: &str = include_str!("../Prelude/DomRuntime.js");

/// D-WEBTIR1=A: data-only fact reported by codegen's TIR coverage gate.
/// The driver/API layer owns the user-facing diagnostic text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebTirUnsupported {
    pub func_name: String,
    pub span: Span,
}

pub type WebEmitResult<T> = Result<T, WebTirUnsupported>;

struct FuncWeb {
    name: String,
    key: String,
    file_prefix: Option<String>,
    bucket: WebBucket,
    marker: Option<WebPartitionMarker>,
    span: Span,
    params: Vec<(String, Type)>,
    return_type: Option<Type>,
    tir: TIR::TFunc,
}

pub fn emit_web(
    bundle: &ProgramBundle,
    _mode: CompileMode,
    _link: Option<&FfiLink>,
) -> WebEmitResult<WebArtifacts> {
    let funcs = collect_web_funcs(bundle);
    let manifest_json = emit_manifest(bundle, &funcs);
    let wasm_rust = emit_wasm_rust(bundle, &funcs)?;
    let js_app = emit_js_app(bundle, &funcs)?;
    Ok(WebArtifacts {
        manifest_json,
        wasm_rust,
        js_app,
        dom_runtime: DOM_RUNTIME.to_string(),
        index_html: emit_index_html(),
        explicit_html_path: bundle.modules[bundle.entry].html_path.clone(),
        command_record: jet_foundation::CliSchema::encode_record(
            &jet_foundation::CliSchema::executable_schema(bundle),
        ),
    })
}

/// D-WEBTIR1=A: every executable web body must pass through the same checked
/// TIR boundary as native code before the AST-shaped web emitter is allowed to
/// read it. A miss is a Jet diagnostic, not silent JS/WASM drift.
pub fn validate_web_tir_support(
    bundle: &ProgramBundle,
    link: Option<&FfiLink>,
) -> Vec<WebTirUnsupported> {
    let extern_funcs = bundle_extern_funcs(bundle);
    let mut diags = Vec::new();
    for (i, module) in bundle.modules.iter().enumerate() {
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            link,
            &extern_funcs,
        );
        populate_cx_from_bundle(&mut cx, bundle, i);
        register_foreign_enum_variants(&mut cx, bundle, i);
        update_cloneability_with_foreign_types(&mut cx, &module.items);
        validate_web_items_tir(
            &module.items,
            &cx,
            bundle,
            None,
            (i != bundle.entry).then_some(module.alias.as_str()),
            bundle.modules[bundle.entry].html_path.is_some(),
            &mut diags,
        );
    }
    diags
}

fn validate_web_items_tir(
    items: &[Item],
    cx: &Cx,
    bundle: &ProgramBundle,
    module_prefix: Option<&str>,
    file_prefix: Option<&str>,
    explicit_html: bool,
    diags: &mut Vec<WebTirUnsupported>,
) {
    for item in items {
        match item {
            Item::Func(f) => {
                let key = module_prefix
                    .map(|m| format!("{m}__{}", f.name))
                    .or_else(|| file_prefix.map(|m| format!("{m}__{}", f.name)))
                    .unwrap_or_else(|| f.name.clone());
                let bucket = bundle.web_partitions.get(&key).copied().unwrap_or(WebBucket::Wasm);
                let has_wasm_export = bundle.modules.iter().any(|m| items_have_wasm_export(&m.items));
                // `dev()` is the host-side programmable dev-server entry. It
                // executes before the web build and is never web-runtime code.
                let emitted = key != "dev"
                    && (bucket == WebBucket::Js || !explicit_html || has_wasm_export);
                validate_web_func_tir(f, cx, bundle, file_prefix, bucket, emitted, diags);
            }
            Item::CodeModule(cm) => {
                if let Some(body) = &cm.body {
                    validate_web_items_tir(
                        body,
                        cx,
                        bundle,
                        Some(&cm.name),
                        file_prefix,
                        explicit_html,
                        diags,
                    );
                }
            }
            _ => {}
        }
    }
}

fn items_have_wasm_export(items: &[Item]) -> bool {
    items.iter().any(|item| match item {
        Item::Func(f) => f.web_marker == Some(WebPartitionMarker::WasmExport),
        Item::CodeModule(cm) => cm.body.as_deref().map(items_have_wasm_export).unwrap_or(false),
        _ => false,
    })
}

fn validate_web_func_tir(
    f: &Func,
    cx: &Cx,
    bundle: &ProgramBundle,
    file_prefix: Option<&str>,
    bucket: WebBucket,
    require_web_emit: bool,
    diags: &mut Vec<WebTirUnsupported>,
) {
    *cx.current_type_params.borrow_mut() = f.type_params.iter().map(|p| p.name.clone()).collect();
    let covered = f.pre.is_empty() && f.post.is_empty() && TIR::tir_covers(f, cx);
    if covered {
        let tir = TIR::lower_web_func(f, cx);
        let supported = if !require_web_emit {
            true
        } else if bucket == WebBucket::Js {
            web_stmts_supported(&tir.body)
                && (tir.ret.is_none() || web_stmts_guarantee_return(&tir.body))
        } else {
            web_wasm_stmts_supported(
                &tir.body,
                bundle,
                file_prefix,
                &tir.web_param_reconstructions,
            )
                && (tir.ret.is_none() || web_stmts_guarantee_return(&tir.body))
                && web_wasm_abi_supported(f, &tir)
        };
        if supported {
            cx.current_type_params.borrow_mut().clear();
            return;
        }
        cx.current_type_params.borrow_mut().clear();
    } else {
        cx.current_type_params.borrow_mut().clear();
    }
    diags.push(WebTirUnsupported {
        func_name: f.name.clone(),
        span: f.name_span,
    });
}

fn web_stmts_guarantee_return(stmts: &[TIR::TStmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        TIR::TStmt::Return(Some(_)) => true,
        TIR::TStmt::If { then_body, else_body: Some(else_body), .. } => {
            web_stmts_guarantee_return(then_body) && web_stmts_guarantee_return(else_body)
        }
        TIR::TStmt::Inline(body) | TIR::TStmt::Region(body) => web_stmts_guarantee_return(body),
        _ => false,
    })
}

fn is_list_int(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if matches!(**inner, Type::Int | Type::IntN { .. }))
}

fn web_wasm_abi_supported(f: &Func, tir: &TIR::TFunc) -> bool {
    let ty_supported = if f.web_marker == Some(WebPartitionMarker::WasmExport) {
        wasm_export_ty
    } else {
        wasm_ty
    };
    flattened_web_params(tir)
        .iter().all(|(_, ty)| ty_supported(ty).is_some())
        // D-JSBIND1: String / [Int] params/returns cross the export boundary as
        // packed (ptr,len) u64 ownership transfers.
        && f.return_type
            .as_ref()
            .map(|ty| {
                matches!(ty, Type::String) || is_list_int(ty) || wasm_export_ty(ty).is_some()
            })
            .unwrap_or(true)
}

fn web_wasm_stmts_supported(
    stmts: &[TIR::TStmt],
    bundle: &ProgramBundle,
    file_prefix: Option<&str>,
    reconstructions: &[TIR::TWebParamReconstruction],
) -> bool {
    stmts.iter().all(|stmt| match stmt {
        TIR::TStmt::LineMarker(_) | TIR::TStmt::Return(None) => true,
        TIR::TStmt::Let { init, .. } | TIR::TStmt::ExprStmt(init) | TIR::TStmt::Return(Some(init)) => web_wasm_expr_supported(init, bundle, file_prefix, reconstructions),
        TIR::TStmt::Assign { value, .. } => web_wasm_expr_supported(value, bundle, file_prefix, reconstructions),
        TIR::TStmt::If { cond: TIR::TIfCond::Plain(cond), then_body, else_body, .. } => {
            web_wasm_expr_supported(cond, bundle, file_prefix, reconstructions)
                && web_wasm_stmts_supported(then_body, bundle, file_prefix, reconstructions)
                && else_body.as_deref().map(|body| web_wasm_stmts_supported(body, bundle, file_prefix, reconstructions)).unwrap_or(true)
        }
        // Inclusive `loop i; start..end` (D-SG8 / S22). Same covered Int arithmetic
        // already used by Wasm if/let — close the JS/Wasm TIR gap for compute loops.
        TIR::TStmt::Range { start, end, step, body, .. } => {
            web_wasm_expr_supported(start, bundle, file_prefix, reconstructions)
                && web_wasm_expr_supported(end, bundle, file_prefix, reconstructions)
                && step
                    .as_ref()
                    .map(|s| web_wasm_expr_supported(s, bundle, file_prefix, reconstructions))
                    .unwrap_or(true)
                && web_wasm_stmts_supported(body, bundle, file_prefix, reconstructions)
        }
        // Plain `loop x; xs` over a list/local (JS already emits `for…of`).
        // Keep method/map/stride/columnar forms on the honest unsupported path.
        TIR::TStmt::ForIn {
            var2: None,
            step: None,
            method_kind: None,
            columnar: false,
            collection,
            body,
            ..
        } => {
            web_wasm_expr_supported(collection, bundle, file_prefix, reconstructions)
                && web_wasm_stmts_supported(body, bundle, file_prefix, reconstructions)
        }
        _ => false,
    })
}

fn web_wasm_expr_supported(
    expr: &TIR::TExpr,
    bundle: &ProgramBundle,
    file_prefix: Option<&str>,
    reconstructions: &[TIR::TWebParamReconstruction],
) -> bool {
    match &expr.kind {
        TIR::TExprKind::IntLit(..) | TIR::TExprKind::FloatLit(_) | TIR::TExprKind::BoolLit(_) | TIR::TExprKind::Local(_) => true,
        TIR::TExprKind::StrLit(parts) => parts.iter().all(|part| matches!(part, TIR::TStrPart::Lit(_))),
        TIR::TExprKind::Binary { lhs, rhs, .. } => web_wasm_expr_supported(lhs, bundle, file_prefix, reconstructions) && web_wasm_expr_supported(rhs, bundle, file_prefix, reconstructions),
        TIR::TExprKind::Unary { operand, .. }
        | TIR::TExprKind::Clone(operand)
        | TIR::TExprKind::MaterializeView(operand)
        | TIR::TExprKind::Print(operand) => {
            web_wasm_expr_supported(operand, bundle, file_prefix, reconstructions)
        }
        TIR::TExprKind::Field { recv, field_rust, boxed: false } => {
            let TIR::TExprKind::Local(local) = &recv.kind else { return false };
            reconstructions.iter().any(|r| {
                r.local_rust == *local && r.fields.iter().any(|(field, _, _)| field == field_rust)
            })
        }
        TIR::TExprKind::Call { name, args } => wasm_callee_bucket(bundle, &local_web_key(file_prefix, name)) == Some(WebBucket::Wasm)
            && args.iter().all(|a| web_wasm_expr_supported(&a.value, bundle, file_prefix, reconstructions)),
        TIR::TExprKind::ModuleCall { form, args } => {
            let key = match form {
                TIR::TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    qualified_web_key(rust_mod, rust_fn)
                }
                TIR::TModuleCallForm::InlineMangled { mangled } => mangled.clone(),
            };
            bundle
                .web_partitions
                .get(&key)
                .copied()
                == Some(WebBucket::Wasm)
                && args
                    .iter()
                    .all(|a| web_wasm_expr_supported(&a.value, bundle, file_prefix, reconstructions))
        }
        _ => false,
    }
}

fn wasm_callee_bucket(bundle: &ProgramBundle, name: &str) -> Option<WebBucket> {
    bundle.web_partitions.get(name).copied()
}

fn web_stmts_supported(stmts: &[TIR::TStmt]) -> bool {
    stmts.iter().all(|stmt| match stmt {
        TIR::TStmt::LineMarker(_) | TIR::TStmt::Return(None) => true,
        TIR::TStmt::Let { init, .. } | TIR::TStmt::ExprStmt(init) | TIR::TStmt::Return(Some(init)) => web_expr_supported(init),
        TIR::TStmt::Assign { value, .. } => web_expr_supported(value),
        TIR::TStmt::If { cond: TIR::TIfCond::Plain(cond), then_body, else_body, .. } => web_expr_supported(cond) && web_stmts_supported(then_body) && else_body.as_deref().map(web_stmts_supported).unwrap_or(true),
        TIR::TStmt::Range { start, end, step, body, .. } => web_expr_supported(start) && web_expr_supported(end) && step.as_ref().map(web_expr_supported).unwrap_or(true) && web_stmts_supported(body),
        TIR::TStmt::ForIn { var2: None, collection, body, .. } => web_expr_supported(collection) && web_stmts_supported(body),
        TIR::TStmt::Inline(body) | TIR::TStmt::Region(body) => web_stmts_supported(body),
        _ => false,
    })
}

fn web_lambda_supported(lam: &TIR::TLambda) -> bool {
    match &lam.executable {
        TIR::TLambdaBody::Expr(expr) => web_expr_supported(expr),
        TIR::TLambdaBody::Block(body) => web_stmts_supported(body),
    }
}

fn web_expr_supported(expr: &TIR::TExpr) -> bool {
    use TIR::TExprKind as E;
    match &expr.kind {
        E::IntLit(..) | E::FloatLit(_) | E::BoolLit(_) | E::CharLit(_) | E::Local(_) | E::ConstInline(_) => true,
        E::StrLit(parts) => parts.iter().all(|p| match p { TIR::TStrPart::Lit(_) => true, TIR::TStrPart::Interp(e, _) => web_expr_supported(e) }),
        E::Binary { lhs, rhs, .. } => web_expr_supported(lhs) && web_expr_supported(rhs),
        E::Unary { operand, .. } | E::Clone(operand) | E::MaterializeView(operand) | E::DistinctRaw(operand) | E::Print(operand) => web_expr_supported(operand),
        E::DistinctCtor { arg, .. } => web_expr_supported(arg),
        E::Field { recv, .. } => web_expr_supported(recv),
        E::StructLit { fields, .. } => fields.iter().all(|(_, e, _)| web_expr_supported(e)),
        E::ListLit(elements) => elements.iter().all(web_expr_supported),
        E::Call { args, .. } | E::MethodCall { args, .. } => args.iter().all(|a| web_expr_supported(&a.value)),
        E::ModuleCall { form: TIR::TModuleCallForm::Qualified { .. } | TIR::TModuleCallForm::InlineMangled { .. }, args } => args.iter().all(|a| web_expr_supported(&a.value)),
        E::CoreCall { module, method, args, .. } => web_core_arity(module, method) == Some(args.len()) && args.iter().all(web_expr_supported),
        E::HandleMethod { recv, op, args } => matches!(op, TIR::THandleOp::UiBackendMethod { .. } | TIR::THandleOp::ReactiveGet | TIR::THandleOp::ReactiveSet | TIR::THandleOp::ReactiveEffectMethod { .. }) && web_expr_supported(recv) && args.iter().all(web_expr_supported),
        E::NumericMethod { recv, op: TIR::TNumericOp::CastAs { .. } | TIR::TNumericOp::FloatToInt { .. } } => web_expr_supported(recv),
        E::OrFallback { value, fallback: TIR::TOrFallback::Value(fallback), .. } => web_expr_supported(value) && web_expr_supported(fallback),
        E::Lambda(lam) => web_lambda_supported(lam),
        E::CoreClosureCall { kind: TIR::TCoreClosureKind::UiReactiveRender { executable, .. } | TIR::TCoreClosureKind::ReactiveEffect { executable, .. } } => web_lambda_supported(executable),
        _ => false,
    }
}

fn emit_index_html() -> String {
    "<!DOCTYPE html>\n\
     <html lang=\"en\">\n\
     <head>\n\
     <meta charset=\"utf-8\">\n\
     <title>jet web app</title>\n\
     <style>\n\
       :root { color-scheme: dark; }\n\
       body {\n\
         margin: 0; min-height: 100vh; display: flex; align-items: center; justify-content: center;\n\
         font: 15px -apple-system, system-ui, sans-serif;\n\
         background: radial-gradient(circle at 20% 20%, #1c2333, #0b0d14 60%); color: #e8ebf5;\n\
       }\n\
       #jet-app { position: relative; min-height: 40px; }\n\
     </style>\n\
     </head>\n\
     <body>\n\
     <div id=\"jet-app\"></div>\n\
     <script type=\"module\">\n\
     import { jet_main } from \"./app.js\";\n\
     jet_main();\n\
     </script>\n\
     </body>\n\
     </html>\n"
        .to_string()
}

fn collect_web_funcs(bundle: &ProgramBundle) -> Vec<FuncWeb> {
    let mut out = Vec::new();
    let extern_funcs = bundle_extern_funcs(bundle);
    for (i, module) in bundle.modules.iter().enumerate() {
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            None,
            &extern_funcs,
        );
        populate_cx_from_bundle(&mut cx, bundle, i);
        register_foreign_enum_variants(&mut cx, bundle, i);
        update_cloneability_with_foreign_types(&mut cx, &module.items);
        collect_module_funcs(
            &module.items,
            module.web_target_ceiling,
            None,
            None,
            (i != bundle.entry).then_some(module.alias.as_str()),
            bundle,
            &cx,
            &mut out,
        );
    }
    out
}

fn collect_module_funcs(
    items: &[Item],
    file_ceiling: Option<WebBucket>,
    module_ceiling: Option<WebBucket>,
    module_prefix: Option<&str>,
    file_prefix: Option<&str>,
    bundle: &ProgramBundle,
    cx: &Cx,
    out: &mut Vec<FuncWeb>,
) {
    let ceiling = module_ceiling.or(file_ceiling);
    let _ = ceiling;
    for item in items {
        match item {
            Item::Func(f) => {
                let key = match module_prefix {
                    Some(m) => format!("{m}__{}", f.name),
                    None => file_prefix
                        .map(|m| format!("{m}__{}", f.name))
                        .unwrap_or_else(|| f.name.clone()),
                };
                let bucket = bundle
                    .web_partitions
                    .get(&key)
                    .copied()
                    .unwrap_or(WebBucket::Wasm);
                out.push(FuncWeb {
                    name: f.name.clone(),
                    key,
                    file_prefix: file_prefix.map(str::to_string),
                    bucket,
                    marker: f.web_marker,
                    span: f.name_span,
                    params: f
                        .params
                        .iter()
                        .map(|p| (p.name.clone(), p.ty.clone()))
                        .collect(),
                    return_type: f.return_type.clone(),
                    tir: TIR::lower_web_func(f, cx),
                });
            }
            Item::CodeModule(cm) => {
                let mod_ceiling = cm.web_target.or(ceiling);
                if let Some(body) = &cm.body {
                    collect_module_funcs(
                        body,
                        file_ceiling,
                        mod_ceiling,
                        Some(&cm.name),
                        file_prefix,
                        bundle,
                        cx,
                        out,
                    );
                }
            }
            _ => {}
        }
    }
}

fn json_quote(s: &str) -> String {
    // JS/JSON string literal — escape controls so Wasm bridge call sites with
    // tab/newline (D-JSBIND1 hostile String params) stay valid source.
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn emit_manifest(bundle: &ProgramBundle, funcs: &[FuncWeb]) -> String {
    let mut parts = Vec::new();
    parts.push("  \"version\": 2".to_string());
    parts.push("  \"status\": \"m2\"".to_string());
    parts.push(format!(
        "  \"target\": {}",
        json_quote(Syntax::BUILD_TARGET_WEB)
    ));
    parts.push("  \"wasmTriple\": \"wasm32-unknown-unknown\"".to_string());
    let entry = funcs
        .iter()
        .find(|f| f.key == "run")
        .map(|f| f.bucket.name())
        .unwrap_or(Syntax::WEB_BUCKET_JS);
    parts.push(format!("  \"entry\": {}", json_quote(entry)));
    parts.push(format!(
        "  \"entryFile\": {}",
        json_quote(&bundle.modules[bundle.entry].display)
    ));
    let mut partition_lines = Vec::new();
    for f in funcs {
        partition_lines.push(format!(
            "    {}: {}",
            json_quote(&f.key),
            json_quote(f.bucket.name())
        ));
    }
    parts.push(format!(
        "  \"partitions\": {{\n{}\n  }}",
        partition_lines.join(",\n")
    ));
    let export_lines: Vec<String> = funcs
        .iter()
        .filter(|f| f.marker == Some(WebPartitionMarker::WasmExport))
        .map(|f| {
            let params: Vec<String> = f
                .params
                .iter()
                .map(|(_, t)| json_quote(&type_show(t)))
                .collect();
            let ret = f
                .return_type
                .as_ref()
                .map(|t| format!(", \"returns\": {}", json_quote(&type_show(t))))
                .unwrap_or_default();
            format!(
                "    {{ \"name\": {}, \"symbol\": {}, \"params\": [{}]{ret} }}",
                json_quote(&f.key),
                json_quote(&wasm_export_symbol(&f.key)),
                params.join(", ")
            )
        })
        .collect();
    parts.push(format!(
        "  \"exports\": [\n{}\n  ]",
        export_lines.join(",\n")
    ));
    format!("{{\n{}\n}}\n", parts.join(",\n"))
}

fn type_show(ty: &Type) -> String {
    match ty {
        Type::Int => "Int".to_string(),
        Type::Float => "Float".to_string(),
        Type::Bool => "Bool".to_string(),
        Type::String => "String".to_string(),
        Type::Char => "Char".to_string(),
        other => format!("{other:?}"),
    }
}

fn wasm_export_symbol(name: &str) -> String {
    format!("jet_export_{name}")
}

fn find_struct_fields<'a>(
    bundle: &'a ProgramBundle,
    name: &str,
) -> Option<&'a [crate::AST::Field]> {
    for module in &bundle.modules {
        if let Some(fields) = struct_fields_in_items(&module.items, name) {
            return Some(fields);
        }
    }
    None
}

fn struct_fields_in_items<'a>(items: &'a [Item], name: &str) -> Option<&'a [crate::AST::Field]> {
    for item in items {
        match item {
            Item::Struct(s) if s.name == name => return Some(&s.fields),
            Item::CodeModule(cm) => {
                if let Some(body) = &cm.body {
                    if let Some(fields) = struct_fields_in_items(body, name) {
                        return Some(fields);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Flatten only from facts carried by checked TIR lowering. This is the Wasm
/// signature source of truth; Web emission never infers a struct layout.
fn flattened_web_params(tir: &TIR::TFunc) -> Vec<(String, Type)> {
    let mut out = Vec::new();
    for (name, ty, _) in &tir.params {
        if let Some(reconstruction) = tir
            .web_param_reconstructions
            .iter()
            .find(|r| r.local_rust == *name)
        {
            out.extend(
                reconstruction
                    .fields
                    .iter()
                    .map(|(_, flat, ty)| (flat.clone(), ty.clone())),
            );
        } else {
            out.push((name.clone(), ty.clone()));
        }
    }
    out
}

fn js_abi_call_args(
    bundle: &ProgramBundle,
    name: &str,
    ty: &Type,
    prelude: &mut String,
) -> Vec<String> {
    if let Type::Named(n) = ty {
        if let Some(fields) = find_struct_fields(bundle, n) {
            if fields
                .iter()
                .all(|f| matches!(f.ty, Type::Int | Type::IntN { .. }))
            {
                let bind = format!("_{name}_flat");
                let kind = format!("struct-{}", n.to_lowercase());
                prelude.push_str(&format!(
                    "  const {bind} = jetDom.marshalAbi({name}, \"{kind}\");\n",
                ));
                return fields
                    .iter()
                    .map(|f| format!("BigInt({bind}.{})", f.name))
                    .collect();
            }
        }
    }
    match ty {
        Type::Int | Type::IntN { .. } => vec![format!("BigInt({name})")],
        Type::String => {
            prelude.push_str(&format!(
                "  const _{name} = jetDom.marshalAbi({name}, \"string\", wasm);\n"
            ));
            vec![format!("_{name}")]
        }
        Type::List(inner) if matches!(**inner, Type::Int | Type::IntN { .. }) => {
            prelude.push_str(&format!(
                "  const _{name} = jetDom.marshalAbi({name}, \"list-int\", wasm);\n"
            ));
            vec![format!("_{name}")]
        }
        _ => vec![name.to_string()],
    }
}

fn emit_wasm_rust(bundle: &ProgramBundle, funcs: &[FuncWeb]) -> WebEmitResult<String> {
    let mut out = String::from(
        "// Generated by jet — wasm32-unknown-unknown module (D-WEBKIND1).\n\
         #![allow(unused)]\n\n",
    );
    let wasm_funcs: Vec<&FuncWeb> = funcs
        .iter()
        .filter(|f| {
            f.bucket == WebBucket::Wasm
                && f.key != "dev"
                && (bundle.modules[bundle.entry].html_path.is_none()
                    || funcs.iter().any(|x| x.marker == Some(WebPartitionMarker::WasmExport)))
        })
        .collect();
    if wasm_funcs.is_empty() {
        out.push_str("#[no_mangle]\npub extern \"C\" fn jet_wasm_nop() {}\n");
        return Ok(out);
    }
    let need_packed_abi = |pred: &dyn Fn(&Type) -> bool| {
        wasm_funcs.iter().any(|f| {
            let export = f.marker == Some(WebPartitionMarker::WasmExport)
                || (f.key == "run"
                    && f.bucket == WebBucket::Wasm
                    && bundle.modules[bundle.entry].html_path.is_none());
            if !export {
                return false;
            }
            f.return_type.as_ref().is_some_and(pred)
                || flattened_web_params(&f.tir).iter().any(|(_, ty)| pred(ty))
        })
    };
    if need_packed_abi(&|ty| matches!(ty, Type::String)) {
        // D-JSBIND1=A: UTF-8 ownership transfer — packed u64 (ptr<<32)|len.
        // Returns: Wasm owns → JS copies → jet_abi_string_free.
        // Params: JS TextEncoder → jet_abi_string_alloc → Wasm takes via jet_abi_string_arg.
        out.push_str(
            "fn jet_abi_string_ret(s: String) -> u64 {\n\
             \x20   let boxed = s.into_bytes().into_boxed_slice();\n\
             \x20   let len = boxed.len() as u32;\n\
             \x20   let ptr = Box::into_raw(boxed) as *mut u8 as u32;\n\
             \x20   ((ptr as u64) << 32) | (len as u64)\n\
             }\n\n\
             fn jet_abi_string_arg(packed: u64) -> String {\n\
             \x20   let ptr = (packed >> 32) as u32;\n\
             \x20   let len = (packed & 0xffff_ffff) as u32;\n\
             \x20   if len == 0 {\n\
             \x20       if ptr != 0 {\n\
             \x20           unsafe {\n\
             \x20               let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, 0));\n\
             \x20           }\n\
             \x20       }\n\
             \x20       return String::new();\n\
             \x20   }\n\
             \x20   unsafe {\n\
             \x20       let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len as usize));\n\
             \x20       String::from_utf8(boxed.into_vec()).expect(\"JS TextEncoder UTF-8\")\n\
             \x20   }\n\
             }\n\n\
             #[no_mangle]\n\
             pub extern \"C\" fn jet_abi_string_alloc(len: u32) -> u32 {\n\
             \x20   let boxed = vec![0u8; len as usize].into_boxed_slice();\n\
             \x20   Box::into_raw(boxed) as *mut u8 as u32\n\
             }\n\n\
             #[no_mangle]\n\
             pub extern \"C\" fn jet_abi_string_free(ptr: u32, len: u32) {\n\
             \x20   if ptr == 0 { return; }\n\
             \x20   unsafe {\n\
             \x20       let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len as usize));\n\
             \x20   }\n\
             }\n\n",
        );
    }
    if need_packed_abi(&is_list_int) {
        // D-JSBIND1=A: [Int] as little-endian i64 payload — packed u64 (ptr<<32)|len.
        // Returns: Wasm owns → JS copies → jet_abi_list_i64_free.
        // Params: JS BigInt64Array → jet_abi_list_i64_alloc → Wasm takes via jet_abi_list_i64_arg.
        out.push_str(
            "fn jet_abi_list_i64_ret(v: Vec<i64>) -> u64 {\n\
             \x20   let boxed = v.into_boxed_slice();\n\
             \x20   let len = boxed.len() as u32;\n\
             \x20   let ptr = Box::into_raw(boxed) as *mut i64 as u32;\n\
             \x20   ((ptr as u64) << 32) | (len as u64)\n\
             }\n\n\
             fn jet_abi_list_i64_arg(packed: u64) -> Vec<i64> {\n\
             \x20   let ptr = (packed >> 32) as u32;\n\
             \x20   let len = (packed & 0xffff_ffff) as u32;\n\
             \x20   if len == 0 {\n\
             \x20       if ptr != 0 {\n\
             \x20           unsafe {\n\
             \x20               let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut i64, 0));\n\
             \x20           }\n\
             \x20       }\n\
             \x20       return Vec::new();\n\
             \x20   }\n\
             \x20   unsafe {\n\
             \x20       let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut i64, len as usize));\n\
             \x20       boxed.into_vec()\n\
             \x20   }\n\
             }\n\n\
             #[no_mangle]\n\
             pub extern \"C\" fn jet_abi_list_i64_alloc(len: u32) -> u32 {\n\
             \x20   let boxed = vec![0i64; len as usize].into_boxed_slice();\n\
             \x20   Box::into_raw(boxed) as *mut i64 as u32\n\
             }\n\n\
             #[no_mangle]\n\
             pub extern \"C\" fn jet_abi_list_i64_free(ptr: u32, len: u32) {\n\
             \x20   if ptr == 0 { return; }\n\
             \x20   unsafe {\n\
             \x20       let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut i64, len as usize));\n\
             \x20   }\n\
             }\n\n",
        );
    }
    let mut emitted_structs = std::collections::HashSet::new();
    for f in &wasm_funcs {
        for reconstruction in &f.tir.web_param_reconstructions {
            if !emitted_structs.insert(reconstruction.rust_type.clone()) {
                continue;
            }
            out.push_str(&format!("struct {} {{\n", reconstruction.rust_type));
            for (field, _, ty) in &reconstruction.fields {
                let rust_ty = wasm_ty(ty).ok_or_else(|| web_emit_error(f))?;
                out.push_str(&format!("    {field}: {rust_ty},\n"));
            }
            out.push_str("}\n\n");
        }
    }
    for f in wasm_funcs {
        let export = f.marker == Some(WebPartitionMarker::WasmExport)
            || (f.key == "run"
                && f.bucket == WebBucket::Wasm
                && bundle.modules[bundle.entry].html_path.is_none());
        emit_wasm_fn(bundle, f, export, &mut out, funcs)?;
    }
    Ok(out)
}

fn emit_wasm_fn(_bundle: &ProgramBundle, f: &FuncWeb, export: bool, out: &mut String, funcs: &[FuncWeb]) -> WebEmitResult<()> {
    let string_ret = matches!(f.return_type.as_ref(), Some(Type::String));
    let list_ret = f.return_type.as_ref().is_some_and(is_list_int);
    let flat = flattened_web_params(&f.tir);
    let string_params = flat.iter().any(|(_, ty)| matches!(ty, Type::String));
    let list_params = flat.iter().any(|(_, ty)| is_list_int(ty));
    let reconstructed_export = export && !f.tir.web_param_reconstructions.is_empty();
    // String / [Int] cannot be bare `extern "C"` — wrap as packed u64.
    let wrapped_export =
        export && (reconstructed_export || string_ret || string_params || list_ret || list_params);
    if wrapped_export {
        out.push_str(&format!(
            "#[no_mangle]\npub extern \"C\" fn {}(",
            wasm_export_symbol(&f.key)
        ));
        let params: Vec<String> = flat
            .iter()
            .map(|(name, ty)| wasm_export_ty(ty).map(|t| format!("{name}: {t}")))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| web_emit_error(f))?;
        out.push_str(&params.join(", "));
        out.push(')');
        if string_ret || list_ret {
            out.push_str(" -> u64 ");
        } else if let Some(ret) = &f.return_type {
            out.push_str(&format!(" -> {} ", wasm_ty(ret).ok_or_else(|| web_emit_error(f))?));
        }
        out.push_str("{\n");
        for (name, ty) in &flat {
            if matches!(ty, Type::String) {
                out.push_str(&format!("    let {name} = jet_abi_string_arg({name});\n"));
            } else if is_list_int(ty) {
                out.push_str(&format!("    let {name} = jet_abi_list_i64_arg({name});\n"));
            }
        }
        for reconstruction in &f.tir.web_param_reconstructions {
            let fields = reconstruction
                .fields
                .iter()
                .map(|(field, flat, _)| format!("{field}: {flat}"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "    let {} = {} {{ {} }};\n",
                reconstruction.local_rust, reconstruction.rust_type, fields
            ));
        }
        let args = f
            .tir
            .params
            .iter()
            .map(|(name, ty, conv)| wasm_export_arg_expr(name, ty, *conv))
            .collect::<Vec<_>>()
            .join(", ");
        if string_ret {
            out.push_str(&format!(
                "    return jet_abi_string_ret(jet_wasm_{}({args}));\n",
                f.key
            ));
        } else if list_ret {
            out.push_str(&format!(
                "    return jet_abi_list_i64_ret(jet_wasm_{}({args}));\n",
                f.key
            ));
        } else if f.return_type.is_some() {
            out.push_str(&format!("    return jet_wasm_{}({args});\n", f.key));
        } else {
            out.push_str(&format!("    jet_wasm_{}({args});\n", f.key));
        }
        out.push_str("}\n\n");
    }

    if export && !wrapped_export {
        out.push_str(&format!(
            "#[no_mangle]\npub extern \"C\" fn {}(",
            wasm_export_symbol(&f.key)
        ));
    } else {
        out.push_str(&format!("fn jet_wasm_{}(", f.key));
    }
    let params: Vec<String> = f.tir.params
        .iter()
        .map(|(name, ty, conv)| {
            let rust_ty = f
                .tir
                .web_param_reconstructions
                .iter()
                .find(|r| r.local_rust == *name)
                .map(|r| r.rust_type.clone())
                .or_else(|| wasm_param_rust_ty(ty, *conv).map(str::to_string));
            rust_ty.map(|t| format!("{name}: {t}"))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| web_emit_error(f))?;
    out.push_str(&params.join(", "));
    out.push(')');
    if let Some(ret) = &f.return_type {
        out.push_str(&format!(" -> {} ", wasm_ty(ret).ok_or_else(|| web_emit_error(f))?));
    }
    out.push_str("{\n");
    emit_wasm_body(
        &f.tir.body,
        out,
        1,
        funcs,
        f.file_prefix.as_deref(),
        &f.tir.web_param_reconstructions,
    )
    .map_err(|()| web_emit_error(f))?;
    out.push_str("}\n\n");
    Ok(())
}

fn wasm_ty(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Int | Type::IntN { signed: true, .. } => Some("i64"),
        Type::IntN { signed: false, .. } => Some("u64"),
        Type::Float | Type::Float32 => Some("f64"),
        Type::Bool => Some("bool"),
        Type::String => Some("String"),
        Type::List(inner) if matches!(**inner, Type::Int | Type::IntN { .. }) => Some("Vec<i64>"),
        _ => None,
    }
}

/// Rust param type matching TIR `param_place` borrow rules (Read String → &String).
fn wasm_param_rust_ty(ty: &Type, conv: AccessConvention) -> Option<&'static str> {
    match (conv, ty) {
        (AccessConvention::Read, Type::String) => Some("&String"),
        (AccessConvention::Write, Type::String) => Some("&mut String"),
        (AccessConvention::Move, Type::String) => Some("String"),
        (AccessConvention::Read, Type::List(inner))
            if matches!(**inner, Type::Int | Type::IntN { .. }) =>
        {
            Some("&Vec<i64>")
        }
        (AccessConvention::Write, Type::List(inner))
            if matches!(**inner, Type::Int | Type::IntN { .. }) =>
        {
            Some("&mut Vec<i64>")
        }
        (AccessConvention::Move, Type::List(inner))
            if matches!(**inner, Type::Int | Type::IntN { .. }) =>
        {
            Some("Vec<i64>")
        }
        (AccessConvention::Read, t) if t.is_scalar() => wasm_ty(t),
        (AccessConvention::Read, _) => None,
        _ => wasm_ty(ty),
    }
}

fn wasm_export_arg_expr(name: &str, ty: &Type, conv: AccessConvention) -> String {
    match (conv, ty) {
        (AccessConvention::Read, Type::String) => format!("&{name}"),
        (AccessConvention::Write, Type::String) => format!("&mut {name}"),
        (AccessConvention::Read, Type::List(inner))
            if matches!(**inner, Type::Int | Type::IntN { .. }) =>
        {
            format!("&{name}")
        }
        (AccessConvention::Write, Type::List(inner))
            if matches!(**inner, Type::Int | Type::IntN { .. }) =>
        {
            format!("&mut {name}")
        }
        _ => name.to_string(),
    }
}

fn wasm_export_ty(ty: &Type) -> Option<&'static str> {
    match ty {
        // Packed (ptr,len) u64 on the C ABI; internal jet_wasm_* still uses String / Vec.
        Type::String => Some("u64"),
        Type::List(inner) if matches!(**inner, Type::Int | Type::IntN { .. }) => Some("u64"),
        _ => wasm_ty(ty),
    }
}

fn emit_wasm_body(
    body: &[TIR::TStmt],
    out: &mut String,
    indent: usize,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
    reconstructions: &[TIR::TWebParamReconstruction],
) -> Result<(), ()> {
    let pad = "    ".repeat(indent);
    for stmt in body {
        match stmt {
            TIR::TStmt::Return(Some(expr)) => out.push_str(&format!("{pad}return {};\n", wasm_emit_expr(expr, funcs, file_prefix, reconstructions)?)),
            TIR::TStmt::ExprStmt(expr) => out.push_str(&format!("{pad}{};\n", wasm_emit_expr(expr, funcs, file_prefix, reconstructions)?)),
            TIR::TStmt::Let { name, init, .. } => out.push_str(&format!("{pad}let mut {} = {};\n", mangle(name), wasm_emit_expr(init, funcs, file_prefix, reconstructions)?)),
            TIR::TStmt::Assign { place, op, value, .. } => out.push_str(&format!("{pad}{place} {}= {};\n", op.as_ref().map(binop).unwrap_or(""), wasm_emit_expr(value, funcs, file_prefix, reconstructions)?)),
            TIR::TStmt::If { cond: TIR::TIfCond::Plain(cond), then_body, else_body, .. } => {
                out.push_str(&format!("{pad}if {} {{\n", wasm_emit_expr(cond, funcs, file_prefix, reconstructions)?));
                emit_wasm_body(then_body, out, indent + 1, funcs, file_prefix, reconstructions)?;
                if let Some(else_body) = else_body {
                    out.push_str(&format!("{pad}}} else {{\n"));
                    emit_wasm_body(else_body, out, indent + 1, funcs, file_prefix, reconstructions)?;
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::Range { var, start, end, step, body, .. } => {
                let start = wasm_emit_expr(start, funcs, file_prefix, reconstructions)?;
                let end = wasm_emit_expr(end, funcs, file_prefix, reconstructions)?;
                let loop_var = mangle(var);
                match step {
                    Some(step) => {
                        let step = wasm_emit_expr(step, funcs, file_prefix, reconstructions)?;
                        out.push_str(&format!("{pad}{{\n"));
                        out.push_str(&format!("{pad}    let _jet_loop_start = {start};\n"));
                        out.push_str(&format!("{pad}    let _jet_loop_end = {end};\n"));
                        out.push_str(&format!("{pad}    let _jet_loop_stride = {step};\n"));
                        out.push_str(&format!(
                            "{pad}    assert!(_jet_loop_stride > 0, \"E0123: loop stride must be positive\");\n"
                        ));
                        out.push_str(&format!(
                            "{pad}    for {loop_var} in (_jet_loop_start..=_jet_loop_end).step_by(_jet_loop_stride as usize) {{\n"
                        ));
                        emit_wasm_body(body, out, indent + 2, funcs, file_prefix, reconstructions)?;
                        out.push_str(&format!("{pad}    }}\n"));
                        out.push_str(&format!("{pad}}}\n"));
                    }
                    None => {
                        out.push_str(&format!(
                            "{pad}for {loop_var} in ({start})..=({end}) {{\n"
                        ));
                        emit_wasm_body(body, out, indent + 1, funcs, file_prefix, reconstructions)?;
                        out.push_str(&format!("{pad}}}\n"));
                    }
                }
            }
            // Plain list/local ForIn — mirror native `.iter().cloned()` (or by-value).
            TIR::TStmt::ForIn {
                var,
                var2: None,
                step: None,
                method_kind: None,
                columnar: false,
                by_value,
                collection,
                body,
                ..
            } => {
                let collection = wasm_emit_expr(collection, funcs, file_prefix, reconstructions)?;
                let loop_var = mangle(var);
                let iter = if *by_value {
                    format!("({collection})")
                } else {
                    format!("({collection}).iter().cloned()")
                };
                out.push_str(&format!("{pad}for {loop_var} in {iter} {{\n"));
                emit_wasm_body(body, out, indent + 1, funcs, file_prefix, reconstructions)?;
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::Return(None) => out.push_str(&format!("{pad}return;\n")),
            TIR::TStmt::LineMarker(_) => {}
            _ => return Err(()),
        }
    }
    Ok(())
}

fn wasm_emit_expr(
    expr: &TIR::TExpr,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
    reconstructions: &[TIR::TWebParamReconstruction],
) -> Result<String, ()> {
    Ok(match &expr.kind {
        TIR::TExprKind::IntLit(n, _) => n.to_string(),
        TIR::TExprKind::FloatLit(n) => n.to_string(),
        TIR::TExprKind::BoolLit(b) => b.to_string(),
        TIR::TExprKind::StrLit(parts) => {
            let mut value = String::new();
            for part in parts {
                let TIR::TStrPart::Lit(text) = part else { return Err(()) };
                value.push_str(text);
            }
            // Owned String — export returns and Jet String locals need String, not &str.
            format!("{}.to_string()", json_quote(&value))
        }
        TIR::TExprKind::Local(name) => name.clone(),
        TIR::TExprKind::Binary { op, lhs, rhs, .. } => format!(
            "({} {} {})",
            wasm_emit_expr(lhs, funcs, file_prefix, reconstructions)?,
            binop(op),
            wasm_emit_expr(rhs, funcs, file_prefix, reconstructions)?
        ),
        TIR::TExprKind::Unary { op, operand } => format!("({}{})", unop(op), wasm_emit_expr(operand, funcs, file_prefix, reconstructions)?),
        TIR::TExprKind::Clone(inner) => format!(
            "({}).clone()",
            wasm_emit_expr(inner, funcs, file_prefix, reconstructions)?
        ),
        TIR::TExprKind::MaterializeView(inner) => format!(
            "({}).to_string()",
            wasm_emit_expr(inner, funcs, file_prefix, reconstructions)?
        ),
        TIR::TExprKind::Print(inner) => format!("println!(\"{{}}\", {})", wasm_emit_expr(inner, funcs, file_prefix, reconstructions)?),
        TIR::TExprKind::Field { recv, field_rust, boxed: false } => {
            let TIR::TExprKind::Local(local) = &recv.kind else { return Err(()) };
            if !reconstructions.iter().any(|r| {
                r.local_rust == *local && r.fields.iter().any(|(field, _, _)| field == field_rust)
            }) {
                return Err(());
            }
            format!("({}).{}", wasm_emit_expr(recv, funcs, file_prefix, reconstructions)?, field_rust)
        }
        TIR::TExprKind::Call { name, args } => {
            let key = local_web_key(file_prefix, name);
            let mut callees = funcs.iter().filter(|f| f.key == key && f.bucket == WebBucket::Wasm);
            callees.next().ok_or(())?;
            if callees.next().is_some() {
                return Err(());
            }
            let symbol = format!("jet_wasm_{key}");
            format!("{symbol}({})", args.iter().map(|a| wasm_emit_expr(&a.value, funcs, file_prefix, reconstructions)).collect::<Result<Vec<_>, _>>()?.join(", "))
        }
        TIR::TExprKind::ModuleCall { form, args } => {
            let key = match form {
                TIR::TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    qualified_web_key(rust_mod, rust_fn)
                }
                TIR::TModuleCallForm::InlineMangled { mangled } => mangled.clone(),
            };
            let mut callees = funcs.iter().filter(|f| f.key == key && f.bucket == WebBucket::Wasm);
            callees.next().ok_or(())?;
            if callees.next().is_some() { return Err(()); }
            format!("jet_wasm_{key}({})", args.iter().map(|a| wasm_emit_expr(&a.value, funcs, file_prefix, reconstructions)).collect::<Result<Vec<_>, _>>()?.join(", "))
        }
        _ => return Err(()),
    })
}

fn web_emit_error(f: &FuncWeb) -> WebTirUnsupported {
    WebTirUnsupported { func_name: f.name.clone(), span: f.span }
}

fn emit_js_app(bundle: &ProgramBundle, funcs: &[FuncWeb]) -> WebEmitResult<String> {
    let mut out = String::from(
        "// Generated by jet — web JS entry (D-WEBBACKEND1).\n\
         import * as jetDom from \"./jet_dom_runtime.js\";\n\n",
    );
    let exports: Vec<&FuncWeb> = funcs
        .iter()
        .filter(|f| f.marker == Some(WebPartitionMarker::WasmExport))
        .collect();
    if !exports.is_empty() {
        out.push_str("let _wasm = null;\n\n");
        out.push_str("async function loadWasm() {\n");
        out.push_str("  if (_wasm) return _wasm;\n");
        out.push_str("  const instance = await jetDom.instantiateWasm(\"./app.wasm\");\n");
        out.push_str("  _wasm = instance.exports;\n");
        out.push_str("  return _wasm;\n");
        out.push_str("}\n\n");
        for f in &exports {
            let args: Vec<String> = f.params.iter().map(|(n, _)| n.clone()).collect();
            out.push_str(&format!(
                "async function bridge_{}({}) {{\n",
                f.key,
                args.join(", ")
            ));
            out.push_str("  const wasm = await loadWasm();\n");
            let sym = wasm_export_symbol(&f.key);
            let mut prelude = String::new();
            let call_args: Vec<String> = f
                .params
                .iter()
                .flat_map(|(n, ty)| js_abi_call_args(bundle, n, ty, &mut prelude))
                .collect();
            out.push_str(&prelude);
            out.push_str(&format!(
                "  const raw = wasm.{sym}({});\n",
                call_args.join(", ")
            ));
            let ret_kind = match f.return_type.as_ref() {
                Some(Type::String) => "string",
                Some(ty) if is_list_int(ty) => "list-int",
                _ => "scalar",
            };
            if ret_kind == "string" || ret_kind == "list-int" {
                out.push_str(&format!(
                    "  return jetDom.unmarshalAbi(raw, \"{ret_kind}\", wasm);\n"
                ));
            } else {
                out.push_str(&format!(
                    "  return jetDom.unmarshalAbi(raw, \"{ret_kind}\");\n"
                ));
            }
            out.push_str("}\n\n");
        }
    }

    let js_funcs: Vec<&FuncWeb> = funcs
        .iter()
        .filter(|f| f.bucket == WebBucket::Js && f.key != "run" && f.key != "dev")
        .collect();
    for f in &js_funcs {
        emit_js_fn(f, &mut out, funcs)?;
    }

    if let Some(main_fn) = funcs.iter().find(|f| f.key == "run") {
        if main_fn.bucket == WebBucket::Js {
            out.push_str("export async function jet_main() {\n");
            out.push_str(&format!(
                "  jetDom.enterRenderScope({});\n",
                json_quote("run")
            ));
            out.push_str("  try {\n");
            emit_tir_js_body(
                &main_fn.tir.body,
                &mut out,
                funcs,
                main_fn.file_prefix.as_deref(),
                2,
            )
                .map_err(|()| web_emit_error(main_fn))?;
            out.push_str("  } finally {\n    jetDom.exitRenderScope();\n  }\n");
            out.push_str("}\n\n");
            out.push_str("const _isMain = typeof process !== \"undefined\" && process.argv[1]?.endsWith(\"app.js\");\n");
            out.push_str("if (_isMain) { jet_main(); }\n");
        } else if bundle.modules[bundle.entry].html_path.is_some() {
            // An explicit host page owns startup. A native-only `run` is not a
            // browser entry and is deliberately absent from the web artifact.
            out.push_str("// Explicit HTML owns startup; native `run` is not emitted.\n");
        } else {
            out.push_str("export async function jet_main() {\n");
            out.push_str("  const wasm = await loadWasm();\n");
            out.push_str(&format!("  wasm.{}();\n", wasm_export_symbol("run")));
            out.push_str("}\n");
        }
    } else {
        out.push_str("export async function jet_main() {}\n");
    }
    Ok(out)
}

fn emit_js_fn(f: &FuncWeb, out: &mut String, all: &[FuncWeb]) -> WebEmitResult<()> {
    // D-DOMGEN1=A (Phase 7 extension): every top-level #Js function is
    // exported, not just `main` — a hand-written host page (index.html) can
    // call any of them directly (e.g. a click handler invoking a Jet-compiled
    // render function), the same way @WasmExport functions are callable via
    // their `bridge_*` wrapper.
    //
    // D-UISHOWCASE1 (c134 Phase 8): every exported function's body runs
    // inside `jetDom.enterRenderScope(name)` / `exitRenderScope()` — see
    // DomRuntime.js. This is what lets `ui.null_backend()` stay a real
    // zero-argument Jet call (no new language surface) while still giving
    // each DOM box a *stable* identity: `enterRenderScope` only resets the
    // "which box am I" counter when this is the OUTERMOST exported call
    // (re-entrant nested exported calls, e.g. a shared paint helper invoked
    // twice for two different cards, just keep incrementing within the
    // caller's scope instead of colliding on one reused box). A repeatedly
    // re-invoked entry point (`render(n)` in 196_ui_web_click.jet) resets to
    // the same first key every call, so its one box is reused in place; an
    // entry point that internally paints several distinct nodes in one call
    // (197_ui_showcase.jet's `initApp`) gets one distinct key per node.
    out.push_str(&format!(
        "export function {}({}) {{\n",
        f.key,
        param_names(&f.params)
    ));
    out.push_str(&format!(
        "  jetDom.enterRenderScope({});\n",
        json_quote(&f.key)
    ));
    out.push_str("  try {\n");
    emit_tir_js_body(&f.tir.body, out, all, f.file_prefix.as_deref(), 2)
        .map_err(|()| web_emit_error(f))?;
    out.push_str("  } finally {\n    jetDom.exitRenderScope();\n  }\n");
    out.push_str("}\n\n");
    Ok(())
}

fn param_names(params: &[(String, Type)]) -> String {
    params
        .iter()
        .map(|(n, _)| n.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn web_name(name: &str) -> &str {
    name.strip_prefix("user_").unwrap_or(name)
}

fn qualified_web_key(rust_mod: &str, rust_fn: &str) -> String {
    format!("{}__{}", web_name(rust_mod), web_name(rust_fn))
}

fn local_web_key(file_prefix: Option<&str>, rust_fn: &str) -> String {
    let name = web_name(rust_fn);
    file_prefix
        .map(|prefix| format!("{prefix}__{name}"))
        .unwrap_or_else(|| name.to_string())
}

fn web_place(name: &str) -> String {
    let name = name.strip_prefix("(*").and_then(|s| s.strip_suffix(')')).unwrap_or(name);
    if let Some(source) = name.strip_prefix("_jet_cap_user_") {
        return source.to_string();
    }
    web_name(name).to_string()
}
fn emit_tir_js_body(
    body: &[TIR::TStmt],
    out: &mut String,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
    indent: usize,
) -> Result<(), ()> {
    let pad = "  ".repeat(indent);
    for stmt in body {
        match stmt {
            TIR::TStmt::LineMarker(_) => {}
            TIR::TStmt::Let { name, init, .. } => out.push_str(&format!("{pad}let {} = {};\n", web_name(name), tir_js_expr(init, funcs, file_prefix)?)),
            TIR::TStmt::Assign { place, op, value, .. } => {
                let assign = op.as_ref().map(|o| format!("{}=", binop(o))).unwrap_or_else(|| "=".to_string());
                out.push_str(&format!("{pad}{} {assign} {};\n", web_name(place), tir_js_expr(value, funcs, file_prefix)?));
            }
            TIR::TStmt::Return(Some(expr)) => out.push_str(&format!("{pad}return {};\n", tir_js_expr(expr, funcs, file_prefix)?)),
            TIR::TStmt::Return(None) => out.push_str(&format!("{pad}return;\n")),
            TIR::TStmt::ExprStmt(expr) => out.push_str(&format!("{pad}{};\n", tir_js_expr(expr, funcs, file_prefix)?)),
            TIR::TStmt::If { cond: TIR::TIfCond::Plain(cond), then_body, else_body, .. } => {
                out.push_str(&format!("{pad}if ({}) {{\n", tir_js_expr(cond, funcs, file_prefix)?));
                emit_tir_js_body(then_body, out, funcs, file_prefix, indent + 1)?;
                if let Some(else_body) = else_body {
                    out.push_str(&format!("{pad}}} else {{\n"));
                    emit_tir_js_body(else_body, out, funcs, file_prefix, indent + 1)?;
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::Range { var, start, end, step, body, .. } => {
                let step = match step { Some(e) => tir_js_expr(e, funcs, file_prefix)?, None => "1".to_string() };
                out.push_str(&format!("{pad}for (let {} = {}; {} <= {}; {} += {step}) {{\n", web_name(var), tir_js_expr(start, funcs, file_prefix)?, web_name(var), tir_js_expr(end, funcs, file_prefix)?, web_name(var)));
                emit_tir_js_body(body, out, funcs, file_prefix, indent + 1)?;
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::ForIn { var, var2: None, collection, body, .. } => {
                out.push_str(&format!("{pad}for (const {} of {}) {{\n", web_name(var), tir_js_expr(collection, funcs, file_prefix)?));
                emit_tir_js_body(body, out, funcs, file_prefix, indent + 1)?;
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::Inline(inner) | TIR::TStmt::Region(inner) => emit_tir_js_body(inner, out, funcs, file_prefix, indent)?,
            _ => return Err(()),
        }
    }
    Ok(())
}

fn tir_call_args(args: &[TIR::TCallArg], funcs: &[FuncWeb], file_prefix: Option<&str>) -> Result<String, ()> {
    Ok(args.iter().map(|a| tir_js_expr(&a.value, funcs, file_prefix)).collect::<Result<Vec<_>, _>>()?.join(", "))
}

fn tir_plain_args(args: &[TIR::TExpr], funcs: &[FuncWeb], file_prefix: Option<&str>) -> Result<Vec<String>, ()> {
    args.iter().map(|a| tir_js_expr(a, funcs, file_prefix)).collect()
}

fn tir_js_expr(expr: &TIR::TExpr, funcs: &[FuncWeb], file_prefix: Option<&str>) -> Result<String, ()> {
    use TIR::TExprKind as E;
    Ok(match &expr.kind {
        E::IntLit(n, _) => n.to_string(),
        E::FloatLit(n) => n.to_string(),
        E::BoolLit(b) => b.to_string(),
        E::CharLit(c) => json_quote(&c.to_string()),
        E::StrLit(parts) => tir_js_string(parts, funcs, file_prefix)?,
        E::Local(name) => web_place(name),
        E::ConstInline(value) => value.clone(),
        E::Binary { op, lhs, rhs, .. } => format!("({} {} {})", tir_js_expr(lhs, funcs, file_prefix)?, binop(op), tir_js_expr(rhs, funcs, file_prefix)?),
        E::Unary { op, operand } => format!("({}{})", unop(op), tir_js_expr(operand, funcs, file_prefix)?),
        E::Clone(inner) | E::MaterializeView(inner) | E::DistinctRaw(inner) => tir_js_expr(inner, funcs, file_prefix)?,
        E::DistinctCtor { arg, .. } => tir_js_expr(arg, funcs, file_prefix)?,
        E::Field { recv, field_rust, .. } => format!("{}.{}", tir_js_expr(recv, funcs, file_prefix)?, web_name(field_rust)),
        E::StructLit { fields, .. } => format!("({{ {} }})", fields.iter().map(|(n, v, _)| Ok(format!("{}: {}", web_name(n), tir_js_expr(v, funcs, file_prefix)?))).collect::<Result<Vec<_>, ()>>()?.join(", ")),
        E::ListLit(elements) => format!("[{}]", elements.iter().map(|element| tir_js_expr(element, funcs, file_prefix)).collect::<Result<Vec<_>, _>>()?.join(", ")),
        E::Call { name, args } => {
            let name = local_web_key(file_prefix, name);
            let args = tir_call_args(args, funcs, file_prefix)?;
            if name == "print" { format!("jetDom.print({args})") }
            else if is_wasm_export(&name, funcs) { format!("await bridge_{name}({args})") }
            else { format!("{name}({args})") }
        }
        E::ModuleCall { form, args } => {
            let key = match form {
                TIR::TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    qualified_web_key(rust_mod, rust_fn)
                }
                TIR::TModuleCallForm::InlineMangled { mangled } => mangled.clone(),
            };
            let args = tir_call_args(args, funcs, file_prefix)?;
            if is_wasm_export(&key, funcs) { format!("await bridge_{key}({args})") }
            else { format!("{key}({args})") }
        }
        E::Print(value) => format!("jetDom.print({})", tir_js_expr(value, funcs, file_prefix)?),
        E::MethodCall { recv, method_rust, args, .. } => format!("{}.{}({})", tir_js_expr(recv, funcs, file_prefix)?, web_name(method_rust), tir_call_args(args, funcs, file_prefix)?),
        E::CoreCall { module, method, args, .. } => tir_core_call(module, method, args, funcs, file_prefix)?,
        E::HandleMethod { recv, op: TIR::THandleOp::UiBackendMethod { method }, args } => {
            let recv = tir_js_expr(recv, funcs, file_prefix)?;
            let a = tir_plain_args(args, funcs, file_prefix)?;
            match (method.as_str(), a.len()) {
                ("measure", 2) => format!("jetDom.measure({}, {})", a[0], a[1]),
                ("layout", 2) => format!("jetDom.layout({recv}, {}, {})", a[0], a[1]),
                ("paint", 1) => format!("jetDom.paint({recv}, {})", a[0]),
                ("commands", 0) => format!("jetDom.commands({recv})"),
                ("on_event", 1) => format!("jetDom.onEvent({recv}, {})", a[0]),
                ("set_focus_group", 1) => format!("jetDom.setFocusGroup({recv}, {})", a[0]),
                ("focused_label", 0) => format!("jetDom.focusedLabel({recv})"),
                _ => return Err(()),
            }
        }
        E::HandleMethod { recv, op: TIR::THandleOp::ReactiveGet, args } if args.is_empty() => format!("{}.get()", tir_js_expr(recv, funcs, file_prefix)?),
        E::HandleMethod { recv, op: TIR::THandleOp::ReactiveSet, args } if args.len() == 1 => format!("{}.set({})", tir_js_expr(recv, funcs, file_prefix)?, tir_js_expr(&args[0], funcs, file_prefix)?),
        E::HandleMethod { recv, op: TIR::THandleOp::ReactiveEffectMethod { method }, args } if args.is_empty() => {
            let recv = tir_js_expr(recv, funcs, file_prefix)?;
            match method.as_str() {
                "unsubscribe" => format!("{recv}.unsubscribe()"),
                "is_active" => format!("{recv}.isActive()"),
                _ => return Err(()),
            }
        }
        E::NumericMethod { recv, op } => match op {
            TIR::TNumericOp::CastAs { dst_rust } if dst_rust.contains("i") || dst_rust.contains("u") => format!("Math.trunc({})", tir_js_expr(recv, funcs, file_prefix)?),
            TIR::TNumericOp::CastAs { .. } => tir_js_expr(recv, funcs, file_prefix)?,
            TIR::TNumericOp::FloatToInt { lower, upper_exclusive, .. } => {
                let value = tir_js_expr(recv, funcs, file_prefix)?;
                format!("(() => {{ const __jet_value = {value}; return Number.isFinite(__jet_value) && __jet_value >= {lower} && __jet_value < {upper_exclusive} ? Math.trunc(__jet_value) : null; }})()")
            }
            _ => return Err(()),
        },
        E::OrFallback { value, fallback: TIR::TOrFallback::Value(fallback), .. } => format!("({} ?? {})", tir_js_expr(value, funcs, file_prefix)?, tir_js_expr(fallback, funcs, file_prefix)?),
        E::Lambda(lam) => {
            let params = lam.source_params.join(", ");
            match &lam.executable {
                TIR::TLambdaBody::Expr(body) => format!("({params}) => ({})", tir_js_expr(body, funcs, file_prefix)?),
                TIR::TLambdaBody::Block(body) => { let mut rendered = String::new(); emit_tir_js_body(body, &mut rendered, funcs, file_prefix, 1)?; format!("({params}) => {{\n{rendered}}}") }
            }
        }
        E::CoreClosureCall { kind: TIR::TCoreClosureKind::UiReactiveRender { executable, .. } } => format!("jetDom.reactiveRender({})", tir_js_lambda(executable, funcs, file_prefix)?),
        E::CoreClosureCall { kind: TIR::TCoreClosureKind::ReactiveEffect { executable, .. } } => format!("jetDom.makeEffect({})", tir_js_lambda(executable, funcs, file_prefix)?),
        _ => return Err(()),
    })
}

fn tir_js_lambda(
    lam: &TIR::TLambda,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
) -> Result<String, ()> {
    let params = lam.source_params.join(", ");
    match &lam.executable {
        TIR::TLambdaBody::Expr(body) => Ok(format!("({params}) => ({})", tir_js_expr(body, funcs, file_prefix)?)),
        TIR::TLambdaBody::Block(body) => {
            let mut rendered = String::new();
            emit_tir_js_body(body, &mut rendered, funcs, file_prefix, 1)?;
            Ok(format!("({params}) => {{\n{rendered}}}"))
        }
    }
}

fn tir_js_string(
    parts: &[TIR::TStrPart],
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
) -> Result<String, ()> {
    if parts.iter().any(|p| matches!(p, TIR::TStrPart::Interp(_, _))) {
        let mut out = String::from("`");
        for part in parts { match part { TIR::TStrPart::Lit(s) => out.push_str(s), TIR::TStrPart::Interp(e, _) => out.push_str(&format!("${{{}}}", tir_js_expr(e, funcs, file_prefix)?)) } }
        out.push('`'); Ok(out)
    } else {
        Ok(json_quote(&parts.iter().filter_map(|p| if let TIR::TStrPart::Lit(s) = p { Some(s.as_str()) } else { None }).collect::<String>()))
    }
}

fn tir_core_call(
    module: &str,
    method: &str,
    args: &[TIR::TExpr],
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
) -> Result<String, ()> {
    let a = tir_plain_args(args, funcs, file_prefix)?;
    let module = module.strip_prefix("core.").unwrap_or(module);
    let storage = if module.ends_with("storage.local") {
        Some("local")
    } else if module.ends_with("storage.session") {
        Some("session")
    } else {
        None
    };
    let required = web_core_arity(module, method).ok_or(())?;
    if a.len() != required {
        return Err(());
    }
    let get = |i: usize| a[i].as_str();
    if let Some(kind) = storage {
        return Ok(match method {
            "get" => format!("jetDom.storageGet(\"{kind}\", {})", get(0)),
            "set" => format!("jetDom.storageSet(\"{kind}\", {}, {})", get(0), get(1)),
            "remove" => format!("jetDom.storageRemove(\"{kind}\", {})", get(0)),
            "clear" => format!("jetDom.storageClear(\"{kind}\")"),
            _ => return Err(()),
        });
    }
    Ok(match method {
        "null_backend" => "jetDom.createBackend()".to_string(),
        "node" => format!("jetDom.makeNode({}, {}, {})", get(0), get(1), get(2)),
        "node_color" => format!("jetDom.makeNode({}, {}, {}, {})", get(0), get(1), get(2), get(3)),
        "node_role" => format!("jetDom.makeNodeRole({}, {}, {}, {})", get(0), get(1), get(2), get(3)),
        "text" => format!("jetDom.makeText({})", get(0)),
        "button" => format!("jetDom.makeButton({})", get(0)),
        "box" => format!("jetDom.makeBox({})", get(0)),
        "constraint" => format!("jetDom.makeConstraint({}, {}, {}, {})", get(0), get(1), get(2), get(3)),
        "rect" => format!("jetDom.makeRect({}, {}, {}, {})", get(0), get(1), get(2), get(3)),
        "key_event" => format!("jetDom.makeKeyEvent({})", get(0)),
        "resize_event" => format!("jetDom.makeResizeEvent({}, {})", get(0), get(1)),
        "aria_role_button" => "jetDom.ariaRoleButton()".to_string(),
        "aria_role_text_input" => "jetDom.ariaRoleTextInput()".to_string(),
        "aria_role_label" => "jetDom.ariaRoleLabel()".to_string(),
        "aria_role_container" => "jetDom.ariaRoleContainer()".to_string(),
        "signal" => format!("jetDom.makeSignal({})", get(0)),
        "on" => format!("jetDom.on({}, {}, {})", get(0), get(1), get(2)),
        "value" => format!("jetDom.value({})", get(0)),
        _ => return Err(()),
    })
}

fn web_core_arity(module: &str, method: &str) -> Option<usize> {
    let module = module.strip_prefix("core.").unwrap_or(module);
    let storage = module.ends_with("storage.local") || module.ends_with("storage.session");
    match (storage, method) {
        (true, "get" | "remove") => Some(1),
        (true, "set") => Some(2),
        (true, "clear") => Some(0),
        (false, "null_backend") => Some(0),
        (false, "node") => Some(3),
        (false, "node_color" | "node_role" | "constraint" | "rect") => Some(4),
        (false, "resize_event") => Some(2),
        (false, "aria_role_button" | "aria_role_text_input" | "aria_role_label" | "aria_role_container") => Some(0),
        (false, "key_event" | "signal" | "value" | "text" | "button" | "box") => Some(1),
        (false, "on") => Some(3),
        _ => None,
    }
}

fn is_wasm_export(name: &str, funcs: &[FuncWeb]) -> bool {
    funcs.iter().any(|f| f.key == name && f.marker == Some(WebPartitionMarker::WasmExport))
}

fn binop(op: &crate::AST::BinOp) -> &'static str {
    use crate::AST::BinOp::*;
    match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Rem => "%",
        BitAnd => "&",
        BitOr => "|",
        BitXor => "^",
        Shl => "<<",
        Shr => ">>",
        Eq => "==",
        Ne => "!=",
        Lt => "<",
        Le => "<=",
        Gt => ">",
        Ge => ">=",
        And => "&&",
        Or => "||",
    }
}
fn unop(op: &crate::AST::UnOp) -> &'static str {
    use crate::AST::UnOp::*;
    match op {
        Neg => "-",
        Not => "!",
    }
}
