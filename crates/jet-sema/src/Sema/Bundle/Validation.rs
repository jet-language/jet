use super::*;
use crate::AST::Param;
use std::collections::BTreeSet;

mod CoreUsage;
pub(crate) use CoreUsage::{
    apply_helper_layer_inference, collect_core_expr, collect_core_lvalue, collect_core_stmts,
    collect_used_core, expand_core_reachable_closure,
};

pub(super) fn qualified_effect_facts(
    modules: &[(String, HashMap<String, EffectSummary>)],
    taint_seeds: &HashMap<String, BTreeSet<String>>,
) -> (
    HashMap<String, EffectSummary>,
    jet_foundation::Facts::ReachabilityResult,
) {
    let mut locations = HashMap::<String, Vec<String>>::new();
    let aliases = modules.iter().map(|(alias, _)| alias.as_str()).collect::<HashSet<_>>();
    for (alias, summaries) in modules {
        for key in summaries.keys() {
            locations.entry(key.clone()).or_default().push(format!("{alias}::{key}"));
        }
    }
    let mut qualified = HashMap::new();
    for (alias, summaries) in modules {
        let local_keys: HashSet<String> = summaries.keys().cloned().collect();
        for (key, summary) in summaries {
            let mut summary = summary.clone();
            let resolve_edge = |edge: &String| {
                if edge == "__jet_panic__" { return edge.clone(); }
                if local_keys.contains(edge) { return format!("{alias}::{edge}"); }
                if let Some((module, symbol)) = edge.split_once('.') {
                    if aliases.contains(module) { return format!("{module}::{symbol}"); }
                }
                locations.get(edge).and_then(|values| (values.len() == 1).then(|| values[0].clone())).unwrap_or_else(|| edge.clone())
            };
            summary.edges = summary.edges.iter().map(&resolve_edge).collect();
            for region in &mut summary.regions {
                region.edges = region.edges.iter().map(&resolve_edge).collect();
            }
            for obligation in &mut summary.callback_obligations {
                obligation.edges = obligation.edges.iter().map(&resolve_edge).collect();
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
                grant: false,
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
            ("main".to_string(), HashMap::from([("root".to_string(), root)])),
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
                f, None, scrubbers, facts, returns, return_types, field_tags, field_types,
                core_imports, diags.as_slice(),
            );
            diags.extend(new);
        }
        Item::Impl(i) => {
            for m in &i.methods {
                let new = check_func_taint(
                    m, Some(&i.type_name), scrubbers, facts, returns, return_types, field_tags,
                    field_types, core_imports, diags.as_slice(),
                );
                diags.extend(new);
            }
        }
        Item::Struct(s) => {
            for m in &s.methods {
                let new = check_func_taint(
                    m, Some(&s.name), scrubbers, facts, returns, return_types, field_tags,
                    field_types, core_imports, diags.as_slice(),
                );
                diags.extend(new);
            }
            for block in &s.trait_impls {
                for m in &block.methods {
                    let new = check_func_taint(
                        m, Some(&s.name), scrubbers, facts, returns, return_types, field_tags,
                        field_types, core_imports, diags.as_slice(),
                    );
                    diags.extend(new);
                }
            }
        }
        Item::Enum(e) => {
            for m in &e.methods {
                let new = check_func_taint(
                    m, Some(&e.name), scrubbers, facts, returns, return_types, field_tags,
                    field_types, core_imports, diags.as_slice(),
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

pub(crate) fn register_func_item(f: &Func, st: &mut ModuleState, diags: &mut Vec<Diagnostic>) {
    if f.name == Syntax::BUILTIN_PRINT
        || f.name == Syntax::BUILTIN_PANIC
        || f.name == Syntax::BUILTIN_REQUIRE
        || f.name == Syntax::BUILTIN_REQUIRE_EQ
    {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", f.name),
            format!("`{}` is provided by the language itself", f.name),
            "choose a different name for this function".to_string(),
            Some(f.name_span),
        ));
        return;
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
    // D-NARG-D2 (E0126): check defaults don't ref later params.
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
            | "Table"
            | "Series"
            | "LazyFrame"
            | "DataJoin"
            | "EncodingLimits"
            | "EncodingError"
            | "CBOROptions"
            | "CBORError"
            | "CBORErrorKind"
            | "EncodingCause"
            | "EncodingFormat"
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
        | Type::Tagged { inner, .. } => type_mentions_encoding_surface(inner),
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
        Type::Tuple(fields) => fields.iter().any(|(_, t)| type_mentions_encoding_surface(t)),
        Type::Union(members) => members.iter().any(type_mentions_encoding_surface),
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::IntN { .. }
        | Type::Float32 => false,
        Type::Quantity { base, .. } => type_mentions_encoding_surface(base),
        Type::ComputeDim(_) => false,
    }
}

/// A function/method signature (params + return) names an encoding surface type.
fn func_sig_mentions_encoding_surface(f: &Func) -> bool {
    f.params.iter().any(|p| type_mentions_encoding_surface(&p.ty))
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
            s.fields.iter().any(|f| type_mentions_encoding_surface(&f.ty))
                || s.methods.iter().any(func_sig_mentions_encoding_surface)
                || s.trait_impls
                    .iter()
                    .any(|b| b.methods.iter().any(func_sig_mentions_encoding_surface))
        }
        Item::Enum(e) => {
            e.variants.iter().any(|v| variant_payload_mentions(&v.payload))
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
            m.params.iter().any(|p| type_mentions_encoding_surface(&p.ty))
                || m.return_type.as_ref().is_some_and(type_mentions_encoding_surface)
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
    effect_facts: &jet_foundation::Facts::FactRegistry,
    owner_type: Option<&str>,
    ct_funcs: &HashMap<String, Func>,
    ct_externs: &HashSet<String>,
    ct_base_dir: &std::path::Path,
    ct_globals: &HashMap<String, crate::Comptime::CtValue>,
    freestanding: bool,
    allow_impure: bool,
    summaries: &mut HashMap<String, EffectSummary>,
    embed_inputs_out: &mut Vec<crate::AST::ComptimeInput>,
    global_addr_taken: &mut HashSet<String>,
    no_alloc: bool,
    no_prelude: bool,
    name_ledger: &mut jet_foundation::Names::NameLedger,
    pending_diagnostics_out: &mut Vec<PendingFunctionDiagnostic>,
    cache: Option<&mut IncrementalSemaCache>,
    cache_allowed: bool,
) -> Vec<Diagnostic> {
    let cache_allowed = cache_allowed && !stmts_have_comptime_evaluation(&function.body);
    let Some(cache) = cache.filter(|_| cache_allowed) else {
        return check_func_body_bundle(
            function,
            module_idx,
            states,
            effect_facts,
            owner_type,
            ct_funcs,
            ct_externs,
            ct_base_dir,
            ct_globals,
            freestanding,
            allow_impure,
            summaries,
            embed_inputs_out,
            global_addr_taken,
            no_alloc,
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
    let input = format!("{function:?}").into_bytes();
    if let Some(hit) = cache.get(&key, &input) {
        *function = hit.function;
        summaries.extend(hit.summaries);
        embed_inputs_out.extend(hit.comptime_inputs);
        global_addr_taken.extend(hit.address_taken);
        name_ledger.merge_references(&hit.name_ledger);
        pending_diagnostics_out.extend(hit.pending_diagnostics);
        return hit.diagnostics;
    }

    let mut local_summaries = HashMap::new();
    let mut local_inputs = Vec::new();
    let mut local_address_taken = HashSet::new();
    let mut local_ledger = name_ledger.body_snapshot();
    let mut local_pending_diagnostics = Vec::new();
    let diagnostics = check_func_body_bundle(
        function,
        module_idx,
        states,
        effect_facts,
        owner_type,
        ct_funcs,
        ct_externs,
        ct_base_dir,
        ct_globals,
        freestanding,
        allow_impure,
        &mut local_summaries,
        &mut local_inputs,
        &mut local_address_taken,
        no_alloc,
        no_prelude,
        &mut local_ledger,
        &mut local_pending_diagnostics,
    );
    summaries.extend(local_summaries.clone());
    embed_inputs_out.extend(local_inputs.clone());
    global_addr_taken.extend(local_address_taken.clone());
    name_ledger.merge_references(&local_ledger);
    pending_diagnostics_out.extend(local_pending_diagnostics.clone());
    if !local_inputs.is_empty() {
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
        },
    );
    diagnostics
}

pub(crate) fn check_module_bodies(
    module: &mut crate::AST::LoadedModule,
    module_idx: usize,
    states: &[ModuleState],
    effect_facts: &jet_foundation::Facts::FactRegistry,
    mode: CompileMode,
    freestanding: bool,
    allow_impure: bool,
    summaries: &mut HashMap<String, EffectSummary>,
    embed_inputs_out: &mut Vec<crate::AST::ComptimeInput>,
    global_addr_taken: &mut HashSet<String>,
    name_ledger: &mut jet_foundation::Names::NameLedger,
    pending_diagnostics_out: &mut Vec<PendingFunctionDiagnostic>,
    mut incremental: Option<&mut IncrementalSemaCache>,
) -> Vec<Diagnostic> {
    let st = &states[module_idx];
    let mut diags = Vec::new();
    // D-MEM1/S7 (D-NOALLOC-SEM1=A): captured once — every function body check
    // below for this module gets the same file-scoped `policy no_alloc` state.
    let no_alloc = module.no_alloc_policy.is_some();
    let no_prelude = module.no_prelude;
    let (ct_funcs, ct_externs, ct_globals) = comptime_context_from_items(&module.items);
    let invalid_serde_impls = invalid_serde_derive_impls(&module.items, &st.trait_reg);
    let ct_base_dir = module
        .path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
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
        function: Func,
    }
    let mut view_jobs = Vec::new();
    for item in &module.items {
        match item {
            Item::Func(function) => view_jobs.push(ViewSummaryJob {
                key: function.name.clone(),
                owner: None,
                trait_name: None,
                function: function.clone(),
            }),
            Item::Struct(definition) => {
                for function in &definition.methods {
                    view_jobs.push(ViewSummaryJob {
                        key: format!("{}::{}", definition.name, function.name),
                        owner: Some(definition.name.clone()),
                        trait_name: None,
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
                    && args.len() == 1 => true,
            Type::Named(name) => {
                seen.insert(name.clone())
                    && registry.struct_fields(name).is_some_and(|fields| {
                        fields.iter().any(|(_, _, field_ty)| {
                            contains_view(registry, field_ty, seen)
                        })
                    })
            }
            Type::Apply { name, args } => {
                args.iter().any(|arg| contains_view(registry, arg, seen))
                    || (seen.insert(name.clone())
                        && registry.struct_fields(name).is_some_and(|fields| {
                            fields.iter().any(|(_, _, field_ty)| {
                                contains_view(registry, field_ty, seen)
                            })
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
        job.function.return_type.as_ref().is_some_and(|return_type| {
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
        let mut trait_candidates = HashMap::<
            (String, String),
            Vec<crate::AST::ViewProvenanceMap>,
        >::new();
        for job in &view_jobs {
            let mut function = job.function.clone();
            let mut scratch_summaries = HashMap::new();
            let mut scratch_inputs = Vec::new();
            let mut scratch_addr_taken = HashSet::new();
            let mut scratch_ledger = name_ledger.body_snapshot();
            let mut scratch_pending_diagnostics = Vec::new();
            let _ = check_func_body_bundle(
                &mut function,
                module_idx,
                states,
                effect_facts,
                job.owner.as_deref(),
                &ct_funcs,
                &ct_externs,
                &ct_base_dir,
                &ct_globals,
                freestanding,
                allow_impure,
                &mut scratch_summaries,
                &mut scratch_inputs,
                &mut scratch_addr_taken,
                no_alloc,
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
    for item in &mut module.items {
        match item {
            Item::Func(f) => {
                diags.extend(check_func_body_incremental(
                    format!("{module_key}::fn:{}", f.name),
                    f,
                    module_idx,
                    states,
                    effect_facts,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    freestanding,
                    allow_impure,
                    summaries,
                    embed_inputs_out,
                    global_addr_taken,
                    no_alloc,
                    no_prelude,
                    name_ledger,
                    pending_diagnostics_out,
                    incremental.as_deref_mut(),
                    cache_allowed,
                ));
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
                        effect_facts,
                        Some(&s.name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        freestanding,
                        allow_impure,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        no_alloc,
                        no_prelude,
                        name_ledger,
                        pending_diagnostics_out,
                        incremental.as_deref_mut(),
                        cache_allowed,
                    ));
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
                        diags.extend(check_func_body_incremental(
                            format!(
                                "{module_key}::struct:{}::trait:{}::method:{}",
                                s.name, block.trait_name, m.name
                            ),
                            m,
                            module_idx,
                            states,
                            effect_facts,
                            Some(&s.name),
                            &ct_funcs,
                            &ct_externs,
                            &ct_base_dir,
                            &ct_globals,
                            freestanding,
                            allow_impure,
                            summaries,
                            embed_inputs_out,
                            global_addr_taken,
                            no_alloc,
                            no_prelude,
                            name_ledger,
                            pending_diagnostics_out,
                            incremental.as_deref_mut(),
                            cache_allowed,
                        ));
                        // Generated serde methods temporarily carry inherited,
                        // inferred bounds solely for sema. Their Rust generics
                        // belong on the enclosing impl, not on the method.
                        m.type_params = if matches!(block.trait_name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE) {
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
                        effect_facts,
                        Some(&e.name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        freestanding,
                        allow_impure,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        no_alloc,
                        no_prelude,
                        name_ledger,
                        pending_diagnostics_out,
                        incremental.as_deref_mut(),
                        cache_allowed,
                    ));
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
                        diags.extend(check_func_body_incremental(
                            format!(
                                "{module_key}::enum:{}::trait:{}::method:{}",
                                e.name, block.trait_name, m.name
                            ),
                            m,
                            module_idx,
                            states,
                            effect_facts,
                            Some(&e.name),
                            &ct_funcs,
                            &ct_externs,
                            &ct_base_dir,
                            &ct_globals,
                            freestanding,
                            allow_impure,
                            summaries,
                            embed_inputs_out,
                            global_addr_taken,
                            no_alloc,
                            no_prelude,
                            name_ledger,
                            pending_diagnostics_out,
                            incremental.as_deref_mut(),
                            cache_allowed,
                        ));
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
                    if i.trait_name.is_none() && own_params.is_empty() {
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
                        effect_facts,
                        Some(&i.type_name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        freestanding,
                        allow_impure,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        no_alloc,
                        no_prelude,
                        name_ledger,
                        pending_diagnostics_out,
                        incremental.as_deref_mut(),
                        cache_allowed,
                    ));
                    m.type_params = own_params;
                }
            }
            Item::Test(t) if mode == CompileMode::Test => {
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
                let mut synthetic = Func {
                    span: t.name_span,
                    is_pub: false,
                    is_package_pub: false,
                    external_type: None,
                    name: format!("__test_{test_name}"),
                    name_span: t.name_span,
                    meta: None,
                    type_params: Vec::new(),
                    params: t.params.clone(),
                    return_type: None,
                    return_type_span: None,
                    return_view_provenance: None,
                    declared_return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
                    is_unsafe: false,
                    unsafe_reason: None,
                    unsafe_span: None,
                    is_pure: false,
                    is_reactive: false,
                reactive_upgrades: Vec::new(),
                    is_replayable: false,
                    replayable_span: None,
                    is_task: false,
                    task_span: None,
                    every: None,
                    task_metadata: None,
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
                    markers: Vec::new(),
                    body: std::mem::take(&mut t.body),
                };
                diags.extend(check_func_body_bundle(
                    &mut synthetic,
                    module_idx,
                    states,
                    effect_facts,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    freestanding,
                    allow_impure,
                    summaries,
                    embed_inputs_out,
                    global_addr_taken,
                    no_alloc,
                    no_prelude,
                    name_ledger,
                    pending_diagnostics_out,
                ));
                t.body = synthetic.body;
            }
            // D-BENCH1: a `#Bench` body type-checks exactly like a `#Test` body
            // (a bare statement list, no params, unit context) — only the mode
            // gate differs.
            Item::Bench(b) if mode == CompileMode::Bench => {
                let Some(bench_name) = b.name.as_deref() else {
                    continue;
                };
                let mut synthetic = Func {
                    span: b.name_span,
                    is_pub: false,
                    is_package_pub: false,
                    external_type: None,
                    name: format!("__bench_{bench_name}"),
                    name_span: b.name_span,
                    meta: None,
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: None,
                    return_type_span: None,
                    return_view_provenance: None,
                    declared_return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
                    is_unsafe: false,
                    unsafe_reason: None,
                    unsafe_span: None,
                    is_pure: false,
                    is_reactive: false,
                reactive_upgrades: Vec::new(),
                    is_replayable: false,
                    replayable_span: None,
                    is_task: false,
                    task_span: None,
                    every: None,
                    task_metadata: None,
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
                    markers: Vec::new(),
                    body: std::mem::take(&mut b.body),
                };
                diags.extend(check_func_body_bundle(
                    &mut synthetic,
                    module_idx,
                    states,
                    effect_facts,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    freestanding,
                    allow_impure,
                    summaries,
                    embed_inputs_out,
                    global_addr_taken,
                    no_alloc,
                    no_prelude,
                    name_ledger,
                    pending_diagnostics_out,
                ));
                b.body = synthetic.body;
            }
            Item::CodeModule(cm) => {
                // D-MOD2: type-check inline-module function bodies. Sibling calls were
                // already rewritten to mangled names by `mangle_inline_sibling_calls`,
                // and the mangled signatures are registered in `st.funcs`.
                if let Some(body) = &mut cm.body {
                    for inner in body.iter_mut() {
                        if let Item::Func(f) = inner {
                            // Inline-module calls use their registered mangled
                            // identity (`module__fn`). Preserve any top-level
                            // same-name summary while the shared body checker
                            // emits this function's local summary.
                            let previous = summaries.remove(&f.name);
                            let pending_start = pending_diagnostics_out.len();
                            diags.extend(check_func_body_incremental(
                                format!("{module_key}::module:{}::fn:{}", cm.name, f.name),
                                f,
                                module_idx,
                                states,
                                effect_facts,
                                None,
                                &ct_funcs,
                                &ct_externs,
                                &ct_base_dir,
                                &ct_globals,
                                freestanding,
                                allow_impure,
                                summaries,
                                embed_inputs_out,
                                global_addr_taken,
                                no_alloc,
                                no_prelude,
                                name_ledger,
                                pending_diagnostics_out,
                                incremental.as_deref_mut(),
                                cache_allowed,
                            ));
                            for pending in &mut pending_diagnostics_out[pending_start..] {
                                pending.function_key = jet_foundation::Names::member_name(&cm.name, &f.name);
                            }
                            if let Some(summary) = summaries.remove(&f.name) {
                                summaries.insert(
                                    jet_foundation::Names::member_name(&cm.name, &f.name),
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
                    params: vec![Param {
                        name: crate::Syntax::KW_SELF.to_string(),
                        name_span: ec.from_span,
                        ty: Type::Named(String::new()),
                        ty_span: ec.from_span,
                        convention: AccessConvention::Move,
                        root: false,
                        default: None,
                        variadic: false,
                        variadic_bound_list: None, declared_view_from_names: None, public_label: None, zone: crate::AST::ParamZone::Either,
                    }],
                    return_type: Some(Type::Named(ec.to_ty.clone())),
                    return_type_span: Some(ec.to_span),
                    return_view_provenance: None,
                    declared_return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
                    is_unsafe: false,
                    unsafe_reason: None,
                    unsafe_span: None,
                    is_pure: false,
                    is_reactive: false,
                reactive_upgrades: Vec::new(),
                    is_replayable: false,
                    replayable_span: None,
                    is_task: false,
                    task_span: None,
                    every: None,
                    task_metadata: None,
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
                    markers: Vec::new(),
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
                diags.extend(check_func_body_bundle(
                    &mut synthetic,
                    module_idx,
                    states,
                    effect_facts,
                    Some(&ec.from_ty),
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    false,
                    false,
                    &mut conversion_summaries,
                    &mut conversion_inputs,
                    &mut conversion_addr_taken,
                    no_alloc,
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
                            "dynamic dispatch can union possible owners, but every implementation must return the same view-bearing shape and access capability"
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
    let _ = st;
    diags
}

fn merge_view_provenance(
    into: &mut crate::AST::ViewProvenanceMap,
    from: &crate::AST::ViewProvenanceMap,
) -> bool {
    if into.len() != from.len() || !into.keys().all(|path| from.contains_key(path)) {
        return false;
    }
    for (path, candidate) in from {
        let existing = into.get_mut(path).expect("view paths were checked above");
        if existing.mutable != candidate.mutable {
            return false;
        }
        existing.sources.extend(candidate.sources.iter().cloned());
    }
    true
}

pub(crate) fn check_func_body_bundle(
    f: &mut Func,
    module_idx: usize,
    states: &[ModuleState],
    effect_facts: &jet_foundation::Facts::FactRegistry,
    owner_type: Option<&str>,
    ct_funcs: &HashMap<String, Func>,
    ct_externs: &HashSet<String>,
    ct_base_dir: &std::path::Path,
    ct_globals: &HashMap<String, crate::Comptime::CtValue>,
    freestanding: bool,
    allow_impure: bool,
    summaries: &mut HashMap<String, EffectSummary>,
    embed_inputs_out: &mut Vec<crate::AST::ComptimeInput>,
    global_addr_taken: &mut HashSet<String>,
    // D-MEM1/S7 (D-NOALLOC-SEM1=A): this module's `policy no_alloc` state.
    _no_alloc: bool,
    // D-PRELUDEX1=A: this file's `#NoPrelude` state.
    no_prelude: bool,
    name_ledger: &mut jet_foundation::Names::NameLedger,
    pending_diagnostics_out: &mut Vec<PendingFunctionDiagnostic>,
) -> Vec<Diagnostic> {
    let st = &states[module_idx];
    let mut ck = Checker {
        funcs: &st.funcs,
        registry: &st.registry,
        effect_facts,
        consts: &st.consts,
        modules: Some(states),
        module_idx,
        imports: &st.imports,
        core_imports: &st.core_imports,
        code_modules: &st.code_modules,
        code_module_identities: &st.code_module_identities,
        unqualified: &st.unqualified,
        unqualified_file: &st.unqualified_file,
        module_path: &st.module_path,
        policy_declarations: &st.policy_declarations,
        rule_facts: st.rule_facts.clone(),
        current_function_span: f.span,
        name_ledger,
        diags: Vec::new(),
        flow: crate::Sema::FlowFacts::FlowFacts {
            depth: 1,
            ..Default::default()
        },
        concrete_unit_values: vec![HashMap::new()],
        suppress_partial_move_root_read: false,
        loop_depth: 0,
        source_nesting: 0,
        loop_labels: Vec::new(),
        collect_item_types: Vec::new(),
        loop_value_frames: Vec::new(),
        pending_loop_value: None,
        last_loop_result_type: None,
        fx_direct: std::collections::BTreeSet::new(),
        fx_direct_spans: HashMap::new(),
        fx_edges: std::collections::BTreeSet::new(),
        fx_maximal: false,
        fx_maximal_span: None,
        region_stack: Vec::new(),
        fx_regions: Vec::new(),
        fx_callback_obligations: Vec::new(),
        fx_pending_diagnostics: Vec::new(),
        fx_memory_events: Vec::new(),
        fx_memory_open: Vec::new(),
        memory_policy_stack: Vec::new(),
        fx_memory_regions: Vec::new(),
        fx_memory_unbounded_control: Vec::new(),
        fx_memory_calls: Vec::new(),
        memory_control_multiplier: Some(1),
        txn_depth: 0,
        det_suppress: 0,
        context_depth: 0,
        context_allocator_active: false,
        // S58 (E2-M13): an `#Unsafe fn` body is itself an audited region — its
        // statements may use low-level ops directly without a nested `#Unsafe`
        // block. Calling such a fn is gated separately (E3103).
        in_unsafe: f.is_unsafe,
        suppress_must_use: false,
        in_pure: f.is_pure,
        no_prelude,
        in_pre_clause: false,
        in_comptime: false,
        compiler_api_allowed: st.allow_compiler_api && f.name == "build",
        ret: f.return_type.clone(),
        fn_name: f.name.clone(),
        current_param_names: f
            .params
            .iter()
            .filter(|param| param.name != crate::Syntax::KW_SELF)
            .map(|param| param.name.clone())
            .collect(),
        expected_type: None,
        iter_borrowed: HashSet::new(),
        noelse_chains_checked: HashSet::new(),
        lending_view_loop_vars: HashSet::new(),
        return_view_provenance: None,
        views_used_in_stmt: Default::default(),
        scoped_loan_read_reported: false,
        call_access_frames: Vec::new(),
        borrow_ctx: false,
        allow_fixed_constructor: false,
        allow_string_view_read: false,
        lambda_escapes: true,
        in_lambda_body: false,
        inferred_lambda_mut_captures: HashSet::new(),
        lambda_params_are_lending_views: false,
        is_task_spawn: false,
        lambda_param_mutable: false,
        lambda_param_is_secret_loan: false,
        view_capture_tasks: HashSet::new(),
        reactive_upgrades: Vec::new(),
        reactive_upgrade_names: HashSet::new(),
        view_borrow_escape_tasks: HashSet::new(),
        current_binding_name: None,
        lambda_binding: None,
        lambda_mut_borrow_stack: vec![HashSet::new()],
        trait_reg: &st.trait_reg,
        ct_funcs,
        ct_externs,
        ct_base_dir,
        ct_globals,
        ct_scopes: vec![HashMap::new()],
        type_param_scope: f.type_params.clone(),
        freestanding,
        allow_impure,
        ct_impure_depth: 0,
        ct_embed_inputs: Vec::new(),
        in_dropped_comptime_arm: false,
        stmt_tail_ptr: std::ptr::null(),
        stmt_tail_len: 0,
        liveness_frames: Vec::new(),
        taskgroup_stack: Vec::new(),
        in_taskgroup_spawn: false,
        inline_addr_taken: HashSet::new(),
    };
    for (active, name, span) in [
        (f.is_pure, crate::Syntax::KW_PURE, f.name_span),
        (f.is_sanitizer, crate::Syntax::KW_SANITIZER, f.name_span),
        (f.is_unsafe, crate::Syntax::KW_UNSAFE, f.unsafe_span.unwrap_or(f.name_span)),
        (f.is_replayable, crate::Syntax::MARKER_REPLAYABLE, f.replayable_span.unwrap_or(f.name_span)),
        (f.is_must_use, crate::Syntax::MARKER_MUST_USE, f.must_use_span.unwrap_or(f.name_span)),
        (f.is_inline, crate::Syntax::MARKER_INLINE, f.inline_span.unwrap_or(f.name_span)),
        (f.is_inline_always, crate::Syntax::MARKER_INLINE, f.inline_span.unwrap_or(f.name_span)),
        (f.is_reactive, crate::Syntax::KW_REACTIVE, f.name_span),
    ] {
        if active && !crate::Policy::rule_allows(name, crate::Policy::RuleSite::Function) {
            ck.diags.push(Diagnostic::error("E0355", format!("`#{name}` cannot attach to a function"), "the compiler-owned applicability registry is shared by parser, sema, formatter, semantic index, and explain".to_string(), "move the rule to one of its registered sites".to_string(), Some(span)));
        }
    }
    ck.check_params_and_body(f, owner_type);
    apply_reactive_upgrade_flags(&mut f.body, &ck.reactive_upgrade_names);
    // D-DATARACE1=C: drain upgrade-report lines onto the function for codegen/`jet report`.
    f.reactive_upgrades = std::mem::take(&mut ck.reactive_upgrades);
    f.return_view_provenance = ck.return_view_provenance.clone();
    if let Some(declared) = f.declared_return_view_provenance.clone() {
        // D-MEMPROVENANCE3=A: inferred sources must be ⊆ declaration; callers
        // see the declared (possibly wider) contract. A bare `from packet`
        // covers every field/index/range projection of that owner.
        //
        // Card #1360: the `from` clause carries access too. Declared maps are
        // parsed with `mutable: false`; keep the inferred write/read capability
        // (Pin / ViewMut vs View) so a returned aggregate can store an exclusive
        // window and the caller may edit through it.
        if let Some(inferred) = f.return_view_provenance.as_ref() {
            for (slot, inferred_prov) in inferred {
                let Some(declared_prov) = declared
                    .get(slot)
                    .or_else(|| declared.get(&Vec::new()))
                else {
                    ck.diags.push(Diagnostic::error(
                        "E2305",
                        "returned view escapes its declared `from` clause".to_string(),
                        "every return path's owners must stay inside the sources named after `from`".to_string(),
                        "widen the `from` clause, or stop returning a view from that owner".to_string(),
                        f.return_type_span.or(Some(f.name_span)),
                    ));
                    continue;
                };
                let allowed = inferred_prov.sources.iter().all(|inferred_path| {
                    declared_prov.sources.iter().any(|declared_path| {
                        declared_path.source == inferred_path.source
                            && inferred_path
                                .projections
                                .starts_with(declared_path.projections.as_slice())
                    })
                });
                if !allowed {
                    ck.diags.push(Diagnostic::error(
                        "E2305",
                        "returned view escapes its declared `from` clause".to_string(),
                        "every return path's owners must stay inside the sources named after `from`".to_string(),
                        "widen the `from` clause, or stop returning a view from that owner".to_string(),
                        f.return_type_span.or(Some(f.name_span)),
                    ));
                }
            }
        }
        let mut merged = declared.clone();
        if let Some(inferred) = f.return_view_provenance.as_ref() {
            // Publish inferred output slots (field paths) so an aggregate that
            // mixes owned data with a stored window does not treat every field
            // as a view into the owner. Sources come from the declared contract
            // (possibly wider); access comes from inference.
            let mut published = crate::AST::ViewProvenanceMap::new();
            for (slot, inferred_prov) in inferred {
                let sources = declared
                    .get(slot)
                    .or_else(|| declared.get(&Vec::new()))
                    .map(|prov| prov.sources.clone())
                    .unwrap_or_else(|| inferred_prov.sources.clone());
                published.insert(
                    slot.clone(),
                    crate::AST::ViewProvenance {
                        sources,
                        mutable: inferred_prov.mutable,
                    },
                );
            }
            for (slot, declared_prov) in &declared {
                if slot.is_empty() || published.contains_key(slot) {
                    continue;
                }
                published.insert(slot.clone(), declared_prov.clone());
            }
            // Bare `from owner` with no inferred field slots still publishes
            // the root contract, with write access if the return type needs it.
            if published.is_empty() {
                for (slot, declared_prov) in merged.iter_mut() {
                    if slot.is_empty() {
                        declared_prov.mutable = inferred.values().any(|prov| prov.mutable);
                    }
                }
                f.return_view_provenance = Some(merged);
            } else {
                f.return_view_provenance = Some(published);
            }
        } else if let Some(ret) = f.return_type.as_ref() {
            // No inferred body facts (e.g. abstract signature): derive access
            // from the return type's view leaves.
            let leaves = ck.view_leaf_paths(ret);
            for (slot, declared_prov) in merged.iter_mut() {
                if slot.is_empty() {
                    declared_prov.mutable = leaves
                        .iter()
                        .any(|(_, access)| *access == ViewAccess::Write);
                } else {
                    declared_prov.mutable = leaves.iter().any(|(path, access)| {
                        path == slot && *access == ViewAccess::Write
                    });
                }
            }
            f.return_view_provenance = Some(merged);
        } else {
            f.return_view_provenance = Some(merged);
        }
    }
    if let Some(owner) = owner_type {
        if let (Some(signature), Some(provenance)) =
            (st.registry.method(owner, &f.name), f.return_view_provenance.clone())
        {
            let _ = signature.return_view_provenance.set(provenance);
        }
    } else {
        if let (Some(signature), Some(provenance)) =
            (st.funcs.get(&f.name), f.return_view_provenance.clone())
        {
            let _ = signature.return_view_provenance.set(provenance);
        }
    }
    // Direct ambient/foreign operations keep their precise body diagnostic.
    // User callees are checked after the shared reachability projection so an
    // inferred-pure callee need not repeat `=[]=>`.
    if f.is_pure {
        ck.diags.extend(check_pure_fn(f, &st.funcs));
    }
    // D-METHODMACRO1=A: the local half of the `#Inline(Always)` check (self-
    // recursion E0917 + size ceiling E0919); roll this function's
    // address-taken names into the whole-program accumulator so the E0918
    // pass after the full bundle check can see them.
    if f.is_inline_always {
        ck.diags.extend(check_inline_always_fn(f));
    }
    // D-SCHEDULE1 (card #505): a bad `#Every(…)` value is E0926.
    ck.diags.extend(check_every_marker(f));
    global_addr_taken.extend(std::mem::take(&mut ck.inline_addr_taken));
    // D-EXPANDCLI1 (card #183): roll this function's resolved ref-owner facts
    // into the whole-bundle accumulator for `jet inspect expand --facts refs`.
    // D-CTEFFECT1 Tier-1: drain embed inputs into the caller's accumulator.
    embed_inputs_out.extend(std::mem::take(&mut ck.ct_embed_inputs));
    // D-EFFECT-OMIT1/D-EFF3: an explicit row is an upper bound, not an effect
    // declaration. Static calls propagate the implementation's inferred body
    // row; dynamic trait calls use the trait method bound separately.
    let direct = std::mem::take(&mut ck.fx_direct);
    for event in &mut ck.fx_memory_events {
        event.source = st.module_path.clone();
        event.provenance = format!("{} in {}", effect_key(owner_type, &f.name), st.module_path);
    }
    for region in &mut ck.fx_memory_regions {
        for event in &mut region.events {
            event.source = st.module_path.clone();
            event.provenance = format!(
                "{} block policy in {}",
                effect_key(owner_type, &f.name),
                st.module_path
            );
        }
    }
    if !ck
        .diags
        .iter()
        .any(|diagnostic| matches!(diagnostic.severity, crate::Diagnostics::Severity::Error))
    {
        pending_diagnostics_out.extend(
            std::mem::take(&mut ck.fx_pending_diagnostics)
                .into_iter()
                .map(|diagnostic| PendingFunctionDiagnostic {
                    function_key: effect_key(owner_type, &f.name),
                    function_span: f.span,
                    diagnostic,
                }),
        );
    }
    summaries.insert(
        effect_key(owner_type, &f.name),
        EffectSummary {
            direct,
            direct_spans: std::mem::take(&mut ck.fx_direct_spans),
            edges: std::mem::take(&mut ck.fx_edges),
            maximal: ck.fx_maximal,
            maximal_span: ck.fx_maximal_span,
            unbounded_trait_dispatch: false,
            regions: std::mem::take(&mut ck.fx_regions),
            callback_obligations: std::mem::take(&mut ck.fx_callback_obligations),
            memory: super::MemoryFacts::MemorySummary {
                events: std::mem::take(&mut ck.fx_memory_events),
                open_dispatches: std::mem::take(&mut ck.fx_memory_open),
                regions: std::mem::take(&mut ck.fx_memory_regions),
                unbounded_control: std::mem::take(&mut ck.fx_memory_unbounded_control),
                calls: std::mem::take(&mut ck.fx_memory_calls),
            },
        },
    );
    ck.diags
}

/// D-DATARACE1=C: mark reactive bindings that crossed a concurrency boundary so
/// codegen can emit the upgrade report comments.
fn apply_reactive_upgrade_flags(stmts: &mut [Stmt], names: &std::collections::HashSet<String>) {
    fn walk(stmts: &mut [Stmt], names: &std::collections::HashSet<String>) {
        for stmt in stmts {
            match stmt {
                Stmt::Val(b) => {
                    if names.contains(&b.name) || b.reactive_shared() {
                        b.reactive_upgrade = true;
                    }
                }
                Stmt::While { body, .. }
                | Stmt::For { body, .. }
                | Stmt::Loop { body, .. }
                | Stmt::CountedLoop { body, .. }
                | Stmt::Unsafe { body, .. }
                | Stmt::Impure { body, .. }
                | Stmt::Reactive { body, .. }
                | Stmt::Shield { body, .. }
                | Stmt::Region { body, .. }
                | Stmt::Policy { body, .. }
                | Stmt::TaskGroup { body, .. }
                | Stmt::Caps { body, .. }
                | Stmt::Grant { body, .. }
                | Stmt::ComptimeBlock { body, .. }
                | Stmt::ContextBlock { body, .. }
                | Stmt::Live { body, .. }
                | Stmt::AssumeDet { body, .. }
                | Stmt::Transact { body, .. }
                | Stmt::Switched { body, .. } => walk(body, names),
                Stmt::Switch { arms, else_body, .. }
                | Stmt::ComptimeSwitch { arms, else_body, .. } => {
                    for arm in arms.iter_mut() {
                        walk(&mut arm.body, names);
                    }
                    if let Some(else_body) = else_body {
                        walk(else_body, names);
                    }
                }
                Stmt::ComptimeIf {
                    then_body,
                    else_body,
                    ..
                } => {
                    walk(then_body, names);
                    if let Some(else_body) = else_body {
                        walk(else_body, names);
                    }
                }
                _ => {}
            }
        }
    }
    walk(stmts, names);
}

pub(crate) fn func_sig_to_fn_type(sig: &FuncSig) -> Type {
    Type::Fn {
        params: sig.params.iter().map(|(_, t)| t.clone()).collect(),
        ret: sig.return_type.clone().map(Box::new),
        effect_bound: None,
        param_contract: None,
        return_view_provenance: sig.return_view_provenance.get(),
    }
}

pub(crate) fn fn_types_compatible(want: &Type, got: &Type) -> bool {
    let (
        Type::Fn {
            params: wp,
            ret: wr,
            ..
        },
        Type::Fn {
            params: gp,
            ret: gr,
            ..
        },
    ) = (want, got)
    else {
        return false;
    };
    if wp.len() != gp.len() {
        return false;
    }
    for (a, b) in wp.iter().zip(gp.iter()) {
        if a != b {
            return false;
        }
    }
    match (wr, gr) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// D-TEST1: which parameter types the property-test runner can synthesize inputs
/// for. The generator (codegen) covers the scalar value types plus `[T]` and
/// `T?` of a generatable element. Anything else (user structs/enums, `Map`,
/// functions, trait objects) has no automatic generator yet, so reject it with a
/// clear error rather than miscompile (I3 — checking lives in sema).
fn property_param_generatable(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::Float32
        | Type::IntN { .. } => true,
        Type::List(inner) | Type::Option(inner) => property_param_generatable(inner),
        Type::FixedList { elem, .. } => property_param_generatable(elem),
        _ => false,
    }
}

/// E0613: a property-test parameter type with no automatic value generator.
pub(super) fn property_param_unsupported(ty: &Type, span: Span) -> Option<Diagnostic> {
    if property_param_generatable(ty) {
        return None;
    }
    Some(Diagnostic::error(
        "E0613",
        format!(
            "a property test can't generate values of type `{}`",
            ty.name()
        ),
        format!(
            "a parameterized `#{} fn` is a property test (D-TEST1): {} generates inputs from each parameter's type, but this type has no built-in generator",
            Syntax::KW_TEST,
            Syntax::LANG_NAME
        ),
        "use a generatable type (Int, Float, Bool, String, Char, a sized integer, or a list/optional of those), or write a plain `#Test \"name\" { … }` block and construct the value yourself".to_string(),
        Some(span),
    ))
}
