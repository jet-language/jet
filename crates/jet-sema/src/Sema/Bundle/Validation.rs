use super::*;
use crate::AST::Param;
use std::collections::BTreeSet;

mod CoreUsage;
mod BodyCheck;
use BodyCheck::*;
pub(crate) use BodyCheck::{
    checker_for_module, fn_types_compatible, func_sig_to_fn_type,
};
pub(crate) use BodyCheck::{collect_diverging_functions, project_divergence_facts};
pub(crate) use CoreUsage::{
    apply_helper_layer_inference, check_ui_capabilities, collect_core_expr, collect_core_lvalue,
    collect_core_stmts, collect_used_core, expand_core_reachable_closure,
};

pub(crate) fn uses_raw_protocol_return(
    trait_name: Option<&str>,
    implementation_generated: bool,
    function_generated: bool,
) -> bool {
    // D-FAILURE-FOUNDATION1: these traits are raw Rust protocol ABIs (String,
    // bool, Ordering, DataTree, and same-type arithmetic) — never the implicit
    // Outcome carrier. A wrapped body would violate the protocol signature for
    // user and derived impls alike (rustc E0053/E0308 behind I2).
    trait_name.is_some_and(|name| {
        matches!(
            name,
            crate::Generics::DISPLAY
                | crate::Generics::DEBUG
                | crate::Generics::ENCODE
                | crate::Generics::DECODE
                | crate::Generics::EQUATABLE
                | crate::Generics::COMPARABLE
                | crate::Generics::ADD
                | crate::Generics::SUB
                | crate::Generics::MUL
                | crate::Generics::DIV
        )
    }) || implementation_generated && function_generated
}

fn shift_span(span: Span, delta: isize) -> Span {
    Span::new(
        (span.start as isize + delta).max(0) as usize,
        (span.end as isize + delta).max(0) as usize,
    )
}

/// Replay a cached function onto the current parse. Sibling body-length
/// edits shift later functions without changing their relative shape, so the
/// cache key still hits; identity spans must come from this parse or
/// liveness treats `run` as a non-root private function (L0104).
pub(super) fn rebase_cached_function(parsed: &Func, mut cached: Func) -> Func {
    cached.span = parsed.span;
    cached.name_span = parsed.name_span;
    cached.return_type_span = parsed.return_type_span;
    for (cached_param, parsed_param) in cached.params.iter_mut().zip(&parsed.params) {
        if cached_param.name == parsed_param.name {
            cached_param.name_span = parsed_param.name_span;
            cached_param.ty_span = parsed_param.ty_span;
        }
    }
    cached
}

pub(super) fn shift_diagnostics(diagnostics: &mut [Diagnostic], delta: isize) {
    if delta == 0 {
        return;
    }
    for diagnostic in diagnostics {
        if let Some(span) = &mut diagnostic.span {
            *span = shift_span(*span, delta);
        }
        if let Some(edit) = &mut diagnostic.edit {
            edit.span = shift_span(edit.span, delta);
        }
        for cause in &mut diagnostic.cause {
            if let Some(span) = &mut cause.span {
                *span = shift_span(*span, delta);
            }
        }
    }
}

pub(super) fn shift_pending_diagnostics(
    pending: &mut [PendingFunctionDiagnostic],
    delta: isize,
) {
    if delta == 0 {
        return;
    }
    for item in pending {
        item.function_span = shift_span(item.function_span, delta);
        shift_diagnostics(std::slice::from_mut(&mut item.diagnostic), delta);
    }
}

pub(super) fn function_cache_debug(function: &Func) -> String {
    relativize_debug_spans(&format!("{function:?}"), function.span.start)
}

fn relativize_debug_spans(input: &str, origin: usize) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut copy_from = 0usize;
    let mut i = 0usize;
    const MARK: &[u8] = b"Span { start: ";
    const END_MARK: &[u8] = b", end: ";
    while i < bytes.len() {
        match bytes[i] {
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i];
                    if c == b'\\' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    if c == q {
                        break;
                    }
                }
            }
            b'S' if bytes[i..].starts_with(MARK) => {
                out.push_str(&input[copy_from..i]);
                i += MARK.len();
                let start_from = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let start: usize = input[start_from..i].parse().unwrap_or(0);
                if !bytes[i..].starts_with(END_MARK) {
                    copy_from = start_from;
                    continue;
                }
                i += END_MARK.len();
                let end_from = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let end: usize = input[end_from..i].parse().unwrap_or(0);
                if i < bytes.len() && bytes[i] == b'}' {
                    i += 1;
                }
                out.push_str(&format!(
                    "Span {{ start: {}, end: {} }}",
                    start.saturating_sub(origin),
                    end.saturating_sub(origin)
                ));
                copy_from = i;
            }
            _ => i += 1,
        }
    }
    out.push_str(&input[copy_from..]);
    out
}

pub(super) fn qualified_effect_facts(
    modules: &[(String, HashMap<String, EffectSummary>)],
    taint_seeds: &HashMap<String, BTreeSet<String>>,
) -> (
    HashMap<String, EffectSummary>,
    jet_foundation::Facts::ReachabilityResult,
) {
    let mut locations = HashMap::<String, Vec<String>>::new();
    let aliases = modules
        .iter()
        .map(|(alias, _)| alias.as_str())
        .collect::<HashSet<_>>();
    for (alias, summaries) in modules {
        for key in summaries.keys() {
            locations
                .entry(key.clone())
                .or_default()
                .push(format!("{alias}::{key}"));
        }
    }
    let mut qualified = HashMap::new();
    for (alias, summaries) in modules {
        let local_keys: HashSet<String> = summaries.keys().cloned().collect();
        for (key, summary) in summaries {
            let mut summary = summary.clone();
            let resolve_edge = |edge: &String| {
                if edge == "__jet_panic__" {
                    return edge.clone();
                }
                if local_keys.contains(edge) {
                    return format!("{alias}::{edge}");
                }
                if let Some((module, symbol)) = edge.split_once('.') {
                    if aliases.contains(module) {
                        return format!("{module}::{symbol}");
                    }
                }
                locations
                    .get(edge)
                    .and_then(|values| (values.len() == 1).then(|| values[0].clone()))
                    .unwrap_or_else(|| edge.clone())
            };
            summary.edges = summary.edges.iter().map(&resolve_edge).collect();
            for region in &mut summary.regions {
                region.edges = region.edges.iter().map(&resolve_edge).collect();
            }
            for obligation in &mut summary.callback_obligations {
                obligation.edges = obligation.edges.iter().map(&resolve_edge).collect();
            }
            for obligation in &mut summary.autodiff_obligations {
                obligation.target = resolve_edge(&obligation.target);
            }
            for call in &mut summary.memory.calls {
                call.callee = resolve_edge(&call.callee);
            }
            for region in &mut summary.memory.regions {
                region.edges = region.edges.iter().map(&resolve_edge).collect();
                for call in &mut region.calls {
                    call.callee = resolve_edge(&call.callee);
                }
            }
            qualified.insert(format!("{alias}::{key}"), summary);
        }
    }
    let mut reachability = solve_reachability(&qualified, taint_seeds);
    for (short, values) in locations.iter().filter(|(_, values)| values.len() == 1) {
        let qualified_key = &values[0];
        if let Some(summary) = qualified.get(qualified_key).cloned() {
            qualified.insert(short.clone(), summary);
        }
        reachability.copy_node(short, qualified_key);
    }
    (qualified, reachability)
}

#[cfg(test)]
mod effect_qualification_tests {
    use super::*;

    #[test]
    fn nested_region_and_callback_edges_are_module_qualified() {
        let root = EffectSummary {
            regions: vec![RegionSummary {
                caps: EffectSet::new(),
                direct: EffectSet::new(),
                edges: ["left.same".to_string()].into_iter().collect(),
                maximal: false,
                caps_span: Span::new(1, 2),
            }],
            callback_obligations: vec![CallbackObligation {
                bound: EffectSet::new(),
                direct: EffectSet::new(),
                edges: ["right.same".to_string()].into_iter().collect(),
                maximal: false,
                span: Span::new(3, 4),
            }],
            ..Default::default()
        };
        let modules = vec![
            (
                "main".to_string(),
                HashMap::from([("root".to_string(), root)]),
            ),
            (
                "left".to_string(),
                HashMap::from([("same".to_string(), EffectSummary::default())]),
            ),
            (
                "right".to_string(),
                HashMap::from([("same".to_string(), EffectSummary::default())]),
            ),
        ];

        let (summaries, _) = qualified_effect_facts(&modules, &HashMap::new());
        let root = &summaries["main::root"];
        assert_eq!(
            root.regions[0].edges,
            EffectSet::from(["left::same".to_string()])
        );
        assert_eq!(
            root.callback_obligations[0].edges,
            EffectSet::from(["right::same".to_string()])
        );
    }
}

/// D-TAINT1: run the taint pass over one item's function/method bodies in the
/// bundle path, using `core_imports` to classify sink calls.
pub(super) fn taint_check_item(
    item: &Item,
    scrubbers: &HashMap<String, String>,
    facts: &jet_foundation::Facts::FactRegistry,
    returns: &HashMap<String, crate::Sema::Taint::TagSet>,
    return_types: &crate::Sema::Taint::ReturnTypes,
    field_tags: &crate::Sema::Taint::FieldTags,
    field_types: &crate::Sema::Taint::FieldTypes,
    core_imports: &HashMap<String, String>,
    diags: &mut Vec<Diagnostic>,
) {
    match item {
        Item::Func(f) => {
            let new = check_func_taint(
                f,
                None,
                scrubbers,
                facts,
                returns,
                return_types,
                field_tags,
                field_types,
                core_imports,
                diags.as_slice(),
            );
            diags.extend(new);
        }
        Item::Impl(i) => {
            for m in &i.methods {
                let new = check_func_taint(
                    m,
                    Some(&i.type_name),
                    scrubbers,
                    facts,
                    returns,
                    return_types,
                    field_tags,
                    field_types,
                    core_imports,
                    diags.as_slice(),
                );
                diags.extend(new);
            }
        }
        Item::Struct(s) => {
            for m in &s.methods {
                let new = check_func_taint(
                    m,
                    Some(&s.name),
                    scrubbers,
                    facts,
                    returns,
                    return_types,
                    field_tags,
                    field_types,
                    core_imports,
                    diags.as_slice(),
                );
                diags.extend(new);
            }
            for block in &s.trait_impls {
                for m in &block.methods {
                    let new = check_func_taint(
                        m,
                        Some(&s.name),
                        scrubbers,
                        facts,
                        returns,
                        return_types,
                        field_tags,
                        field_types,
                        core_imports,
                        diags.as_slice(),
                    );
                    diags.extend(new);
                }
            }
        }
        Item::Enum(e) => {
            for m in &e.methods {
                let new = check_func_taint(
                    m,
                    Some(&e.name),
                    scrubbers,
                    facts,
                    returns,
                    return_types,
                    field_tags,
                    field_types,
                    core_imports,
                    diags.as_slice(),
                );
                diags.extend(new);
            }
        }
        Item::Test(t) => {
            let new = crate::Sema::Taint::check_body_tags(
                &t.body,
                scrubbers,
                facts,
                returns,
                return_types,
                field_tags,
                field_types,
                core_imports,
                diags.as_slice(),
            );
            diags.extend(new);
        }
        Item::ErrorConv(ec) => {
            let new = crate::Sema::Taint::check_body_tags(
                &ec.body,
                scrubbers,
                facts,
                returns,
                return_types,
                field_tags,
                field_types,
                core_imports,
                diags.as_slice(),
            );
            diags.extend(new);
        }
        _ => {}
    }
}

pub(crate) fn register_func_item(
    f: &Func,
    st: &mut ModuleState,
    diags: &mut Vec<Diagnostic>,
    prelude_enabled: bool,
) {
    if f.name == Syntax::BUILTIN_ASSERT_EQ {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", f.name),
            format!("`{}` is provided by the language itself", f.name),
            "choose a different name for this function".to_string(),
            Some(f.name_span),
        ));
        return;
    }
    if prelude_enabled && crate::Sema::Prelude::is_prelude_name(&f.name) {
        diags.push(crate::Sema::Prelude::shadow_warning(&f.name, f.name_span));
    }
    if name_defined(&f.name, &st.funcs, &st.registry, &st.consts) {
        diags.push(Diagnostic::error(
            "E0105",
            format!("`{}` is defined twice", f.name),
            "every function needs a unique name so calls aren't ambiguous".to_string(),
            "rename or remove one of the definitions".to_string(),
            Some(f.name_span),
        ));
        return;
    }
    // L2401: advisory — public fn with a positional Bool parameter.
    if f.is_pub {
        for p in &f.params {
            if matches!(p.ty, Type::Bool) && p.name != Syntax::KW_SELF && p.default.is_none() {
                diags.push(Diagnostic::lint(
                    "L2401",
                    format!(
                        "public function `{}` has a positional `Bool` parameter `{}`",
                        f.name, p.name
                    ),
                    "positional booleans are easy to transpose at the call site".to_string(),
                    format!(
                        "callers can write `{}: true` to make the intent clear (S61 labels)",
                        p.name
                    ),
                    Some(p.name_span),
                ));
            }
        }
    }
    // E0126: check defaults don't reference later params.
    check_default_forward_refs(&f.params, &f.name, diags);
    st.funcs.insert(f.name.clone(), func_to_sig(f));
}

/// Core value/container + opaque-handle type names backed by `jet_std`.
/// Naming one in an annotation needs the Core prelude
/// even without a method call for the expression walker to observe.
fn is_encoding_surface_type(name: &str) -> bool {
    // Annotations may spell the type module-qualified (`encoding.EncodingError`,
    // `json.JSONReader`); match on the final path segment.
    let base = name.rsplit('.').next().unwrap_or(name);
    matches!(
        base,
        "DataTree"
            | "EncodingFormat"
            | "DataJoin"
            | "Query"
            | "DataGroupedQuery"
            | "Group"
            | "EncodingLimits"
            | "EncodingError"
            | "CBOROptions"
            | "CBORError"
            | "CBORErrorKind"
            | "EncodingCause"
            | "EncodingErrorKind"
            | "DataEvent"
            | "JSONReader"
            | "JSONWriter"
            | "JSONLReader"
            | "JSONLWriter"
            | "CSVReader"
            | "CSVWriter"
            | "XMLReader"
            | "XMLWriter"
            | "CBORReader"
            | "CBORWriter"
    )
}

/// True when `ty` (or any type nested inside it) names a `core.encoding` surface
/// type. Recurses through every type-carrying `Type` variant.
fn type_mentions_encoding_surface(ty: &Type) -> bool {
    match ty {
        Type::Named(name) => is_encoding_surface_type(name),
        Type::Apply { name, args } => {
            is_encoding_surface_type(name) || args.iter().any(type_mentions_encoding_surface)
        }
        Type::TraitObject(names) => names.iter().any(|n| is_encoding_surface_type(n)),
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::Tagged { inner, .. }
        | Type::InlineRange { base: inner, .. } => type_mentions_encoding_surface(inner),
        Type::FixedList { elem, .. } => type_mentions_encoding_surface(elem),
        Type::Map { key, value, .. } => {
            type_mentions_encoding_surface(key) || type_mentions_encoding_surface(value)
        }
        Type::Result { ok, err } => {
            type_mentions_encoding_surface(ok) || type_mentions_encoding_surface(err)
        }
        Type::Fn { params, ret, .. } => {
            params.iter().any(type_mentions_encoding_surface)
                || ret.as_deref().is_some_and(type_mentions_encoding_surface)
        }
        Type::Tuple(fields) => fields
            .iter()
            .any(|(_, t)| type_mentions_encoding_surface(t)),
        Type::Union(members) => members.iter().any(type_mentions_encoding_surface),
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::IntN { .. }
        | Type::Float32 => false,
        Type::Quantity { base, .. } => type_mentions_encoding_surface(base),
        Type::Measure(_) => false,
    }
}

/// A function/method signature (params + return) names an encoding surface type.
fn func_sig_mentions_encoding_surface(f: &Func) -> bool {
    f.params
        .iter()
        .any(|p| type_mentions_encoding_surface(&p.ty))
        || f.return_type
            .as_ref()
            .is_some_and(type_mentions_encoding_surface)
}

/// Scan every annotation position in a module for a `core.encoding` surface type
/// (struct fields, enum payloads, function/method/trait signatures, type-alias
/// targets, associated-type impls). Runtime usage always constructs handles via
/// a format-module call the expression walker already sees; this only covers the
/// annotation-only case (a signature that names a handle constructed elsewhere).
fn module_annotations_mention_encoding_surface(module: &crate::AST::LoadedModule) -> bool {
    fn variant_payload_mentions(payload: &VariantPayload) -> bool {
        match payload {
            VariantPayload::Unit => false,
            VariantPayload::Single(ty, _) => type_mentions_encoding_surface(ty),
            VariantPayload::Named(fields) => {
                fields.iter().any(|f| type_mentions_encoding_surface(&f.ty))
            }
        }
    }
    module.items.iter().any(|item| match item {
        Item::Func(f) => func_sig_mentions_encoding_surface(f),
        Item::Struct(s) => {
            s.fields
                .iter()
                .any(|f| type_mentions_encoding_surface(&f.ty))
                || s.methods.iter().any(func_sig_mentions_encoding_surface)
                || s.trait_impls
                    .iter()
                    .any(|b| b.methods.iter().any(func_sig_mentions_encoding_surface))
        }
        Item::Enum(e) => {
            e.variants
                .iter()
                .any(|v| variant_payload_mentions(&v.payload))
                || e.methods.iter().any(func_sig_mentions_encoding_surface)
                || e.trait_impls
                    .iter()
                    .any(|b| b.methods.iter().any(func_sig_mentions_encoding_surface))
        }
        Item::Impl(i) => {
            i.methods.iter().any(func_sig_mentions_encoding_surface)
                || i.assoc_type_impls
                    .iter()
                    .any(|(_, _, ty)| type_mentions_encoding_surface(ty))
        }
        Item::Trait(t) => t.methods.iter().any(|m| {
            m.params
                .iter()
                .any(|p| type_mentions_encoding_surface(&p.ty))
                || m.return_type
                    .as_ref()
                    .is_some_and(type_mentions_encoding_surface)
        }),
        Item::TypeAlias(a) => type_mentions_encoding_surface(&a.target),
        _ => false,
    })
}

#[allow(clippy::too_many_arguments)]
fn check_func_body_incremental(
    key: String,
    function: &mut Func,
    module_idx: usize,
    states: &[ModuleState],
    plugin_interfaces: &PluginInterfaceRegistry,
    devtools_registry: &jet_foundation::AST::DevtoolsRegistry,
    effect_facts: &jet_foundation::Facts::FactRegistry,
    owner_type: Option<&str>,
    raw_protocol_return: bool,
    ct_funcs: &HashMap<String, Func>,
    ct_checked_funcs: &HashMap<String, Func>,
    ct_externs: &HashSet<String>,
    ct_base_dir: &std::path::Path,
    ct_globals: &HashMap<String, crate::Comptime::CtValue>,
    no_os: bool,
    gates: crate::Policy::GateSet,
    summaries: &mut HashMap<String, EffectSummary>,
    embed_inputs_out: &mut Vec<crate::AST::ComptimeInput>,
    global_addr_taken: &mut HashSet<String>,
    no_prelude: bool,
    name_ledger: &mut jet_foundation::Names::NameLedger,
    pending_diagnostics_out: &mut Vec<PendingFunctionDiagnostic>,
    mut cache: Option<&mut IncrementalSemaCache>,
    cache_allowed: bool,
) -> Vec<Diagnostic> {
    let cache_allowed = cache_allowed && !stmts_have_comptime_evaluation(&function.body);
    let Some(cache) = cache.as_deref_mut().filter(|_| cache_allowed) else {
        if let Some(cache) = cache.as_deref_mut() {
            cache.record_recompute(key);
        }
        return check_func_body_bundle_checked(
            function,
            module_idx,
            states,
            plugin_interfaces,
            devtools_registry,
            effect_facts,
            owner_type,
            raw_protocol_return,
            ct_funcs,
            ct_checked_funcs,
            states[module_idx].items.as_slice(),
            ct_externs,
            ct_base_dir,
            ct_globals,
            no_os,
            gates,
            summaries,
            embed_inputs_out,
            global_addr_taken,
            no_prelude,
            name_ledger,
            pending_diagnostics_out,
        );
    };
    // The checked function contains source spans used by diagnostics and IDE
    // facts. Include them in the cache input so whitespace-only edits cannot
    // reuse stale positions even when the canonical AST is unchanged. Build
    // this recursive Debug form only when the caller can actually use the
    // cache: deep fluent expressions can exceed the ordinary test-thread stack,
    // and disabled-cache checks have no fingerprint consumer.
    let mut input = function_cache_debug(function).into_bytes();
    input.push(if raw_protocol_return { 1 } else { 0 });
    if let Some(hit) = cache.get(&key, &input) {
        if hit.uses_exact_int {
            states[module_idx].exact_int_reachable.set(true);
        }
        let delta = function.span.start as isize - hit.function.span.start as isize;
        *function = rebase_cached_function(function, hit.function);
        summaries.extend(hit.summaries);
        embed_inputs_out.extend(hit.comptime_inputs);
        global_addr_taken.extend(hit.address_taken);
        name_ledger.merge_references(&hit.name_ledger);
        name_ledger.merge_structure_facts(&hit.name_ledger);
        let mut pending = hit.pending_diagnostics;
        shift_pending_diagnostics(&mut pending, delta);
        pending_diagnostics_out.extend(pending);
        let mut diagnostics = hit.diagnostics;
        shift_diagnostics(&mut diagnostics, delta);
        return diagnostics;
    }

    let mut local_summaries = HashMap::new();
    let mut local_inputs = Vec::new();
    let mut local_address_taken = HashSet::new();
    let mut local_ledger = name_ledger.body_snapshot();
    let mut local_pending_diagnostics = Vec::new();
    let (diagnostics, uses_exact_int) = check_func_body_bundle_with_usage_checked(
        function,
        module_idx,
        states,
        plugin_interfaces,
        devtools_registry,
        effect_facts,
        owner_type,
        raw_protocol_return,
        ct_funcs,
        ct_checked_funcs,
        states[module_idx].items.as_slice(),
        ct_externs,
        ct_base_dir,
        ct_globals,
        no_os,
        gates,
        &mut local_summaries,
        &mut local_inputs,
        &mut local_address_taken,
        no_prelude,
        &mut local_ledger,
        &mut local_pending_diagnostics,
    );
    summaries.extend(local_summaries.clone());
    embed_inputs_out.extend(local_inputs.clone());
    global_addr_taken.extend(local_address_taken.clone());
    name_ledger.merge_references(&local_ledger);
    name_ledger.merge_structure_facts(&local_ledger);
    pending_diagnostics_out.extend(local_pending_diagnostics.clone());
    if !local_inputs.is_empty() {
        cache.record_recompute(key);
        return diagnostics;
    }
    cache.store(
        key,
        CachedFunctionBody {
            input,
            function: function.clone(),
            diagnostics: diagnostics.clone(),
            summaries: local_summaries,
            comptime_inputs: local_inputs,
            address_taken: local_address_taken,
            name_ledger: local_ledger,
            pending_diagnostics: local_pending_diagnostics,
            uses_exact_int,
        },
    );
    diagnostics
}

/// I4: a diagnostic is a product, and a lint is advice addressed to whoever
/// wrote the code. A compiler-generated derive body has no author and no
/// user-typeable span — every statement in it carries the owning declaration's
/// name span (`Registration/Derives.rs` builds `self.f == rhs.f` at
/// `s.name_span`), so a lint from that body names a construct the user never
/// wrote at a line that cannot contain the defect. `L0502` did exactly that for
/// any struct or enum with a `Float` field, because the package auto-derive
/// default gives every type an `Equatable.equal` body.
///
/// `TraitImplBlock::compiler_generated` is the parser-unforgeable provenance
/// (a source `impl T.Trait` is always `false`; jet-foundation `AST/items.rs`),
/// and it is the only fact consulted here — no second gate, table, or lint
/// exemption list (I8). Errors are untouched: a generated body that fails to
/// type-check is still an internal compiler fault, not a silent pass.
fn author_facing_diagnostics(
    compiler_generated: bool,
    mut diagnostics: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    if compiler_generated {
        diagnostics.retain(|d| matches!(d.severity, crate::Diagnostics::Severity::Error));
    }
    diagnostics
}

fn has_callable_policy(function: &crate::AST::Func) -> bool {
    function.markers.iter().any(|marker| {
        marker.name == Syntax::MARKER_POLICY
            && crate::AST::CallablePolicyChain::parse(&marker.expr_args_owned()).is_ok()
    })
}

fn collect_callable_policy_functions(
    functions: impl IntoIterator<Item = crate::AST::Func>,
    targets: &mut Vec<crate::AST::Func>,
) {
    targets.extend(functions.into_iter().filter(has_callable_policy));
}

fn callable_policy_targets(items: &[crate::AST::Item]) -> Vec<crate::AST::Func> {
    let mut targets = Vec::new();
    for item in items {
        match item {
            Item::Func(function) if has_callable_policy(function) => targets.push(function.clone()),
            Item::Struct(definition) => {
                collect_callable_policy_functions(definition.methods.clone(), &mut targets);
                for implementation in &definition.trait_impls {
                    collect_callable_policy_functions(implementation.methods.clone(), &mut targets);
                }
            }
            Item::Enum(definition) => {
                collect_callable_policy_functions(definition.methods.clone(), &mut targets);
                for implementation in &definition.trait_impls {
                    collect_callable_policy_functions(implementation.methods.clone(), &mut targets);
                }
            }
            Item::Impl(implementation) => {
                collect_callable_policy_functions(implementation.methods.clone(), &mut targets);
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    targets.extend(callable_policy_targets(body));
                }
            }
            Item::GenericModule(module) => targets.extend(callable_policy_targets(&module.body)),
            _ => {}
        }
    }
    targets
}

/// D-STRUCT-POLICY1=A: `apply(user_policy(...), target)` is a checked
/// callable transformation even when `target` has no `#Policy` marker of its
/// own. Record the named targets from the already-checked call arguments so
/// they use the same generated wrapper seam as marker-decorated targets.
fn collect_callable_policy_apply_uses(
    items: &mut [crate::AST::Item],
    uses: &mut HashMap<String, Vec<String>>,
) {
    fn collect_func(function: &mut crate::AST::Func, uses: &mut HashMap<String, Vec<String>>) {
        for statement in &mut function.body {
            statement.for_each_expr_mut(|expr| {
                let crate::AST::Expr::Call(call) = expr else {
                    return;
                };
                if call.name != "apply" {
                    return;
                }
                let Some(last) = call.args.last() else {
                    return;
                };
                let Some(chain) = last.flags.callable_policy.as_ref() else {
                    return;
                };
                let target_name = match &last.expr {
                    crate::AST::Expr::Ident(name, _) => Some(name.clone()),
                    crate::AST::Expr::Paren(inner, _) => match inner.as_ref() {
                        crate::AST::Expr::Ident(name, _) => Some(name.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                let Some(target_name) = target_name else {
                    return;
                };
                let policy_names = chain
                    .policies
                    .iter()
                    .filter(|policy| !crate::AST::CallablePolicyChain::is_builtin(&policy.name))
                    .map(|policy| policy.name.clone())
                    .collect::<Vec<_>>();
                if policy_names.is_empty() {
                    return;
                }
                let entry = uses.entry(target_name).or_default();
                for policy_name in policy_names {
                    if !entry.contains(&policy_name) {
                        entry.push(policy_name);
                    }
                }
            });
        }
    }

    for item in items {
        match item {
            crate::AST::Item::Func(function) => collect_func(function, uses),
            crate::AST::Item::Struct(definition) => {
                for function in &mut definition.methods {
                    collect_func(function, uses);
                }
                for implementation in &mut definition.trait_impls {
                    for function in &mut implementation.methods {
                        collect_func(function, uses);
                    }
                }
            }
            crate::AST::Item::Enum(definition) => {
                for function in &mut definition.methods {
                    collect_func(function, uses);
                }
                for implementation in &mut definition.trait_impls {
                    for function in &mut implementation.methods {
                        collect_func(function, uses);
                    }
                }
            }
            crate::AST::Item::Impl(implementation) => {
                for function in &mut implementation.methods {
                    collect_func(function, uses);
                }
            }
            crate::AST::Item::CodeModule(module) => {
                if let Some(body) = &mut module.body {
                    collect_callable_policy_apply_uses(body, uses);
                }
            }
            crate::AST::Item::GenericModule(module) => {
                collect_callable_policy_apply_uses(&mut module.body, uses)
            }
            _ => {}
        }
    }
}

fn collect_named_callable_policy_functions<'a>(
    functions: impl IntoIterator<Item = &'a crate::AST::Func>,
    names: &HashSet<String>,
    targets: &mut Vec<crate::AST::Func>,
) {
    targets.extend(
        functions
            .into_iter()
            .filter(|function| names.contains(&function.name))
            .cloned(),
    );
}

fn named_callable_policy_targets(
    items: &[crate::AST::Item],
    names: &HashSet<String>,
    targets: &mut Vec<crate::AST::Func>,
) {
    for item in items {
        match item {
            crate::AST::Item::Func(function) if names.contains(&function.name) => {
                targets.push(function.clone())
            }
            crate::AST::Item::Struct(definition) => {
                collect_named_callable_policy_functions(&definition.methods, names, targets);
                for implementation in &definition.trait_impls {
                    collect_named_callable_policy_functions(
                        &implementation.methods,
                        names,
                        targets,
                    );
                }
            }
            crate::AST::Item::Enum(definition) => {
                collect_named_callable_policy_functions(&definition.methods, names, targets);
                for implementation in &definition.trait_impls {
                    collect_named_callable_policy_functions(
                        &implementation.methods,
                        names,
                        targets,
                    );
                }
            }
            crate::AST::Item::Impl(implementation) => {
                collect_named_callable_policy_functions(&implementation.methods, names, targets);
            }
            crate::AST::Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    named_callable_policy_targets(body, names, targets);
                }
            }
            crate::AST::Item::GenericModule(module) => {
                named_callable_policy_targets(&module.body, names, targets)
            }
            _ => {}
        }
    }
}

/// D-STRUCT-POLICY1=A: one checked wrapper function is emitted for each
/// policy/target pair. The name is compiler-private and deterministic so the
/// shared TIR can call it without carrying policy semantics in an engine.
pub(crate) fn callable_policy_wrapper_name(policy: &str, target: &str) -> String {
    crate::AST::CallablePolicyChain::user_wrapper_name(policy, target)
}

#[derive(Clone)]
struct ComptimeStageJob {
    owner: Option<String>,
    raw_protocol_return: bool,
    function: Func,
}

fn comptime_stage_jobs(items: &[Item]) -> HashMap<String, ComptimeStageJob> {
    let mut jobs = HashMap::new();
    let mut insert = |key: String, owner: Option<String>, raw_protocol_return: bool, function: &Func| {
        jobs.insert(
            key,
            ComptimeStageJob {
                owner,
                raw_protocol_return,
                function: function.clone(),
            },
        );
    };
    for item in items {
        match item {
            Item::Func(function) => {
                insert(function.name.clone(), None, false, function);
            }
            Item::Struct(definition) => {
                for function in &definition.methods {
                    insert(
                        format!("{}::{}", definition.name, function.name),
                        Some(definition.name.clone()),
                        false,
                        function,
                    );
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        insert(
                            format!("{}::{}", definition.name, function.name),
                            Some(definition.name.clone()),
                            uses_raw_protocol_return(
                                Some(&implementation.trait_name),
                                implementation.compiler_generated,
                                function.compiler_generated,
                            ),
                            function,
                        );
                    }
                }
            }
            Item::Enum(definition) => {
                for function in &definition.methods {
                    insert(
                        format!("{}::{}", definition.name, function.name),
                        Some(definition.name.clone()),
                        false,
                        function,
                    );
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        insert(
                            format!("{}::{}", definition.name, function.name),
                            Some(definition.name.clone()),
                            uses_raw_protocol_return(
                                Some(&implementation.trait_name),
                                implementation.compiler_generated,
                                function.compiler_generated,
                            ),
                            function,
                        );
                    }
                }
            }
            Item::Impl(implementation) => {
                for function in &implementation.methods {
                    insert(
                        format!("{}::{}", implementation.type_name, function.name),
                        Some(implementation.type_name.clone()),
                        uses_raw_protocol_return(
                            implementation.trait_name.as_deref(),
                            false,
                            function.compiler_generated,
                        ),
                        function,
                    );
                }
            }
            _ => {}
        }
    }
    jobs
}

fn comptime_stage_roots(
    items: &[Item],
    raw_funcs: &HashMap<String, Func>,
) -> (HashSet<String>, bool) {
    let mut names = HashSet::new();
    let mut all_methods = false;
    let mut visit_function = |function: &Func| {
        if !stmts_have_comptime_evaluation(&function.body) {
            return;
        }
        for statement in &function.body {
            statement.for_each_expr(|expression| {
                let is_method = matches!(expression, Expr::MethodCall { .. });
                if is_method
                    && matches!(
                        expression,
                        Expr::MethodCall {
                            recv_type: None, ..
                        }
                    )
                {
                    all_methods = true;
                }
                if matches!(
                    expression,
                    Expr::Call(..) | Expr::Ident(..) | Expr::MethodCall { .. }
                ) {
                    names.extend(
                        crate::Comptime::reachable_owned_function_names(expression, raw_funcs),
                    );
                }
            });
        }
    };
    for item in items {
        match item {
            Item::Func(function) => visit_function(function),
            Item::Struct(definition) => {
                definition.methods.iter().for_each(&mut visit_function);
                definition
                    .trait_impls
                    .iter()
                    .flat_map(|implementation| &implementation.methods)
                    .for_each(&mut visit_function);
            }
            Item::Enum(definition) => {
                definition.methods.iter().for_each(&mut visit_function);
                definition
                    .trait_impls
                    .iter()
                    .flat_map(|implementation| &implementation.methods)
                    .for_each(&mut visit_function);
            }
            Item::Impl(implementation) => {
                implementation.methods.iter().for_each(&mut visit_function);
            }
            _ => {}
        }
    }
    (names, all_methods)
}

pub(crate) fn check_module_bodies(
    module: &mut crate::AST::LoadedModule,
    module_idx: usize,
    states: &[ModuleState],
    plugin_interfaces: &PluginInterfaceRegistry,
    devtools_registry: &jet_foundation::AST::DevtoolsRegistry,
    effect_facts: &jet_foundation::Facts::FactRegistry,
    mode: CompileMode,
    no_os: bool,
    gates: crate::Policy::GateSet,
    summaries: &mut HashMap<String, EffectSummary>,
    embed_inputs_out: &mut Vec<crate::AST::ComptimeInput>,
    global_addr_taken: &mut HashSet<String>,
    name_ledger: &mut jet_foundation::Names::NameLedger,
    pending_diagnostics_out: &mut Vec<PendingFunctionDiagnostic>,
    mut incremental: Option<&mut IncrementalSemaCache>,
) -> Vec<Diagnostic> {
    let st = &states[module_idx];
    let mut diags = Vec::new();
    let no_prelude = module.no_prelude;
    let (ct_funcs, ct_externs, ct_globals) = comptime_context_from_items(&module.items);
    let invalid_serde_impls = invalid_serde_derive_impls(&module.items, &st.trait_reg);
    let ct_base_dir = module
        .path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let stage_jobs = comptime_stage_jobs(&module.items);
    let mut raw_eval_funcs = ct_funcs.clone();
    for (key, job) in &stage_jobs {
        raw_eval_funcs.insert(key.clone(), job.function.clone());
    }
    let (stage_names, stage_all_methods) =
        comptime_stage_roots(&module.items, &raw_eval_funcs);
    let mut stage_keys = HashSet::new();
    for (key, job) in &stage_jobs {
        if stage_all_methods && job.owner.is_some()
            || stage_names.contains(key)
            || stage_names.contains(&job.function.name)
        {
            stage_keys.insert(key.clone());
        }
    }
    let mut staged_unqualified = stage_names.clone();
    for key in &stage_keys {
        if let Some(job) = stage_jobs.get(key) {
            staged_unqualified.insert(job.function.name.clone());
        }
    }
    let mut checked_ct_funcs = HashMap::new();
    for key in stage_keys {
        let Some(job) = stage_jobs.get(&key) else {
            continue;
        };
        let mut function = job.function.clone();
        let mut stage_summaries = HashMap::new();
        let mut stage_inputs = Vec::new();
        let mut stage_addresses = HashSet::new();
        let mut stage_ledger = name_ledger.body_snapshot();
        let mut stage_pending = Vec::new();
        let (stage_diagnostics, _) = check_func_body_bundle_deferred(
            &mut function,
            module_idx,
            states,
            plugin_interfaces,
            devtools_registry,
            effect_facts,
            job.owner.as_deref(),
            job.raw_protocol_return,
            &ct_funcs,
            &ct_externs,
            &ct_base_dir,
            &ct_globals,
            no_os,
            gates,
            &mut stage_summaries,
            &mut stage_inputs,
            &mut stage_addresses,
            no_prelude,
            &mut stage_ledger,
            &mut stage_pending,
        );
        if stage_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error)
        {
            checked_ct_funcs.insert(key.clone(), function.clone());
            if job.owner.is_some() {
                checked_ct_funcs.insert(
                    format!(
                        "{}::{}",
                        job.owner.as_deref().unwrap_or_default(),
                        function.name
                    ),
                    function.clone(),
                );
            }
            checked_ct_funcs.insert(function.name.clone(), function);
        }
    }
    // A direct comptime root can be a top-level function that does not need a
    // job-specific owner context. Keep that path explicit without staging
    // unrelated runtime helpers.
    for name in staged_unqualified {
        if checked_ct_funcs.contains_key(&name) {
            continue;
        }
        let Some(function) = ct_funcs.get(&name) else {
            continue;
        };
        let mut function = function.clone();
        let mut stage_summaries = HashMap::new();
        let mut stage_inputs = Vec::new();
        let mut stage_addresses = HashSet::new();
        let mut stage_ledger = name_ledger.body_snapshot();
        let mut stage_pending = Vec::new();
        let (stage_diagnostics, _) = check_func_body_bundle_deferred(
            &mut function,
            module_idx,
            states,
            plugin_interfaces,
            devtools_registry,
            effect_facts,
            None,
            false,
            &ct_funcs,
            &ct_externs,
            &ct_base_dir,
            &ct_globals,
            no_os,
            gates,
            &mut stage_summaries,
            &mut stage_inputs,
            &mut stage_addresses,
            no_prelude,
            &mut stage_ledger,
            &mut stage_pending,
        );
        if stage_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error)
        {
            checked_ct_funcs.insert(name, function.clone());
            checked_ct_funcs.insert(function.name.clone(), function);
        }
    }
    // D-MEM-VIEWRET1=B: resolve callable view summaries before the real body
    // pass so declaration order cannot affect a public owner contract. Each
    // iteration checks pristine clones and publishes only the canonical fact;
    // diagnostics and other analysis products are discarded. Tentative facts
    // let mutually recursive SCCs converge; the real pass below still rejects
    // any path that ultimately conflicts or cannot stabilize.
    #[derive(Clone)]
    struct ViewSummaryJob {
        key: String,
        owner: Option<String>,
        trait_name: Option<String>,
        raw_protocol_return: bool,
        function: Func,
    }
    let mut view_jobs = Vec::new();
    for item in &module.items {
        match item {
            Item::Func(function) => view_jobs.push(ViewSummaryJob {
                key: function.name.clone(),
                owner: None,
                trait_name: None,
                raw_protocol_return: false,
                function: function.clone(),
            }),
            Item::Struct(definition) => {
                for function in &definition.methods {
                    view_jobs.push(ViewSummaryJob {
                        key: format!("{}::{}", definition.name, function.name),
                        owner: Some(definition.name.clone()),
                        trait_name: None,
                        raw_protocol_return: false,
                        function: function.clone(),
                    });
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        view_jobs.push(ViewSummaryJob {
                            key: format!(
                                "{}::{}::{}",
                                definition.name, implementation.trait_name, function.name
                            ),
                            owner: Some(definition.name.clone()),
                            trait_name: Some(implementation.trait_name.clone()),
                            raw_protocol_return: uses_raw_protocol_return(
                                Some(implementation.trait_name.as_str()),
                                implementation.compiler_generated,
                                function.compiler_generated,
                            ),
                            function: function.clone(),
                        });
                    }
                }
            }
            Item::Enum(definition) => {
                for function in &definition.methods {
                    view_jobs.push(ViewSummaryJob {
                        key: format!("{}::{}", definition.name, function.name),
                        owner: Some(definition.name.clone()),
                        trait_name: None,
                        raw_protocol_return: false,
                        function: function.clone(),
                    });
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        view_jobs.push(ViewSummaryJob {
                            key: format!(
                                "{}::{}::{}",
                                definition.name, implementation.trait_name, function.name
                            ),
                            owner: Some(definition.name.clone()),
                            trait_name: Some(implementation.trait_name.clone()),
                            raw_protocol_return: uses_raw_protocol_return(
                                Some(implementation.trait_name.as_str()),
                                implementation.compiler_generated,
                                function.compiler_generated,
                            ),
                            function: function.clone(),
                        });
                    }
                }
            }
            Item::Impl(implementation) => {
                for function in &implementation.methods {
                    view_jobs.push(ViewSummaryJob {
                        key: format!(
                            "{}::{}::{}",
                            implementation.type_name,
                            implementation.trait_name.as_deref().unwrap_or("inherent"),
                            function.name
                        ),
                        owner: Some(implementation.type_name.clone()),
                        trait_name: implementation.trait_name.clone(),
                        raw_protocol_return: uses_raw_protocol_return(
                            implementation.trait_name.as_deref(),
                            false,
                            function.compiler_generated,
                        ),
                        function: function.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    fn contains_view(registry: &TypeRegistry, ty: &Type, seen: &mut HashSet<String>) -> bool {
        match ty {
            // D-PIN1=A: `Pin<T>` borrows its owner's storage, so it carries
            // provenance across a signature exactly like `View`/`ViewMut`.
            Type::Apply { name, args }
                if matches!(name.as_str(), "View" | "ViewMut" | Syntax::TYPE_PIN)
                    && args.len() == 1 =>
            {
                true
            }
            Type::Named(name) => {
                seen.insert(name.clone())
                    && registry.struct_fields(name).is_some_and(|fields| {
                        fields
                            .iter()
                            .any(|(_, _, field_ty)| contains_view(registry, field_ty, seen))
                    })
            }
            Type::Apply { name, args } => {
                args.iter().any(|arg| contains_view(registry, arg, seen))
                    || (seen.insert(name.clone())
                        && registry.struct_fields(name).is_some_and(|fields| {
                            fields
                                .iter()
                                .any(|(_, _, field_ty)| contains_view(registry, field_ty, seen))
                        }))
            }
            Type::Option(inner)
            | Type::List(inner)
            | Type::Shared(inner)
            | Type::Tagged { inner, .. } => contains_view(registry, inner, seen),
            Type::Result { ok, err } => {
                contains_view(registry, ok, seen) || contains_view(registry, err, seen)
            }
            Type::Map { key, value, .. } => {
                contains_view(registry, key, seen) || contains_view(registry, value, seen)
            }
            Type::Tuple(fields) => fields
                .iter()
                .any(|(_, field_ty)| contains_view(registry, field_ty, seen)),
            Type::FixedList { elem, .. } => contains_view(registry, elem, seen),
            Type::Fn { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| contains_view(registry, param, seen))
                    || ret
                        .as_deref()
                        .is_some_and(|ret| contains_view(registry, ret, seen))
            }
            _ => false,
        }
    }
    view_jobs.retain(|job| {
        job.function
            .return_type
            .as_ref()
            .is_some_and(|return_type| {
                contains_view(&st.registry, return_type, &mut HashSet::new())
            })
    });
    view_jobs.sort_by(|left, right| left.key.cmp(&right.key));
    let trait_job_counts = view_jobs.iter().fold(
        HashMap::<(String, String), usize>::new(),
        |mut counts, job| {
            if let Some(trait_name) = &job.trait_name {
                *counts
                    .entry((trait_name.clone(), job.function.name.clone()))
                    .or_default() += 1;
            }
            counts
        },
    );
    for _ in 0..=view_jobs.len() {
        let mut trait_candidates =
            HashMap::<(String, String), Vec<crate::AST::ViewProvenanceMap>>::new();
        for job in &view_jobs {
            let mut function = job.function.clone();
            let mut scratch_summaries = HashMap::new();
            let mut scratch_inputs = Vec::new();
            let mut scratch_addr_taken = HashSet::new();
            let mut scratch_ledger = name_ledger.body_snapshot();
            let mut scratch_pending_diagnostics = Vec::new();
            let _ = check_func_body_bundle_checked(
                &mut function,
                module_idx,
                states,
                plugin_interfaces,
                devtools_registry,
                effect_facts,
                job.owner.as_deref(),
                job.raw_protocol_return,
                &ct_funcs,
                &checked_ct_funcs,
                st.items.as_slice(),
                &ct_externs,
                &ct_base_dir,
                &ct_globals,
                no_os,
                gates,
                &mut scratch_summaries,
                &mut scratch_inputs,
                &mut scratch_addr_taken,
                no_prelude,
                &mut scratch_ledger,
                &mut scratch_pending_diagnostics,
            );
            if let (Some(trait_name), Some(provenance)) =
                (&job.trait_name, function.return_view_provenance)
            {
                trait_candidates
                    .entry((trait_name.clone(), function.name.clone()))
                    .or_default()
                    .push(provenance);
            }
        }
        for (key, candidates) in trait_candidates {
            if candidates.len() != trait_job_counts.get(&key).copied().unwrap_or(0) {
                continue;
            }
            let Some(first) = candidates.first() else {
                continue;
            };
            let mut contract = first.clone();
            if !candidates
                .iter()
                .skip(1)
                .all(|candidate| merge_view_provenance(&mut contract, candidate))
            {
                continue;
            }
            if let Some(signature) = st
                .trait_reg
                .traits
                .get(&key.0)
                .and_then(|info| info.methods.get(&key.1))
            {
                let _ = signature.return_view_provenance.set(contract);
            }
        }
    }
    let cache_allowed = view_jobs.is_empty();
    let module_key = module.display.clone();
    // D-FAIL-CONV2=A: the shipped `impl <CoreError> -> Err` conversions are
    // demand-driven — which ones a module needs is only known once every body
    // in it has recorded its `TryConvert::Typed` facts. So the items are
    // appended at the END of this walk (see the `index == len` arm below) and
    // the walk keeps going, which routes each injected body through the
    // `Item::ErrorConv` arm exactly like a user-declared conversion. Injecting
    // after this function returns instead left the shipped bodies unchecked:
    // `Err("{self}")` stayed an ordinary `Expr::Call`, never normalized into
    // the default-error `Err` struct literal, and codegen's TIR gate refused
    // it as an uncovered construct (an I2 abort, not a user diagnostic).
    let mut index = 0;
    let mut conversions_injected = false;
    loop {
        if index == module.items.len() {
            if conversions_injected {
                break;
            }
            conversions_injected = true;
            diags.extend(super::super::Prelude::inject_exercised_error_conversions(
                module,
            ));
            continue;
        }
        let item = &mut module.items[index];
        index += 1;
        match item {
            Item::Func(f) => {
                diags.extend(check_func_body_incremental(
                    format!("{module_key}::fn:{}", f.name),
                    f,
                    module_idx,
                    states,
                    plugin_interfaces,
                    devtools_registry,
                    effect_facts,
                    None,
                    false,
                    &ct_funcs,
                    &checked_ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    no_os,
                    gates,
                    summaries,
                    embed_inputs_out,
                    global_addr_taken,
                    no_prelude,
                    name_ledger,
                    pending_diagnostics_out,
                    incremental.as_deref_mut(),
                    cache_allowed,
                ));
                checked_ct_funcs.insert(f.name.clone(), f.clone());
            }
            Item::Struct(s) => {
                for m in &mut s.methods {
                    let own_params = std::mem::take(&mut m.type_params);
                    if own_params.is_empty() {
                        m.type_params = s.type_params.clone();
                    }
                    diags.extend(check_func_body_incremental(
                        format!("{module_key}::struct:{}::method:{}", s.name, m.name),
                        m,
                        module_idx,
                        states,
                        plugin_interfaces,
                        devtools_registry,
                        effect_facts,
                        Some(&s.name),
                        false,
                        &ct_funcs,
                        &checked_ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        no_os,
                        gates,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        no_prelude,
                        name_ledger,
                        pending_diagnostics_out,
                        incremental.as_deref_mut(),
                        cache_allowed,
                    ));
                    checked_ct_funcs.insert(format!("{}::{}", s.name, m.name), m.clone());
                    checked_ct_funcs.insert(m.name.clone(), m.clone());
                    m.type_params = own_params;
                }
                // Trait impls nested in a struct are real method bodies too.
                // They inherit the struct's generic parameters, just as the
                // Rust impl emitted for them does.  Temporarily expose those
                // parameters to the ordinary body checker while preserving the
                // parsed method signature for codegen.
                for block in &mut s.trait_impls {
                    for m in &mut block.methods {
                        let own_params = std::mem::take(&mut m.type_params);
                        m.type_params = if own_params.is_empty() {
                            s.type_params.clone()
                        } else {
                            own_params.clone()
                        };
                        diags.extend(author_facing_diagnostics(
                            block.compiler_generated,
                            check_func_body_incremental(
                                format!(
                                    "{module_key}::struct:{}::trait:{}::method:{}",
                                    s.name, block.trait_name, m.name
                                ),
                                m,
                                module_idx,
                                states,
                                plugin_interfaces,
                                devtools_registry,
                                effect_facts,
                                Some(&s.name),
                                uses_raw_protocol_return(
                                    Some(&block.trait_name),
                                    block.compiler_generated,
                                    m.compiler_generated,
                                ),
                                &ct_funcs,
                                &checked_ct_funcs,
                                &ct_externs,
                                &ct_base_dir,
                                &ct_globals,
                                no_os,
                                gates,
                                summaries,
                                embed_inputs_out,
                                global_addr_taken,
                                no_prelude,
                                name_ledger,
                                pending_diagnostics_out,
                                incremental.as_deref_mut(),
                                cache_allowed,
                            ),
                        ));
                        checked_ct_funcs.insert(format!("{}::{}", s.name, m.name), m.clone());
                        checked_ct_funcs.insert(m.name.clone(), m.clone());
                        // Generated serde methods temporarily carry inherited,
                        // inferred bounds solely for sema. Their Rust generics
                        // belong on the enclosing impl, not on the method.
                        m.type_params = if matches!(
                            block.trait_name.as_str(),
                            crate::Generics::ENCODE | crate::Generics::DECODE
                        ) {
                            Vec::new()
                        } else {
                            own_params
                        };
                    }
                }
            }
            Item::Enum(e) => {
                for m in &mut e.methods {
                    let own_params = std::mem::take(&mut m.type_params);
                    if own_params.is_empty() {
                        m.type_params = e.type_params.clone();
                    }
                    diags.extend(check_func_body_incremental(
                        format!("{module_key}::enum:{}::method:{}", e.name, m.name),
                        m,
                        module_idx,
                        states,
                        plugin_interfaces,
                        devtools_registry,
                        effect_facts,
                        Some(&e.name),
                        false,
                        &ct_funcs,
                        &checked_ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        no_os,
                        gates,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        no_prelude,
                        name_ledger,
                        pending_diagnostics_out,
                        incremental.as_deref_mut(),
                        cache_allowed,
                    ));
                    checked_ct_funcs.insert(format!("{}::{}", e.name, m.name), m.clone());
                    checked_ct_funcs.insert(m.name.clone(), m.clone());
                    m.type_params = own_params;
                }
                for block in &mut e.trait_impls {
                    for m in &mut block.methods {
                        let own_params = std::mem::take(&mut m.type_params);
                        m.type_params = if own_params.is_empty() {
                            e.type_params.clone()
                        } else {
                            own_params.clone()
                        };
                        diags.extend(author_facing_diagnostics(
                            block.compiler_generated,
                            check_func_body_incremental(
                                format!(
                                    "{module_key}::enum:{}::trait:{}::method:{}",
                                    e.name, block.trait_name, m.name
                                ),
                                m,
                                module_idx,
                                states,
                                plugin_interfaces,
                                devtools_registry,
                                effect_facts,
                                Some(&e.name),
                                uses_raw_protocol_return(
                                    Some(&block.trait_name),
                                    block.compiler_generated,
                                    m.compiler_generated,
                                ),
                                &ct_funcs,
                                &checked_ct_funcs,
                                &ct_externs,
                                &ct_base_dir,
                                &ct_globals,
                                no_os,
                                gates,
                                summaries,
                                embed_inputs_out,
                                global_addr_taken,
                                no_prelude,
                                name_ledger,
                                pending_diagnostics_out,
                                incremental.as_deref_mut(),
                                cache_allowed,
                            ),
                        ));
                        checked_ct_funcs.insert(format!("{}::{}", e.name, m.name), m.clone());
                        checked_ct_funcs.insert(m.name.clone(), m.clone());
                        m.type_params = if matches!(
                            block.trait_name.as_str(),
                            crate::Generics::ENCODE | crate::Generics::DECODE
                        ) {
                            Vec::new()
                        } else {
                            own_params
                        };
                    }
                }
            }
            Item::Impl(i) => {
                if i.trait_name.as_deref().is_some_and(|trait_name| {
                    i.is_generated_serde
                        && invalid_serde_impls
                            .contains(&(i.type_name.clone(), trait_name.to_string()))
                }) {
                    continue;
                }
                let owner_params = st
                    .trait_reg
                    .struct_params
                    .get(&i.type_name)
                    .or_else(|| st.trait_reg.enum_params.get(&i.type_name));
                for m in &mut i.methods {
                    let own_params = std::mem::take(&mut m.type_params);
                    // An implementation method sees the owner's generic
                    // parameters whether the impl names a trait or not. A
                    // typed derive body can fill a method hole with `T`, and
                    // the generated `impl Type.Trait` must have the same
                    // scope as a hand-written trait impl.
                    if own_params.is_empty() {
                        m.type_params = owner_params.cloned().unwrap_or_default();
                    } else {
                        m.type_params = own_params.clone();
                    }
                    diags.extend(check_func_body_incremental(
                        format!(
                            "{module_key}::impl:{}::{}::method:{}",
                            i.type_name,
                            i.trait_name.as_deref().unwrap_or("inherent"),
                            m.name
                        ),
                        m,
                        module_idx,
                        states,
                        plugin_interfaces,
                        devtools_registry,
                        effect_facts,
                        Some(&i.type_name),
                        uses_raw_protocol_return(
                            i.trait_name.as_deref(),
                            false,
                            m.compiler_generated,
                        ),
                        &ct_funcs,
                        &checked_ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        no_os,
                        gates,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        no_prelude,
                        name_ledger,
                        pending_diagnostics_out,
                        incremental.as_deref_mut(),
                        cache_allowed,
                    ));
                    checked_ct_funcs.insert(format!("{}::{}", i.type_name, m.name), m.clone());
                    checked_ct_funcs.insert(m.name.clone(), m.clone());
                    m.type_params = own_params;
                }
            }
            Item::Test(t) if matches!(mode, CompileMode::Test | CompileMode::TestOverride) => {
                let Some(test_name) = t.name.as_deref() else {
                    continue;
                };
                // D-TEST1: a parameterized `#Test fn` is a property test — its
                // params must be generatable types so the runner can synthesize
                // inputs. Validate before checking the body so the error points at
                // the offending param type.
                for p in &t.params {
                    if let Some(d) = property_param_unsupported(&p.ty, p.ty_span) {
                        diags.push(d);
                    }
                }
                let mut synthetic =
                    Func::implicit_run(std::mem::take(&mut t.body), t.span);
                synthetic.name = format!("__test_{test_name}");
                synthetic.name_span = t.name_span;
                synthetic.params = t.params.clone();
                diags.extend(check_func_body_bundle_checked(
                    &mut synthetic,
                    module_idx,
                    states,
                    plugin_interfaces,
                    devtools_registry,
                    effect_facts,
                    None,
                    false,
                    &ct_funcs,
                    &checked_ct_funcs,
                    st.items.as_slice(),
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    no_os,
                    gates,
                    summaries,
                    embed_inputs_out,
                    global_addr_taken,
                    no_prelude,
                    name_ledger,
                    pending_diagnostics_out,
                ));
                t.body = synthetic.body;
            }
            Item::CodeModule(cm) => {
                // Type-check inline-module function bodies. Sibling calls were
                // already rewritten to mangled names by `mangle_inline_sibling_calls`,
                // and the mangled signatures are registered in `st.funcs`.
                if let Some(body) = &mut cm.body {
                    for inner in body.iter_mut() {
                        if let Item::Func(f) = inner {
                            // Inline-module calls use their registered mangled
                            // identity (`__jet_module__fn`). Preserve any top-level
                            // same-name summary while the shared body checker
                            // emits this function's local summary.
                            let previous = summaries.remove(&f.name);
                            let pending_start = pending_diagnostics_out.len();
                            diags.extend(
                                check_func_body_bundle_scoped_checked(
                                    f,
                                    module_idx,
                                    states,
                                    plugin_interfaces,
                                    devtools_registry,
                                    effect_facts,
                                    None,
                                    false,
                                    &ct_funcs,
                                    &checked_ct_funcs,
                                    st.items.as_slice(),
                                    &ct_externs,
                                    &ct_base_dir,
                                    &ct_globals,
                                    no_os,
                                    gates,
                                    summaries,
                                    embed_inputs_out,
                                    global_addr_taken,
                                    no_prelude,
                                    name_ledger,
                                    pending_diagnostics_out,
                                    Some(&cm.name),
                                )
                                .0,
                            );
                            for pending in &mut pending_diagnostics_out[pending_start..] {
                                pending.function_key =
                                    jet_foundation::Names::member_name(&cm.name, &f.name);
                            }
                            if let Some(summary) = summaries.remove(&f.name) {
                                summaries.insert(
                                    crate::Sema::inline_effect_key(&cm.name, &f.name),
                                    summary,
                                );
                            }
                            if let Some(summary) = previous {
                                summaries.insert(f.name.clone(), summary);
                            }
                        }
                    }
                }
            }
            Item::ErrorConv(ec) => {
                let mut synthetic = Func {
                    span: ec.body_span,
                    is_comptime: false,
                    is_pub: false,
                    is_package_pub: false,
                    external_type: None,
                    name: format!(
                        "__errconv_{}_to_{}",
                        ec.from_ty.replace('.', "_"),
                        ec.to_ty.replace('.', "_")
                    ),
                    name_span: ec.from_span,
                    meta: None,
                    type_params: Vec::new(),
                    head_pattern: None,
                    params: vec![Param {
                        name: crate::Syntax::KW_SELF.to_string(),
                        name_span: ec.from_span,
                        ty: Type::Named(String::new()),
                        ty_span: ec.from_span,
                        convention: AccessConvention::Move,
                        root: false,
                        default: None,
                        variadic: false,
                        variadic_bound_list: None,
                        declared_view_from_names: None,
                        public_label: None,
                        zone: crate::AST::ParamZone::Either,
                    }],
                    return_type: Some(Type::Named(ec.to_ty.clone())),
                    return_type_span: Some(ec.to_span),
                    return_view_provenance: None,
                    declared_return_view_provenance: None,
                    gc_return: false,
                    diverges: false,
                    gc_scope: false,
                    is_unsafe: false,
                    unsafe_reason: None,
                    unsafe_span: None,
                    is_pure: false,
                    is_reactive: false,
                    reactive_upgrades: Vec::new(),
                    is_replayable: false,
                    replayable_span: None,
                    is_job: false,
                    job_span: None,
                    every: None,
                    job_metadata: None,
                    is_must_use: false,
                    must_use_span: None,
                    maturity: None,
                    maturity_span: None,
                    kernel: None,
                    is_inline: false,
                    is_inline_always: false,
                    inline_span: None,
                    is_sanitizer: false,
                    scrub_tag: None,
                    declared_effects: None,
                    effect_via: None,
                    state_requires: None,
                    state_transition: None,
                    web_marker: None,
                    pre: Vec::new(),
                    post: Vec::new(),
                    inline_foreign: None,
                    undo: None,
                    markers: Vec::new(),
                    compiler_generated: false,
                    body: std::mem::take(&mut ec.body),
                };
                // Error-conversion bodies are checked like functions, but they are
                // not functions: do not publish their synthetic names or local
                // analysis artifacts into the program-wide accumulators.
                let mut conversion_summaries = HashMap::new();
                let mut conversion_inputs = Vec::new();
                let mut conversion_addr_taken = HashSet::new();
                let mut conversion_ledger = name_ledger.body_snapshot();
                let mut conversion_pending_diagnostics = Vec::new();
                diags.extend(check_func_body_bundle_checked(
                    &mut synthetic,
                    module_idx,
                    states,
                    plugin_interfaces,
                    devtools_registry,
                    effect_facts,
                    Some(&ec.from_ty),
                    false,
                    &ct_funcs,
                    &checked_ct_funcs,
                    st.items.as_slice(),
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    false,
                    crate::Policy::GateSet::default(),
                    &mut conversion_summaries,
                    &mut conversion_inputs,
                    &mut conversion_addr_taken,
                    no_prelude,
                    &mut conversion_ledger,
                    &mut conversion_pending_diagnostics,
                ));
                pending_diagnostics_out.extend(conversion_pending_diagnostics);
                ec.body = synthetic.body;
            }
            _ => {}
        }
    }
    // D-MEMPROVENANCE2=A: a trait method publishes the union of every
    // compatible implementation source before TIR.
    let mut trait_view_contracts: HashMap<
        (String, String),
        (crate::AST::ViewProvenanceMap, crate::Diagnostics::Span),
    > = HashMap::new();
    let mut record_trait_methods =
        |trait_name: &str, methods: &[Func], diags: &mut Vec<Diagnostic>| {
            for method in methods {
                let Some(provenance) = method.return_view_provenance.clone() else {
                    continue;
                };
                if provenance.is_empty() {
                    continue;
                }
                let key = (trait_name.to_string(), method.name.clone());
                if let Some((existing, _)) = trait_view_contracts.get_mut(&key) {
                    if !merge_view_provenance(existing, &provenance) {
                        diags.push(Diagnostic::error(
                            "E2305",
                            format!("implementations of `{}.{}` disagree about returned view slots", trait_name, method.name),
                            "dynamic dispatch can union possible owners, but every implementation must return the same view-bearing shape and access marker"
                                .to_string(),
                            "return the same read or write view slots in every implementation"
                                .to_string(),
                            Some(method.name_span),
                        ));
                    }
                } else {
                    trait_view_contracts.insert(key, (provenance, method.name_span));
                }
            }
        };
    for item in &module.items {
        match item {
            Item::Impl(implementation) => {
                if let Some(trait_name) = implementation.trait_name.as_deref() {
                    record_trait_methods(trait_name, &implementation.methods, &mut diags);
                }
            }
            Item::Struct(definition) => {
                for implementation in &definition.trait_impls {
                    record_trait_methods(
                        &implementation.trait_name,
                        &implementation.methods,
                        &mut diags,
                    );
                }
            }
            Item::Enum(definition) => {
                for implementation in &definition.trait_impls {
                    record_trait_methods(
                        &implementation.trait_name,
                        &implementation.methods,
                        &mut diags,
                    );
                }
            }
            _ => {}
        }
    }
    for ((trait_name, method_name), (provenance, _)) in trait_view_contracts {
        if let Some(signature) = st
            .trait_reg
            .traits
            .get(&trait_name)
            .and_then(|info| info.methods.get(&method_name))
        {
            // Prefer a declared `from` on the trait method when present.
            if signature.declared_return_view_provenance.is_none() {
                let _ = signature.return_view_provenance.set(provenance);
            }
        }
    }
    // D-MEMPROVENANCE3=A: trait methods with a declared `from` publish that
    // contract for every implementation and for open dispatch.
    for item in &module.items {
        let Item::Trait(trait_def) = item else {
            continue;
        };
        for method in &trait_def.methods {
            let Some(declared) = method.declared_return_view_provenance.clone() else {
                continue;
            };
            if let Some(signature) = st
                .trait_reg
                .traits
                .get(&trait_def.name)
                .and_then(|info| info.methods.get(&method.name))
            {
                let _ = signature.return_view_provenance.set(declared.clone());
            }
        }
    }
    // D-STRUCT-POLICY1=A: check each declared wrapper against the complete
    // signature at its use site. `call` is an ordinary typed function
    // parameter in this checking slice, so the wrapper can inspect policy
    // parameters, invoke the captured callable, and return its result without
    // changing the wrapped function type.
    let mut generated_policy_wrappers = Vec::new();
    let mut generated_policy_wrapper_names = HashSet::new();
    let mut apply_policy_uses = HashMap::new();
    collect_callable_policy_apply_uses(&mut module.items, &mut apply_policy_uses);
    let apply_target_names = apply_policy_uses.keys().cloned().collect::<HashSet<_>>();
    let mut policy_targets = callable_policy_targets(&module.items);
    named_callable_policy_targets(&module.items, &apply_target_names, &mut policy_targets);
    for target in policy_targets {
        let target_sig = st
            .funcs
            .get(&target.name)
            .cloned()
            .unwrap_or_else(|| crate::Sema::func_to_sig(&target));
        let call_ty = func_sig_to_fn_type(&target_sig);
        let mut policy_names = Vec::new();
        for marker in target
            .markers
            .iter()
            .filter(|marker| marker.name == Syntax::MARKER_POLICY)
        {
            let Ok(chain) =
                crate::AST::CallablePolicyChain::parse(&marker.expr_args_owned()) else {
                continue;
            };
            for policy in chain.policies {
                if !policy_names.contains(&policy.name) {
                    policy_names.push(policy.name);
                }
            }
        }
        if let Some(applied_policies) = apply_policy_uses.get(&target.name) {
            for policy_name in applied_policies {
                if !policy_names.contains(policy_name) {
                    policy_names.push(policy_name.clone());
                }
            }
        }
        for policy_name in policy_names {
            let policy_key = (st.package_scope.clone(), policy_name.clone());
            let Some((_declaring_module, declaration)) =
                st.callable_policy_declarations.get(&policy_key)
            else {
                continue;
            };
            let wrapper_name = callable_policy_wrapper_name(&policy_name, &target.name);
            let mut wrapper = Func::implicit_run(declaration.body.clone(), declaration.span);
            wrapper.name = wrapper_name.clone();
            wrapper.name_span = declaration.name_span;
            wrapper.params = declaration.params.clone();
            wrapper.params.push(Param {
                convention: crate::AST::AccessConvention::Read,
                root: false,
                name: "call".to_string(),
                name_span: declaration.name_span,
                public_label: None,
                zone: crate::AST::ParamZone::Either,
                ty: call_ty.clone(),
                ty_span: declaration.name_span,
                default: None,
                variadic: false,
                variadic_bound_list: None,
                declared_view_from_names: None,
            });
            wrapper
                .params
                .extend(target.params.iter().cloned().map(|mut param| {
                    // The outer callable has already had defaults resolved by
                    // its checked call contract. The generated wrapper receives
                    // every target argument explicitly from its closure.
                    param.default = None;
                    param
                }));
            wrapper.return_type = target_sig.return_type.clone();
            wrapper.return_type_span = target.return_type_span;
            wrapper.return_view_provenance = target.return_view_provenance.clone();
            wrapper.declared_return_view_provenance =
                target.declared_return_view_provenance.clone();
            wrapper.compiler_generated = true;
            let mut wrapper_summaries = HashMap::new();
            let mut wrapper_inputs = Vec::new();
            let mut wrapper_addresses = HashSet::new();
            let mut wrapper_ledger = name_ledger.body_snapshot();
            let mut wrapper_pending = Vec::new();
            diags.extend(check_func_body_bundle_checked(
                &mut wrapper,
                module_idx,
                states,
                plugin_interfaces,
                devtools_registry,
                effect_facts,
                None,
                false,
                &ct_funcs,
                &checked_ct_funcs,
                st.items.as_slice(),
                &ct_externs,
                &ct_base_dir,
                &ct_globals,
                no_os,
                gates,
                &mut wrapper_summaries,
                &mut wrapper_inputs,
                &mut wrapper_addresses,
                no_prelude,
                &mut wrapper_ledger,
                &mut wrapper_pending,
            ));
            for pending in &mut wrapper_pending {
                pending.function_key = format!("policy:{}::{}", policy_name, target.name);
            }
            pending_diagnostics_out.extend(wrapper_pending);
            if generated_policy_wrapper_names.insert(wrapper.name.clone()) {
                generated_policy_wrappers.push(crate::AST::Item::Func(wrapper));
            }
        }
    }
    module.items.extend(generated_policy_wrappers);
    let _ = st;
    diags
}
