//! D-WASM1=A (c123 M1): classify functions into JS/WASM buckets and diagnose crossings.

use crate::Diagnostics::Diagnostic;
use crate::Generics::{DECODE, ENCODE};
use crate::Syntax::{self, WebBucket, WebPartitionMarker};
use crate::AST::{EnumDef, Func, Item, ProgramBundle, StructDef, Type, VariantPayload};
use jet_foundation::WebPartition::{partition_effect_key, partition_key};
use std::collections::HashMap;

use super::effect_key;
use super::effect_set_has_root;
use super::inline_effect_key;
use super::Effect;
use super::EffectSet;
use super::EffectSummary;

#[derive(Debug, Clone)]
pub(crate) struct FuncWebMeta {
    key: String,
    effect_key: String,
    name: String,
    name_span: crate::Diagnostics::Span,
    marker: Option<WebPartitionMarker>,
    ceiling: Option<WebBucket>,
    params: Vec<Type>,
    return_type: Option<Type>,
}

#[derive(Debug, Default)]
struct ABITypeIndex {
    structs: HashMap<String, StructDef>,
    enums: HashMap<String, EnumDef>,
}

impl ABITypeIndex {
    fn from_bundle(bundle: &ProgramBundle) -> Self {
        let mut out = Self::default();
        for module in &bundle.modules {
            collect_abi_types(&module.items, &mut out);
        }
        out
    }
}

fn collect_abi_types(items: &[Item], out: &mut ABITypeIndex) {
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
    let mut result = s.clone();
    result.methods.clear();
    result.trait_impls.clear();
    result
}

fn clone_enum(e: &crate::AST::EnumDef) -> EnumDef {
    let mut result = e.clone();
    result.methods.clear();
    result.trait_impls.clear();
    result
}

fn has_codable_derive(derives: &[(String, crate::Diagnostics::Span)]) -> bool {
    let mut encode = false;
    let mut decode = false;
    for (d, _) in derives {
        match d.as_str() {
            Syntax::MARKER_CODABLE => return true,
            d if d == ENCODE => encode = true,
            d if d == DECODE => decode = true,
            _ => {}
        }
    }
    encode && decode
}

fn is_abi_safe_type_full(ty: &Type, idx: &ABITypeIndex) -> bool {
    if Syntax::is_abi_safe_type(ty) {
        return true;
    }
    match ty {
        Type::Named(n) => idx.is_codable_named(n, &[]),
        Type::Apply { name, args } => idx.is_codable_named(name, args),
        _ => false,
    }
}

impl ABITypeIndex {
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

/// Apply the checked sema policy to one callable. All backends reuse this
/// precedence: explicit marker, Browser effect, inherited ceiling, Stream
/// producer, then the Wasm default.
pub fn checked_web_bucket(
    marker: Option<WebPartitionMarker>,
    ceiling: Option<WebBucket>,
    effects: &EffectSet,
    return_type: Option<&Type>,
) -> WebBucket {
    if let Some(m) = marker {
        return m.bucket();
    }
    if effect_set_has_root(effects, Effect::Browser) {
        return WebBucket::JS;
    }
    if let Some(c) = ceiling {
        return c;
    }
    // D-CONC-STREAM1=A / D-STREAMYIELD1: a `Stream<T>` is a resumable
    // producer. The browser adapter is JavaScript's generator protocol; keep
    // the default web bucket honest so an unmarked stream does not fall into
    // the synchronous Wasm ABI and silently lose yield/close behavior.
    if return_type.is_some_and(is_stream_type) {
        return WebBucket::JS;
    }
    WebBucket::Wasm
}

fn is_stream_type(ty: &Type) -> bool {
    matches!(ty, Type::Apply { name, args } if name == Syntax::TYPE_STREAM && args.len() == 1)
}

fn funcs_have_web_markers(functions: &[Func]) -> bool {
    functions.iter().any(|function| function.web_marker.is_some())
}

fn items_have_web_markers(items: &[Item]) -> bool {
    for item in items {
        match item {
            Item::Func(f) if f.web_marker.is_some() => return true,
            Item::Struct(s)
                if funcs_have_web_markers(&s.methods)
                    || s
                        .trait_impls
                        .iter()
                        .any(|implementation| funcs_have_web_markers(&implementation.methods)) =>
            {
                return true;
            }
            Item::Enum(e)
                if funcs_have_web_markers(&e.methods)
                    || e
                        .trait_impls
                        .iter()
                        .any(|implementation| funcs_have_web_markers(&implementation.methods)) =>
            {
                return true;
            }
            Item::Impl(implementation) if funcs_have_web_markers(&implementation.methods) => {
                return true;
            }
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
fn push_func_meta(
    function: &Func,
    owner_type: Option<&str>,
    file_ceiling: Option<WebBucket>,
    module_ceiling: Option<WebBucket>,
    module_prefix: Option<&str>,
    file_alias: &str,
    is_entry: bool,
    out: &mut Vec<FuncWebMeta>,
) {
    let ceiling = module_ceiling.or(file_ceiling);
    let local_key = match owner_type {
        Some(owner) => effect_key(Some(owner), &function.name),
        None => match module_prefix {
            Some(module) => inline_effect_key(module, &function.name),
            None => effect_key(None, &function.name),
        },
    };
    let function_name = owner_type
        .map(|owner| format!("{owner}::{}", function.name))
        .unwrap_or_else(|| function.name.clone());
    let file_prefix = (!is_entry && module_prefix.is_none()).then_some(file_alias);
    let key = partition_key(file_prefix, module_prefix, &function_name);
    out.push(FuncWebMeta {
        key,
        effect_key: partition_effect_key(file_alias, &local_key),
        name: function_name,
        name_span: function.name_span,
        marker: function.web_marker,
        ceiling,
        params: function.params.iter().map(|p| p.ty.clone()).collect(),
        return_type: function.return_type.clone(),
    });
}

fn collect_method_meta(
    methods: &[Func],
    owner_type: &str,
    file_ceiling: Option<WebBucket>,
    module_ceiling: Option<WebBucket>,
    module_prefix: Option<&str>,
    file_alias: &str,
    is_entry: bool,
    out: &mut Vec<FuncWebMeta>,
) {
    for method in methods {
        push_func_meta(
            method,
            Some(owner_type),
            file_ceiling,
            module_ceiling,
            module_prefix,
            file_alias,
            is_entry,
            out,
        );
    }
}

fn collect_funcs(
    items: &[Item],
    file_ceiling: Option<WebBucket>,
    module_ceiling: Option<WebBucket>,
    module_prefix: Option<&str>,
    file_alias: &str,
    is_entry: bool,
    out: &mut Vec<FuncWebMeta>,
) {
    let ceiling = module_ceiling.or(file_ceiling);
    for item in items {
        match item {
            Item::Func(function) => push_func_meta(
                function,
                None,
                file_ceiling,
                module_ceiling,
                module_prefix,
                file_alias,
                is_entry,
                out,
            ),
            Item::Struct(structure) => {
                collect_method_meta(
                    &structure.methods,
                    &structure.name,
                    file_ceiling,
                    module_ceiling,
                    module_prefix,
                    file_alias,
                    is_entry,
                    out,
                );
                for implementation in &structure.trait_impls {
                    collect_method_meta(
                        &implementation.methods,
                        &structure.name,
                        file_ceiling,
                        module_ceiling,
                        module_prefix,
                        file_alias,
                        is_entry,
                        out,
                    );
                }
            }
            Item::Enum(enumeration) => {
                collect_method_meta(
                    &enumeration.methods,
                    &enumeration.name,
                    file_ceiling,
                    module_ceiling,
                    module_prefix,
                    file_alias,
                    is_entry,
                    out,
                );
                for implementation in &enumeration.trait_impls {
                    collect_method_meta(
                        &implementation.methods,
                        &enumeration.name,
                        file_ceiling,
                        module_ceiling,
                        module_prefix,
                        file_alias,
                        is_entry,
                        out,
                    );
                }
            }
            Item::Impl(implementation) => collect_method_meta(
                &implementation.methods,
                &implementation.type_name,
                file_ceiling,
                module_ceiling,
                module_prefix,
                file_alias,
                is_entry,
                out,
            ),
            Item::CodeModule(cm) => {
                let mod_ceiling = cm.web_target.or(ceiling);
                if let Some(body) = &cm.body {
                    collect_funcs(
                        body,
                        file_ceiling,
                        mod_ceiling,
                        Some(&cm.name),
                        file_alias,
                        is_entry,
                        out,
                    );
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
        Type::Fn {
            params,
            ret,
            effect_bound,
            ..
        } => format!("Fn {{ params: {params:?}, ret: {ret:?}, effect_bound: {effect_bound:?} }}"),
        other => format!("{other:?}"),
    }
}

fn marker_show(marker: Option<WebPartitionMarker>) -> &'static str {
    match marker {
        Some(m) => m.name(),
        None => "—",
    }
}

fn reason_show(f: &FuncWebMeta, effects: &EffectSet) -> String {
    if let Some(m) = f.marker {
        return format!("#{}", marker_show(Some(m)));
    }
    if effect_set_has_root(effects, Effect::Browser) {
        return "inferred: Browser effect".to_string();
    }
    if let Some(c) = f.ceiling {
        return format!("#{}({})", Syntax::MARKER_TARGET, c.name());
    }
    if f.return_type.as_ref().is_some_and(is_stream_type) {
        return "inferred: Stream producer".to_string();
    }
    if f.name == "run" {
        return "entry".to_string();
    }
    "inferred: pure / no Browser effect".to_string()
}

fn effects_show(effects: &EffectSet) -> String {
    let mut names: Vec<&str> = effects.iter().map(|e| e.as_str()).collect();
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
        "Web partition report".to_string(),
        "================================".to_string(),
    ];
    let mut rows: Vec<_> = metas.iter().collect();
    rows.sort_by(|a, b| a.key.cmp(&b.key));
    for f in rows {
        let bucket = partitions.get(&f.key).copied().unwrap_or(WebBucket::Wasm);
        let effects = solved.get(&f.effect_key).cloned().unwrap_or_default();
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

fn check_abi_export(f: &FuncWebMeta, idx: &ABITypeIndex, diags: &mut Vec<Diagnostic>) {
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
    if bucket == WebBucket::Wasm && effect_set_has_root(effects, Effect::Browser) {
        diags.push(Syntax::web_target_browser(&f.name, Some(f.name_span)));
    }
}

/// The browser artifact has no argv-to-Jet adapter for a parameterized
/// `run`. Reject that checked CLI shape in sema before codegen instead of
/// allowing a CLI-only entry to reach Web emission.
fn check_web_entry_adapter(bundle: &ProgramBundle, diags: &mut Vec<Diagnostic>) {
    if !bundle.web_partition_enforced {
        return;
    }
    let Some(entry) = bundle.modules.get(bundle.entry) else {
        return;
    };
    let has_selected_executable = entry.items.iter().any(|item| {
        matches!(
            item,
            Item::Const(value)
                if value.resolved_output.as_ref().is_some_and(|output| {
                    output.selected
                        && output.kind == crate::AST::OutputKind::Executable
                })
        )
    });
    if has_selected_executable {
        return;
    }
    if jet_foundation::CLISchema::entry_schema_for_bundle(bundle).is_none() {
        return;
    }
    let Some(run) = entry.items.iter().find_map(|item| match item {
        Item::Func(function) if function.name == "run" => Some(function),
        _ => None,
    }) else {
        return;
    };
    if run.params.is_empty() {
        return;
    }
    diags.push(Diagnostic::from_row(
        "E-WEB-TIR-UNSUPPORTED",
        &[("fn", "run (typed CLI entry adapter)")],
        Some(run.name_span),
    ));
}

/// Walk the bundle, assign buckets, and emit partition / ABI diagnostics.
pub fn check_web_partition(
    bundle: &mut ProgramBundle,
    summaries: &HashMap<String, EffectSummary>,
    solved: &HashMap<String, EffectSet>,
) -> Vec<Diagnostic> {
    let mut metas = Vec::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        collect_funcs(
            &module.items,
            module.web_target_ceiling,
            None,
            None,
            &module.alias,
            module_idx == bundle.entry,
            &mut metas,
        );
    }

    let mut partitions: HashMap<String, WebBucket> = HashMap::new();
    for f in &metas {
        let effects = solved.get(&f.effect_key).cloned().unwrap_or_default();
        let bucket = checked_web_bucket(f.marker, f.ceiling, &effects, f.return_type.as_ref());
        partitions.insert(f.key.clone(), bucket);
    }

    // D-WEBTIR1 / D-CONC-STREAM1: a JS generator can only be consumed by JS,
    // and a JS caller cannot invoke an ordinary Wasm function without the
    // explicit WasmExport bridge. Promote unmarked functions along those call
    // edges until the partition graph is closed. Explicit #Target choices stay
    // authoritative and are reported by the existing crossing diagnostic.
    loop {
        let mut changed = false;
        for (caller_key, summary) in summaries {
            let Some(caller_meta) = metas
                .iter()
                .find(|meta| meta.effect_key.as_str() == caller_key.as_str())
            else {
                continue;
            };
            let caller_bucket = partitions
                .get(&caller_meta.key)
                .copied()
                .unwrap_or(WebBucket::Wasm);
            for callee_key in &summary.edges {
                let Some(callee_meta) = metas
                    .iter()
                    .find(|meta| meta.effect_key.as_str() == callee_key.as_str())
                else {
                    continue;
                };
                let callee_bucket = partitions
                    .get(&callee_meta.key)
                    .copied()
                    .unwrap_or(WebBucket::Wasm);
                if caller_bucket == WebBucket::Wasm
                    && callee_bucket == WebBucket::JS
                    && caller_meta.marker.is_none()
                    && caller_meta.ceiling.is_none()
                {
                    if partitions.insert(caller_meta.key.clone(), WebBucket::JS)
                        != Some(WebBucket::JS)
                    {
                        changed = true;
                    }
                } else if caller_bucket == WebBucket::JS
                    && callee_bucket == WebBucket::Wasm
                    && callee_meta.marker.is_none()
                    && callee_meta.ceiling.is_none()
                {
                    if partitions.insert(callee_meta.key.clone(), WebBucket::JS)
                        != Some(WebBucket::JS)
                    {
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    bundle.web_partitions = partitions.clone();

    if bundle.web_partition_enforced {
        bundle.web_partition_report = Some(format_partition_report(&metas, &partitions, solved));
    }

    if !web_partition_active(bundle) {
        return Vec::new();
    }

    let abi_idx = ABITypeIndex::from_bundle(bundle);
    let mut diags = Vec::new();
    check_web_entry_adapter(bundle, &mut diags);
    for f in &metas {
        let effects = solved.get(&f.effect_key).cloned().unwrap_or_default();
        let bucket = partitions.get(&f.key).copied().unwrap_or(WebBucket::Wasm);
        check_abi_export(f, &abi_idx, &mut diags);
        check_target_browser(f, bucket, &effects, &mut diags);
    }

    let meta_by_effect_key: HashMap<&str, &FuncWebMeta> =
        metas.iter().map(|m| (m.effect_key.as_str(), m)).collect();

    for (caller_key, summary) in summaries {
        let Some(caller_meta) = meta_by_effect_key.get(caller_key.as_str()) else {
            continue;
        };
        let caller_bucket = &partitions[&caller_meta.key];
        for callee_key in &summary.edges {
            let Some(callee_meta) = meta_by_effect_key.get(callee_key.as_str()) else {
                continue;
            };
            let callee_bucket = &partitions[&callee_meta.key];
            if caller_bucket != callee_bucket {
                let wasm_export_bridge = *caller_bucket == WebBucket::JS
                    && *callee_bucket == WebBucket::Wasm
                    && callee_meta.marker == Some(WebPartitionMarker::WasmExport);
                if wasm_export_bridge {
                    continue;
                }
                diags.push(Syntax::web_cross_partition(
                    &caller_meta.key,
                    &callee_meta.name,
                    *caller_bucket,
                    *callee_bucket,
                    Some(caller_meta.name_span),
                ));
            }
        }
    }

    diags
}
