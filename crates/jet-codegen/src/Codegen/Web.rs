//! D-WEBBACKEND1 / D-WEBKIND1 / D-DOMGEN1 (c123 M2): WASM + JS web backend emission.

use super::{
    build_cx_items, bundle_extern_funcs, populate_cx_from_bundle, register_foreign_enum_variants,
    update_cloneability_with_foreign_types, mangle, mangle_variant, user_type_rust, Cx, TIR,
};
use crate::Diagnostics::Span;
use crate::Sema::CompileMode;
use crate::Syntax;
use crate::AST::{AccessConvention, FfiLink, Func, Item, ProgramBundle, Type};
use jet_foundation::WebPartition::{partition_key, WebBucket, WebPartitionMarker};

/// Generated web backend artifacts (WASM Rust, JS loader/app, DOM shim, manifest).
#[derive(Debug, Clone)]
pub struct WebArtifacts {
    pub manifest_json: String,
    pub wasm_rust: String,
    pub js_app: String,
    /// Source Map v3 for `js_app`, ready for publication as `app.js.map`.
    pub js_source_map: String,
    /// Stable Jet source names (project-relative `/` paths) matching the JS map.
    pub source_names: Vec<String>,
    /// Exact Jet source bytes parallel to `source_names` (`sourcesContent`).
    pub source_contents: Vec<String>,
    pub dom_runtime: String,
    /// D-DOMGEN1=A (Phase 7 extension): a minimal host page that loads
    /// `app.js` as an ES module and runs `jet_main()` — so `jet build --target
    /// web` produces something openable in a browser, not just source files.
    /// Generic on purpose: it doesn't know about any app-specific exported
    /// function beyond `jet_main`. An example that wants real interactivity
    /// (a button calling an exported function) ships its own companion HTML
    /// alongside the `.jet` source instead of relying on this default.
    pub index_html: String,
    /// D-HTMLPAIR1 (ratified 2026-07-01, c134): the entry file's `#HTML("path.html")`
    /// marker, if any — relative to the `.jet` source's own directory.
    pub explicit_html_path: Option<String>,
    /// D-SHAPE-CLI-CARRIER1=A: canonical record embedded as a Wasm custom
    /// section by the artifact writer before publication/signing.
    pub command_record: Vec<u8>,
}

const DOM_RUNTIME: &str = include_str!("../Prelude/DomRuntime.js");
const INLINE_HANDLER_PLACEHOLDER: &str = "/*__JET_INLINE_HANDLER__*/null";

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
    source_path: String,
    /// Project-relative source name used in Source Map `sources` (never a host path).
    source_name: String,
    source_marker: String,
    file_prefix: Option<String>,
    bucket: WebBucket,
    marker: Option<WebPartitionMarker>,
    span: Span,
    params: Vec<(String, Type)>,
    return_type: Option<Type>,
    tir: TIR::TFunc,
}

struct JSSource {
    display: String,
    name: String,
    content: String,
}

struct JSMapping {
    generated_line: usize,
    generated_column: usize,
    source: usize,
    original_line: usize,
}

pub fn emit_web(
    bundle: &ProgramBundle,
    _mode: CompileMode,
    _link: Option<&FfiLink>,
) -> WebEmitResult<WebArtifacts> {
    let source_marker = js_source_marker(bundle);
    let sources = js_sources(bundle);
    let funcs = collect_web_funcs(bundle, &source_marker, &sources);
    let wasm_rust = emit_wasm_rust(bundle, &funcs)?;
    let (js_app, js_source_map, handlers) =
        emit_js_app(bundle, &funcs, &sources, &source_marker)?;
    let manifest_json = emit_manifest(bundle, &funcs, &handlers, &js_source_map);
    Ok(WebArtifacts {
        manifest_json,
        wasm_rust,
        js_app,
        js_source_map,
        source_names: sources.iter().map(|s| s.name.clone()).collect(),
        source_contents: sources.iter().map(|s| s.content.clone()).collect(),
        dom_runtime: DOM_RUNTIME.to_string(),
        index_html: emit_index_html(),
        explicit_html_path: bundle.modules[bundle.entry].html_path.clone(),
        command_record: jet_foundation::CLISchema::encode_record(
            &jet_foundation::CLISchema::executable_schema(bundle),
        ),
    })
}

/// Join rustc Wasm DWARF line rows through `// jet:line` markers to a Source Map v3
/// whose generated column is the Wasm file byte offset (line always 0).
pub fn build_wasm_jet_source_map(
    wasm: &[u8],
    rust_src: &str,
    source_names: &[String],
    source_contents: &[String],
) -> Result<String, String> {
    let code_off = jet_foundation::WasmDebug::code_section_payload_offset(wasm)
        .map_err(|e| format!("wasm code section: {e:?}"))?
        .ok_or_else(|| "wasm module has no Code section".to_string())?;
    let dwarf = jet_foundation::WasmDebug::parse_debug_line(wasm)
        .map_err(|e| format!("wasm .debug_line: {e:?}"))?;
    let rust_to_jet = rust_marker_table(rust_src);
    let name_index: std::collections::HashMap<&str, usize> = source_names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();

    let mut mappings = Vec::new();
    let mut last_offset = None;
    for row in &dwarf {
        if !row.is_stmt || row.end_sequence || row.line == 0 {
            continue;
        }
        // Only rows that land in our generated guest Rust.
        if !row.file.ends_with("app_wasm.rs") && !row.file.contains("app_wasm.rs") {
            // rustc may record just the basename or a staging path.
            let base = std::path::Path::new(&row.file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(row.file.as_str());
            if base != "app_wasm.rs" {
                continue;
            }
        }
        let Some((source_name, jet_line)) = rust_to_jet.jet_for_rust_line(row.line as usize) else {
            continue;
        };
        let Some(&source) = name_index.get(source_name.as_str()) else {
            continue;
        };
        let file_offset = code_off
            .checked_add(row.address as usize)
            .ok_or_else(|| "wasm mapping offset overflow".to_string())?;
        if last_offset == Some(file_offset) {
            continue;
        }
        last_offset = Some(file_offset);
        mappings.push(JSMapping {
            generated_line: 0,
            generated_column: file_offset,
            source,
            original_line: jet_line.saturating_sub(1),
        });
    }
    mappings.sort_by_key(|m| (m.generated_line, m.generated_column, m.source, m.original_line));
    let encoded = encode_source_mappings(&mappings);
    let names = source_names
        .iter()
        .map(|n| json_quote(n))
        .collect::<Vec<_>>()
        .join(",");
    let contents = source_contents
        .iter()
        .map(|c| json_quote(c))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"version\":3,\"file\":\"app.wasm\",\"sources\":[{names}],\"sourcesContent\":[{contents}],\"names\":[],\"mappings\":{}}}\n",
        json_quote(&encoded)
    ))
}

struct RustMarkerTable {
    /// rust line -> (source_name, jet line)
    rows: std::collections::BTreeMap<usize, (String, usize)>,
}

impl RustMarkerTable {
    fn jet_for_rust_line(&self, rust_line: usize) -> Option<(String, usize)> {
        self.rows
            .range(..=rust_line)
            .next_back()
            .map(|(_, v)| v.clone())
    }
}

fn rust_marker_table(rust_src: &str) -> RustMarkerTable {
    let mut rows = std::collections::BTreeMap::new();
    let mut pending_source: Option<String> = None;
    let mut pending_line: Option<usize> = None;
    for (i, line) in rust_src.lines().enumerate() {
        let rust_line = i + 1;
        let trim = line.trim_start();
        if let Some(name) = trim.strip_prefix("// jet:source-map source=") {
            pending_source = Some(name.trim().to_string());
            continue;
        }
        if let Some(n) = trim.strip_prefix("// jet:line ") {
            pending_line = n.trim().parse().ok();
            continue;
        }
        if let (Some(source), Some(jet_line)) = (pending_source.as_ref(), pending_line.take()) {
            rows.entry(rust_line)
                .or_insert_with(|| (source.clone(), jet_line));
        }
    }
    RustMarkerTable { rows }
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
                let key = partition_key(file_prefix, module_prefix, &f.name);
                let bucket = bundle.web_partitions.get(&key).copied().unwrap_or(WebBucket::Wasm);
                let has_wasm_export = bundle.modules.iter().any(|m| items_have_wasm_export(&m.items));
                // `dev()` is the host-side programmable dev-server entry. It
                // executes before the web build and is never web-runtime code.
                let emitted = key != "dev"
                    && (bucket == WebBucket::JS || !explicit_html || has_wasm_export);
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
        } else if bucket == WebBucket::JS {
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
                && web_wasm_abi_supported(f, &tir, bundle)
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
        TIR::TStmt::EnumMatch {
            arms, else_body, ..
        } => {
            arms.iter().all(|arm| web_stmts_guarantee_return(&arm.body))
                && else_body
                    .as_ref()
                    .is_none_or(|body| web_stmts_guarantee_return(body))
        }
        // Value/range arm tables are total only with an `else` (D-IF3).
        TIR::TStmt::MixedSwitch {
            arms,
            else_body: Some(else_body),
            ..
        } => {
            arms.iter().all(|(_, body)| web_stmts_guarantee_return(body))
                && web_stmts_guarantee_return(else_body)
        }
        TIR::TStmt::RangeSwitch { arms, else_body, .. } => {
            arms.iter().all(|(_, _, body)| web_stmts_guarantee_return(body))
                && web_stmts_guarantee_return(else_body)
        }
        TIR::TStmt::Inline(body) | TIR::TStmt::Region(body) | TIR::TStmt::Impure(body) => {
            web_stmts_guarantee_return(body)
        }
        _ => false,
    })
}

fn web_match_pattern_supported(pattern: &TIR::TPattern) -> bool {
    match &pattern.pattern {
        crate::AST::Pattern::Variant { .. }
        | crate::AST::Pattern::Ok { .. }
        | crate::AST::Pattern::Err { .. }
        | crate::AST::Pattern::Present { .. }
        | crate::AST::Pattern::Absent(_) => true,
        _ => false,
    }
}

fn web_if_cond_supported(cond: &TIR::TIfCond) -> bool {
    match cond {
        TIR::TIfCond::Plain(expr) => web_expr_supported(expr),
        TIR::TIfCond::And { left, right } => {
            web_if_cond_supported(left) && web_if_cond_supported(right)
        }
        TIR::TIfCond::IsNone { subj } => web_expr_supported(subj),
        TIR::TIfCond::IfLet { pattern, subj } | TIR::TIfCond::Matches { pattern, subj } => {
            web_match_pattern_supported(pattern) && web_expr_supported(subj)
        }
    }
}

fn web_wasm_if_cond_supported(
    cond: &TIR::TIfCond,
    bundle: &ProgramBundle,
    file_prefix: Option<&str>,
    reconstructions: &[TIR::TWebParamReconstruction],
) -> bool {
    match cond {
        TIR::TIfCond::Plain(expr) => {
            web_wasm_expr_supported(expr, bundle, file_prefix, reconstructions)
        }
        TIR::TIfCond::And { left, right } => {
            web_wasm_if_cond_supported(left, bundle, file_prefix, reconstructions)
                && web_wasm_if_cond_supported(right, bundle, file_prefix, reconstructions)
        }
        TIR::TIfCond::IsNone { subj } => {
            web_wasm_expr_supported(subj, bundle, file_prefix, reconstructions)
        }
        TIR::TIfCond::IfLet { pattern, subj } | TIR::TIfCond::Matches { pattern, subj } => {
            web_match_pattern_supported(pattern)
                && web_wasm_expr_supported(subj, bundle, file_prefix, reconstructions)
        }
    }
}

fn is_list_int(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if matches!(**inner, Type::Int | Type::IntN { .. }))
}

fn is_list_string(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if matches!(**inner, Type::String))
}

fn is_map_string_int(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Map { key, value, .. }
            if matches!(**key, Type::String)
                && matches!(**value, Type::Int)
    )
}

fn web_wasm_abi_supported(f: &Func, tir: &TIR::TFunc, bundle: &ProgramBundle) -> bool {
    let params_supported = flattened_web_params(tir).iter().all(|(_, ty)| {
        if f.web_marker == Some(WebPartitionMarker::WasmExport) {
            wasm_export_ty(ty).is_some()
        } else {
            wasm_internal_ty(ty, bundle).is_some()
        }
    });
    params_supported
        // D-JSBIND1: String / [Int] / [String] / [String: Int] params/returns
        // cross the export boundary as packed (ptr,len) u64 ownership transfers.
        && f.return_type
            .as_ref()
            .map(|ty| {
                matches!(ty, Type::String)
                    || is_list_int(ty)
                    || is_list_string(ty)
                    || is_map_string_int(ty)
                    || if f.web_marker == Some(WebPartitionMarker::WasmExport) {
                        wasm_export_ty(ty).is_some()
                    } else {
                        wasm_internal_ty(ty, bundle).is_some()
                    }
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
        TIR::TStmt::LineMarker(_) | TIR::TStmt::SourceSpan(_) | TIR::TStmt::Return(None) => true,
        TIR::TStmt::Let { init, .. } | TIR::TStmt::ExprStmt(init) | TIR::TStmt::Return(Some(init)) => web_wasm_expr_supported(init, bundle, file_prefix, reconstructions),
        TIR::TStmt::Assign { value, .. } => web_wasm_expr_supported(value, bundle, file_prefix, reconstructions),
        TIR::TStmt::If { cond, then_body, else_body, .. } => {
            web_wasm_if_cond_supported(cond, bundle, file_prefix, reconstructions)
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
        // Plain `loop x; xs` / `loop i, x; xs` over a list/local (JS already emits
        // `for…of` / `.entries()`). Keep method/map/stride/columnar forms unsupported.
        TIR::TStmt::ForIn {
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
        TIR::TStmt::EnumMatch {
            scrutinee,
            arms,
            else_body,
            ..
        } => {
            web_wasm_expr_supported(scrutinee, bundle, file_prefix, reconstructions)
                && arms.iter().all(|arm| {
                    web_match_pattern_supported(&arm.pattern)
                        && web_wasm_stmts_supported(
                            &arm.body,
                            bundle,
                            file_prefix,
                            reconstructions,
                        )
                })
                && else_body.as_ref().is_none_or(|body| {
                    web_wasm_stmts_supported(body, bundle, file_prefix, reconstructions)
                })
        }
        // `if x == { 0 -> … }` / mixed value+range tables — same if/else-if chain native emits.
        TIR::TStmt::MixedSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            web_wasm_expr_supported(subject, bundle, file_prefix, reconstructions)
                && arms.iter().all(|(cond, body)| {
                    web_wasm_expr_supported(cond, bundle, file_prefix, reconstructions)
                        && web_wasm_stmts_supported(body, bundle, file_prefix, reconstructions)
                })
                && else_body.as_ref().is_none_or(|body| {
                    web_wasm_stmts_supported(body, bundle, file_prefix, reconstructions)
                })
        }
        TIR::TStmt::RangeSwitch {
            subject,
            arms,
            else_body,
        } => {
            web_wasm_expr_supported(subject, bundle, file_prefix, reconstructions)
                && arms.iter().all(|(_, _, body)| {
                    web_wasm_stmts_supported(body, bundle, file_prefix, reconstructions)
                })
                && web_wasm_stmts_supported(else_body, bundle, file_prefix, reconstructions)
        }
        TIR::TStmt::Loop { body, .. } => {
            web_wasm_stmts_supported(body, bundle, file_prefix, reconstructions)
        }
        TIR::TStmt::While { cond, body, .. } => {
            web_wasm_expr_supported(cond, bundle, file_prefix, reconstructions)
                && web_wasm_stmts_supported(body, bundle, file_prefix, reconstructions)
        }
        TIR::TStmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            web_wasm_stmts_supported(
                std::slice::from_ref(init.as_ref()),
                bundle,
                file_prefix,
                reconstructions,
            ) && web_wasm_expr_supported(cond, bundle, file_prefix, reconstructions)
                && step.as_ref().is_none_or(|s| {
                    web_wasm_stmts_supported(
                        std::slice::from_ref(s.as_ref()),
                        bundle,
                        file_prefix,
                        reconstructions,
                    )
                })
                && web_wasm_stmts_supported(body, bundle, file_prefix, reconstructions)
        }
        TIR::TStmt::Break(_) | TIR::TStmt::Continue(_) => true,
        TIR::TStmt::IndexAssign {
            base,
            index,
            value,
            ..
        } => {
            web_wasm_expr_supported(base, bundle, file_prefix, reconstructions)
                && web_wasm_expr_supported(index, bundle, file_prefix, reconstructions)
                && web_wasm_expr_supported(value, bundle, file_prefix, reconstructions)
        }
        TIR::TStmt::Inline(body) | TIR::TStmt::Region(body) | TIR::TStmt::Impure(body) => {
            web_wasm_stmts_supported(body, bundle, file_prefix, reconstructions)
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
        TIR::TExprKind::IntLit(..)
        | TIR::TExprKind::FloatLit(_)
        | TIR::TExprKind::BoolLit(_)
        | TIR::TExprKind::Local(_) => true,
        TIR::TExprKind::Uninit => match &expr.ty {
            Type::FixedList { elem, .. } => wasm_internal_ty(elem, bundle).is_some(),
            ty => wasm_internal_ty(ty, bundle).is_some(),
        },
        TIR::TExprKind::StrLit(parts) => parts.iter().all(|part| match part {
            TIR::TStrPart::Lit(_) => true,
            TIR::TStrPart::Interp(value, _) => {
                matches!(
                    value.ty,
                    Type::Int
                        | Type::IntN { .. }
                        | Type::Float
                        | Type::Float32
                        | Type::Bool
                        | Type::String
                ) && web_wasm_expr_supported(value, bundle, file_prefix, reconstructions)
            }
        }),
        TIR::TExprKind::Binary { lhs, rhs, .. } => web_wasm_expr_supported(lhs, bundle, file_prefix, reconstructions) && web_wasm_expr_supported(rhs, bundle, file_prefix, reconstructions),
        TIR::TExprKind::Unary { operand, .. }
        | TIR::TExprKind::Clone(operand)
        | TIR::TExprKind::MaterializeView(operand)
        | TIR::TExprKind::Print(operand) => {
            web_wasm_expr_supported(operand, bundle, file_prefix, reconstructions)
        }
        TIR::TExprKind::Borrow { place, .. } => {
            web_wasm_expr_supported(place, bundle, file_prefix, reconstructions)
        }
        TIR::TExprKind::Field { recv, boxed: false, .. } => {
            web_wasm_expr_supported(recv, bundle, file_prefix, reconstructions)
        }
        TIR::TExprKind::StructLit { fields, .. } => fields
            .iter()
            .all(|(_, value, _)| web_wasm_expr_supported(value, bundle, file_prefix, reconstructions)),
        TIR::TExprKind::ListLit(elements) => elements.iter().all(|e| {
            web_wasm_expr_supported(e, bundle, file_prefix, reconstructions)
        }),
        TIR::TExprKind::Present(inner)
        | TIR::TExprKind::Ok(inner)
        | TIR::TExprKind::Err(inner) => {
            web_wasm_expr_supported(inner, bundle, file_prefix, reconstructions)
        }
        TIR::TExprKind::Absent => true,
        TIR::TExprKind::EnumLit { payload, .. } => match payload {
            TIR::TEnumPayload::Unit => true,
            TIR::TEnumPayload::Positional(args) => args.iter().all(|arg| {
                web_wasm_expr_supported(&arg.value, bundle, file_prefix, reconstructions)
            }),
            TIR::TEnumPayload::Named(fields) => fields.iter().all(|(_, arg)| {
                web_wasm_expr_supported(&arg.value, bundle, file_prefix, reconstructions)
            }),
        },
        TIR::TExprKind::Index {
            base,
            index,
            ..
        } => {
            web_wasm_expr_supported(base, bundle, file_prefix, reconstructions)
                && web_wasm_expr_supported(index, bundle, file_prefix, reconstructions)
        }
        TIR::TExprKind::Call { name, args, .. } => wasm_callee_bucket(bundle, &local_web_key(file_prefix, name)) == Some(WebBucket::Wasm)
            && args.iter().all(|a| web_wasm_expr_supported(&a.value, bundle, file_prefix, reconstructions)),
        TIR::TExprKind::MethodCall {
            recv, method, args, ..
        } => {
            method.name == "display"
                && !method.mangled
                && args.is_empty()
                && matches!(
                    &recv.ty,
                    Type::Named(type_name)
                        if bundle_has_explicit_unit_display(bundle, type_name)
                )
                && web_wasm_expr_supported(recv, bundle, file_prefix, reconstructions)
        }
        TIR::TExprKind::ModuleCall { form, args, .. } => {
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
        TIR::TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            web_wasm_if_cond_supported(cond, bundle, file_prefix, reconstructions)
                && web_wasm_stmts_supported(
                    then_body,
                    bundle,
                    file_prefix,
                    reconstructions,
                )
                && web_wasm_expr_supported(
                    then_value,
                    bundle,
                    file_prefix,
                    reconstructions,
                )
                && web_wasm_stmts_supported(
                    else_body,
                    bundle,
                    file_prefix,
                    reconstructions,
                )
                && web_wasm_expr_supported(
                    else_value,
                    bundle,
                    file_prefix,
                    reconstructions,
                )
        }
        TIR::TExprKind::Unit => true,
        _ => false,
    }
}

fn wasm_callee_bucket(bundle: &ProgramBundle, name: &str) -> Option<WebBucket> {
    bundle.web_partitions.get(name).copied()
}

fn web_stmts_supported(stmts: &[TIR::TStmt]) -> bool {
    stmts.iter().all(|stmt| match stmt {
        TIR::TStmt::LineMarker(_) | TIR::TStmt::SourceSpan(_) | TIR::TStmt::Return(None) => true,
        TIR::TStmt::Let { init, .. } | TIR::TStmt::ExprStmt(init) | TIR::TStmt::Return(Some(init)) => web_expr_supported(init),
        TIR::TStmt::Assign { value, .. } => web_expr_supported(value),
        TIR::TStmt::If { cond, then_body, else_body, .. } => {
            web_if_cond_supported(cond)
                && web_stmts_supported(then_body)
                && else_body.as_deref().map(web_stmts_supported).unwrap_or(true)
        }
        TIR::TStmt::Range { start, end, step, body, .. } => web_expr_supported(start) && web_expr_supported(end) && step.as_ref().map(web_expr_supported).unwrap_or(true) && web_stmts_supported(body),
        TIR::TStmt::ForIn {
            step: None,
            method_kind: None,
            columnar: false,
            collection,
            body,
            ..
        } => web_expr_supported(collection) && web_stmts_supported(body),
        TIR::TStmt::EnumMatch {
            scrutinee,
            arms,
            else_body,
            ..
        } => {
            web_expr_supported(scrutinee)
                && arms.iter().all(|arm| {
                    web_match_pattern_supported(&arm.pattern)
                        && web_stmts_supported(&arm.body)
                })
                && else_body
                    .as_ref()
                    .is_none_or(|body| web_stmts_supported(body))
        }
        // `if x == { 0 -> … }` / mixed value+range tables (D-IF3) — arm conds are
        // already plain `TExpr`s; HostCall-backed pattern arms stay gated out.
        TIR::TStmt::MixedSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            web_expr_supported(subject)
                && arms.iter().all(|(cond, body)| {
                    web_expr_supported(cond) && web_stmts_supported(body)
                })
                && else_body
                    .as_ref()
                    .is_none_or(|body| web_stmts_supported(body))
        }
        TIR::TStmt::RangeSwitch {
            subject,
            arms,
            else_body,
        } => {
            web_expr_supported(subject)
                && arms
                    .iter()
                    .all(|(_, _, body)| web_stmts_supported(body))
                && web_stmts_supported(else_body)
        }
        // Infinite / while / counted loops + unlabeled break/next (labeled too).
        // `break value` stays unsupported — JS has no break-with-value.
        TIR::TStmt::Loop { body, .. } => web_stmts_supported(body),
        TIR::TStmt::While { cond, body, .. } => {
            web_expr_supported(cond) && web_stmts_supported(body)
        }
        TIR::TStmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            web_stmts_supported(std::slice::from_ref(init.as_ref()))
                && web_expr_supported(cond)
                && step
                    .as_ref()
                    .is_none_or(|s| web_stmts_supported(std::slice::from_ref(s.as_ref())))
                && web_stmts_supported(body)
        }
        TIR::TStmt::Break(_) | TIR::TStmt::Continue(_) => true,
        TIR::TStmt::IndexAssign {
            base,
            index,
            value,
            ..
        } => {
            web_expr_supported(base)
                && web_expr_supported(index)
                && web_expr_supported(value)
        }
        TIR::TStmt::Inline(body) | TIR::TStmt::Region(body) | TIR::TStmt::Impure(body) => {
            web_stmts_supported(body)
        }
        _ => false,
    })
}

fn web_lambda_supported(lam: &TIR::TLambda) -> bool {
    match &lam.executable {
        TIR::TLambdaBody::Expr(expr) => web_expr_supported(expr),
        TIR::TLambdaBody::Block(body) => web_stmts_supported(body),
    }
}

/// An IfExpr uses a JS IIFE. Outer-function control cannot cross that boundary.
fn web_stmts_safe_in_js_iife(stmts: &[TIR::TStmt]) -> bool {
    stmts.iter().all(|stmt| match stmt {
        TIR::TStmt::Return(_) | TIR::TStmt::Break(_) | TIR::TStmt::Continue(_) => false,
        TIR::TStmt::If {
            then_body,
            else_body,
            ..
        } => {
            web_stmts_safe_in_js_iife(then_body)
                && else_body
                    .as_deref()
                    .is_none_or(web_stmts_safe_in_js_iife)
        }
        TIR::TStmt::Range { body, .. }
        | TIR::TStmt::ForIn { body, .. }
        | TIR::TStmt::Loop { body, .. }
        | TIR::TStmt::While { body, .. }
        | TIR::TStmt::CountedLoop { body, .. } => web_stmts_safe_in_js_iife(body),
        TIR::TStmt::EnumMatch {
            arms, else_body, ..
        } => {
            arms.iter()
                .all(|arm| web_stmts_safe_in_js_iife(&arm.body))
                && else_body
                    .as_deref()
                    .is_none_or(web_stmts_safe_in_js_iife)
        }
        TIR::TStmt::MixedSwitch {
            arms, else_body, ..
        } => {
            arms.iter()
                .all(|(_, body)| web_stmts_safe_in_js_iife(body))
                && else_body
                    .as_deref()
                    .is_none_or(web_stmts_safe_in_js_iife)
        }
        TIR::TStmt::RangeSwitch {
            arms, else_body, ..
        } => {
            arms.iter()
                .all(|(_, _, body)| web_stmts_safe_in_js_iife(body))
                && web_stmts_safe_in_js_iife(else_body)
        }
        TIR::TStmt::Inline(body) | TIR::TStmt::Region(body) | TIR::TStmt::Impure(body) => {
            web_stmts_safe_in_js_iife(body)
        }
        _ => true,
    })
}

/// JS DOM backend methods with a `jetDom.*` lowering (must match `tir_js_expr`).
fn web_js_ui_backend_method_supported(method: &str, argc: usize) -> bool {
    matches!(
        (method, argc),
        ("measure", 2)
            | ("layout", 2)
            | ("paint", 1)
            | ("mount", 1)
            | ("mount", 2)
            | ("commands", 0)
            | ("on_event", 1)
            | ("set_focus_group", 1)
            | ("focused_label", 0)
    )
}

/// Handle calls the JS preflight and emitter both understand (D-WEBTIR1).
fn web_js_handle_method_supported(op: &TIR::THandleOp, argc: usize) -> bool {
    match op {
        TIR::THandleOp::UiBackendMethod { method } => {
            web_js_ui_backend_method_supported(method, argc)
        }
        TIR::THandleOp::ReactiveGet => argc == 0,
        TIR::THandleOp::ReactiveSet => argc == 1,
        TIR::THandleOp::ReactiveEffectMethod { method } => {
            argc == 0 && matches!(method.as_str(), "unsubscribe" | "is_active")
        }
        _ => false,
    }
}

fn web_expr_supported(expr: &TIR::TExpr) -> bool {
    use TIR::TExprKind as E;
    match &expr.kind {
        E::IntLit(..) | E::FloatLit(_) | E::BoolLit(_) | E::CharLit(_) | E::Local(_)
        | E::Unit | E::DefaultLit | E::CtLit(_) | E::Uninit => true,
        E::StrLit(parts) => parts.iter().all(|p| match p { TIR::TStrPart::Lit(_) => true, TIR::TStrPart::Interp(e, _) => web_expr_supported(e) }),
        E::Binary { lhs, rhs, .. } => web_expr_supported(lhs) && web_expr_supported(rhs),
        E::Unary { operand, .. } | E::Clone(operand) | E::MaterializeView(operand) | E::DistinctRaw(operand) | E::Print(operand) => web_expr_supported(operand),
        E::Borrow { place, .. } => web_expr_supported(place),
        E::DistinctCtor { arg, .. } => web_expr_supported(arg),
        E::Field { recv, .. } => web_expr_supported(recv),
        E::StructLit { fields, .. } => fields.iter().all(|(_, e, _)| web_expr_supported(e)),
        E::EnumLit { payload, .. } => match payload {
            TIR::TEnumPayload::Unit => true,
            TIR::TEnumPayload::Positional(args) => {
                args.iter().all(|arg| web_expr_supported(&arg.value))
            }
            TIR::TEnumPayload::Named(fields) => {
                fields.iter().all(|(_, arg)| web_expr_supported(&arg.value))
            }
        },
        E::ListLit(elements) => elements.iter().all(web_expr_supported),
        E::MapLit(entries) => entries
            .iter()
            .all(|(key, value)| web_expr_supported(key) && web_expr_supported(value)),
        E::Index { base, index, .. } => web_expr_supported(base) && web_expr_supported(index),
        E::Present(inner) | E::Ok(inner) | E::Err(inner) => web_expr_supported(inner),
        E::Absent => true,
        E::Call { args, .. } | E::MethodCall { args, .. } => args.iter().all(|a| web_expr_supported(&a.value)),
        E::ModuleCall { form: TIR::TModuleCallForm::Qualified { .. } | TIR::TModuleCallForm::InlineMangled { .. }, args, .. } => args.iter().all(|a| web_expr_supported(&a.value)),
        E::CoreCall { module, method, args, .. } => {
            let arity_ok = if method == "mount" {
                // D-UI-MOUNT1=A: 2-arg or 3-arg (constraint) — same as tir_core_call.
                matches!(args.len(), 2 | 3)
            } else {
                web_core_arity(module, method) == Some(args.len())
            };
            arity_ok && args.iter().all(web_expr_supported)
        }
        E::HandleMethod { recv, op, args } => {
            web_js_handle_method_supported(op, args.len())
                && web_expr_supported(recv)
                && args.iter().all(web_expr_supported)
        }
        E::NumericMethod { recv, op: TIR::TNumericOp::CastAs { .. } | TIR::TNumericOp::FloatToInt { .. } } => web_expr_supported(recv),
        E::OrFallback { value, fallback: TIR::TOrFallback::Value(fallback), .. } => web_expr_supported(value) && web_expr_supported(fallback),
        E::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            web_if_cond_supported(cond)
                && web_stmts_supported(then_body)
                && web_stmts_safe_in_js_iife(then_body)
                && web_expr_supported(then_value)
                && web_stmts_supported(else_body)
                && web_stmts_safe_in_js_iife(else_body)
                && web_expr_supported(else_value)
        }
        E::Lambda(lam) => web_lambda_supported(lam),
        E::CoreClosureCall { kind: TIR::TCoreClosureKind::UiReactiveRender { executable, .. } | TIR::TCoreClosureKind::ReactiveEffect { executable, .. } } => web_lambda_supported(executable),
        E::CoreClosureCall {
            kind: TIR::TCoreClosureKind::UiButtonOnClick {
                label,
                executable,
                ..
            },
        } => web_expr_supported(label) && web_lambda_supported(executable),
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

fn collect_web_funcs(
    bundle: &ProgramBundle,
    source_marker: &str,
    sources: &[JSSource],
) -> Vec<FuncWeb> {
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
        cx.debug_linemap = true;
        populate_cx_from_bundle(&mut cx, bundle, i);
        register_foreign_enum_variants(&mut cx, bundle, i);
        update_cloneability_with_foreign_types(&mut cx, &module.items);
        let source_name = sources
            .iter()
            .find(|source| source.display == module.display)
            .map(|source| source.name.clone())
            .unwrap_or_else(|| "source.jet".to_string());
        collect_module_funcs(
            &module.items,
            &module.display,
            &source_name,
            source_marker,
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
    source_path: &str,
    source_name: &str,
    source_marker: &str,
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
                let key = partition_key(file_prefix, module_prefix, &f.name);
                let bucket = bundle
                    .web_partitions
                    .get(&key)
                    .copied()
                    .unwrap_or(WebBucket::Wasm);
                out.push(FuncWeb {
                    name: f.name.clone(),
                    key,
                    source_path: source_path.to_string(),
                    source_name: source_name.to_string(),
                    source_marker: source_marker.to_string(),
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
                        source_path,
                        source_name,
                        source_marker,
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

fn js_source_marker(bundle: &ProgramBundle) -> String {
    source_marker_for_texts(bundle.modules.iter().map(|module| module.source.as_str()))
}

/// Pick a marker prefix absent from every source text. One linear scan per
/// source records the longest underscore run after the base; the result uses
/// one more underscore. O(total bytes) and allocation bounded by max run + base.
fn source_marker_for_texts<'a>(texts: impl Iterator<Item = &'a str>) -> String {
    const BASE: &str = "//# __jet_source_map";
    let mut max_run = None;
    for source in texts {
        for (at, _) in source.match_indices(BASE) {
            let run = source[at + BASE.len()..]
                .chars()
                .take_while(|&c| c == '_')
                .count();
            max_run = Some(max_run.map_or(run, |best: usize| best.max(run)));
        }
    }
    match max_run {
        None => BASE.to_string(),
        Some(run) => format!("{BASE}{}", "_".repeat(run + 1)),
    }
}

fn js_sources(bundle: &ProgramBundle) -> Vec<JSSource> {
    let mut sources: Vec<_> = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(index, module)| {
            let relative = module
                .path
                .strip_prefix(&bundle.project_root)
                .ok()
                .filter(|path| !path.as_os_str().is_empty())
                .or_else(|| {
                    let display = std::path::Path::new(&module.display);
                    (!display.is_absolute()).then_some(display)
                });
            let file = module
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("source.jet");
            let fallback = if index == bundle.entry {
                file.to_string()
            } else {
                format!("deps/{}/{file}", module.alias)
            };
            let name = relative
                .map(safe_source_name)
                .filter(|name| !name.is_empty())
                .unwrap_or(fallback);
            JSSource {
                display: module.display.clone(),
                name,
                content: module.source.clone(),
            }
        })
        .collect();
    sources.sort_by(|a, b| a.name.cmp(&b.name));
    sources
}

fn safe_source_name(path: &std::path::Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn js_source_index(sources: &[JSSource], display: &str) -> usize {
    sources
        .iter()
        .position(|source| source.display == display)
        .expect("every emitted web function belongs to a bundle source")
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

fn emit_manifest(
    bundle: &ProgramBundle,
    funcs: &[FuncWeb],
    handlers: &[(String, String)],
    js_source_map: &str,
) -> String {
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
    parts.push(format!(
        "  \"traceMap\": {}",
        json_quote(&hex(&trace_map(bundle, funcs, handlers)))
    ));
    parts.push(format!(
        "  \"sourceMap\": {}",
        json_quote(&hex(js_source_map))
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

fn trace_map(bundle: &ProgramBundle, funcs: &[FuncWeb], handlers: &[(String, String)]) -> String {
    let mut lines = Vec::new();
    for module in &bundle.modules {
        lines.push(format!("source\t{}\t{}", hex(&module.display), crate::SHA256::sha256_hex(module.source.as_bytes())));
    }
    for func in funcs {
        lines.push(format!("symbol\t{}\t{}\tfn", hex(&func.source_path), hex(&func.key)));
    }
    for (path, symbol) in handlers {
        lines.push(format!("symbol\t{}\t{}\thandler", hex(path), hex(symbol)));
    }
    lines.sort();
    lines.join("\n")
}

fn hex(text: &str) -> String {
    text.as_bytes().iter().map(|byte| format!("{byte:02x}")).collect()
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
            .find(|r| r.local.rust_name() == *name)
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

fn web_recon_rust_type(ty: &Type) -> String {
    match ty {
        Type::Named(name) => user_type_rust(name),
        _ => mangle("AnonWebParam"),
    }
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
        Type::List(inner) if matches!(**inner, Type::String) => {
            prelude.push_str(&format!(
                "  const _{name} = jetDom.marshalAbi({name}, \"list-string\", wasm);\n"
            ));
            vec![format!("_{name}")]
        }
        ty if is_map_string_int(ty) => {
            prelude.push_str(&format!(
                "  const _{name} = jetDom.marshalAbi({name}, \"map-string-int\", wasm);\n"
            ));
            vec![format!("_{name}")]
        }
        _ => vec![name.to_string()],
    }
}

fn find_named_web_type(items: &[Item], name: &str) -> bool {
    items.iter().any(|item| match item {
        Item::Struct(def) => def.name == name && def.type_params.is_empty(),
        Item::Enum(def) => def.name == name && def.type_params.is_empty(),
        Item::UnitFamily(family) => family
            .distinct_defs()
            .iter()
            .any(|definition| definition.name == name),
        Item::CodeModule(module) => module
            .body
            .as_ref()
            .is_some_and(|body| find_named_web_type(body, name)),
        _ => false,
    })
}

fn items_have_explicit_unit_display(items: &[Item], type_name: &str) -> bool {
    let unit = items.iter().any(|item| match item {
        Item::UnitFamily(family) => family
            .distinct_defs()
            .iter()
            .any(|definition| definition.name == type_name),
        Item::CodeModule(module) => module
            .body
            .as_ref()
            .is_some_and(|body| items_have_explicit_unit_display(body, type_name)),
        _ => false,
    });
    let display = items.iter().any(|item| match item {
        Item::Impl(implementation) => {
            implementation.type_name == type_name
                && implementation.trait_name.as_deref() == Some(Syntax::TRAIT_DISPLAY)
        }
        Item::CodeModule(module) => module
            .body
            .as_ref()
            .is_some_and(|body| items_have_explicit_unit_display(body, type_name)),
        _ => false,
    });
    unit && display
}

fn bundle_module_index_for_alias(bundle: &ProgramBundle, alias: &str) -> Option<usize> {
    let entry = &bundle.modules[bundle.entry];
    entry
        .imports
        .iter()
        .find_map(|import| {
            (import.import_alias() == alias)
                .then(|| {
                    bundle
                        .import_targets
                        .get(&(bundle.entry, import.span))
                        .copied()
                })
                .flatten()
        })
        .or_else(|| {
            bundle
                .modules
                .iter()
                .position(|module| module.alias == alias)
        })
}

fn bundle_has_explicit_unit_display(bundle: &ProgramBundle, type_name: &str) -> bool {
    if let Some((alias, leaf)) = type_name.split_once('.') {
        return bundle_module_index_for_alias(bundle, alias)
            .and_then(|index| bundle.modules.get(index))
            .is_some_and(|module| items_have_explicit_unit_display(&module.items, leaf));
    }
    items_have_explicit_unit_display(&bundle.modules[bundle.entry].items, type_name)
}

fn bundle_has_named_web_type(bundle: &ProgramBundle, name: &str) -> bool {
    if let Some((alias, leaf)) = name.split_once('.') {
        return bundle_module_index_for_alias(bundle, alias)
            .and_then(|index| bundle.modules.get(index))
            .is_some_and(|module| find_named_web_type(&module.items, leaf));
    }
    bundle
        .modules
        .iter()
        .any(|module| find_named_web_type(&module.items, name))
}

fn collect_wasm_unions(
    ty: &Type,
    unions: &mut std::collections::BTreeMap<String, Vec<Type>>,
) {
    if let Type::Union(members) = ty {
        unions
            .entry(crate::AST::union_enum_name(members))
            .or_insert_with(|| members.clone());
    }
    match ty {
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::Tagged { inner, .. }
        | Type::FixedList { elem: inner, .. } => collect_wasm_unions(inner, unions),
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            collect_wasm_unions(key, unions);
            collect_wasm_unions(value, unions);
        }
        Type::Apply { args, .. } | Type::Union(args) => {
            for arg in args {
                collect_wasm_unions(arg, unions);
            }
        }
        Type::Tuple(fields) => {
            for (_, field) in fields {
                collect_wasm_unions(field, unions);
            }
        }
        Type::Fn { params, ret, .. } => {
            for param in params {
                collect_wasm_unions(param, unions);
            }
            if let Some(ret) = ret {
                collect_wasm_unions(ret, unions);
            }
        }
        _ => {}
    }
}

fn collect_item_wasm_unions(
    items: &[Item],
    unions: &mut std::collections::BTreeMap<String, Vec<Type>>,
) {
    for item in items {
        match item {
            Item::Struct(def) => {
                for field in &def.fields {
                    collect_wasm_unions(&field.ty, unions);
                }
            }
            Item::Enum(def) => {
                for variant in &def.variants {
                    match &variant.payload {
                        crate::AST::VariantPayload::Single(ty, _) => {
                            collect_wasm_unions(ty, unions)
                        }
                        crate::AST::VariantPayload::Named(fields) => {
                            for field in fields {
                                collect_wasm_unions(&field.ty, unions);
                            }
                        }
                        crate::AST::VariantPayload::Unit => {}
                    }
                }
            }
            Item::Func(func) => {
                for param in &func.params {
                    collect_wasm_unions(&param.ty, unions);
                }
                if let Some(ret) = &func.return_type {
                    collect_wasm_unions(ret, unions);
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_item_wasm_unions(body, unions);
                }
            }
            _ => {}
        }
    }
}

fn emit_wasm_named_types(items: &[Item], bundle: &ProgramBundle, out: &mut String) {
    for item in items {
        match item {
            Item::Struct(def) if def.type_params.is_empty() => {
                let fields = def
                    .fields
                    .iter()
                    .map(|field| {
                        Some(format!(
                            "    {}: {},\n",
                            mangle(&field.name),
                            wasm_internal_ty(&field.ty, bundle)?
                        ))
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(|parts| parts.concat());
                if let Some(fields) = fields {
                    out.push_str(&format!(
                        "#[derive(Clone)]\nstruct {} {{\n{fields}}}\n\n",
                        user_type_rust(&def.name)
                    ));
                }
            }
            Item::Enum(def) if def.type_params.is_empty() => {
                let variants = def
                    .variants
                    .iter()
                    .map(|variant| {
                        let head = mangle_variant(&variant.name);
                        Some(match &variant.payload {
                            crate::AST::VariantPayload::Unit => format!("    {head},\n"),
                            crate::AST::VariantPayload::Single(ty, _) => format!(
                                "    {head}({}),\n",
                                wasm_internal_ty(ty, bundle)?
                            ),
                            crate::AST::VariantPayload::Named(fields) => format!(
                                "    {head} {{ {} }},\n",
                                fields
                                    .iter()
                                    .map(|field| Some(format!(
                                        "{}: {}",
                                        mangle(&field.name),
                                        wasm_internal_ty(&field.ty, bundle)?
                                    )))
                                    .collect::<Option<Vec<_>>>()?
                                    .join(", ")
                            ),
                        })
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(|parts| parts.concat());
                if let Some(variants) = variants {
                    out.push_str(&format!(
                        "#[derive(Clone)]\nenum {} {{\n{variants}}}\n\n",
                        user_type_rust(&def.name)
                    ));
                }
            }
            Item::UnitFamily(family) => {
                for definition in family.distinct_defs() {
                    out.push_str(&format!(
                        "#[derive(Clone, Copy)]\nstruct {}(f64);\n\n",
                        user_type_rust(&definition.name)
                    ));
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    emit_wasm_named_types(body, bundle, out);
                }
            }
            _ => {}
        }
    }
}

fn emit_wasm_user_types(bundle: &ProgramBundle, out: &mut String) -> WebEmitResult<()> {
    let mut unions = std::collections::BTreeMap::new();
    for module in &bundle.modules {
        collect_item_wasm_unions(&module.items, &mut unions);
    }
    for (name, members) in unions {
        let variants = members
            .iter()
            .map(|member| {
                Some(format!(
                    "    {}({}),\n",
                    crate::AST::union_member_tag(member),
                    wasm_internal_ty(member, bundle)?
                ))
            })
            .collect::<Option<Vec<_>>>()
            .map(|parts| parts.concat());
        if let Some(variants) = variants {
            out.push_str(&format!(
                "#[derive(Clone)]\nenum {} {{\n{variants}}}\n\n",
                user_type_rust(&name)
            ));
        }
    }
    for module in &bundle.modules {
        emit_wasm_named_types(&module.items, bundle, out);
    }
    Ok(())
}

fn web_stmts_use_uninit(stmts: &[TIR::TStmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        TIR::TStmt::Let { init, .. } => matches!(init.kind, TIR::TExprKind::Uninit),
        TIR::TStmt::If {
            then_body,
            else_body,
            ..
        } => {
            web_stmts_use_uninit(then_body)
                || else_body
                    .as_deref()
                    .is_some_and(web_stmts_use_uninit)
        }
        TIR::TStmt::Range { body, .. }
        | TIR::TStmt::ForIn { body, .. }
        | TIR::TStmt::Loop { body, .. }
        | TIR::TStmt::While { body, .. }
        | TIR::TStmt::Inline(body)
        | TIR::TStmt::Region(body)
        | TIR::TStmt::Impure(body) => web_stmts_use_uninit(body),
        TIR::TStmt::CountedLoop {
            init, step, body, ..
        } => {
            web_stmts_use_uninit(std::slice::from_ref(init.as_ref()))
                || step.as_ref().is_some_and(|step| {
                    web_stmts_use_uninit(std::slice::from_ref(step.as_ref()))
                })
                || web_stmts_use_uninit(body)
        }
        TIR::TStmt::EnumMatch {
            arms, else_body, ..
        } => {
            arms.iter().any(|arm| web_stmts_use_uninit(&arm.body))
                || else_body
                    .as_ref()
                    .is_some_and(|body| web_stmts_use_uninit(body))
        }
        TIR::TStmt::MixedSwitch {
            arms, else_body, ..
        } => {
            arms.iter().any(|(_, body)| web_stmts_use_uninit(body))
                || else_body
                    .as_ref()
                    .is_some_and(|body| web_stmts_use_uninit(body))
        }
        TIR::TStmt::RangeSwitch {
            arms, else_body, ..
        } => {
            arms.iter().any(|(_, _, body)| web_stmts_use_uninit(body))
                || web_stmts_use_uninit(else_body)
        }
        _ => false,
    })
}

fn emit_wasm_uninit_storage(out: &mut String) {
    out.push_str("mod jet_uninit_semantics {\n");
    out.push_str(super::UNINIT_PRELUDE);
    out.push_str("}\n\n");
    out.push_str("mod jet_mem {\n");
    out.push_str(
        "pub use super::jet_uninit_semantics::{JetUninit, JetUninitFixed};\n",
    );
    out.push_str("}\n\n");
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
    if wasm_funcs
        .iter()
        .any(|func| web_stmts_use_uninit(&func.tir.body))
    {
        emit_wasm_uninit_storage(&mut out);
    }
    out.push_str(
        "trait __jet_Display { fn display(&self) -> String; }\n\
         trait JetDisplay { fn jet_display(&self) -> String; }\n\n",
    );
    out.push_str(WASM_ARITH_PRELUDE);
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
    if need_packed_abi(&is_list_string) {
        // D-JSBIND1=A: [String] as contiguous LE blob —
        // [count:u32][len0:u32][utf8…][len1:u32][utf8…]… packed u64 (ptr<<32)|byte_len.
        // Empty → 0. Returns: Wasm owns → JS copies → jet_abi_list_string_free.
        // Params: JS TextEncoder → jet_abi_list_string_alloc → jet_abi_list_string_arg.
        out.push_str(
            "fn jet_abi_list_string_ret(v: Vec<String>) -> u64 {\n\
             \x20   if v.is_empty() {\n\
             \x20       return 0;\n\
             \x20   }\n\
             \x20   let mut buf: Vec<u8> = Vec::new();\n\
             \x20   let count = v.len() as u32;\n\
             \x20   buf.extend_from_slice(&count.to_le_bytes());\n\
             \x20   for s in &v {\n\
             \x20       let bytes = s.as_bytes();\n\
             \x20       let len = bytes.len() as u32;\n\
             \x20       buf.extend_from_slice(&len.to_le_bytes());\n\
             \x20       buf.extend_from_slice(bytes);\n\
             \x20   }\n\
             \x20   let boxed = buf.into_boxed_slice();\n\
             \x20   let byte_len = boxed.len() as u32;\n\
             \x20   let ptr = Box::into_raw(boxed) as *mut u8 as u32;\n\
             \x20   ((ptr as u64) << 32) | (byte_len as u64)\n\
             }\n\n\
             fn jet_abi_list_string_arg(packed: u64) -> Vec<String> {\n\
             \x20   let ptr = (packed >> 32) as u32;\n\
             \x20   let byte_len = (packed & 0xffff_ffff) as u32;\n\
             \x20   if byte_len == 0 {\n\
             \x20       if ptr != 0 {\n\
             \x20           unsafe {\n\
             \x20               let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, 0));\n\
             \x20           }\n\
             \x20       }\n\
             \x20       return Vec::new();\n\
             \x20   }\n\
             \x20   unsafe {\n\
             \x20       let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, byte_len as usize));\n\
             \x20       let buf = boxed.into_vec();\n\
             \x20       let mut i = 0usize;\n\
             \x20       assert!(buf.len() >= 4, \"list-string header\");\n\
             \x20       let count = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;\n\
             \x20       i = 4;\n\
             \x20       let mut out = Vec::with_capacity(count);\n\
             \x20       for _ in 0..count {\n\
             \x20           assert!(i + 4 <= buf.len(), \"list-string len\");\n\
             \x20           let len = u32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as usize;\n\
             \x20           i += 4;\n\
             \x20           assert!(i + len <= buf.len(), \"list-string bytes\");\n\
             \x20           out.push(String::from_utf8(buf[i..i + len].to_vec()).expect(\"JS UTF-8\"));\n\
             \x20           i += len;\n\
             \x20       }\n\
             \x20       out\n\
             \x20   }\n\
             }\n\n\
             #[no_mangle]\n\
             pub extern \"C\" fn jet_abi_list_string_alloc(byte_len: u32) -> u32 {\n\
             \x20   let boxed = vec![0u8; byte_len as usize].into_boxed_slice();\n\
             \x20   Box::into_raw(boxed) as *mut u8 as u32\n\
             }\n\n\
             #[no_mangle]\n\
             pub extern \"C\" fn jet_abi_list_string_free(ptr: u32, byte_len: u32) {\n\
             \x20   if ptr == 0 { return; }\n\
             \x20   unsafe {\n\
             \x20       let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, byte_len as usize));\n\
             \x20   }\n\
             }\n\n",
        );
    }
    if need_packed_abi(&is_map_string_int) {
        // D-JSBIND1=A: [String: Int] as contiguous LE blob —
        // [count:u32][keyLen:u32][utf8…][val:i64 LE]… packed u64 (ptr<<32)|byte_len.
        // Entries encoded in BTreeMap key order. Empty → 0.
        out.push_str(
            "fn jet_abi_map_string_i64_ret(m: std::collections::BTreeMap<String, i64>) -> u64 {\n\
             \x20   if m.is_empty() {\n\
             \x20       return 0;\n\
             \x20   }\n\
             \x20   let mut buf: Vec<u8> = Vec::new();\n\
             \x20   let count = m.len() as u32;\n\
             \x20   buf.extend_from_slice(&count.to_le_bytes());\n\
             \x20   for (k, v) in &m {\n\
             \x20       let bytes = k.as_bytes();\n\
             \x20       let len = bytes.len() as u32;\n\
             \x20       buf.extend_from_slice(&len.to_le_bytes());\n\
             \x20       buf.extend_from_slice(bytes);\n\
             \x20       buf.extend_from_slice(&v.to_le_bytes());\n\
             \x20   }\n\
             \x20   let boxed = buf.into_boxed_slice();\n\
             \x20   let byte_len = boxed.len() as u32;\n\
             \x20   let ptr = Box::into_raw(boxed) as *mut u8 as u32;\n\
             \x20   ((ptr as u64) << 32) | (byte_len as u64)\n\
             }\n\n\
             fn jet_abi_map_string_i64_arg(packed: u64) -> std::collections::BTreeMap<String, i64> {\n\
             \x20   let ptr = (packed >> 32) as u32;\n\
             \x20   let byte_len = (packed & 0xffff_ffff) as u32;\n\
             \x20   if byte_len == 0 {\n\
             \x20       if ptr != 0 {\n\
             \x20           unsafe {\n\
             \x20               let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, 0));\n\
             \x20           }\n\
             \x20       }\n\
             \x20       return std::collections::BTreeMap::new();\n\
             \x20   }\n\
             \x20   unsafe {\n\
             \x20       let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, byte_len as usize));\n\
             \x20       let buf = boxed.into_vec();\n\
             \x20       let mut i = 0usize;\n\
             \x20       assert!(buf.len() >= 4, \"map-string-int header\");\n\
             \x20       let count = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;\n\
             \x20       i = 4;\n\
             \x20       let mut out = std::collections::BTreeMap::new();\n\
             \x20       for _ in 0..count {\n\
             \x20           let len_end = i.checked_add(4).expect(\"map-string-int key len overflow\");\n\
             \x20           assert!(len_end <= buf.len(), \"map-string-int key len\");\n\
             \x20           let len = u32::from_le_bytes(buf[i..len_end].try_into().unwrap()) as usize;\n\
             \x20           i = len_end;\n\
             \x20           let key_end = i.checked_add(len).expect(\"map-string-int key overflow\");\n\
             \x20           let value_end = key_end.checked_add(8).expect(\"map-string-int value overflow\");\n\
             \x20           assert!(value_end <= buf.len(), \"map-string-int entry\");\n\
             \x20           let key = String::from_utf8(buf[i..key_end].to_vec()).expect(\"JS UTF-8\");\n\
             \x20           let val = i64::from_le_bytes(buf[key_end..value_end].try_into().unwrap());\n\
             \x20           i = value_end;\n\
             \x20           out.insert(key, val);\n\
             \x20       }\n\
             \x20       assert_eq!(i, buf.len(), \"map-string-int trailing bytes\");\n\
             \x20       out\n\
             \x20   }\n\
             }\n\n\
             #[no_mangle]\n\
             pub extern \"C\" fn jet_abi_map_string_i64_alloc(byte_len: u32) -> u32 {\n\
             \x20   let boxed = vec![0u8; byte_len as usize].into_boxed_slice();\n\
             \x20   Box::into_raw(boxed) as *mut u8 as u32\n\
             }\n\n\
             #[no_mangle]\n\
             pub extern \"C\" fn jet_abi_map_string_i64_free(ptr: u32, byte_len: u32) {\n\
             \x20   if ptr == 0 { return; }\n\
             \x20   unsafe {\n\
             \x20       let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, byte_len as usize));\n\
             \x20   }\n\
             }\n\n",
        );
    }
    emit_wasm_user_types(bundle, &mut out)?;
    let extern_funcs = bundle_extern_funcs(bundle);
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            None,
            &extern_funcs,
        );
        populate_cx_from_bundle(&mut cx, bundle, module_index);
        for item in &module.items {
            if let Item::Impl(implementation) = item {
                if implementation.trait_name.as_deref() == Some(Syntax::TRAIT_DISPLAY)
                    && cx.unit_labels.contains_key(&implementation.type_name)
                {
                    super::Items::emit_external_trait_impl(&cx, implementation, None, &mut out);
                }
            }
        }
    }
    let mut emitted_structs = std::collections::HashSet::new();
    for f in &wasm_funcs {
        for reconstruction in &f.tir.web_param_reconstructions {
            if matches!(&reconstruction.ty, Type::Named(_)) {
                continue;
            }
            let rust_type = web_recon_rust_type(&reconstruction.ty);
            if !emitted_structs.insert(rust_type.clone()) {
                continue;
            }
            out.push_str(&format!("struct {} {{\n", rust_type));
            for (field, _, ty) in &reconstruction.fields {
                let rust_ty =
                    wasm_internal_ty(ty, bundle).ok_or_else(|| web_emit_error(f))?;
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

fn emit_wasm_fn(bundle: &ProgramBundle, f: &FuncWeb, export: bool, out: &mut String, funcs: &[FuncWeb]) -> WebEmitResult<()> {
    let string_ret = matches!(f.return_type.as_ref(), Some(Type::String));
    let list_ret = f.return_type.as_ref().is_some_and(is_list_int);
    let list_string_ret = f.return_type.as_ref().is_some_and(is_list_string);
    let map_ret = f.return_type.as_ref().is_some_and(is_map_string_int);
    let flat = flattened_web_params(&f.tir);
    let string_params = flat.iter().any(|(_, ty)| matches!(ty, Type::String));
    let list_params = flat.iter().any(|(_, ty)| is_list_int(ty));
    let list_string_params = flat.iter().any(|(_, ty)| is_list_string(ty));
    let map_params = flat.iter().any(|(_, ty)| is_map_string_int(ty));
    let reconstructed_export = export && !f.tir.web_param_reconstructions.is_empty();
    // String / [Int] / [String] / [String: Int] cannot be bare `extern "C"` — wrap as packed u64.
    let wrapped_export = export
        && (reconstructed_export
            || string_ret
            || string_params
            || list_ret
            || list_params
            || list_string_ret
            || list_string_params
            || map_ret
            || map_params);
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
        if string_ret || list_ret || list_string_ret || map_ret {
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
            } else if is_list_string(ty) {
                out.push_str(&format!("    let {name} = jet_abi_list_string_arg({name});\n"));
            } else if is_map_string_int(ty) {
                out.push_str(&format!(
                    "    let {name} = jet_abi_map_string_i64_arg({name});\n"
                ));
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
                reconstruction.local.rust_name(),
                web_recon_rust_type(&reconstruction.ty),
                fields
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
        } else if list_string_ret {
            out.push_str(&format!(
                "    return jet_abi_list_string_ret(jet_wasm_{}({args}));\n",
                f.key
            ));
        } else if map_ret {
            out.push_str(&format!(
                "    return jet_abi_map_string_i64_ret(jet_wasm_{}({args}));\n",
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
                .find(|r| r.local.rust_name() == *name)
                .map(|r| web_recon_rust_type(&r.ty))
                .or_else(|| wasm_param_rust_ty(ty, *conv, bundle));
            rust_ty.map(|t| format!("{name}: {t}"))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| web_emit_error(f))?;
    out.push_str(&params.join(", "));
    out.push(')');
    if let Some(ret) = &f.return_type {
        out.push_str(&format!(
            " -> {} ",
            wasm_internal_ty(ret, bundle).ok_or_else(|| web_emit_error(f))?
        ));
    }
    out.push_str("{\n");
    out.push_str(&format!("    // jet:source-map source={}\n", f.source_name));
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
        Type::List(inner) if matches!(**inner, Type::String) => Some("Vec<String>"),
        Type::Map { key, value, .. }
            if matches!(**key, Type::String) && matches!(**value, Type::Int) =>
        {
            Some("std::collections::BTreeMap<String, i64>")
        }
        _ => None,
    }
}

fn wasm_storage_ty(ty: &Type) -> Option<String> {
    Some(match ty {
        Type::Int | Type::IntN { signed: true, .. } => "i64".to_string(),
        Type::IntN { signed: false, .. } => "u64".to_string(),
        Type::Float | Type::Float32 => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "String".to_string(),
        Type::List(inner) => format!("Vec<{}>", wasm_storage_ty(inner)?),
        Type::FixedList { elem, len, .. } => {
            format!("[{}; {len}]", wasm_storage_ty(elem)?)
        }
        // D-FAIL-CARRIER1=A: the wasm module names the one carrier every
        // other tier names. `T?` is the view whose report is the clean absence.
        Type::Option(inner) => format!("JetOutcome<{}, JetAbsent>", wasm_storage_ty(inner)?),
        Type::Result { ok, err } => format!(
            "JetOutcome<{}, {}>",
            wasm_storage_ty(ok)?,
            wasm_storage_ty(err)?
        ),
        Type::Map { key, value, .. } => format!(
            "std::collections::BTreeMap<{}, {}>",
            wasm_storage_ty(key)?,
            wasm_storage_ty(value)?
        ),
        Type::Named(name) if name == Syntax::TYPE_ERR => "JetErr".to_string(),
        Type::Named(name) => user_type_rust(name.rsplit('.').next().unwrap_or(name)),
        Type::Union(members) => user_type_rust(&crate::AST::union_enum_name(members)),
        _ => return None,
    })
}

fn wasm_internal_ty(ty: &Type, bundle: &ProgramBundle) -> Option<String> {
    Some(match ty {
        Type::FixedList { elem, len, .. } => {
            format!("[{}; {len}]", wasm_internal_ty(elem, bundle)?)
        }
        Type::Option(inner) => format!("JetOutcome<{}, JetAbsent>", wasm_internal_ty(inner, bundle)?),
        Type::Result { ok, err } => format!(
            "JetOutcome<{}, {}>",
            wasm_internal_ty(ok, bundle)?,
            wasm_internal_ty(err, bundle)?
        ),
        Type::Union(members) => user_type_rust(&crate::AST::union_enum_name(members)),
        Type::Named(name) if name == Syntax::TYPE_ERR => "JetErr".to_string(),
        Type::Named(name) if bundle_has_named_web_type(bundle, name) => {
            user_type_rust(name.rsplit('.').next().unwrap_or(name))
        }
        _ => wasm_ty(ty)?.to_string(),
    })
}

/// Rust param type matching TIR `param_place` borrow rules (Read String → &String).
fn wasm_param_rust_ty(
    ty: &Type,
    conv: AccessConvention,
    bundle: &ProgramBundle,
) -> Option<String> {
    let owned = wasm_internal_ty(ty, bundle)?;
    match (conv, ty) {
        (AccessConvention::Read, Type::String) => Some("&String".to_string()),
        (AccessConvention::Write, Type::String) => Some("&mut String".to_string()),
        (AccessConvention::Move, Type::String) => Some("String".to_string()),
        (AccessConvention::Read, Type::List(inner))
            if matches!(**inner, Type::Int | Type::IntN { .. }) =>
        {
            Some("&Vec<i64>".to_string())
        }
        (AccessConvention::Write, Type::List(inner))
            if matches!(**inner, Type::Int | Type::IntN { .. }) =>
        {
            Some("&mut Vec<i64>".to_string())
        }
        (AccessConvention::Move, Type::List(inner))
            if matches!(**inner, Type::Int | Type::IntN { .. }) =>
        {
            Some("Vec<i64>".to_string())
        }
        (AccessConvention::Read, Type::List(inner)) if matches!(**inner, Type::String) => {
            Some("&Vec<String>".to_string())
        }
        (AccessConvention::Write, Type::List(inner)) if matches!(**inner, Type::String) => {
            Some("&mut Vec<String>".to_string())
        }
        (AccessConvention::Move, Type::List(inner)) if matches!(**inner, Type::String) => {
            Some("Vec<String>".to_string())
        }
        (AccessConvention::Read, ty) if is_map_string_int(ty) => {
            Some("&std::collections::BTreeMap<String, i64>".to_string())
        }
        (AccessConvention::Write, ty) if is_map_string_int(ty) => {
            Some("&mut std::collections::BTreeMap<String, i64>".to_string())
        }
        (AccessConvention::Move, ty) if is_map_string_int(ty) => {
            Some("std::collections::BTreeMap<String, i64>".to_string())
        }
        (AccessConvention::Read, t) if t.is_scalar() => Some(owned),
        (AccessConvention::Read, _) => Some(format!("&{owned}")),
        (AccessConvention::Write, _) => Some(format!("&mut {owned}")),
        (AccessConvention::Move, _) => Some(owned),
    }
}

fn wasm_export_arg_expr(name: &str, ty: &Type, conv: AccessConvention) -> String {
    match (conv, ty) {
        (AccessConvention::Read, Type::String) => format!("&{name}"),
        (AccessConvention::Write, Type::String) => format!("&mut {name}"),
        (AccessConvention::Read, Type::List(inner))
            if matches!(**inner, Type::Int | Type::IntN { .. })
                || matches!(**inner, Type::String) =>
        {
            format!("&{name}")
        }
        (AccessConvention::Write, Type::List(inner))
            if matches!(**inner, Type::Int | Type::IntN { .. })
                || matches!(**inner, Type::String) =>
        {
            format!("&mut {name}")
        }
        (AccessConvention::Read, ty) if is_map_string_int(ty) => format!("&{name}"),
        (AccessConvention::Write, ty) if is_map_string_int(ty) => format!("&mut {name}"),
        _ => name.to_string(),
    }
}

fn wasm_export_ty(ty: &Type) -> Option<&'static str> {
    match ty {
        // Packed (ptr,len) u64 on the C ABI; internal jet_wasm_* still uses String / Vec.
        Type::String => Some("u64"),
        Type::List(inner) if matches!(**inner, Type::Int | Type::IntN { .. }) => Some("u64"),
        Type::List(inner) if matches!(**inner, Type::String) => Some("u64"),
        ty if is_map_string_int(ty) => Some("u64"),
        _ => wasm_ty(ty),
    }
}

fn wasm_enum_head(enum_type: &str, variant: &str) -> String {
    let variant = if enum_type.starts_with("__JetUnion_") {
        variant.to_string()
    } else {
        mangle_variant(variant)
    };
    format!("{}::{variant}", user_type_rust(enum_type))
}

fn wasm_emit_enum_arg(
    arg: &TIR::TEnumArg,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
    reconstructions: &[TIR::TWebParamReconstruction],
) -> Result<String, ()> {
    let mut value = wasm_emit_expr(&arg.value, funcs, file_prefix, reconstructions)?;
    if arg.clone {
        value = format!("({value}).clone()");
    }
    if arg.boxed {
        value = format!("Box::new({value})");
    }
    Ok(value)
}

fn wasm_emit_call_arg(
    arg: &TIR::TCallArg,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
    reconstructions: &[TIR::TWebParamReconstruction],
) -> Result<String, ()> {
    let uninit_borrow = match &arg.value.kind {
        TIR::TExprKind::Local(local) if local.uninit_fixed && arg.mut_borrow => {
            Some(format!("({}).as_array_mut()", local.rust_place()))
        }
        TIR::TExprKind::Local(local) if local.uninit_fixed && arg.borrow => {
            Some(format!("({}).as_array()", local.rust_place()))
        }
        _ => None,
    };
    let mut value = match &uninit_borrow {
        Some(value) => value.clone(),
        None => wasm_emit_expr(&arg.value, funcs, file_prefix, reconstructions)?,
    };
    if arg.clone || arg.arc_clone {
        value = format!("({value}).clone()");
    }
    if arg.widen_to_vec {
        value = format!("({value}).to_vec()");
    }
    if let Some(Type::Union(members)) = &arg.widen_to_union {
        value = format!(
            "{}({value})",
            wasm_enum_head(
                &crate::AST::union_enum_name(members),
                &crate::AST::union_member_tag(&arg.value.ty),
            )
        );
    }
    if arg.fn_coerce.is_some() {
        return Err(());
    }
    if arg.borrow && uninit_borrow.is_none() {
        value = format!("&({value})");
    } else if arg.mut_borrow && uninit_borrow.is_none() {
        value = format!("&mut ({value})");
    }
    Ok(value)
}

fn wasm_match_arm_pattern(pattern: &TIR::TPattern) -> Result<String, ()> {
    match &pattern.pattern {
        crate::AST::Pattern::Ok { binding, .. } => Ok(format!("Ok({})", mangle(binding))),
        crate::AST::Pattern::Err { binding, .. } => Ok(format!("Err({})", mangle(binding))),
        crate::AST::Pattern::Present { binding, .. } => Ok(format!("Ok({})", mangle(binding))),
        crate::AST::Pattern::Absent(_) => Ok("Err(JetAbsent)".to_string()),
        crate::AST::Pattern::Variant {
            variant, bindings, ..
        } => {
            let owner = pattern.enum_type.as_deref().ok_or(())?;
            let head = wasm_enum_head(owner, variant);
            if bindings.is_empty() {
                return Ok(head);
            }
            let slots = bindings
                .iter()
                .map(|slot| match slot {
                    crate::AST::PatSlot::Wildcard => "_".to_string(),
                    crate::AST::PatSlot::Bind { name, .. } => mangle(name),
                    crate::AST::PatSlot::Range { lo, hi } => format!("_ @ {lo}..={hi}"),
                })
                .collect::<Vec<_>>()
                .join(", ");
            Ok(format!("{head}({slots})"))
        }
        _ => Err(()),
    }
}

fn wasm_if_let_pattern(pattern: &TIR::TPattern) -> Result<String, ()> {
    // Binding-position patterns for `if let` — same spelling as match arms for
    // Result/Option; variants keep the enum head form.
    wasm_match_arm_pattern(pattern)
}

fn emit_wasm_if_head(
    cond: &TIR::TIfCond,
    out: &mut String,
    indent: usize,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
    reconstructions: &[TIR::TWebParamReconstruction],
) -> Result<(), ()> {
    let pad = "    ".repeat(indent);
    let head = match cond {
        TIR::TIfCond::Plain(expr) => {
            format!(
                "if {} {{",
                wasm_emit_expr(expr, funcs, file_prefix, reconstructions)?
            )
        }
        TIR::TIfCond::IsNone { subj } => {
            format!(
                "if ({}).is_err() {{",
                wasm_emit_expr(subj, funcs, file_prefix, reconstructions)?
            )
        }
        TIR::TIfCond::IfLet { pattern, subj } => {
            format!(
                "if let {} = {} {{",
                wasm_if_let_pattern(pattern)?,
                wasm_emit_expr(subj, funcs, file_prefix, reconstructions)?
            )
        }
        TIR::TIfCond::Matches { pattern, subj } => {
            format!(
                "if matches!(&({}), {}) {{",
                wasm_emit_expr(subj, funcs, file_prefix, reconstructions)?,
                wasm_match_arm_pattern(pattern)?
            )
        }
        TIR::TIfCond::And { .. } => return Err(()),
    };
    out.push_str(&format!("{pad}{head}\n"));
    Ok(())
}

fn emit_wasm_if(
    cond: &TIR::TIfCond,
    then_body: &[TIR::TStmt],
    else_body: Option<&[TIR::TStmt]>,
    out: &mut String,
    indent: usize,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
    reconstructions: &[TIR::TWebParamReconstruction],
) -> Result<(), ()> {
    let pad = "    ".repeat(indent);
    if let TIR::TIfCond::And { left, right } = cond {
        emit_wasm_if_head(left, out, indent, funcs, file_prefix, reconstructions)?;
        emit_wasm_if(
            right,
            then_body,
            else_body,
            out,
            indent + 1,
            funcs,
            file_prefix,
            reconstructions,
        )?;
        if let Some(else_body) = else_body {
            out.push_str(&format!("{pad}}} else {{\n"));
            emit_wasm_body(else_body, out, indent + 1, funcs, file_prefix, reconstructions)?;
        }
        out.push_str(&format!("{pad}}}\n"));
        return Ok(());
    }
    emit_wasm_if_head(cond, out, indent, funcs, file_prefix, reconstructions)?;
    emit_wasm_body(then_body, out, indent + 1, funcs, file_prefix, reconstructions)?;
    if let Some(else_body) = else_body {
        out.push_str(&format!("{pad}}} else {{\n"));
        emit_wasm_body(else_body, out, indent + 1, funcs, file_prefix, reconstructions)?;
    }
    out.push_str(&format!("{pad}}}\n"));
    Ok(())
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
            TIR::TStmt::Assign { place, op, value, line, .. } => {
                let value_ty = value.ty.clone();
                let value = wasm_emit_expr(value, funcs, file_prefix, reconstructions)?;
                // D-EXPSEM1=A / D-FLOORDIV1=A: Rust has no `**=` and no `/%=`,
                // so those compounds read the place, call the shared Prelude
                // helper, and write the result back.
                let prelude_of = |target: &str| {
                    op.and_then(|op| {
                        wasm_prelude_call(op, target, &value, &value_ty, file_prefix, *line)
                    })
                };
                match place {
                    TIR::TPlace::Local(local) if local.uninit_scalar => {
                        let place = local.rust_place();
                        let read = format!("({place}).read().clone()");
                        match op {
                            Some(_) if prelude_of(&read).is_some() => {
                                let call = prelude_of(&read).expect("checked just above");
                                out.push_str(&format!("{pad}{place}.write({call});\n"));
                            }
                            Some(op) => out.push_str(&format!(
                                "{pad}{place}.write(({place}).read().clone() {} {value});\n",
                                binop(op).ok_or(())?
                            )),
                            None => {
                                out.push_str(&format!("{pad}{place}.write({value});\n"))
                            }
                        }
                    }
                    TIR::TPlace::Local(local) if local.uninit_fixed => {
                        out.push_str(&format!(
                            "{pad}{}.write_array({value});\n",
                            local.rust_place()
                        ));
                    }
                    _ if prelude_of(&wasm_tir_place(place)?).is_some() => {
                        let target = wasm_tir_place(place)?;
                        let call = prelude_of(&target).expect("checked just above");
                        out.push_str(&format!("{pad}{target} = {call};\n"));
                    }
                    _ => out.push_str(&format!(
                        "{pad}{} {}= {value};\n",
                        wasm_tir_place(place)?,
                        match op { Some(op) => binop(op).ok_or(())?, None => "" }
                    )),
                }
            }
            TIR::TStmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                emit_wasm_if(
                    cond,
                    then_body,
                    else_body.as_deref(),
                    out,
                    indent,
                    funcs,
                    file_prefix,
                    reconstructions,
                )?;
            }
            TIR::TStmt::Range { var, start, end, step, exclusive, body, .. } => {
                let start = wasm_emit_expr(start, funcs, file_prefix, reconstructions)?;
                let end = wasm_emit_expr(end, funcs, file_prefix, reconstructions)?;
                let loop_var = mangle(var);
                let range_op = if *exclusive { ".." } else { "..=" };
                match step {
                    Some(step) => {
                        let step = wasm_emit_expr(step, funcs, file_prefix, reconstructions)?;
                        out.push_str(&format!("{pad}{{\n"));
                        out.push_str(&format!("{pad}    let __jet_loop_start = {start};\n"));
                        out.push_str(&format!("{pad}    let __jet_loop_end = {end};\n"));
                        out.push_str(&format!("{pad}    let __jet_loop_stride = {step};\n"));
                        out.push_str(&format!(
                            "{pad}    assert!(__jet_loop_stride > 0, \"E0123: loop stride must be positive\");\n"
                        ));
                        out.push_str(&format!(
                            "{pad}    for {loop_var} in (__jet_loop_start{range_op}__jet_loop_end).step_by(__jet_loop_stride as usize) {{\n"
                        ));
                        emit_wasm_body(body, out, indent + 2, funcs, file_prefix, reconstructions)?;
                        out.push_str(&format!("{pad}    }}\n"));
                        out.push_str(&format!("{pad}}}\n"));
                    }
                    None => {
                        out.push_str(&format!(
                            "{pad}for {loop_var} in ({start}){range_op}({end}) {{\n"
                        ));
                        emit_wasm_body(body, out, indent + 1, funcs, file_prefix, reconstructions)?;
                        out.push_str(&format!("{pad}}}\n"));
                    }
                }
            }
            // Plain list/local ForIn — mirror native `.iter().cloned()` (or by-value).
            // D-RANGE-EXCL1=C: two-binding is index then item via `.enumerate()`.
            TIR::TStmt::ForIn {
                var,
                var2,
                step: None,
                method_kind: None,
                columnar: false,
                by_value,
                collection,
                body,
                ..
            } => {
                let collection = wasm_emit_expr(collection, funcs, file_prefix, reconstructions)?;
                let iter = if *by_value {
                    format!("({collection})")
                } else {
                    format!("({collection}).iter().cloned()")
                };
                match var2 {
                    Some(v2) => {
                        out.push_str(&format!(
                            "{pad}for (__jet_i, __jet_item) in {iter}.enumerate() {{\n"
                        ));
                        out.push_str(&format!(
                            "{pad}    let {} = __jet_i as i64;\n",
                            mangle(var)
                        ));
                        out.push_str(&format!(
                            "{pad}    let {} = __jet_item;\n",
                            mangle(v2)
                        ));
                        emit_wasm_body(body, out, indent + 1, funcs, file_prefix, reconstructions)?;
                        out.push_str(&format!("{pad}}}\n"));
                    }
                    None => {
                        let loop_var = mangle(var);
                        out.push_str(&format!("{pad}for {loop_var} in {iter} {{\n"));
                        emit_wasm_body(body, out, indent + 1, funcs, file_prefix, reconstructions)?;
                        out.push_str(&format!("{pad}}}\n"));
                    }
                }
            }
            TIR::TStmt::Loop { label, body } => {
                out.push_str(&format!("{pad}{}loop {{\n", wasm_label_prefix(label)));
                emit_wasm_body(body, out, indent + 1, funcs, file_prefix, reconstructions)?;
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::While { label, cond, body } => {
                out.push_str(&format!(
                    "{pad}{}while {} {{\n",
                    wasm_label_prefix(label),
                    wasm_emit_expr(cond, funcs, file_prefix, reconstructions)?
                ));
                emit_wasm_body(body, out, indent + 1, funcs, file_prefix, reconstructions)?;
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::CountedLoop {
                label,
                init,
                cond,
                step,
                body,
            } => {
                out.push_str(&format!("{pad}{{\n"));
                emit_wasm_body(
                    std::slice::from_ref(init.as_ref()),
                    out,
                    indent + 1,
                    funcs,
                    file_prefix,
                    reconstructions,
                )?;
                let inner = "    ".repeat(indent + 1);
                if step.is_some() {
                    out.push_str(&format!("{inner}let mut __jet_loop_first = true;\n"));
                }
                out.push_str(&format!(
                    "{inner}{}loop {{\n",
                    wasm_label_prefix(label)
                ));
                let body_pad = "    ".repeat(indent + 2);
                if let Some(step) = step {
                    out.push_str(&format!(
                        "{body_pad}if __jet_loop_first {{ __jet_loop_first = false; }} else {{\n"
                    ));
                    emit_wasm_body(
                        std::slice::from_ref(step.as_ref()),
                        out,
                        indent + 3,
                        funcs,
                        file_prefix,
                        reconstructions,
                    )?;
                    out.push_str(&format!("{body_pad}}}\n"));
                }
                out.push_str(&format!(
                    "{body_pad}if !({}) {{ break; }}\n",
                    wasm_emit_expr(cond, funcs, file_prefix, reconstructions)?
                ));
                emit_wasm_body(body, out, indent + 2, funcs, file_prefix, reconstructions)?;
                out.push_str(&format!("{inner}}}\n"));
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::Break(label) => match label {
                Some(name) => out.push_str(&format!(
                    "{pad}break '{};\n",
                    mangle(name)
                )),
                None => out.push_str(&format!("{pad}break;\n")),
            },
            TIR::TStmt::Continue(label) => match label {
                Some(name) => out.push_str(&format!(
                    "{pad}continue '{};\n",
                    mangle(name)
                )),
                None => out.push_str(&format!("{pad}continue;\n")),
            },
            TIR::TStmt::IndexAssign {
                base,
                index,
                is_map,
                value,
                uninit,
            } => {
                let b = match &base.kind {
                    TIR::TExprKind::Local(local) if *uninit && local.uninit_fixed => {
                        local.rust_place()
                    }
                    _ => wasm_emit_expr(base, funcs, file_prefix, reconstructions)?,
                };
                let i = wasm_emit_expr(index, funcs, file_prefix, reconstructions)?;
                let v = wasm_emit_expr(value, funcs, file_prefix, reconstructions)?;
                if *is_map {
                    out.push_str(&format!(
                        "{pad}{{ let __jet_v = {v}; ({b}).insert(({i}).clone(), __jet_v); }}\n"
                    ));
                } else if *uninit {
                    out.push_str(&format!(
                        "{pad}{{ let __jet_v = {v}; ({b}).write({i} as usize, __jet_v); }}\n"
                    ));
                } else {
                    out.push_str(&format!(
                        "{pad}{{ let __jet_v = {v}; ({b})[{i} as usize] = __jet_v; }}\n"
                    ));
                }
            }
            TIR::TStmt::EnumMatch {
                scrutinee,
                clone_subject,
                arms,
                else_body,
                fallthrough,
            } => {
                let mut subject =
                    wasm_emit_expr(scrutinee, funcs, file_prefix, reconstructions)?;
                if *clone_subject {
                    subject = format!("({subject}).clone()");
                }
                out.push_str(&format!("{pad}match {subject} {{\n"));
                for arm in arms {
                    out.push_str(&format!(
                        "{pad}    {} => {{\n",
                        wasm_match_arm_pattern(&arm.pattern)?
                    ));
                    emit_wasm_body(
                        &arm.body,
                        out,
                        indent + 2,
                        funcs,
                        file_prefix,
                        reconstructions,
                    )?;
                    out.push_str(&format!("{pad}    }},\n"));
                }
                if let Some(body) = else_body {
                    out.push_str(&format!("{pad}    _ => {{\n"));
                    emit_wasm_body(
                        body,
                        out,
                        indent + 2,
                        funcs,
                        file_prefix,
                        reconstructions,
                    )?;
                    out.push_str(&format!("{pad}    }},\n"));
                } else if *fallthrough {
                    out.push_str(&format!("{pad}    _ => std::process::abort(),\n"));
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            // Value / mixed arm tables — same if/else-if chain as native MixedSwitch.
            TIR::TStmt::MixedSwitch {
                subject,
                arms,
                else_body,
                ..
            } => {
                let subject_str =
                    wasm_emit_expr(subject, funcs, file_prefix, reconstructions)?;
                out.push_str(&format!("{pad}{{\n"));
                let inner = "    ".repeat(indent + 1);
                out.push_str(&format!(
                    "{inner}let __jet_switch_subject = &({subject_str});\n"
                ));
                for (i, (cond, body)) in arms.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "} else if" };
                    out.push_str(&format!(
                        "{inner}{kw} {} {{\n",
                        wasm_emit_expr(cond, funcs, file_prefix, reconstructions)?
                    ));
                    emit_wasm_body(
                        body,
                        out,
                        indent + 2,
                        funcs,
                        file_prefix,
                        reconstructions,
                    )?;
                }
                match else_body {
                    None if !arms.is_empty() => {
                        out.push_str(&format!("{inner}}}\n"));
                    }
                    None => {}
                    Some(body) if arms.is_empty() => {
                        emit_wasm_body(
                            body,
                            out,
                            indent + 1,
                            funcs,
                            file_prefix,
                            reconstructions,
                        )?;
                    }
                    Some(body) => {
                        out.push_str(&format!("{inner}}} else {{\n"));
                        emit_wasm_body(
                            body,
                            out,
                            indent + 2,
                            funcs,
                            file_prefix,
                            reconstructions,
                        )?;
                        out.push_str(&format!("{inner}}}\n"));
                    }
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::RangeSwitch {
                subject,
                arms,
                else_body,
            } => {
                let subject_str =
                    wasm_emit_expr(subject, funcs, file_prefix, reconstructions)?;
                out.push_str(&format!("{pad}{{\n"));
                let inner = "    ".repeat(indent + 1);
                out.push_str(&format!(
                    "{inner}let __jet_switch_subject = &({subject_str});\n"
                ));
                for (i, (lo, hi, body)) in arms.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "} else if" };
                    out.push_str(&format!(
                        "{inner}{kw} ({subject_str} >= {lo} && {subject_str} <= {hi}) {{\n"
                    ));
                    emit_wasm_body(
                        body,
                        out,
                        indent + 2,
                        funcs,
                        file_prefix,
                        reconstructions,
                    )?;
                }
                out.push_str(&format!("{inner}}} else {{\n"));
                emit_wasm_body(
                    else_body,
                    out,
                    indent + 2,
                    funcs,
                    file_prefix,
                    reconstructions,
                )?;
                out.push_str(&format!("{inner}}}\n"));
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::Inline(body)
            | TIR::TStmt::Region(body)
            | TIR::TStmt::Impure(body) => {
                emit_wasm_body(body, out, indent, funcs, file_prefix, reconstructions)?
            }
            TIR::TStmt::Return(None) => out.push_str(&format!("{pad}return;\n")),
            TIR::TStmt::LineMarker(line) => {
                out.push_str(&format!("{pad}// jet:line {line}\n"));
            }
            TIR::TStmt::SourceSpan(_) => {}
            _ => return Err(()),
        }
    }
    Ok(())
}

fn emit_wasm_if_value(
    cond: &TIR::TIfCond,
    then_body: &[TIR::TStmt],
    then_value: &TIR::TExpr,
    else_body: &[TIR::TStmt],
    else_value: &TIR::TExpr,
    out: &mut String,
    indent: usize,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
    reconstructions: &[TIR::TWebParamReconstruction],
) -> Result<(), ()> {
    let pad = "    ".repeat(indent);
    if let TIR::TIfCond::And { left, right } = cond {
        emit_wasm_if_head(
            left,
            out,
            indent,
            funcs,
            file_prefix,
            reconstructions,
        )?;
        emit_wasm_if_value(
            right,
            then_body,
            then_value,
            else_body,
            else_value,
            out,
            indent + 1,
            funcs,
            file_prefix,
            reconstructions,
        )?;
        out.push_str(&format!("{pad}}} else {{\n"));
        emit_wasm_body(
            else_body,
            out,
            indent + 1,
            funcs,
            file_prefix,
            reconstructions,
        )?;
        out.push_str(&format!(
            "{}{}\n{pad}}}",
            "    ".repeat(indent + 1),
            wasm_emit_expr(else_value, funcs, file_prefix, reconstructions)?
        ));
        return Ok(());
    }

    emit_wasm_if_head(
        cond,
        out,
        indent,
        funcs,
        file_prefix,
        reconstructions,
    )?;
    emit_wasm_body(
        then_body,
        out,
        indent + 1,
        funcs,
        file_prefix,
        reconstructions,
    )?;
    out.push_str(&format!(
        "{}{}\n{pad}}} else {{\n",
        "    ".repeat(indent + 1),
        wasm_emit_expr(then_value, funcs, file_prefix, reconstructions)?
    ));
    emit_wasm_body(
        else_body,
        out,
        indent + 1,
        funcs,
        file_prefix,
        reconstructions,
    )?;
    out.push_str(&format!(
        "{}{}\n{pad}}}",
        "    ".repeat(indent + 1),
        wasm_emit_expr(else_value, funcs, file_prefix, reconstructions)?
    ));
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
        TIR::TExprKind::Unit => "()".to_string(),
        TIR::TExprKind::StrLit(parts) => {
            if parts.len() == 1 {
                if let TIR::TStrPart::Lit(text) = &parts[0] {
                    return Ok(format!("{}.to_string()", json_quote(text)));
                }
            }
            let mut value = String::from("{ let mut __jet_s = String::new(); ");
            for part in parts {
                match part {
                    TIR::TStrPart::Lit(text) if !text.is_empty() => {
                        value.push_str(&format!("__jet_s.push_str({}); ", json_quote(text)));
                    }
                    TIR::TStrPart::Lit(_) => {}
                    TIR::TStrPart::Interp(interp, format) => {
                        let spec = match format {
                            crate::AST::StrFormat::Display => "{}",
                            crate::AST::StrFormat::Debug => "{:?}",
                            crate::AST::StrFormat::Fixed(_) => {
                                unreachable!("Fixed interpolation lowers to core.fmt.decimal")
                            }
                            crate::AST::StrFormat::Unit(_) => {
                                unreachable!("Unit interpolation lowers to a String")
                            }
                        };
                        value.push_str(&format!(
                            "__jet_s.push_str(&format!({:?}, {})); ",
                            spec,
                            wasm_emit_expr(interp, funcs, file_prefix, reconstructions)?
                        ));
                    }
                }
            }
            value.push_str("__jet_s }");
            value
        }
        TIR::TExprKind::Local(local) if local.uninit_scalar => {
            format!("({}).read().clone()", local.rust_place())
        }
        TIR::TExprKind::Local(local) if local.uninit_fixed => {
            format!("({}).read_array()", local.rust_place())
        }
        TIR::TExprKind::Local(local) => local.rust_place(),
        TIR::TExprKind::Uninit => match &expr.ty {
            Type::FixedList { elem, len, .. } => format!(
                "jet_mem::JetUninitFixed::<{}, {len}>::new()",
                wasm_storage_ty(elem).ok_or(())?
            ),
            ty => format!(
                "jet_mem::JetUninit::<{}>::new()",
                wasm_storage_ty(ty).ok_or(())?
            ),
        },
        // D-EXPSEM1=A / D-FLOORDIV1=A: `^` and `/%` call the shared Prelude
        // helpers (Prelude/Core/Power.rs, Prelude/Core/Division.rs) — the same
        // source the native build runs — because Rust spells neither operator.
        TIR::TExprKind::Binary {
            op: op @ (crate::AST::BinOp::Pow
                | crate::AST::BinOp::FloorDiv
                | crate::AST::BinOp::Mod
                | crate::AST::BinOp::Rem),
            lhs,
            rhs,
            line,
            ..
        } => {
            let l = wasm_emit_expr(lhs, funcs, file_prefix, reconstructions)?;
            let r = wasm_emit_expr(rhs, funcs, file_prefix, reconstructions)?;
            wasm_prelude_call(*op, &l, &r, &expr.ty, file_prefix, *line)
                .expect("the match arm admits only Prelude-carried operators")
        }
        TIR::TExprKind::Binary { op, lhs, rhs, .. } => format!(
            "({} {} {})",
            wasm_emit_expr(lhs, funcs, file_prefix, reconstructions)?,
            binop(op).ok_or(())?,
            wasm_emit_expr(rhs, funcs, file_prefix, reconstructions)?
        ),
        TIR::TExprKind::Unary { op, operand } => format!("({}{})", unop(op), wasm_emit_expr(operand, funcs, file_prefix, reconstructions)?),
        TIR::TExprKind::Clone(inner) => format!(
            "({}).clone()",
            wasm_emit_expr(inner, funcs, file_prefix, reconstructions)?
        ),
        TIR::TExprKind::Borrow { place, mutable } => {
            if let TIR::TExprKind::Local(local) = &place.kind {
                if local.uninit_fixed {
                    return Ok(if *mutable {
                        format!("({}).as_array_mut()", local.rust_place())
                    } else {
                        format!("({}).as_array()", local.rust_place())
                    });
                }
            }
            let place = wasm_emit_expr(place, funcs, file_prefix, reconstructions)?;
            if *mutable {
                format!("&mut ({place})")
            } else {
                format!("&({place})")
            }
        }
        TIR::TExprKind::MaterializeView(inner) => format!(
            "({}).to_string()",
            wasm_emit_expr(inner, funcs, file_prefix, reconstructions)?
        ),
        TIR::TExprKind::Print(inner) => format!("println!(\"{{}}\", {})", wasm_emit_expr(inner, funcs, file_prefix, reconstructions)?),
        TIR::TExprKind::Field { recv, field, boxed: false } => {
            let recv_expr = wasm_emit_expr(recv, funcs, file_prefix, reconstructions)?;
            if matches!(&recv.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_ERR) {
                match field.as_str() {
                    "message" => format!("jet_err_message(&({recv_expr}))"),
                    "code" => format!("jet_err_code(&({recv_expr}))"),
                    "cause" => format!("jet_err_cause(&({recv_expr}))"),
                    _ => return Err(()),
                }
            } else {
                format!("({}).{}", recv_expr, mangle(field))
            }
        }
        TIR::TExprKind::StructLit { fields, .. } => {
            let Type::Named(name) = &expr.ty else {
                return Err(());
            };
            if name == jet_foundation::Syntax::TYPE_ERR {
                let value = |wanted: &str| {
                    fields
                        .iter()
                        .find(|(field, _, _)| field == wanted)
                        .map(|(_, value, _)| value)
                        .ok_or(())
                };
                let message = wasm_emit_expr(value("message")?, funcs, file_prefix, reconstructions)?;
                let code = wasm_emit_expr(value("code")?, funcs, file_prefix, reconstructions)?;
                let cause = wasm_emit_expr(value("cause")?, funcs, file_prefix, reconstructions)?;
                return Ok(format!("jet_err({message}, {code}, {cause})"));
            }
            format!(
                "{} {{ {} }}",
                user_type_rust(name),
                fields
                    .iter()
                    .map(|(field, value, boxed)| {
                        let mut value =
                            wasm_emit_expr(value, funcs, file_prefix, reconstructions)?;
                        if *boxed {
                            value = format!("Box::new({value})");
                        }
                        Ok(format!("{}: {value}", mangle(field)))
                    })
                    .collect::<Result<Vec<_>, ()>>()?
                    .join(", ")
            )
        }
        TIR::TExprKind::ListLit(elements) => {
            let elements = elements
                .iter()
                .map(|e| wasm_emit_expr(e, funcs, file_prefix, reconstructions))
                .collect::<Result<Vec<_>, ()>>()?
                .join(", ");
            if matches!(expr.ty, Type::FixedList { .. }) {
                format!("[{elements}]")
            } else {
                format!("vec![{elements}]")
            }
        }
        TIR::TExprKind::Present(inner) => format!(
            "Ok({})",
            wasm_emit_expr(inner, funcs, file_prefix, reconstructions)?
        ),
        TIR::TExprKind::Absent => "Err(JetAbsent)".to_string(),
        TIR::TExprKind::Ok(inner) => format!(
            "Ok({})",
            wasm_emit_expr(inner, funcs, file_prefix, reconstructions)?
        ),
        TIR::TExprKind::Err(inner) => format!(
            "Err({})",
            wasm_emit_expr(inner, funcs, file_prefix, reconstructions)?
        ),
        TIR::TExprKind::EnumLit {
            enum_type,
            variant,
            payload,
        } => {
            let head = wasm_enum_head(enum_type, variant);
            match payload {
                TIR::TEnumPayload::Unit => head,
                TIR::TEnumPayload::Positional(args) => format!(
                    "{head}({})",
                    args.iter()
                        .map(|arg| wasm_emit_enum_arg(
                            arg,
                            funcs,
                            file_prefix,
                            reconstructions
                        ))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ")
                ),
                TIR::TEnumPayload::Named(fields) => format!(
                    "{head} {{ {} }}",
                    fields
                        .iter()
                        .map(|(field, arg)| Ok(format!(
                            "{field}: {}",
                            wasm_emit_enum_arg(arg, funcs, file_prefix, reconstructions)?
                        )))
                        .collect::<Result<Vec<_>, ()>>()?
                        .join(", ")
                ),
            }
        }
        TIR::TExprKind::Index {
            base,
            index,
            is_map: true,
            ..
        } => format!(
            "({}).get(&({})).cloned().expect(\"index miss\")",
            wasm_emit_expr(base, funcs, file_prefix, reconstructions)?,
            wasm_emit_expr(index, funcs, file_prefix, reconstructions)?,
        ),
        TIR::TExprKind::Index {
            base,
            index,
            is_map: false,
            uninit_fixed,
            ..
        } => {
            let base = match &base.kind {
                TIR::TExprKind::Local(local) if *uninit_fixed && local.uninit_fixed => {
                    local.rust_place()
                }
                _ => wasm_emit_expr(base, funcs, file_prefix, reconstructions)?,
            };
            format!(
                "({base})[{} as usize].clone()",
                wasm_emit_expr(index, funcs, file_prefix, reconstructions)?,
            )
        }
        TIR::TExprKind::Call { name, args, .. } => {
            let key = local_web_key(file_prefix, name);
            let mut callees = funcs.iter().filter(|f| f.key == key && f.bucket == WebBucket::Wasm);
            callees.next().ok_or(())?;
            if callees.next().is_some() {
                return Err(());
            }
            let symbol = format!("jet_wasm_{key}");
            format!("{symbol}({})", args.iter().map(|a| wasm_emit_call_arg(a, funcs, file_prefix, reconstructions)).collect::<Result<Vec<_>, _>>()?.join(", "))
        }
        TIR::TExprKind::MethodCall {
            recv, method, args, ..
        } => format!(
            "({}).{}({})",
            wasm_emit_expr(recv, funcs, file_prefix, reconstructions)?,
            method.rust(),
            args.iter()
                .map(|arg| wasm_emit_call_arg(arg, funcs, file_prefix, reconstructions))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        ),
        TIR::TExprKind::ModuleCall { form, args, .. } => {
            let key = match form {
                TIR::TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    qualified_web_key(rust_mod, rust_fn)
                }
                TIR::TModuleCallForm::InlineMangled { mangled } => mangled.clone(),
            };
            let mut callees = funcs.iter().filter(|f| f.key == key && f.bucket == WebBucket::Wasm);
            callees.next().ok_or(())?;
            if callees.next().is_some() { return Err(()); }
            format!("jet_wasm_{key}({})", args.iter().map(|a| wasm_emit_call_arg(a, funcs, file_prefix, reconstructions)).collect::<Result<Vec<_>, _>>()?.join(", "))
        }
        TIR::TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            let mut rendered = String::new();
            emit_wasm_if_value(
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
                &mut rendered,
                0,
                funcs,
                file_prefix,
                reconstructions,
            )?;
            rendered
        }
        _ => return Err(()),
    })
}

fn web_emit_error(f: &FuncWeb) -> WebTirUnsupported {
    WebTirUnsupported { func_name: f.name.clone(), span: f.span }
}

fn emit_js_app(
    bundle: &ProgramBundle,
    funcs: &[FuncWeb],
    sources: &[JSSource],
    source_marker: &str,
) -> WebEmitResult<(String, String, Vec<(String, String)>)> {
    let mut out = String::from(
        "// Generated by jet — web JS entry (D-WEBBACKEND1).\n\
         import * as jetDom from \"./jet_dom_runtime.js\";\n\n",
    );
    out.push_str(JS_POWER_PRELUDE);
    let mut handlers = Vec::new();
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
            out.push_str("  const _perfStarted = jetDom.perfNow();\n  try {\n");
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
                Some(ty) if is_list_string(ty) => "list-string",
                Some(ty) if is_map_string_int(ty) => "map-string-int",
                _ => "scalar",
            };
            if ret_kind == "string"
                || ret_kind == "list-int"
                || ret_kind == "list-string"
                || ret_kind == "map-string-int"
            {
                out.push_str(&format!(
                    "  return jetDom.unmarshalAbi(raw, \"{ret_kind}\", wasm);\n"
                ));
            } else {
                out.push_str(&format!(
                    "  return jetDom.unmarshalAbi(raw, \"{ret_kind}\");\n"
                ));
            }
            out.push_str(&format!(
                "  }} finally {{ jetDom.perfRecord({}, \"wasm\", _perfStarted); }}\n",
                json_quote(&f.key)
            ));
            out.push_str("}\n\n");
        }
    }

    let js_funcs: Vec<&FuncWeb> = funcs
        .iter()
        .filter(|f| f.bucket == WebBucket::JS && f.key != "run" && f.key != "dev")
        .collect();
    for f in &js_funcs {
        emit_js_fn(f, &mut out, funcs, sources, &mut handlers)?;
    }

    if let Some(main_fn) = funcs.iter().find(|f| f.key == "run") {
        if main_fn.bucket == WebBucket::JS {
            out.push_str("export async function jet_main() {\n");
            out.push_str(&format!(
                "  jetDom.enterRenderScope({});\n",
                json_quote("run")
            ));
            out.push_str("  try {\n");
            let mut body = String::new();
            body.push_str(&format!(
                "    {source_marker} file {}\n",
                js_source_index(sources, &main_fn.source_path)
            ));
            emit_tir_js_body(
                &main_fn.tir.body,
                &mut body,
                funcs,
                main_fn.file_prefix.as_deref(),
                2,
            )
            .map_err(|()| web_emit_error(main_fn))?;
            out.push_str(&bind_inline_handler_symbols(&body, main_fn, &mut handlers));
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
    let (js_app, js_source_map) = finish_js_source_map(&out, sources, source_marker);
    Ok((js_app, js_source_map, handlers))
}

fn emit_js_fn(
    f: &FuncWeb,
    out: &mut String,
    all: &[FuncWeb],
    sources: &[JSSource],
    handlers: &mut Vec<(String, String)>,
) -> WebEmitResult<()> {
    // D-DOMGEN1=A (Phase 7 extension): every top-level #JS function is
    // exported, not just `main` — a hand-written host page (index.html) can
    // call any of them directly (e.g. a click handler invoking a Jet-compiled
    // render function), the same way #WasmExport functions are callable via
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
    let mut body = String::new();
    body.push_str(&format!(
        "    {} file {}\n",
        f.source_marker,
        js_source_index(sources, &f.source_path)
    ));
    emit_tir_js_body(&f.tir.body, &mut body, all, f.file_prefix.as_deref(), 2)
        .map_err(|()| web_emit_error(f))?;
    let async_kw = if body.contains("await bridge_") {
        "async "
    } else {
        ""
    };
    out.push_str(&format!(
        "export {async_kw}function {}({}) {{\n",
        f.key,
        param_names(&f.params)
    ));
    out.push_str(&format!(
        "  jetDom.enterRenderScope({});\n",
        json_quote(&f.key)
    ));
    out.push_str("  try {\n");
    out.push_str(&bind_inline_handler_symbols(&body, f, handlers));
    out.push_str("  } finally {\n    jetDom.exitRenderScope();\n  }\n");
    out.push_str("}\n\n");
    Ok(())
}

fn bind_inline_handler_symbols(body: &str, owner: &FuncWeb, handlers: &mut Vec<(String, String)>) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    let mut index = 0usize;
    while let Some(at) = rest.find(INLINE_HANDLER_PLACEHOLDER) {
        out.push_str(&rest[..at]);
        let symbol = format!("{}$handler{index}", owner.key);
        out.push_str(&json_quote(&symbol));
        handlers.push((owner.source_path.clone(), symbol));
        rest = &rest[at + INLINE_HANDLER_PLACEHOLDER.len()..];
        index += 1;
    }
    out.push_str(rest);
    out
}

fn finish_js_source_map(
    raw: &str,
    sources: &[JSSource],
    source_marker: &str,
) -> (String, String) {
    let mut js = String::with_capacity(raw.len());
    let mut mappings = Vec::new();
    let mut generated_line = 0usize;
    let mut source = None;
    let mut pending_line = None;
    let file_marker = format!("{source_marker} file ");
    let line_marker = format!("{source_marker} line ");

    for line in raw.split_inclusive('\n') {
        let marker = line.trim();
        if let Some(index) = marker.strip_prefix(&file_marker) {
            let index = index
                .parse::<usize>()
                .expect("web source-file marker must contain an index");
            assert!(index < sources.len(), "web source-file marker is in range");
            source = Some(index);
            continue;
        }
        if let Some(original_line) = marker.strip_prefix(&line_marker) {
            pending_line = Some(
                original_line
                    .parse::<usize>()
                    .expect("web source-line marker must contain a line"),
            );
            continue;
        }

        if let (Some(source), Some(original_line)) = (source, pending_line) {
            if let Some(generated_column) = line
                .char_indices()
                .find_map(|(byte, c)| (!matches!(c, ' ' | '\t' | '\r' | '\n')).then_some(byte))
            {
                mappings.push(JSMapping {
                    generated_line,
                    generated_column: line[..generated_column].encode_utf16().count(),
                    source,
                    original_line: original_line.saturating_sub(1),
                });
                pending_line = None;
            }
        }

        js.push_str(line);
        generated_line += line.bytes().filter(|byte| *byte == b'\n').count();
    }

    let mappings = encode_source_mappings(&mappings);
    let source_names = sources
        .iter()
        .map(|source| json_quote(&source.name))
        .collect::<Vec<_>>()
        .join(",");
    let source_contents = sources
        .iter()
        .map(|source| json_quote(&source.content))
        .collect::<Vec<_>>()
        .join(",");
    let map = format!(
        "{{\"version\":3,\"file\":\"app.js\",\"sources\":[{source_names}],\"sourcesContent\":[{source_contents}],\"names\":[],\"mappings\":{}}}\n",
        json_quote(&mappings)
    );
    (js, map)
}

fn encode_source_mappings(mappings: &[JSMapping]) -> String {
    let mut out = String::new();
    let mut generated_line = 0usize;
    let mut generated_column = 0i64;
    let mut source = 0i64;
    let mut original_line = 0i64;
    let mut original_column = 0i64;
    let mut first_segment = true;

    for mapping in mappings {
        while generated_line < mapping.generated_line {
            out.push(';');
            generated_line += 1;
            generated_column = 0;
            first_segment = true;
        }
        if !first_segment {
            out.push(',');
        }
        let next_generated_column = mapping.generated_column as i64;
        let next_source = mapping.source as i64;
        let next_original_line = mapping.original_line as i64;
        encode_base64_vlq(next_generated_column - generated_column, &mut out);
        encode_base64_vlq(next_source - source, &mut out);
        encode_base64_vlq(next_original_line - original_line, &mut out);
        encode_base64_vlq(-original_column, &mut out);
        generated_column = next_generated_column;
        source = next_source;
        original_line = next_original_line;
        original_column = 0;
        first_segment = false;
    }
    out
}

fn encode_base64_vlq(value: i64, out: &mut String) {
    const BASE64: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut value = if value < 0 {
        ((-value) << 1) | 1
    } else {
        value << 1
    };
    loop {
        let mut digit = (value & 31) as usize;
        value >>= 5;
        if value != 0 {
            digit |= 32;
        }
        out.push(BASE64[digit] as char);
        if value == 0 {
            break;
        }
    }
}

fn param_names(params: &[(String, Type)]) -> String {
    params
        .iter()
        .map(|(n, _)| n.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn web_name(name: &str) -> &str {
    name.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX).unwrap_or(name)
}

fn qualified_web_key(rust_mod: &str, rust_fn: &str) -> String {
    partition_key(None, Some(web_name(rust_mod)), web_name(rust_fn))
}

fn local_web_key(file_prefix: Option<&str>, rust_fn: &str) -> String {
    partition_key(file_prefix, None, web_name(rust_fn))
}

fn web_place(name: &str) -> String {
    let name = name.strip_prefix("(*").and_then(|s| s.strip_suffix(')')).unwrap_or(name);
    if let Some(source) = name.strip_prefix("__jet_cap_") {
        return source.to_string();
    }
    web_name(name).to_string()
}

fn web_local(local: &TIR::TLocal) -> String {
    web_place(&local.rust_place())
}

fn web_tir_place(place: &TIR::TPlace) -> Result<String, ()> {
    match place {
        TIR::TPlace::Local(local) => Ok(web_local(local)),
        TIR::TPlace::Expr(_) => Err(()),
    }
}

fn wasm_tir_place(place: &TIR::TPlace) -> Result<String, ()> {
    match place {
        TIR::TPlace::Local(local) => Ok(local.rust_place()),
        TIR::TPlace::Expr(_) => Err(()),
    }
}

fn js_label_prefix(label: &Option<String>) -> String {
    match label {
        Some(name) => format!("{}: ", mangle(name)),
        None => String::new(),
    }
}

fn js_break_stmt(label: &Option<String>) -> String {
    match label {
        Some(name) => format!("break {};", mangle(name)),
        None => "break;".to_string(),
    }
}

fn js_continue_stmt(label: &Option<String>) -> String {
    match label {
        Some(name) => format!("continue {};", mangle(name)),
        None => "continue;".to_string(),
    }
}

fn wasm_label_prefix(label: &Option<String>) -> String {
    match label {
        Some(name) => format!("'{}: ", mangle(name)),
        None => String::new(),
    }
}

fn js_match_tag(pattern: &crate::AST::Pattern) -> Result<&'static str, ()> {
    match pattern {
        crate::AST::Pattern::Ok { .. } => Ok("Ok"),
        crate::AST::Pattern::Err { .. } => Ok("Err"),
        crate::AST::Pattern::Present { .. } => Ok("Some"),
        crate::AST::Pattern::Absent(_) => Ok("None"),
        _ => Err(()),
    }
}

fn js_pattern_test(pattern: &crate::AST::Pattern, subject: &str) -> Result<String, ()> {
    let mut tests = match pattern {
        crate::AST::Pattern::Variant {
            variant, bindings, ..
        } => {
            let mut tests = vec![format!(
                "({subject}).tag === {}",
                json_quote(variant)
            )];
            for (index, slot) in bindings.iter().enumerate() {
                if let crate::AST::PatSlot::Range { lo, hi } = slot {
                    tests.push(format!(
                        "({subject}).values[{index}] >= {lo} && ({subject}).values[{index}] <= {hi}"
                    ));
                }
            }
            tests
        }
        crate::AST::Pattern::Absent(_) => {
            vec![format!("({subject}).tag === \"None\"")]
        }
        other => vec![format!(
            "({subject}).tag === \"{}\"",
            js_match_tag(other)?
        )],
    };
    if tests.len() == 1 {
        Ok(tests.pop().expect("one pattern test"))
    } else {
        Ok(tests.join(" && "))
    }
}

fn emit_js_pattern_bindings(
    pattern: &crate::AST::Pattern,
    subject: &str,
    out: &mut String,
    indent: usize,
) {
    let pad = "  ".repeat(indent);
    match pattern {
        crate::AST::Pattern::Variant { bindings, .. } => {
            for (index, slot) in bindings.iter().enumerate() {
                if let crate::AST::PatSlot::Bind { name, .. } = slot {
                    if name != "_" {
                        out.push_str(&format!(
                            "{pad}const {} = ({subject}).values[{index}];\n",
                            web_name(name)
                        ));
                    }
                }
            }
        }
        crate::AST::Pattern::Ok { binding, .. }
        | crate::AST::Pattern::Err { binding, .. }
        | crate::AST::Pattern::Present { binding, .. }
            if binding != "_" =>
        {
            out.push_str(&format!(
                "{pad}const {} = ({subject}).values[0];\n",
                web_name(binding)
            ));
        }
        _ => {}
    }
}

fn emit_js_if_head(
    cond: &TIR::TIfCond,
    out: &mut String,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
    indent: usize,
) -> Result<usize, ()> {
    let pad = "  ".repeat(indent);
    match cond {
        TIR::TIfCond::Plain(expr) => {
            out.push_str(&format!(
                "{pad}if ({}) {{\n",
                tir_js_expr(expr, funcs, file_prefix)?
            ));
            Ok(0)
        }
        TIR::TIfCond::IsNone { subj } => {
            out.push_str(&format!(
                "{pad}if (({}).tag === \"None\") {{\n",
                tir_js_expr(subj, funcs, file_prefix)?
            ));
            Ok(0)
        }
        TIR::TIfCond::IfLet { pattern, subj } => {
            let subj = tir_js_expr(subj, funcs, file_prefix)?;
            let inner = "  ".repeat(indent + 1);
            out.push_str(&format!(
                "{pad}{{\n{inner}const __jet_if_subject = {subj};\n{inner}if ({}) {{\n",
                js_pattern_test(&pattern.pattern, "__jet_if_subject")?
            ));
            emit_js_pattern_bindings(
                &pattern.pattern,
                "__jet_if_subject",
                out,
                indent + 2,
            );
            Ok(1)
        }
        TIR::TIfCond::Matches { pattern, subj } => {
            let subj = tir_js_expr(subj, funcs, file_prefix)?;
            let inner = "  ".repeat(indent + 1);
            out.push_str(&format!(
                "{pad}{{\n{inner}const __jet_match_subject = {subj};\n{inner}if ({}) {{\n",
                js_pattern_test(&pattern.pattern, "__jet_match_subject")?
            ));
            Ok(1)
        }
        TIR::TIfCond::And { .. } => return Err(()),
    }
}

fn emit_js_if(
    cond: &TIR::TIfCond,
    then_body: &[TIR::TStmt],
    else_body: Option<&[TIR::TStmt]>,
    out: &mut String,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
    indent: usize,
) -> Result<(), ()> {
    let pad = "  ".repeat(indent);
    if let TIR::TIfCond::And { left, right } = cond {
        let extra = emit_js_if_head(left, out, funcs, file_prefix, indent)?;
        let head_indent = indent + extra;
        let head_pad = "  ".repeat(head_indent);
        emit_js_if(
            right,
            then_body,
            else_body,
            out,
            funcs,
            file_prefix,
            head_indent + 1,
        )?;
        if let Some(else_body) = else_body {
            out.push_str(&format!("{head_pad}}} else {{\n"));
            emit_tir_js_body(
                else_body,
                out,
                funcs,
                file_prefix,
                head_indent + 1,
            )?;
        }
        out.push_str(&format!("{head_pad}}}\n"));
        if extra != 0 {
            out.push_str(&format!("{pad}}}\n"));
        }
        return Ok(());
    }
    let extra = emit_js_if_head(cond, out, funcs, file_prefix, indent)?;
    let head_indent = indent + extra;
    let head_pad = "  ".repeat(head_indent);
    emit_tir_js_body(
        then_body,
        out,
        funcs,
        file_prefix,
        head_indent + 1,
    )?;
    if let Some(else_body) = else_body {
        out.push_str(&format!("{head_pad}}} else {{\n"));
        emit_tir_js_body(
            else_body,
            out,
            funcs,
            file_prefix,
            head_indent + 1,
        )?;
    }
    out.push_str(&format!("{head_pad}}}\n"));
    if extra != 0 {
        out.push_str(&format!("{pad}}}\n"));
    }
    Ok(())
}

fn emit_js_if_value(
    cond: &TIR::TIfCond,
    then_body: &[TIR::TStmt],
    then_value: &TIR::TExpr,
    else_body: &[TIR::TStmt],
    else_value: &TIR::TExpr,
    out: &mut String,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
    indent: usize,
) -> Result<(), ()> {
    let pad = "  ".repeat(indent);
    if let TIR::TIfCond::And { left, right } = cond {
        let extra = emit_js_if_head(left, out, funcs, file_prefix, indent)?;
        let head_indent = indent + extra;
        let head_pad = "  ".repeat(head_indent);
        emit_js_if_value(
            right,
            then_body,
            then_value,
            else_body,
            else_value,
            out,
            funcs,
            file_prefix,
            head_indent + 1,
        )?;
        out.push_str(&format!("{head_pad}}} else {{\n"));
        emit_tir_js_body(
            else_body,
            out,
            funcs,
            file_prefix,
            head_indent + 1,
        )?;
        out.push_str(&format!(
            "{}return {};\n{head_pad}}}\n",
            "  ".repeat(head_indent + 1),
            tir_js_expr(else_value, funcs, file_prefix)?
        ));
        if extra != 0 {
            out.push_str(&format!("{pad}}}\n"));
        }
        return Ok(());
    }

    let extra = emit_js_if_head(cond, out, funcs, file_prefix, indent)?;
    let head_indent = indent + extra;
    let head_pad = "  ".repeat(head_indent);
    emit_tir_js_body(
        then_body,
        out,
        funcs,
        file_prefix,
        head_indent + 1,
    )?;
    out.push_str(&format!(
        "{}return {};\n{head_pad}}} else {{\n",
        "  ".repeat(head_indent + 1),
        tir_js_expr(then_value, funcs, file_prefix)?
    ));
    emit_tir_js_body(
        else_body,
        out,
        funcs,
        file_prefix,
        head_indent + 1,
    )?;
    out.push_str(&format!(
        "{}return {};\n{head_pad}}}\n",
        "  ".repeat(head_indent + 1),
        tir_js_expr(else_value, funcs, file_prefix)?
    ));
    if extra != 0 {
        out.push_str(&format!("{pad}}}\n"));
    }
    Ok(())
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
            TIR::TStmt::SourceSpan(_) => {}
            TIR::TStmt::LineMarker(line) => {
                let source_marker = &funcs
                    .first()
                    .expect("a web body has at least one collected function")
                    .source_marker;
                out.push_str(&format!("{pad}{source_marker} line {line}\n"));
            }
            TIR::TStmt::Let { name, init, .. } => out.push_str(&format!("{pad}let {} = {};\n", web_name(name), tir_js_expr(init, funcs, file_prefix)?)),
            TIR::TStmt::Assign { place, op, value, .. } => {
                let target = web_tir_place(place)?;
                let v = tir_js_expr(value, funcs, file_prefix)?;
                // D-EXPSEM1=A / D-FLOORDIV1=A: JavaScript's `**=` is the
                // floating-point power and it has no rounding division at all,
                // so those compounds read the place and call the JS preamble.
                if let Some(call) = op.and_then(|op| js_prelude_call(op, &target, &v, &value.ty)) {
                    out.push_str(&format!("{pad}{target} = {call};\n"));
                } else if matches!(value.ty, Type::Float | Type::Float32) {
                    // Float compound assigns leave BigInt land (D-INTDIV1).
                    match op {
                        Some(o) => {
                            let sym = binop(o).ok_or(())?;
                            out.push_str(&format!(
                                "{pad}{target} = Number({target}) {sym} Number({v});\n"
                            ));
                        }
                        None => out.push_str(&format!("{pad}{target} = {v};\n")),
                    }
                } else {
                    let assign = match op {
                        Some(o) => format!("{}=", binop(o).ok_or(())?),
                        None => "=".to_string(),
                    };
                    out.push_str(&format!("{pad}{target} {assign} {v};\n"));
                }
            }
            TIR::TStmt::Return(Some(expr)) => out.push_str(&format!("{pad}return {};\n", tir_js_expr(expr, funcs, file_prefix)?)),
            TIR::TStmt::Return(None) => out.push_str(&format!("{pad}return;\n")),
            TIR::TStmt::ExprStmt(expr) => out.push_str(&format!("{pad}{};\n", tir_js_expr(expr, funcs, file_prefix)?)),
            TIR::TStmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                emit_js_if(
                    cond,
                    then_body,
                    else_body.as_deref(),
                    out,
                    funcs,
                    file_prefix,
                    indent,
                )?;
            }
            TIR::TStmt::Range { var, start, end, step, exclusive, body, .. } => {
                let step = match step {
                    Some(e) => tir_js_expr(e, funcs, file_prefix)?,
                    None => "1n".to_string(),
                };
                let cmp = if *exclusive { "<" } else { "<=" };
                out.push_str(&format!("{pad}for (let {} = {}; {} {cmp} {}; {} += {step}) {{\n", web_name(var), tir_js_expr(start, funcs, file_prefix)?, web_name(var), tir_js_expr(end, funcs, file_prefix)?, web_name(var)));
                emit_tir_js_body(body, out, funcs, file_prefix, indent + 1)?;
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::ForIn { var, var2, collection, body, .. } => {
                match var2 {
                    Some(v2) => {
                        // D-RANGE-EXCL1=C: sequence two-binding → index then item.
                        // `.entries()` yields a Number index; whole-number locals
                        // are BigInt on this tier, so lift the index once.
                        out.push_str(&format!(
                            "{pad}for (const [__jet_i, {}] of {}.entries()) {{\n{pad}  let {} = BigInt(__jet_i);\n",
                            web_name(v2),
                            tir_js_expr(collection, funcs, file_prefix)?,
                            web_name(var),
                        ));
                    }
                    None => {
                        out.push_str(&format!(
                            "{pad}for (const {} of {}) {{\n",
                            web_name(var),
                            tir_js_expr(collection, funcs, file_prefix)?
                        ));
                    }
                }
                emit_tir_js_body(body, out, funcs, file_prefix, indent + 1)?;
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::Loop { label, body } => {
                out.push_str(&format!(
                    "{pad}{}while (true) {{\n",
                    js_label_prefix(label)
                ));
                emit_tir_js_body(body, out, funcs, file_prefix, indent + 1)?;
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::While { label, cond, body } => {
                out.push_str(&format!(
                    "{pad}{}while ({}) {{\n",
                    js_label_prefix(label),
                    tir_js_expr(cond, funcs, file_prefix)?
                ));
                emit_tir_js_body(body, out, funcs, file_prefix, indent + 1)?;
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::CountedLoop {
                label,
                init,
                cond,
                step,
                body,
            } => {
                // Mirror native: outer scope, optional first-pass skip of step, then
                // retest + body. Continue re-enters at the top (D-LOOP-CONTINUE2).
                out.push_str(&format!("{pad}{{\n"));
                emit_tir_js_body(
                    std::slice::from_ref(init.as_ref()),
                    out,
                    funcs,
                    file_prefix,
                    indent + 1,
                )?;
                let inner = "  ".repeat(indent + 1);
                if step.is_some() {
                    out.push_str(&format!("{inner}let __jet_loop_first = true;\n"));
                }
                out.push_str(&format!(
                    "{inner}{}while (true) {{\n",
                    js_label_prefix(label)
                ));
                let body_pad = "  ".repeat(indent + 2);
                if let Some(step) = step {
                    out.push_str(&format!(
                        "{body_pad}if (__jet_loop_first) {{ __jet_loop_first = false; }} else {{\n"
                    ));
                    emit_tir_js_body(
                        std::slice::from_ref(step.as_ref()),
                        out,
                        funcs,
                        file_prefix,
                        indent + 3,
                    )?;
                    out.push_str(&format!("{body_pad}}}\n"));
                }
                out.push_str(&format!(
                    "{body_pad}if (!({})) {{ break; }}\n",
                    tir_js_expr(cond, funcs, file_prefix)?
                ));
                emit_tir_js_body(body, out, funcs, file_prefix, indent + 2)?;
                out.push_str(&format!("{inner}}}\n"));
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::Break(label) => {
                out.push_str(&format!("{pad}{}\n", js_break_stmt(label)));
            }
            TIR::TStmt::Continue(label) => {
                out.push_str(&format!("{pad}{}\n", js_continue_stmt(label)));
            }
            TIR::TStmt::IndexAssign {
                base,
                index,
                is_map,
                value,
                ..
            } => {
                let b = tir_js_expr(base, funcs, file_prefix)?;
                let i = tir_js_expr(index, funcs, file_prefix)?;
                let v = if matches!(
                    &base.ty,
                    Type::Map { value, .. }
                        if matches!(**value, Type::Int | Type::IntN { .. })
                ) {
                    tir_js_abi_int_expr(value, funcs, file_prefix)?
                } else {
                    tir_js_expr(value, funcs, file_prefix)?
                };
                if *is_map {
                    out.push_str(&format!("{pad}{b}.set({i}, {v});\n"));
                } else {
                    out.push_str(&format!("{pad}{b}[Number({i})] = {v};\n"));
                }
            }
            TIR::TStmt::EnumMatch {
                scrutinee,
                arms,
                else_body,
                fallthrough,
                ..
            } => {
                out.push_str(&format!(
                    "{pad}{{\n{pad}  const __jet_match = {};\n",
                    tir_js_expr(scrutinee, funcs, file_prefix)?
                ));
                let inner = "  ".repeat(indent + 1);
                for (index, arm) in arms.iter().enumerate() {
                    let keyword = if index == 0 { "if" } else { "} else if" };
                    out.push_str(&format!(
                        "{inner}{keyword} ({}) {{\n",
                        js_pattern_test(&arm.pattern.pattern, "__jet_match")?
                    ));
                    emit_js_pattern_bindings(
                        &arm.pattern.pattern,
                        "__jet_match",
                        out,
                        indent + 2,
                    );
                    emit_tir_js_body(&arm.body, out, funcs, file_prefix, indent + 2)?;
                }
                match (arms.is_empty(), else_body, *fallthrough) {
                    (true, Some(body), _) => {
                        emit_tir_js_body(body, out, funcs, file_prefix, indent + 1)?;
                    }
                    (true, None, true) => out.push_str(&format!(
                        "{inner}throw new Error(\"jet: exhaustiveness bug\");\n"
                    )),
                    (true, None, false) => {}
                    (false, Some(body), _) => {
                        out.push_str(&format!("{inner}}} else {{\n"));
                        emit_tir_js_body(body, out, funcs, file_prefix, indent + 2)?;
                        out.push_str(&format!("{inner}}}\n"));
                    }
                    (false, None, true) => out.push_str(&format!(
                        "{inner}}} else {{\n{inner}  throw new Error(\"jet: exhaustiveness bug\");\n{inner}}}\n"
                    )),
                    (false, None, false) => out.push_str(&format!("{inner}}}\n")),
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            // Value / mixed arm tables — if/else-if chain (native MixedSwitch parity).
            TIR::TStmt::MixedSwitch {
                subject,
                arms,
                else_body,
                ..
            } => {
                out.push_str(&format!("{pad}{{\n"));
                let inner = "  ".repeat(indent + 1);
                // Bind subject for parity with native (HostCall pattern arms stay gated out).
                out.push_str(&format!(
                    "{inner}const __jet_switch_subject = {};\n",
                    tir_js_expr(subject, funcs, file_prefix)?
                ));
                for (i, (cond, body)) in arms.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "} else if" };
                    out.push_str(&format!(
                        "{inner}{kw} ({}) {{\n",
                        tir_js_expr(cond, funcs, file_prefix)?
                    ));
                    emit_tir_js_body(body, out, funcs, file_prefix, indent + 2)?;
                }
                match else_body {
                    None if !arms.is_empty() => {
                        out.push_str(&format!("{inner}}}\n"));
                    }
                    None => {}
                    Some(body) if arms.is_empty() => {
                        emit_tir_js_body(body, out, funcs, file_prefix, indent + 1)?;
                    }
                    Some(body) => {
                        out.push_str(&format!("{inner}}} else {{\n"));
                        emit_tir_js_body(body, out, funcs, file_prefix, indent + 2)?;
                        out.push_str(&format!("{inner}}}\n"));
                    }
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::RangeSwitch {
                subject,
                arms,
                else_body,
            } => {
                out.push_str(&format!("{pad}{{\n"));
                let inner = "  ".repeat(indent + 1);
                out.push_str(&format!(
                    "{inner}const __jet_switch_subject = {};\n",
                    tir_js_expr(subject, funcs, file_prefix)?
                ));
                for (i, (lo, hi, body)) in arms.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "} else if" };
                    out.push_str(&format!(
                        "{inner}{kw} (__jet_switch_subject >= {lo} && __jet_switch_subject <= {hi}) {{\n"
                    ));
                    emit_tir_js_body(body, out, funcs, file_prefix, indent + 2)?;
                }
                out.push_str(&format!("{inner}}} else {{\n"));
                emit_tir_js_body(else_body, out, funcs, file_prefix, indent + 2)?;
                out.push_str(&format!("{inner}}}\n"));
                out.push_str(&format!("{pad}}}\n"));
            }
            TIR::TStmt::Inline(inner)
            | TIR::TStmt::Region(inner)
            | TIR::TStmt::Impure(inner) => {
                emit_tir_js_body(inner, out, funcs, file_prefix, indent)?
            }
            _ => return Err(()),
        }
    }
    Ok(())
}

fn tir_call_args(args: &[TIR::TCallArg], funcs: &[FuncWeb], file_prefix: Option<&str>) -> Result<String, ()> {
    Ok(args
        .iter()
        .map(|arg| {
            let value = tir_js_expr(&arg.value, funcs, file_prefix)?;
            if matches!(arg.widen_to_union, Some(Type::Union(_))) {
                Ok(format!(
                    "{{ tag: {}, values: [{value}] }}",
                    json_quote(&crate::AST::union_member_tag(&arg.value.ty))
                ))
            } else if members_are_unused(arg) {
                Ok(value)
            } else {
                Err(())
            }
        })
        .collect::<Result<Vec<_>, ()>>()?
        .join(", "))
}

fn members_are_unused(arg: &TIR::TCallArg) -> bool {
    arg.fn_coerce.is_none() && !arg.widen_to_vec
}

fn tir_plain_args(args: &[TIR::TExpr], funcs: &[FuncWeb], file_prefix: Option<&str>) -> Result<Vec<String>, ()> {
    args.iter().map(|a| tir_js_expr(a, funcs, file_prefix)).collect()
}

fn tir_js_abi_int_expr(
    expr: &TIR::TExpr,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
) -> Result<String, ()> {
    use TIR::TExprKind as E;
    match &expr.kind {
        E::IntLit(n, _) => Ok(format!("{n}n")),
        // `!` leaves BigInt land through the wrapper below, because the helper
        // it needs answers an ordinary number.
        E::Unary { op, operand } if !matches!(op, crate::AST::UnOp::Not) => Ok(format!(
            "(-{})",
            tir_js_abi_int_expr(operand, funcs, file_prefix)?
        )),
        // D-EXPSEM1=A / D-FLOORDIV1=A: `^` and `/%` have no BigInt operator here
        // — they fall through to the wrapper below, which wraps the ordinary
        // JS-tier result.
        E::Binary { op, lhs, rhs, .. }
            if !matches!(
                op,
                crate::AST::BinOp::Pow
                    | crate::AST::BinOp::FloorDiv
                    | crate::AST::BinOp::Mod
                    | crate::AST::BinOp::Rem
            ) =>
        {
            Ok(format!(
                "({} {} {})",
                tir_js_abi_int_expr(lhs, funcs, file_prefix)?,
                binop(op).ok_or(())?,
                tir_js_abi_int_expr(rhs, funcs, file_prefix)?
            ))
        }
        E::Clone(inner) | E::MaterializeView(inner) | E::DistinctRaw(inner) => {
            tir_js_abi_int_expr(inner, funcs, file_prefix)
        }
        _ => Ok(format!(
            "BigInt({})",
            tir_js_expr(expr, funcs, file_prefix)?
        )),
    }
}

fn tir_js_err_field(
    expr: &TIR::TExpr,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
) -> Result<String, ()> {
    use TIR::TExprKind as E;
    match &expr.kind {
        // JS Err values are the web boundary shape, not the internal tagged
        // Option carrier. Marshal present values directly and use null for an
        // absent optional code/cause.
        E::Present(inner) => tir_js_expr(inner, funcs, file_prefix),
        E::Absent => Ok("null".to_string()),
        _ => tir_js_expr(expr, funcs, file_prefix),
    }
}

fn tir_js_expr(expr: &TIR::TExpr, funcs: &[FuncWeb], file_prefix: Option<&str>) -> Result<String, ()> {
    use TIR::TExprKind as E;
    Ok(match &expr.kind {
        // Whole numbers are JS BigInt on this tier (I9 / #1485): a plain
        // numeric literal is only exact to 2^53.
        E::IntLit(n, _) => format!("{n}n"),
        E::FloatLit(n) => n.to_string(),
        E::BoolLit(b) => b.to_string(),
        E::CharLit(c) => json_quote(&c.to_string()),
        E::StrLit(parts) => tir_js_string(parts, funcs, file_prefix)?,
        E::Local(local) => web_local(local),
        E::Unit | E::DefaultLit => "void 0".to_string(),
        E::CtLit(value) => value.serialize(),
        E::Uninit => match &expr.ty {
            Type::FixedList { len, .. } => format!("Array({len})"),
            _ => "void 0".to_string(),
        },
        E::HostCall(_) => return Err(()),
        // D-EXPSEM1=A / D-FLOORDIV1=A: `^` and `/%` call the JS preamble, which
        // carries the same rules the Prelude helpers do.
        E::Binary {
            op: op @ (crate::AST::BinOp::Pow
                | crate::AST::BinOp::FloorDiv
                | crate::AST::BinOp::Mod
                | crate::AST::BinOp::Rem),
            lhs,
            rhs,
            ..
        } => js_prelude_call(
            *op,
            &tir_js_expr(lhs, funcs, file_prefix)?,
            &tir_js_expr(rhs, funcs, file_prefix)?,
            &expr.ty,
        )
        .expect("the match arm admits only Prelude-carried operators"),
        // Float results (D-INTDIV1 `Int / Int` → Float, float arithmetic) leave
        // BigInt land through `Number(...)` so JS does floating-point math.
        E::Binary { op, lhs, rhs, .. }
            if matches!(expr.ty, Type::Float | Type::Float32) =>
        {
            format!(
                "(Number({}) {} Number({}))",
                tir_js_expr(lhs, funcs, file_prefix)?,
                binop(op).ok_or(())?,
                tir_js_expr(rhs, funcs, file_prefix)?
            )
        }
        E::Binary { op, lhs, rhs, .. } => format!("({} {} {})", tir_js_expr(lhs, funcs, file_prefix)?, binop(op).ok_or(())?, tir_js_expr(rhs, funcs, file_prefix)?),
        E::Unary { op, operand } => js_unary_call(
            op,
            &operand.ty,
            &tir_js_expr(operand, funcs, file_prefix)?,
        ),
        E::Clone(inner) | E::MaterializeView(inner) | E::DistinctRaw(inner) => tir_js_expr(inner, funcs, file_prefix)?,
        E::Borrow { place, .. } => tir_js_expr(place, funcs, file_prefix)?,
        E::DistinctCtor { arg, .. } => tir_js_expr(arg, funcs, file_prefix)?,
        E::Field { recv, field, .. } => format!("{}.{}", tir_js_expr(recv, funcs, file_prefix)?, web_name(field)),
        E::StructLit { fields, .. }
            if matches!(&expr.ty, Type::Named(name) if name == Syntax::TYPE_ERR) =>
        {
            let value = |wanted: &str| {
                fields
                    .iter()
                    .find(|(field, _, _)| field == wanted)
                    .map(|(_, value, _)| value)
                    .ok_or(())
            };
            let message = tir_js_err_field(value("message")?, funcs, file_prefix)?;
            let code = tir_js_err_field(value("code")?, funcs, file_prefix)?;
            let cause = tir_js_err_field(value("cause")?, funcs, file_prefix)?;
            format!("({{ message: {message}, code: {code}, cause: {cause} }})")
        }
        E::StructLit { fields, .. } => format!("({{ {} }})", fields.iter().map(|(n, v, _)| Ok(format!("{}: {}", web_name(n), tir_js_expr(v, funcs, file_prefix)?))).collect::<Result<Vec<_>, ()>>()?.join(", ")),
        E::EnumLit {
            variant, payload, ..
        } => {
            let values = match payload {
                TIR::TEnumPayload::Unit => Vec::new(),
                TIR::TEnumPayload::Positional(args) => args
                    .iter()
                    .map(|arg| tir_js_expr(&arg.value, funcs, file_prefix))
                    .collect::<Result<Vec<_>, _>>()?,
                TIR::TEnumPayload::Named(fields) => fields
                    .iter()
                    .map(|(_, arg)| tir_js_expr(&arg.value, funcs, file_prefix))
                    .collect::<Result<Vec<_>, _>>()?,
            };
            format!(
                "{{ tag: {}, values: [{}] }}",
                json_quote(variant),
                values.join(", ")
            )
        }
        E::ListLit(elements) => format!("[{}]", elements.iter().map(|element| tir_js_expr(element, funcs, file_prefix)).collect::<Result<Vec<_>, _>>()?.join(", ")),
        E::Present(inner) => format!(
            "{{ tag: \"Some\", values: [{}] }}",
            tir_js_expr(inner, funcs, file_prefix)?
        ),
        E::Absent => "{ tag: \"None\", values: [] }".to_string(),
        E::Ok(inner) => format!(
            "{{ tag: \"Ok\", values: [{}] }}",
            tir_js_expr(inner, funcs, file_prefix)?
        ),
        E::Err(inner) => format!(
            "{{ tag: \"Err\", values: [{}] }}",
            tir_js_expr(inner, funcs, file_prefix)?
        ),
        E::MapLit(entries) => {
            let abi_int_values = matches!(
                &expr.ty,
                Type::Map { value, .. }
                    if matches!(**value, Type::Int | Type::IntN { .. })
            );
            format!(
                "new Map([{}])",
                entries
                    .iter()
                    .map(|(key, value)| Ok(format!(
                        "[{}, {}]",
                        tir_js_expr(key, funcs, file_prefix)?,
                        if abi_int_values {
                            tir_js_abi_int_expr(value, funcs, file_prefix)?
                        } else {
                            tir_js_expr(value, funcs, file_prefix)?
                        },
                    )))
                    .collect::<Result<Vec<_>, ()>>()?
                    .join(", ")
            )
        }
        E::Index {
            base,
            index,
            is_map,
            ..
        } => {
            let b = tir_js_expr(base, funcs, file_prefix)?;
            let i = tir_js_expr(index, funcs, file_prefix)?;
            if *is_map {
                format!("{b}.get({i})")
            } else {
                // JS array indices are ordinary numbers; BigInt keys throw.
                format!("{b}[Number({i})]")
            }
        },
        E::Call { name, args, .. } => {
            let name = local_web_key(file_prefix, name);
            let args = tir_call_args(args, funcs, file_prefix)?;
            if name == "print" { format!("jetDom.print({args})") }
            else if is_wasm_export(&name, funcs) { format!("await bridge_{name}({args})") }
            else { format!("{name}({args})") }
        }
        E::ModuleCall { form, args, .. } => {
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
        E::MethodCall { recv, method, args, .. } => format!("{}.{}({})", tir_js_expr(recv, funcs, file_prefix)?, web_name(&method.rust()), tir_call_args(args, funcs, file_prefix)?),
        E::CoreCall { module, method, args, .. } => tir_core_call(module, method, args, funcs, file_prefix)?,
        E::HandleMethod { recv, op, args } => {
            if !web_js_handle_method_supported(op, args.len()) {
                return Err(());
            }
            let recv = tir_js_expr(recv, funcs, file_prefix)?;
            let a = tir_plain_args(args, funcs, file_prefix)?;
            match op {
                TIR::THandleOp::UiBackendMethod { method } => match (method.as_str(), a.len()) {
                    ("measure", 2) => format!("jetDom.measure({}, {})", a[0], a[1]),
                    ("layout", 2) => format!("jetDom.layout({recv}, {}, {})", a[0], a[1]),
                    ("paint", 1) => format!("jetDom.paint({recv}, {})", a[0]),
                    ("mount", 1) => format!("jetDom.mount({recv}, {})", a[0]),
                    ("mount", 2) => format!("jetDom.mount({recv}, {}, {})", a[0], a[1]),
                    ("commands", 0) => format!("jetDom.commands({recv})"),
                    ("on_event", 1) => format!("jetDom.onEvent({recv}, {})", a[0]),
                    ("set_focus_group", 1) => {
                        format!("jetDom.setFocusGroup({recv}, {})", a[0])
                    }
                    ("focused_label", 0) => format!("jetDom.focusedLabel({recv})"),
                    _ => return Err(()),
                },
                TIR::THandleOp::ReactiveGet if a.is_empty() => {
                    format!("{recv}.get()")
                }
                TIR::THandleOp::ReactiveSet if a.len() == 1 => {
                    format!("{recv}.set({})", a[0])
                }
                TIR::THandleOp::ReactiveEffectMethod { method } if a.is_empty() => {
                    match method.as_str() {
                        "unsubscribe" => format!("{recv}.unsubscribe()"),
                        "is_active" => format!("{recv}.isActive()"),
                        _ => return Err(()),
                    }
                }
                _ => return Err(()),
            }
        }
        E::NumericMethod { recv, op } => match op {
            TIR::TNumericOp::CastAs { dst_rust }
                if dst_rust.contains("i") || dst_rust.contains("u") =>
            {
                format!(
                    "BigInt(Math.trunc(Number({})))",
                    tir_js_expr(recv, funcs, file_prefix)?
                )
            }
            // Float cast leaves BigInt land.
            TIR::TNumericOp::CastAs { .. } => {
                format!("Number({})", tir_js_expr(recv, funcs, file_prefix)?)
            }
            TIR::TNumericOp::FloatToInt {
                lower,
                upper_exclusive,
                ..
            } => {
                let value = tir_js_expr(recv, funcs, file_prefix)?;
                format!(
                    "(() => {{ const __jet_value = Number({value}); return Number.isFinite(__jet_value) && __jet_value >= {lower} && __jet_value < {upper_exclusive} ? {{ tag: \"Some\", values: [BigInt(Math.trunc(__jet_value))] }} : {{ tag: \"None\", values: [] }}; }})()"
                )
            }
            _ => return Err(()),
        },
        E::OrFallback { value, fallback: TIR::TOrFallback::Value(fallback), .. } => {
            // Tagged Option/Result unwrap; also accepts legacy nullish from older emit.
            format!(
                "(((__jet_v) => (__jet_v != null && (__jet_v.tag === \"Some\" || __jet_v.tag === \"Ok\") ? __jet_v.values[0] : {}))({}))",
                tir_js_expr(fallback, funcs, file_prefix)?,
                tir_js_expr(value, funcs, file_prefix)?
            )
        },
        E::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            let mut rendered = String::new();
            emit_js_if_value(
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
                &mut rendered,
                funcs,
                file_prefix,
                1,
            )?;
            if rendered.contains("await bridge_") {
                format!("await (async () => {{\n{rendered}}})()")
            } else {
                format!("(() => {{\n{rendered}}})()")
            }
        }
        E::Lambda(lam) => tir_js_lambda(lam, funcs, file_prefix)?,
        E::CoreClosureCall { kind: TIR::TCoreClosureKind::UiReactiveRender { executable, .. } } => format!("jetDom.reactiveRender({})", tir_js_lambda(executable, funcs, file_prefix)?),
        E::CoreClosureCall { kind: TIR::TCoreClosureKind::ReactiveEffect { executable, .. } } => format!("jetDom.makeEffect({})", tir_js_lambda(executable, funcs, file_prefix)?),
        E::CoreClosureCall {
            kind: TIR::TCoreClosureKind::UiButtonOnClick {
                label,
                executable,
                ..
            },
        } => format!(
            "jetDom.makeButton({}, {})",
            tir_js_expr(label, funcs, file_prefix)?,
            tir_js_lambda(executable, funcs, file_prefix)?
        ),
        _ => return Err(()),
    })
}

fn tir_js_lambda(
    lam: &TIR::TLambda,
    funcs: &[FuncWeb],
    file_prefix: Option<&str>,
) -> Result<String, ()> {
    let params = lam.source_params.join(", ");
    let async_kw = |body: &str| {
        if body.contains("await bridge_") {
            "async "
        } else {
            ""
        }
    };
    match &lam.executable {
        TIR::TLambdaBody::Expr(body) => {
            let expr = tir_js_expr(body, funcs, file_prefix)?;
            Ok(format!(
                "{}({params}) => ({expr})",
                async_kw(&expr),
            ))
        }
        TIR::TLambdaBody::Block(body) => {
            let mut rendered = String::new();
            emit_tir_js_body(body, &mut rendered, funcs, file_prefix, 1)?;
            Ok(format!(
                "{}({params}) => {{\n{rendered}}}",
                async_kw(&rendered),
            ))
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
    let required = web_core_arity(module, method);
    if let Some(required) = required {
        if a.len() != required {
            return Err(());
        }
    } else if method == "mount" {
        if a.len() != 2 && a.len() != 3 {
            return Err(());
        }
    } else {
        return Err(());
    }
    let get = |i: usize| a[i].as_str();
    if method == "on" {
        let symbol = funcs.iter().find(|func| func.key == get(2)).map(|func| json_quote(&func.key)).unwrap_or_else(|| INLINE_HANDLER_PLACEHOLDER.into());
        return Ok(format!(
            "jetDom.on({}, {}, {}, {symbol})",
            get(0),
            get(1),
            get(2)
        ));
    }
    if method == "mount" {
        return Ok(if a.len() == 2 {
            format!("jetDom.mount({}, {})", get(0), get(1))
        } else {
            format!("jetDom.mount({}, {}, {})", get(0), get(1), get(2))
        });
    }
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

/// D-EXPOP1=A / D-EXPSEM1=A / D-FLOORDIV1=A: the wasm module runs the same
/// arithmetic source as the native Prelude — the very same files, included
/// verbatim. `jet_panic` is the only tier-local piece: the wasm module has no
/// runtime reporter, so a trap is a wasm panic.
const WASM_ARITH_PRELUDE: &str = concat!(
    "fn jet_panic(file: &str, line: u32, message: &str) -> ! {\n",
    "    panic!(\"{}:{}: {}\", file, line, message)\n",
    "}\n\n",
    // D-FAIL-CARRIER1=A: the very same carrier file the native prelude puts
    // first, so `T?` and `T ? E` mean one thing on the web tier too.
    include_str!("../../../jet-foundation/src/Outcome.rs"),
    "\n",
    include_str!("../Prelude/Core/Power.rs"),
    "\n",
    include_str!("../Prelude/Core/Division.rs"),
    "\n"
);

/// D-EXPSEM1=A / D-FLOORDIV1=A: the operators Rust has no symbol for. Each one
/// calls the same Prelude helper the native build calls, from the copy of that
/// Prelude file the wasm module includes. `None` means the operator is an
/// ordinary Rust one and the caller emits it directly.
///
/// The whole-number helpers carry the source position so their trap can name
/// the line the author wrote; the float helpers never trap and take neither.
fn wasm_prelude_call(
    op: crate::AST::BinOp,
    lhs: &str,
    rhs: &str,
    ty: &Type,
    file_prefix: Option<&str>,
    line: u32,
) -> Option<String> {
    use crate::AST::BinOp;
    let float = matches!(ty, Type::Float | Type::Float32);
    let file = file_prefix.unwrap_or_default();
    Some(match op {
        BinOp::Pow if float => format!("({lhs}).jet_pow({rhs})"),
        BinOp::Pow => format!("({lhs}).jet_pow(({rhs}) as i128, {file:?}, {line})"),
        BinOp::FloorDiv if float => format!("({lhs}).jet_floordiv({rhs})"),
        BinOp::FloorDiv => format!("({lhs}).jet_floordiv({rhs}, {file:?}, {line})"),
        BinOp::Mod => format!("({lhs}).jet_mod({rhs}, {file:?}, {line})"),
        BinOp::Rem => format!("({lhs}).jet_trunc_rem({rhs}, {file:?}, {line})"),
        _ => return None,
    })
}

/// D-EXPOP1=A / D-EXPSEM1=A: the JS tier's copy of the one power rule
/// (`Prelude/Core/Power.rs`). A whole-number power stays exact and stops the
/// moment it leaves the range JavaScript can hold exactly; a negative exponent
/// has no whole-number answer. `throw` is how this tier reports a trap.
///
/// JavaScript's own `**` is a floating-point power: it neither stays exact nor
/// traps, so `^` on whole numbers never lowers to it. Floats do use it, since
/// there the two operations agree.
const JS_POWER_PRELUDE: &str = concat!(
    // Every whole-number operator below computes in BigInt and clamps to 64
    // bits with `BigInt.asIntN`, because JavaScript's own number operators are
    // doubles (`*` stops being exact at 2^53) and its bitwise operators are
    // 32-bit. BigInt is the only way this tier can carry the Prelude's rule
    // rather than approximate it. Results stay BigInt (D-INTBIG1 / I9): returning
    // `Number(value)` silently rounded every answer above 2^53.
    "const JET_I64_MIN = -(2n ** 63n);\n",
    "const JET_I64_MAX = 2n ** 63n - 1n;\n",
    "function jet_i64(value, message) {\n",
    "  if (value < JET_I64_MIN || value > JET_I64_MAX) throw new Error(message);\n",
    "  return value;\n",
    "}\n\n",
    "function jet_pow(base, exponent) {\n",
    "  const e = BigInt(exponent);\n",
    "  if (e < 0n) {\n",
    "    throw new Error(\"a negative exponent has no whole-number result ",
    "(make the base a Float to raise it to a negative power)\");\n",
    "  }\n",
    "  const overflow = \"this power overflows the value's type ",
    "(the result is outside its range)\";\n",
    "  const b = BigInt(base);\n",
    "  if (b !== 0n && b !== 1n && b !== -1n && e > 63n) {\n",
    "    throw new Error(overflow);\n",
    "  }\n",
    "  return jet_i64(b ** e, overflow);\n",
    "}\n\n",
    // D-BITNOT1=A: `!` on a whole number turns over every one of its 64 bits.
    // JavaScript's `~` works on 32 bits, so it is not the same operation.
    "function jet_bitnot(value, bits, signed) {\n",
    "  const flipped = ~BigInt(value);\n",
    "  return signed ? BigInt.asIntN(bits, flipped) : BigInt.asUintN(bits, flipped);\n",
    "}\n\n",
    // D-FLOORDIV1=A: the JS tier's copy of the one floor-division rule
    // (`Prelude/Core/Division.rs`). Whole numbers trap on a zero divisor, the
    // same as `/`, and on the one pair whose quotient leaves the range.
    "function jet_floordiv(left, right) {\n",
    "  const a = BigInt(left);\n",
    "  const b = BigInt(right);\n",
    "  if (b === 0n) throw new Error(\"divided by zero\");\n",
    "  let quotient = a / b;\n",
    "  if (a % b !== 0n && (a < 0n) !== (b < 0n)) quotient -= 1n;\n",
    "  return jet_i64(quotient, \"this division overflows the value's type ",
    "(the result is outside its range)\");\n",
    "}\n\n",
    // Floats round down with no trap: a zero divisor gives an infinity, exactly
    // as `/` does.
    "function jet_floordiv_float(left, right) {\n",
    "  return Math.floor(Number(left) / Number(right));\n",
    "}\n\n",
    // D-MODSEM1=A: the floored modulo. JavaScript's `%` is the truncated
    // remainder, which Jet spells `%%`, so the answer is corrected onto the
    // divisor's side of zero.
    "function jet_mod(left, right) {\n",
    "  const a = BigInt(left);\n",
    "  const b = BigInt(right);\n",
    "  if (b === 0n) throw new Error(\"divided by zero\");\n",
    "  let remainder = a % b;\n",
    "  if (remainder !== 0n && (remainder < 0n) !== (b < 0n)) remainder += b;\n",
    "  return BigInt.asIntN(64, remainder);\n",
    "}\n\n",
    // D-MODSEM1=A: the truncated remainder, which is the sign JavaScript's own
    // `%` already gives — this adds the zero-divisor trap and the 64-bit range.
    "function jet_trunc_rem(left, right) {\n",
    "  const b = BigInt(right);\n",
    "  if (b === 0n) throw new Error(\"divided by zero\");\n",
    "  return BigInt.asIntN(64, BigInt(left) % b);\n",
    "}\n\n"
);

/// D-EXPSEM1=A: one call shape for `^` in the JS tier. Floats take the
/// JavaScript power, which agrees with the Prelude float power; whole numbers
/// take `jet_pow`, which carries the exact, trapping rule.
fn js_prelude_call(op: crate::AST::BinOp, lhs: &str, rhs: &str, ty: &Type) -> Option<String> {
    use crate::AST::BinOp;
    let float = matches!(ty, Type::Float | Type::Float32);
    Some(match op {
        BinOp::Pow if float => format!("Math.pow(Number({lhs}), Number({rhs}))"),
        BinOp::Pow => format!("jet_pow({lhs}, {rhs})"),
        // A float divisor of zero gives an infinity, exactly as `/` does, so
        // only the whole-number helper traps.
        BinOp::FloorDiv if float => format!("jet_floordiv_float({lhs}, {rhs})"),
        BinOp::FloorDiv => format!("jet_floordiv({lhs}, {rhs})"),
        BinOp::Mod => format!("jet_mod({lhs}, {rhs})"),
        BinOp::Rem => format!("jet_trunc_rem({lhs}, {rhs})"),
        _ => return None,
    })
}

/// The operator symbol for the wasm Rust tier and the JS tier, where the two
/// languages agree. `None` means the operation has no symbol on this tier and
/// the caller must reach for the Prelude helper instead: `^` (D-EXPSEM1), `/%`
/// (D-FLOORDIV1), and the floored `%` (D-MODSEM1) are all shaped that way,
/// because JavaScript's `**`, `/`, and `%` are each a different operation.
fn binop(op: &crate::AST::BinOp) -> Option<&'static str> {
    use crate::AST::BinOp::*;
    Some(match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Rem => "%",
        Pow | FloorDiv | Mod => return None,
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
    })
}

/// The Rust spelling. Rust's `!` is already the bitwise complement on whole
/// numbers and the logical one on `Bool` (D-BITNOT1=A), so one symbol covers
/// both there.
fn unop(op: &crate::AST::UnOp) -> &'static str {
    use crate::AST::UnOp::*;
    match op {
        Neg => "-",
        Not => "!",
    }
}

/// D-BITNOT1=A: JavaScript splits what Rust joins. `!` there is the logical
/// negation only — on a number it asks "is this zero", which is not what `!`
/// means in Jet. The bitwise complement is `~`.
fn js_unary_call(op: &crate::AST::UnOp, ty: &Type, operand: &str) -> String {
    use crate::AST::UnOp::*;
    match op {
        Neg => format!("(-{operand})"),
        Not if matches!(ty, Type::Bool) => format!("(!{operand})"),
        // D-BITNOT1=A: JavaScript's `~` turns over 32 bits, so it answers -1
        // where Jet answers -4294967297. The preamble helper carries the rule
        // the Prelude runs, and it needs the operand's own width: `!U8.{5}` is
        // 250, not -6.
        Not => {
            let (bits, signed) = match ty {
                Type::IntN { bits, signed } => (*bits, *signed),
                _ => (64, true),
            };
            format!("jet_bitnot({operand}, {bits}, {signed})")
        }
    }
}

#[cfg(test)]
mod tir_contract_tests {
    use super::*;
    use super::TIR::THandleOp;

    #[test]
    fn js_handle_preflight_matches_emit_table() {
        assert!(web_js_ui_backend_method_supported("paint", 1));
        assert!(!web_js_ui_backend_method_supported("frame_lines", 0));
        assert!(!web_js_ui_backend_method_supported("label", 1));
        assert!(web_js_handle_method_supported(&THandleOp::ReactiveGet, 0));
        assert!(!web_js_handle_method_supported(&THandleOp::ReactiveGet, 1));
        assert!(web_js_handle_method_supported(
            &THandleOp::ReactiveEffectMethod {
                method: "is_active".to_string(),
            },
            0,
        ));
        assert!(!web_js_handle_method_supported(
            &THandleOp::ReactiveEffectMethod {
                method: "pause".to_string(),
            },
            0,
        ));
    }
}

#[cfg(test)]
mod source_map_tests {
    use super::*;

    #[test]
    fn source_map_markers_strip_and_encode_v3_segments() {
        let sources = vec![JSSource {
            display: "main.jet".to_string(),
            name: "main.jet".to_string(),
            content: "first\nsecond\n".to_string(),
        }];
        let raw = concat!(
            "const label = \"😀\";\n",
            "  //# __jet_source_map file 0\n",
            "  //# __jet_source_map line 2\n",
            "  run();\n",
        );

        let (js, map) = finish_js_source_map(raw, &sources, "//# __jet_source_map");

        assert_eq!(js, "const label = \"😀\";\n  run();\n");
        assert!(map.contains("\"mappings\":\";EACA\""), "{map}");

        let mut negative = String::new();
        encode_base64_vlq(-123, &mut negative);
        assert_eq!(negative, "3H");
    }

    #[test]
    fn source_marker_selection_is_bounded_against_underscore_runs() {
        let underscores = "_".repeat(65_536);
        let hostile = format!("print(\"//# __jet_source_map{underscores}\")\n");
        let marker = source_marker_for_texts(std::iter::once(hostile.as_str()));
        assert_eq!(marker.len(), "//# __jet_source_map".len() + 65_537);
        assert!(
            !hostile.contains(&marker),
            "selected marker must not occur in hostile source"
        );
        assert_eq!(
            source_marker_for_texts(std::iter::once("fn run() {}")),
            "//# __jet_source_map"
        );
    }
}
