//! D-WEBBACKEND1 / D-WEBKIND1 / D-DOMGEN1 (c123 M2): WASM + JS web backend emission.

use crate::AST::{Expr, FfiLink, Item, ProgramBundle, Stmt, Type};
use crate::Sema::CompileMode;
use crate::Syntax;
use jet_foundation::WebPartition::{WebBucket, WebPartitionMarker};

/// Generated web backend artifacts (WASM Rust, JS loader/app, DOM shim, manifest).
#[derive(Debug, Clone)]
pub struct WebArtifacts {
    pub manifest_json: String,
    pub wasm_rust: String,
    pub js_app: String,
    pub dom_runtime: String,
}

const DOM_RUNTIME: &str = include_str!("../Prelude/DomRuntime.js");

#[derive(Clone)]
struct FuncWeb {
    name: String,
    _key: String,
    bucket: WebBucket,
    marker: Option<WebPartitionMarker>,
    params: Vec<(String, Type)>,
    return_type: Option<Type>,
    body: Vec<Stmt>,
}

pub fn emit_web(bundle: &ProgramBundle, _mode: CompileMode, _link: Option<&FfiLink>) -> WebArtifacts {
    let funcs = collect_web_funcs(bundle);
    let manifest_json = emit_manifest(bundle, &funcs);
    let wasm_rust = emit_wasm_rust(bundle, &funcs);
    let js_app = emit_js_app(bundle, &funcs);
    WebArtifacts {
        manifest_json,
        wasm_rust,
        js_app,
        dom_runtime: DOM_RUNTIME.to_string(),
    }
}

fn collect_web_funcs(bundle: &ProgramBundle) -> Vec<FuncWeb> {
    let mut out = Vec::new();
    for module in &bundle.modules {
        collect_module_funcs(
            &module.items,
            module.web_target_ceiling,
            None,
            None,
            bundle,
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
    bundle: &ProgramBundle,
    out: &mut Vec<FuncWeb>,
) {
    let ceiling = module_ceiling.or(file_ceiling);
    let _ = ceiling;
    for item in items {
        match item {
            Item::Func(f) => {
                let key = match module_prefix {
                    Some(m) => format!("{m}__{}", f.name),
                    None => f.name.clone(),
                };
                let bucket = bundle
                    .web_partitions
                    .get(&key)
                    .copied()
                    .unwrap_or(WebBucket::Wasm);
                out.push(FuncWeb {
                    name: f.name.clone(),
                    _key: key,
                    bucket,
                    marker: f.web_marker,
                    params: f
                        .params
                        .iter()
                        .map(|p| (p.name.clone(), p.ty.clone()))
                        .collect(),
                    return_type: f.return_type.clone(),
                    body: f.body.clone(),
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
                        bundle,
                        out,
                    );
                }
            }
            _ => {}
        }
    }
}

fn json_quote(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\").replace('"', "\\\"")
    )
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
        .find(|f| f.name == "main")
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
            json_quote(&f.name),
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
                json_quote(&f.name),
                json_quote(&wasm_export_symbol(&f.name)),
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

fn find_struct_fields<'a>(bundle: &'a ProgramBundle, name: &str) -> Option<&'a [crate::AST::Field]> {
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

/// Flatten `#[Codable]` struct parameters into scalar WASM params (D-JSBIND1).
fn flatten_abi_params(bundle: &ProgramBundle, params: &[(String, Type)]) -> Vec<(String, Type)> {
    let mut out = Vec::new();
    for (name, ty) in params {
        if let Type::Named(n) = ty {
            if let Some(fields) = find_struct_fields(bundle, n) {
                if fields.iter().all(|f| matches!(f.ty, Type::Int | Type::IntN { .. })) {
                    for field in fields {
                        out.push((format!("{name}_{}", field.name), field.ty.clone()));
                    }
                    continue;
                }
            }
        }
        out.push((name.clone(), ty.clone()));
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
            if fields.iter().all(|f| matches!(f.ty, Type::Int | Type::IntN { .. })) {
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
        Type::List(inner) if matches!(**inner, Type::Int | Type::IntN { .. }) => {
            prelude.push_str(&format!(
                "  const _{name} = jetDom.marshalAbi({name}, \"list-int\");\n"
            ));
            vec![format!("_{name}")]
        }
        _ => vec![name.to_string()],
    }
}

fn emit_wasm_rust(bundle: &ProgramBundle, funcs: &[FuncWeb]) -> String {
    let mut out = String::from(
        "// Generated by jet — wasm32-unknown-unknown module (D-WEBKIND1).\n\
         #![allow(unused)]\n\n",
    );
    let wasm_funcs: Vec<&FuncWeb> = funcs
        .iter()
        .filter(|f| f.bucket == WebBucket::Wasm)
        .collect();
    if wasm_funcs.is_empty() {
        out.push_str("#[no_mangle]\npub extern \"C\" fn jet_wasm_nop() {}\n");
        return out;
    }
    for f in wasm_funcs {
        let export = f.marker == Some(WebPartitionMarker::WasmExport)
            || (f.name == "main" && f.bucket == WebBucket::Wasm);
        if export {
            emit_wasm_fn(bundle, f, export, &mut out);
        }
    }
    out
}

fn emit_wasm_fn(bundle: &ProgramBundle, f: &FuncWeb, export: bool, out: &mut String) {
    if export {
        out.push_str(&format!(
            "#[no_mangle]\npub extern \"C\" fn {}(",
            wasm_export_symbol(&f.name)
        ));
    } else {
        out.push_str(&format!("fn jet_wasm_{}(", f.name));
    }
    let flat = flatten_abi_params(bundle, &f.params);
    let params: Vec<String> = flat
        .iter()
        .map(|(name, ty)| format!("{name}: {}", wasm_ty(ty)))
        .collect();
    out.push_str(&params.join(", "));
    out.push(')');
    if let Some(ret) = &f.return_type {
        out.push_str(&format!(" -> {} ", wasm_ty(ret)));
    }
    out.push_str("{\n");
    if let Some(ret) = &f.return_type {
        if let Some(expr) = wasm_body_return(&f.body) {
            out.push_str(&format!("    {}\n", wasm_emit_expr(expr)));
        } else {
            out.push_str(&format!("    {}\n", wasm_default(ret)));
        }
    }
    out.push_str("}\n\n");
}

fn wasm_ty(ty: &Type) -> &'static str {
    match ty {
        Type::Int | Type::IntN { signed: true, .. } => "i64",
        Type::IntN { signed: false, .. } => "u64",
        Type::Float | Type::Float32 => "f64",
        Type::Bool => "bool",
        _ => "i64",
    }
}

fn wasm_default(ty: &Type) -> String {
    match ty {
        Type::Float | Type::Float32 => "0.0".to_string(),
        Type::Bool => "false".to_string(),
        _ => "0".to_string(),
    }
}

fn wasm_body_return(body: &[Stmt]) -> Option<&Expr> {
    for stmt in body.iter().rev() {
        if let Stmt::Return(Some(expr), _) = stmt {
            return Some(expr);
        }
    }
    for stmt in body.iter().rev() {
        match stmt {
            Stmt::Expr(e) => return Some(e),
            Stmt::Val(b) => return Some(&b.init),
            _ => {}
        }
    }
    None
}

fn wasm_emit_expr(expr: &Expr) -> String {
    match expr {
        Expr::Int(n, _, _) => n.to_string(),
        Expr::Float(n, _, _) => n.to_string(),
        Expr::Bool(b, _) => b.to_string(),
        Expr::Ident(name, _) => name.clone(),
        Expr::Binary(op, lhs, rhs, _) => format!(
            "({} {} {})",
            wasm_emit_expr(lhs),
            binop(op),
            wasm_emit_expr(rhs)
        ),
        Expr::Unary(op, inner, _) => format!("({}{})", unop(op), wasm_emit_expr(inner)),
        Expr::Paren(inner, _) => wasm_emit_expr(inner),
        Expr::Call(call) => {
            if call.args.len() == 1 {
                if call.name == "take" {
                    return wasm_emit_expr(&call.args[0].expr);
                }
            }
            "0".to_string()
        }
        _ => "0".to_string(),
    }
}

fn emit_js_app(bundle: &ProgramBundle, funcs: &[FuncWeb]) -> String {
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
            out.push_str(&format!("async function bridge_{}({}) {{\n", f.name, args.join(", ")));
            out.push_str("  const wasm = await loadWasm();\n");
            let sym = wasm_export_symbol(&f.name);
            let mut prelude = String::new();
            let call_args: Vec<String> = f
                .params
                .iter()
                .flat_map(|(n, ty)| js_abi_call_args(bundle, n, ty, &mut prelude))
                .collect();
            out.push_str(&prelude);
            out.push_str(&format!("  const raw = wasm.{sym}({});\n", call_args.join(", ")));
            out.push_str("  return jetDom.unmarshalAbi(raw, \"scalar\");\n");
            out.push_str("}\n\n");
        }
    }

    let js_funcs: Vec<&FuncWeb> = funcs
        .iter()
        .filter(|f| f.bucket == WebBucket::Js && f.name != "main")
        .collect();
    for f in &js_funcs {
        emit_js_fn(f, &mut out, funcs);
    }

    if let Some(main_fn) = funcs.iter().find(|f| f.name == "main") {
        if main_fn.bucket == WebBucket::Js {
            out.push_str("export async function jet_main() {\n");
            emit_js_body(&main_fn.body, &mut out, funcs, 1);
            out.push_str("}\n\n");
            out.push_str("const _isMain = typeof process !== \"undefined\" && process.argv[1]?.endsWith(\"app.js\");\n");
            out.push_str("if (_isMain) { jet_main(); }\n");
        } else {
            out.push_str("export async function jet_main() {\n");
            out.push_str("  const wasm = await loadWasm();\n");
            out.push_str(&format!("  wasm.{}();\n", wasm_export_symbol("main")));
            out.push_str("}\n");
        }
    } else {
        out.push_str("export async function jet_main() {}\n");
    }
    out
}

fn emit_js_fn(f: &FuncWeb, out: &mut String, all: &[FuncWeb]) {
    out.push_str(&format!("function {}({}) {{\n", f.name, param_names(&f.params)));
    emit_js_body(&f.body, out, all, 1);
    if let Some(ret) = &f.return_type {
        if !body_has_return(&f.body) {
            out.push_str(&format!("  return {};\n", js_default(ret)));
        }
    }
    out.push_str("}\n\n");
}

fn param_names(params: &[(String, Type)]) -> String {
    params
        .iter()
        .map(|(n, _)| n.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn body_has_return(body: &[Stmt]) -> bool {
    body.iter().any(|s| matches!(s, Stmt::Return(_, _)))
}

fn js_default(ty: &Type) -> &'static str {
    match ty {
        Type::String => "\"\"",
        Type::Bool => "false",
        Type::Float | Type::Float32 => "0.0",
        _ => "0",
    }
}

fn emit_js_body(body: &[Stmt], out: &mut String, funcs: &[FuncWeb], indent: usize) {
    let pad = "  ".repeat(indent);
    for stmt in body {
        match stmt {
            Stmt::Expr(expr) => {
                if let Some(line) = emit_js_stmt_expr(expr, funcs) {
                    out.push_str(&format!("{pad}{line}\n"));
                }
            }
            Stmt::Val(b) => {
                let init = js_emit_expr(&b.init, funcs);
                out.push_str(&format!("{pad}let {} = {init};\n", b.name));
            }
            Stmt::Return(Some(expr), _) => {
                out.push_str(&format!("{pad}return {};\n", js_emit_expr(expr, funcs)));
            }
            Stmt::Return(None, _) => out.push_str(&format!("{pad}return;\n")),
            Stmt::For { var, kind, body, .. } => match kind {
                crate::AST::ForKind::Range { start, end, .. } => {
                    out.push_str(&format!(
                        "{pad}for (let {var} = {}; {var} < {}; {var}++) {{\n",
                        js_emit_expr(start, funcs),
                        js_emit_expr(end, funcs),
                    ));
                    emit_js_body(body, out, funcs, indent + 1);
                    out.push_str(&format!("{pad}}}\n"));
                }
                crate::AST::ForKind::In { collection } => {
                    out.push_str(&format!(
                        "{pad}for (const {var} of {}) {{\n",
                        js_emit_expr(collection, funcs)
                    ));
                    emit_js_body(body, out, funcs, indent + 1);
                    out.push_str(&format!("{pad}}}\n"));
                }
            },
            Stmt::If(if_stmt) => {
                out.push_str(&format!(
                    "{pad}if ({}) {{\n",
                    js_emit_expr(&if_stmt.cond, funcs)
                ));
                emit_js_body(&if_stmt.then_body, out, funcs, indent + 1);
                match &if_stmt.else_branch {
                    Some(crate::AST::ElseBranch::Else(else_body)) => {
                        out.push_str(&format!("{pad}}} else {{\n"));
                        emit_js_body(else_body, out, funcs, indent + 1);
                    }
                    Some(crate::AST::ElseBranch::ElseIf(next)) => {
                        out.push_str(&format!("{pad}}} else "));
                        emit_js_body(&[Stmt::If(*next.clone())], out, funcs, indent);
                        return;
                    }
                    None => {}
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            _ => {}
        }
    }
}

fn emit_js_stmt_expr(expr: &Expr, funcs: &[FuncWeb]) -> Option<String> {
    match expr {
        Expr::Call(call) => {
            if call.name == "print" && call.args.len() == 1 {
                return Some(format!(
                    "jetDom.print({});",
                    js_emit_expr(&call.args[0].expr, funcs)
                ));
            }
            js_emit_call_expr(call, funcs).map(|e| format!("{e};"))
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => Some(format!(
            "{};",
            emit_js_method(receiver, method, args, funcs)
        )),
        _ => None,
    }
}

fn ui_method_call(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    funcs: &[FuncWeb],
) -> Option<String> {
    if let Expr::Ident(ns, _) = receiver {
        if ns == "ui" {
            return ui_dom_call(&format!("ui.{method}"), args, funcs);
        }
    }
    None
}

fn emit_js_method(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    funcs: &[FuncWeb],
) -> String {
    if let Some(dom) = ui_method_call(receiver, method, args, funcs) {
        return dom;
    }
    let recv = js_emit_expr(receiver, funcs);
    let arg_exprs: Vec<String> = args.iter().map(|a| js_emit_expr(&a.expr, funcs)).collect();
    match (method, arg_exprs.len()) {
        ("measure", 2) => format!(
            "jetDom.measure({}, {})",
            arg_exprs[0], arg_exprs[1]
        ),
        ("layout", 2) => format!(
            "jetDom.layout({recv}, {}, {})",
            arg_exprs[0], arg_exprs[1]
        ),
        ("paint", 1) => format!("jetDom.paint({recv}, {})", arg_exprs[0]),
        ("commands", 0) => format!("jetDom.commands({recv})"),
        ("on_event", 1) => format!("jetDom.onEvent({recv}, {})", arg_exprs[0]),
        ("get", 0) => format!("{recv}.get()"),
        ("set", 1) => format!("{recv}.set({})", arg_exprs[0]),
        _ => format!("/* {method} */ undefined"),
    }
}

fn js_emit_expr(expr: &Expr, funcs: &[FuncWeb]) -> String {
    match expr {
        Expr::Int(n, _, _) => n.to_string(),
        Expr::Float(n, _, _) => n.to_string(),
        Expr::Bool(b, _) => b.to_string(),
        Expr::Str(parts, _) => js_string_lit(parts),
        Expr::Ident(name, _) => name.clone(),
        Expr::Field(base, field, _) => format!("{}.{}", js_emit_expr(base, funcs), field),
        Expr::Binary(op, lhs, rhs, _) => format!(
            "({} {} {})",
            js_emit_expr(lhs, funcs),
            binop(op),
            js_emit_expr(rhs, funcs)
        ),
        Expr::Unary(op, inner, _) => format!("({}{})", unop(op), js_emit_expr(inner, funcs)),
        Expr::Paren(inner, _) => js_emit_expr(inner, funcs),
        Expr::Call(call) => js_emit_call_expr(call, funcs).unwrap_or_else(|| "undefined".to_string()),
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => emit_js_method(receiver, method, args, funcs),
        _ => "undefined".to_string(),
    }
}

fn js_string_lit(parts: &[crate::AST::StrPart]) -> String {
    if parts.iter().any(|p| matches!(p, crate::AST::StrPart::Interp(_, _))) {
        let mut s = String::from("`");
        for part in parts {
            match part {
                crate::AST::StrPart::Lit(t) => s.push_str(t),
                crate::AST::StrPart::Interp(e, _) => {
                    s.push_str("${");
                    s.push_str(&js_emit_expr(e, &[]));
                    s.push('}');
                }
            }
        }
        s.push('`');
        s
    } else {
        let mut lit = String::new();
        for part in parts {
            if let crate::AST::StrPart::Lit(t) = part {
                lit.push_str(t);
            }
        }
        json_quote(&lit)
    }
}

fn js_emit_call_expr(call: &crate::AST::Call, funcs: &[FuncWeb]) -> Option<String> {
    if is_wasm_export(&call.name, funcs) {
        let args: Vec<String> = call
            .args
            .iter()
            .map(|a| js_emit_expr(&a.expr, funcs))
            .collect();
        return Some(format!(
            "await bridge_{}({})",
            call.name,
            args.join(", ")
        ));
    }
    if let Some(dom) = ui_dom_call(&call.name, &call.args, funcs) {
        return Some(dom);
    }
    if call.name == "take" && call.args.len() == 1 {
        return Some(js_emit_expr(&call.args[0].expr, funcs));
    }
    let args: Vec<String> = call
        .args
        .iter()
        .map(|a| js_emit_expr(&a.expr, funcs))
        .collect();
    Some(format!("{}({})", call.name, args.join(", ")))
}

fn is_wasm_export(name: &str, funcs: &[FuncWeb]) -> bool {
    funcs
        .iter()
        .any(|f| f.name == name && f.marker == Some(WebPartitionMarker::WasmExport))
}

fn ui_dom_call(name: &str, args: &[crate::AST::CallArg], funcs: &[FuncWeb]) -> Option<String> {
    let short = name.rsplit('.').next().unwrap_or(name);
    let a = |i: usize| {
        args.get(i)
            .map(|arg| js_emit_expr(&arg.expr, funcs))
            .unwrap_or_else(|| "0".to_string())
    };
    let rendered = match short {
        "null_backend" => "jetDom.createBackend()".to_string(),
        "node" => format!("jetDom.makeNode({}, {}, {})", a(0), a(1), a(2)),
        "constraint" => format!(
            "jetDom.makeConstraint({}, {}, {}, {})",
            a(0),
            a(1),
            a(2),
            a(3)
        ),
        "rect" => format!("jetDom.makeRect({}, {}, {}, {})", a(0), a(1), a(2), a(3)),
        "key_event" => format!("jetDom.makeKeyEvent({})", a(0)),
        _ => return None,
    };
    Some(rendered)
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
