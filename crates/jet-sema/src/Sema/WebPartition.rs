//! D-WASM1=A (c123 M1): classify functions into JS/WASM buckets and diagnose crossings.

use crate::Diagnostics::Diagnostic;
use crate::Generics::{DECODE, ENCODE};
use crate::Syntax::{self, WebBucket, WebPartitionMarker};
use crate::AST::{EnumDef, Item, ProgramBundle, StructDef, Type, VariantPayload};
use std::collections::HashMap;

use super::effect_key;
use super::Effect;
use super::EffectSet;
use super::EffectSummary;

#[derive(Debug, Clone)]
pub(crate) struct FuncWebMeta {
    key: String,
    name: String,
    name_span: crate::Diagnostics::Span,
    marker: Option<WebPartitionMarker>,
    ceiling: Option<WebBucket>,
    params: Vec<Type>,
    return_type: Option<Type>,
}

#[derive(Debug, Default)]
struct AbiTypeIndex {
    structs: HashMap<String, StructDef>,
    enums: HashMap<String, EnumDef>,
}

impl AbiTypeIndex {
    fn from_bundle(bundle: &ProgramBundle) -> Self {
        let mut out = Self::default();
        for module in &bundle.modules {
            collect_abi_types(&module.items, &mut out);
        }
        out
    }
}

fn collect_abi_types(items: &[Item], out: &mut AbiTypeIndex) {
    for item in items {
        match item {
            Item::Struct(s) => {
                out.structs.insert(s.name.clone(), clone_struct(s));
            }
            Item::Enum(e) => {
                out.enums.insert(e.name.clone(), clone_enum(e));
            }
            Item::CodeModule(cm) => {
                if let Some(body) = &cm.body {
                    collect_abi_types(body, out);
                }
            }
            _ => {}
        }
    }
}

fn clone_struct(s: &StructDef) -> StructDef {
    StructDef {
        is_pub: s.is_pub,
        is_package_pub: s.is_package_pub,
        name: s.name.clone(),
        name_span: s.name_span,
        type_params: s.type_params.clone(),
        fields: s.fields.clone(),
        methods: Vec::new(),
        trait_impls: Vec::new(),
        derives: s.derives.clone(),
        is_published_schema: s.is_published_schema,
        published_schema_span: s.published_schema_span,
        is_single_use: s.is_single_use,
        single_use_span: s.single_use_span,
        is_must_use: s.is_must_use,
        must_use_span: s.must_use_span,
        layout: s.layout.clone(),
        layout_span: s.layout_span,
        serde_markers: s.serde_markers.clone(),
        type_markers: s.type_markers.clone(),
    }
}

fn clone_variant(v: &crate::AST::Variant) -> crate::AST::Variant {
    crate::AST::Variant {
        name: v.name.clone(),
        name_span: v.name_span,
        payload: v.payload.clone(),
        serde_markers: v.serde_markers.clone(),
    }
}

fn clone_enum(e: &crate::AST::EnumDef) -> EnumDef {
    EnumDef {
        is_pub: e.is_pub,
        is_package_pub: e.is_package_pub,
        name: e.name.clone(),
        name_span: e.name_span,
        type_params: e.type_params.clone(),
        variants: e.variants.iter().map(clone_variant).collect(),
        methods: Vec::new(),
        trait_impls: Vec::new(),
        derives: e.derives.clone(),
        is_single_use: e.is_single_use,
        single_use_span: e.single_use_span,
        is_must_use: e.is_must_use,
        must_use_span: e.must_use_span,
        serde_markers: e.serde_markers.clone(),
        type_markers: e.type_markers.clone(),
        groups: e.groups.clone(),
    }
}

fn has_codable_derive(derives: &[(String, crate::Diagnostics::Span)]) -> bool {
    let mut encode = false;
    let mut decode = false;
    for (d, _) in derives {
        match d.as_str() {
            Syntax::ATTR_CODABLE => return true,
            d if d == ENCODE => encode = true,
            d if d == DECODE => decode = true,
            _ => {}
        }
    }
    encode && decode
}

fn is_abi_safe_type_full(ty: &Type, idx: &AbiTypeIndex) -> bool {
    if Syntax::is_abi_safe_type(ty) {
        return true;
    }
    match ty {
        Type::Named(n) => idx.is_codable_named(n, &[]),
        Type::Apply { name, args } => idx.is_codable_named(name, args),
        _ => false,
    }
}

impl AbiTypeIndex {
    fn is_codable_named(&self, name: &str, args: &[Type]) -> bool {
        if let Some(s) = self.structs.get(name) {
            if !has_codable_derive(&s.derives) {
                return false;
            }
            if !s.type_params.is_empty() && !args.is_empty() {
                if args.len() != s.type_params.len() {
                    return false;
                }
                return args.iter().all(|t| is_abi_safe_type_full(t, self));
            }
            return s.fields.iter().all(|f| is_abi_safe_type_full(&f.ty, self));
        }
        if let Some(e) = self.enums.get(name) {
            if !has_codable_derive(&e.derives) {
                return false;
            }
            if !e.type_params.is_empty() && !args.is_empty() {
                if args.len() != e.type_params.len() {
                    return false;
                }
                return args.iter().all(|t| is_abi_safe_type_full(t, self));
            }
            return e.variants.iter().all(|v| match &v.payload {
                VariantPayload::Unit => true,
                VariantPayload::Single(t, _) => is_abi_safe_type_full(t, self),
                VariantPayload::Named(fields) => {
                    fields.iter().all(|f| is_abi_safe_type_full(&f.ty, self))
                }
            });
        }
        false
    }
}

/// Infer partition bucket for one function.
fn assign_bucket(
    marker: Option<WebPartitionMarker>,
    ceiling: Option<WebBucket>,
    effects: &EffectSet,
) -> WebBucket {
    if let Some(m) = marker {
        return m.bucket();
    }
    if effects.contains(&Effect::Browser) {
        return WebBucket::Js;
    }
    if let Some(c) = ceiling {
        return c;
    }
    WebBucket::Wasm
}

fn items_have_web_markers(items: &[Item]) -> bool {
    for item in items {
        match item {
            Item::Func(f) if f.web_marker.is_some() => return true,
            Item::CodeModule(cm) => {
                if cm.web_target.is_some() {
                    return true;
                }
                if let Some(body) = &cm.body {
                    if items_have_web_markers(body) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Partition diagnostics run only for web builds or sources that opt in via markers.
fn web_partition_active(bundle: &ProgramBundle) -> bool {
    if bundle.web_partition_enforced {
        return true;
    }
    for module in &bundle.modules {
        if module.web_target_ceiling.is_some() {
            return true;
        }
        if items_have_web_markers(&module.items) {
            return true;
        }
    }
    false
}

fn collect_funcs(
    items: &[Item],
    file_ceiling: Option<WebBucket>,
    module_ceiling: Option<WebBucket>,
    module_prefix: Option<&str>,
    out: &mut Vec<FuncWebMeta>,
) {
    let ceiling = module_ceiling.or(file_ceiling);
    for item in items {
        match item {
            Item::Func(f) => {
                let key = match module_prefix {
                    Some(m) => format!("{m}__{}", f.name),
                    None => effect_key(None, &f.name),
                };
                out.push(FuncWebMeta {
                    key,
                    name: f.name.clone(),
                    name_span: f.name_span,
                    marker: f.web_marker,
                    ceiling,
                    params: f.params.iter().map(|p| p.ty.clone()).collect(),
                    return_type: f.return_type.clone(),
                });
            }
            Item::CodeModule(cm) => {
                let mod_ceiling = cm.web_target.or(ceiling);
                if let Some(body) = &cm.body {
                    collect_funcs(body, file_ceiling, mod_ceiling, Some(&cm.name), out);
                }
            }
            _ => {}
        }
    }
}

fn type_show(ty: &Type) -> String {
    match ty {
        Type::Int => "Int".to_string(),
        Type::Float => "Float".to_string(),
        Type::Bool => "Bool".to_string(),
        Type::String => "String".to_string(),
        Type::Char => "Char".to_string(),
        Type::Named(n) => n.clone(),
        Type::List(inner) => format!("[{}]", type_show(inner)),
        Type::Map { value, .. } => format!("Map<String, {}>", type_show(value)),
        other => format!("{other:?}"),
    }
}

fn marker_show(marker: Option<WebPartitionMarker>) -> &'static str {
    match marker {
        Some(WebPartitionMarker::Wasm) => Syntax::ATTR_WASM,
        Some(WebPartitionMarker::Js) => Syntax::ATTR_JS,
        Some(WebPartitionMarker::WasmExport) => Syntax::ATTR_WASM_EXPORT,
        None => "—",
    }
}

fn reason_show(f: &FuncWebMeta, effects: &EffectSet) -> String {
    if let Some(m) = f.marker {
        return format!("#{}", marker_show(Some(m)));
    }
    if effects.contains(&Effect::Browser) {
        return "inferred: Browser effect".to_string();
    }
    if let Some(c) = f.ceiling {
        return format!("#{}({})", Syntax::ATTR_TARGET, c.name());
    }
    if f.name == "run" {
        return "entry".to_string();
    }
    "inferred: pure / no Browser effect".to_string()
}

fn effects_show(effects: &EffectSet) -> String {
    let mut names: Vec<&str> = effects.iter().map(|&e| e.name()).collect();
    names.sort_unstable();
    if names.is_empty() {
        "(pure)".to_string()
    } else {
        names.join(", ")
    }
}

/// Human-readable partition report for `jet build --target web --explain-partition`.
pub fn format_partition_report(
    metas: &[FuncWebMeta],
    partitions: &HashMap<String, WebBucket>,
    solved: &HashMap<String, EffectSet>,
) -> String {
    let mut lines = vec![
        "Web partition report (D-WASM1)".to_string(),
        "================================".to_string(),
    ];
    let mut rows: Vec<_> = metas.iter().collect();
    rows.sort_by(|a, b| a.key.cmp(&b.key));
    for f in rows {
        let bucket = partitions.get(&f.key).copied().unwrap_or(WebBucket::Wasm);
        let effects = solved.get(&f.key).cloned().unwrap_or_default();
        lines.push(format!(
            "{:<24} -> {:<4}  {}  effects: {}",
            f.name,
            bucket.name(),
            reason_show(f, &effects),
            effects_show(&effects),
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn check_abi_export(f: &FuncWebMeta, idx: &AbiTypeIndex, diags: &mut Vec<Diagnostic>) {
    let Some(WebPartitionMarker::WasmExport) = f.marker else {
        return;
    };
    for p in &f.params {
        if !is_abi_safe_type_full(p, idx) {
            diags.push(Syntax::web_abi_type(
                &type_show(p),
                "on a `#WasmExport` parameter",
                Some(f.name_span),
            ));
        }
    }
    if let Some(ret) = &f.return_type {
        if !is_abi_safe_type_full(ret, idx) {
            diags.push(Syntax::web_abi_type(
                &type_show(ret),
                "as a `#WasmExport` return type",
                Some(f.name_span),
            ));
        }
    }
}

fn check_target_browser(
    f: &FuncWebMeta,
    bucket: WebBucket,
    effects: &EffectSet,
    diags: &mut Vec<Diagnostic>,
) {
    if bucket == WebBucket::Wasm && effects.contains(&Effect::Browser) {
        diags.push(Syntax::web_target_browser(&f.name, Some(f.name_span)));
    }
}

/// Walk the bundle, assign buckets, and emit partition / ABI diagnostics.
pub fn check_web_partition(
    bundle: &mut ProgramBundle,
    summaries: &HashMap<String, EffectSummary>,
    solved: &HashMap<String, EffectSet>,
) -> Vec<Diagnostic> {
    let mut metas = Vec::new();
    for module in &bundle.modules {
        collect_funcs(
            &module.items,
            module.web_target_ceiling,
            None,
            None,
            &mut metas,
        );
    }

    let mut partitions: HashMap<String, WebBucket> = HashMap::new();
    for f in &metas {
        let effects = solved.get(&f.key).cloned().unwrap_or_default();
        let bucket = assign_bucket(f.marker, f.ceiling, &effects);
        partitions.insert(f.key.clone(), bucket);
    }

    bundle.web_partitions = partitions.clone();

    if bundle.web_partition_enforced {
        bundle.web_partition_report = Some(format_partition_report(&metas, &partitions, solved));
    }

    if !web_partition_active(bundle) {
        return Vec::new();
    }

    let abi_idx = AbiTypeIndex::from_bundle(bundle);
    let mut diags = Vec::new();
    for f in &metas {
        let effects = solved.get(&f.key).cloned().unwrap_or_default();
        let bucket = partitions.get(&f.key).copied().unwrap_or(WebBucket::Wasm);
        check_abi_export(f, &abi_idx, &mut diags);
        check_target_browser(f, bucket, &effects, &mut diags);
    }

    let meta_by_key: HashMap<&str, &FuncWebMeta> =
        metas.iter().map(|m| (m.key.as_str(), m)).collect();

    for (caller_key, summary) in summaries {
        let Some(caller_bucket) = partitions.get(caller_key) else {
            continue;
        };
        let Some(caller_meta) = meta_by_key.get(caller_key.as_str()) else {
            continue;
        };
        for callee_key in &summary.edges {
            let Some(callee_bucket) = partitions.get(callee_key) else {
                continue;
            };
            if caller_bucket != callee_bucket {
                let callee_meta = meta_by_key.get(callee_key.as_str());
                let wasm_export_bridge = *caller_bucket == WebBucket::Js
                    && *callee_bucket == WebBucket::Wasm
                    && callee_meta.and_then(|m| m.marker) == Some(WebPartitionMarker::WasmExport);
                if wasm_export_bridge {
                    continue;
                }
                let callee_name = callee_key
                    .rsplit("__")
                    .next()
                    .unwrap_or(callee_key.as_str());
                diags.push(Syntax::web_cross_partition(
                    &caller_meta.key,
                    callee_name,
                    *caller_bucket,
                    *callee_bucket,
                    Some(caller_meta.name_span),
                ));
            }
        }
    }

    diags
}
