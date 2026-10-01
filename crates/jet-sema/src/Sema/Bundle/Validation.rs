use super::*;
use crate::AST::Param;
use std::collections::BTreeSet;

mod CoreUsage;
mod BodyCheck;
use BodyCheck::*;
pub(crate) use BodyCheck::{
    checker_for_module, fn_types_compatible, func_sig_to_fn_type, BodyProducts,
};
pub(crate) use BodyCheck::{
    collect_diverging_functions, collect_fixed_length_write_params, project_divergence_facts,
};
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
pub(crate) fn uses_raw_protocol_function_return(
    trait_name: Option<&str>,
    implementation_generated: bool,
    function: &crate::AST::Func,
) -> bool {
    uses_raw_protocol_return(
        trait_name,
        implementation_generated,
        function.compiler_generated,
    ) || (function.compiler_generated && function.state_transition.is_some())
}


/// `sealed` holds the recorded nodes of sealed dependency modules (#2517
/// S2) as `(alias, [(local key, summary)])`. Their edges are already
/// qualified, so they join the graph as recorded and take part in short-name
/// resolution like checked nodes.
pub(super) fn qualified_effect_facts(
    modules: &[(String, HashMap<String, EffectSummary>)],
    sealed: &[(String, Vec<(String, EffectSummary)>)],
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
    for (alias, nodes) in sealed {
        for (key, _) in nodes {
            locations
                .entry(key.clone())
                .or_default()
                .push(format!("{alias}::{key}"));
        }
    }
    let mut qualified = HashMap::new();
    for (alias, nodes) in sealed {
        for (key, summary) in nodes {
            qualified.insert(format!("{alias}::{key}"), summary.clone());
        }
    }
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
            for fact in &mut summary.discarded_results {
                fact.callee = resolve_edge(&fact.callee);
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

        let (summaries, _) = qualified_effect_facts(&modules, &[], &HashMap::new());
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

/// Check one function body through the incremental cache. `ledger` and
/// `products` are this body's own (a ledger `body_snapshot` and empty
/// products), so a miss can store them as the body's cached result.
#[allow(clippy::too_many_arguments)]
fn check_func_body_cached(
    cx: &BodyContext<'_>,
    key: String,
    function: &mut Func,
    owner_type: Option<&str>,
    raw_protocol_return: bool,
    ct_checked_funcs: &HashMap<String, Func>,
    ledger: &mut jet_foundation::Names::NameLedger,
    products: &mut BodyProducts,
    cache: &mut IncrementalSemaCache,
    cache_allowed: bool,
) -> Vec<Diagnostic> {
    let check = |function: &mut Func,
                 ledger: &mut jet_foundation::Names::NameLedger,
                 products: &mut BodyProducts| {
        check_func_body(
            cx,
            function,
            owner_type,
            raw_protocol_return,
            None,
            Some(ct_checked_funcs),
            ledger,
            products,
        )
    };
    if !(cache_allowed && !stmts_have_comptime_evaluation(&function.body)) {
        cache.record_recompute(key);
        return check(function, ledger, products);
    }
    // The checked function contains source spans used by diagnostics and IDE
    // facts. Include them in the cache input so whitespace-only edits cannot
    // reuse stale positions even when the canonical AST is unchanged. Build
    // this recursive Debug form only when the caller can actually use the
    // cache: deep fluent expressions can exceed the ordinary test-thread stack,
    // and disabled-cache checks have no fingerprint consumer.
    let mut input = format!("{function:?}").into_bytes();
    input.push(if raw_protocol_return { 1 } else { 0 });
    if let Some(hit) = cache.get(&key, &input) {
        *function = hit.function;
        products.summaries.extend(hit.summaries);
        products.embed_inputs.extend(hit.comptime_inputs);
        products.addr_taken.extend(hit.address_taken);
        ledger.merge_references(&hit.name_ledger);
        ledger.merge_structure_facts(&hit.name_ledger);
        products.pending_diagnostics.extend(hit.pending_diagnostics);
        products.uses_exact_int |= hit.uses_exact_int;
        products
            .devtools_publications
            .extend(hit.devtools_publications);
        return hit.diagnostics;
    }

    let diagnostics = check(function, ledger, products);
    if !products.embed_inputs.is_empty() {
        cache.record_recompute(key);
        return diagnostics;
    }
    cache.store(
        key,
        CachedFunctionBody {
            input,
            function: function.clone(),
            diagnostics: diagnostics.clone(),
            summaries: products.summaries.clone(),
            comptime_inputs: Vec::new(),
            address_taken: products.addr_taken.clone(),
            name_ledger: ledger.clone(),
            pending_diagnostics: products.pending_diagnostics.clone(),
            uses_exact_int: products.uses_exact_int,
            devtools_publications: products.devtools_publications.clone(),
        },
    );
    diagnostics
}

/// I4: a diagnostic is a product, and a lint is advice addressed to whoever
/// wrote the code. A compiler-generated derive body has no author and no
/// user-typeable span: it is expanded from the provider template in
/// `Prelude/Derives.jet`, so its spans are template offsets, not positions in
/// the file that declares the type. A lint from that body names a construct
/// the user never wrote. `L0502` did exactly that for any struct or enum with
/// a `Float` field, because the package auto-derive default gives every type
/// an `Equatable.equal` body.
///
/// `TraitImplBlock::compiler_generated` is the parser-unforgeable provenance
/// (a source `impl T.Trait` is always `false`; jet-foundation `AST/items.rs`),
/// and it is the only fact consulted here — no second gate, table, or lint
/// exemption list (I8). Errors are kept, not silenced; they land on `anchor`,
/// the block's trait span, which a derive sets to the type's name (the only
/// authored location the derive has), so an edit to the template never moves
/// them onto an unrelated line of the author's file.
fn author_facing_diagnostics(
    generated_anchor: Option<crate::Diagnostics::Span>,
    mut diagnostics: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let Some(anchor) = generated_anchor else {
        return diagnostics;
    };
    diagnostics.retain(|d| matches!(d.severity, crate::Diagnostics::Severity::Error));
    for diagnostic in &mut diagnostics {
        diagnostic.span = Some(anchor);
        diagnostic.labels.clear();
    }
    // Every statement of the body now reports at the same place; one report
    // per distinct problem is enough.
    diagnostics.dedup_by(|later, earlier| later.code == earlier.code && later.what == earlier.what);
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
struct ComptimeStageJob<'a> {
    owner: Option<String>,
    raw_protocol_return: bool,
    function: &'a Func,
}

/// Every function and method of `items`, borrowed: the staged pass clones
/// only the few bodies it checks, not the whole module (#3661).
fn comptime_stage_jobs(items: &[Item]) -> HashMap<String, ComptimeStageJob<'_>> {
    let mut jobs = HashMap::new();
    let mut insert = |key: String, owner: Option<String>, raw_protocol_return: bool, function| {
        jobs.insert(
            key,
            ComptimeStageJob {
                owner,
                raw_protocol_return,
                function,
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
                            uses_raw_protocol_function_return(
                                Some(&implementation.trait_name),
                                implementation.compiler_generated,
                                function,
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
                            uses_raw_protocol_function_return(
                                Some(&implementation.trait_name),
                                implementation.compiler_generated,
                                function,
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
                        uses_raw_protocol_function_return(
                            implementation.trait_name.as_deref(),
                            false,
                            function,
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
    raw_funcs: &HashMap<String, &Func>,
) -> (HashSet<String>, bool) {
    // Gather the direct roots of every call site first, then take one closure
    // over their union: a closure per call site repeated the same call-graph
    // walk for each site, which grew with call sites times reachable code.
    let mut roots = HashSet::new();
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
                    crate::Comptime::reachable_owned_function_roots(
                        expression, raw_funcs, &mut roots,
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
    (
        crate::Comptime::reachable_owned_function_closure(roots, raw_funcs),
        all_methods,
    )
}

/// The staged comptime pass: check every function a compile-time site can
/// reach, without running any compile-time expression, so the real pass can
/// hand them to the evaluator. Returns the staged functions that checked
/// without errors. Staged checks run no evaluation and keep no products, so
/// they are independent jobs on the worker pool; the table is filled in key
/// order afterwards.
fn stage_comptime_functions(
    cx: &BodyContext<'_>,
    items: &[Item],
    ct_funcs: &HashMap<String, Func>,
    name_ledger: &jet_foundation::Names::NameLedger,
) -> HashMap<String, Func> {
    let stage_jobs = comptime_stage_jobs(items);
    let mut raw_eval_funcs = ct_funcs
        .iter()
        .map(|(name, function)| (name.clone(), function))
        .collect::<HashMap<_, _>>();
    for (key, job) in &stage_jobs {
        raw_eval_funcs.insert(key.clone(), job.function);
    }
    let (stage_names, stage_all_methods) = comptime_stage_roots(items, &raw_eval_funcs);
    let mut stage_keys = BTreeSet::new();
    for (key, job) in &stage_jobs {
        if stage_all_methods && job.owner.is_some()
            || stage_names.contains(key)
            || stage_names.contains(&job.function.name)
        {
            stage_keys.insert(key.clone());
        }
    }
    let mut staged_unqualified = stage_names.into_iter().collect::<BTreeSet<_>>();
    for key in &stage_keys {
        if let Some(job) = stage_jobs.get(key) {
            staged_unqualified.insert(job.function.name.clone());
        }
    }
    let stage = |owner: Option<&str>, raw_protocol_return: bool, function: &Func| {
        let mut function = function.clone();
        let mut ledger = name_ledger.body_snapshot();
        let mut products = BodyProducts::default();
        let diagnostics = check_func_body(
            cx,
            &mut function,
            owner,
            raw_protocol_return,
            None,
            None,
            &mut ledger,
            &mut products,
        );
        if products.uses_exact_int {
            cx.states[cx.module_idx].exact_int_reachable.set(true);
        }
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error)
            .then_some(function)
    };
    let keyed = stage_keys
        .into_iter()
        .filter_map(|key| stage_jobs.get(&key).map(|job| (key, job)))
        .collect::<Vec<_>>();
    let staged = super::Parallel::map_checked(keyed, |(key, job)| {
        (
            key,
            job.owner.clone(),
            stage(job.owner.as_deref(), job.raw_protocol_return, job.function),
        )
    });
    let mut checked_ct_funcs = HashMap::new();
    for (key, owner, function) in staged {
        let Some(function) = function else {
            continue;
        };
        checked_ct_funcs.insert(key, function.clone());
        if let Some(owner) = owner {
            checked_ct_funcs.insert(format!("{owner}::{}", function.name), function.clone());
        }
        checked_ct_funcs.insert(function.name.clone(), function);
    }
    // A direct comptime root can be a top-level function that does not need a
    // job-specific owner context. Keep that path explicit without staging
    // unrelated runtime helpers.
    let unqualified = staged_unqualified
        .into_iter()
        .filter(|name| !checked_ct_funcs.contains_key(name))
        .filter_map(|name| ct_funcs.get(&name).map(|function| (name, function)))
        .collect::<Vec<_>>();
    let staged = super::Parallel::map_checked(unqualified, |(name, function)| {
        (name, stage(None, false, function))
    });
    for (name, function) in staged {
        let Some(function) = function else {
            continue;
        };
        checked_ct_funcs.insert(name, function.clone());
        checked_ct_funcs.insert(function.name.clone(), function);
    }
    checked_ct_funcs
}

/// The synthetic function an error conversion body is checked as. It takes
/// the conversion's body; the caller moves the checked body back.
fn error_conversion_function(ec: &mut crate::AST::ErrorConvDef) -> Func {
    Func {
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
    }
}

/// One body of a module's main checking pass.
struct BodyJob<'m> {
    owner: Option<String>,
    raw_protocol_return: bool,
    kind: BodyJobKind<'m>,
}

enum BodyJobKind<'m> {
    /// A top-level function or a method, checked in place.
    Function {
        function: &'m mut Func,
        cache_key: String,
        /// The names under which the checked body joins `checked_ct_funcs`.
        ct_keys: Vec<String>,
        /// A method is checked with its owner's generic parameters in scope;
        /// these are the parameters its declaration keeps afterwards.
        restore_type_params: Option<Vec<crate::AST::TypeParam>>,
        /// For a method of a compiler-generated trait-impl block, the block's
        /// trait span (the derived type's name): where its diagnostics land.
        generated_anchor: Option<crate::Diagnostics::Span>,
    },
    /// A `#Test` block, checked as a synthetic function.
    Test {
        test: &'m mut crate::AST::TestDef,
        function: Func,
        param_diagnostics: Vec<Diagnostic>,
    },
    /// A function of an inline code module, checked under its imports.
    InlineModule { function: &'m mut Func, module: String },
    /// An error conversion, checked as a synthetic function.
    ErrorConv {
        conversion: &'m mut crate::AST::ErrorConvDef,
        function: Func,
    },
}

impl BodyJob<'_> {
    fn function(&self) -> &Func {
        match &self.kind {
            BodyJobKind::Function { function, .. } => &**function,
            BodyJobKind::InlineModule { function, .. } => &**function,
            BodyJobKind::Test { function, .. } => function,
            BodyJobKind::ErrorConv { function, .. } => function,
        }
    }

    fn function_mut(&mut self) -> &mut Func {
        match &mut self.kind {
            BodyJobKind::Function { function, .. } => &mut **function,
            BodyJobKind::InlineModule { function, .. } => &mut **function,
            BodyJobKind::Test { function, .. } => function,
            BodyJobKind::ErrorConv { function, .. } => function,
        }
    }
}

/// The main pass's body jobs for `items`, in source order. A method gets its
/// owner's generic parameters here, before its check; the job restores the
/// declaration's own parameters when it is merged.
fn collect_body_jobs<'m>(
    items: &'m mut [Item],
    st: &ModuleState,
    module_key: &str,
    mode: CompileMode,
    invalid_serde_impls: &HashSet<(String, String)>,
) -> Vec<BodyJob<'m>> {
    let mut jobs = Vec::new();
    for item in items.iter_mut() {
        match item {
            Item::Func(f) => jobs.push(BodyJob {
                owner: None,
                raw_protocol_return: false,
                kind: BodyJobKind::Function {
                    cache_key: format!("{module_key}::fn:{}", f.name),
                    ct_keys: vec![f.name.clone()],
                    restore_type_params: None,
                    generated_anchor: None,
                    function: f,
                },
            }),
            Item::Struct(s) => {
                push_owner_method_jobs(
                    &mut jobs,
                    module_key,
                    "struct",
                    &s.name,
                    &s.type_params,
                    &mut s.methods,
                    &mut s.trait_impls,
                );
            }
            Item::Enum(e) => {
                push_owner_method_jobs(
                    &mut jobs,
                    module_key,
                    "enum",
                    &e.name,
                    &e.type_params,
                    &mut e.methods,
                    &mut e.trait_impls,
                );
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
                let type_name = i.type_name.clone();
                let trait_name = i.trait_name.clone();
                for m in i.methods.iter_mut() {
                    let own_params = std::mem::take(&mut m.type_params);
                    // An implementation method sees the owner's generic
                    // parameters whether the impl names a trait or not. A
                    // typed derive body can fill a method hole with `T`, and
                    // the generated `impl Type.Trait` must have the same
                    // scope as a hand-written trait impl.
                    m.type_params = if own_params.is_empty() {
                        owner_params.cloned().unwrap_or_default()
                    } else {
                        own_params.clone()
                    };
                    let raw_protocol_return =
                        uses_raw_protocol_function_return(trait_name.as_deref(), false, m);
                    jobs.push(BodyJob {
                        owner: Some(type_name.clone()),
                        raw_protocol_return,
                        kind: BodyJobKind::Function {
                            cache_key: format!(
                                "{module_key}::impl:{}::{}::method:{}",
                                type_name,
                                trait_name.as_deref().unwrap_or("inherent"),
                                m.name
                            ),
                            ct_keys: vec![format!("{}::{}", type_name, m.name), m.name.clone()],
                            restore_type_params: Some(own_params),
                            generated_anchor: None,
                            function: m,
                        },
                    });
                }
            }
            Item::Test(t) if matches!(mode, CompileMode::Test | CompileMode::TestOverride) => {
                let Some(test_name) = t.name.clone() else {
                    continue;
                };
                // D-TEST1: a parameterized `#Test fn` is a property test — its
                // params must be generatable types so the runner can synthesize
                // inputs. Validate before checking the body so the error points
                // at the offending param type.
                let param_diagnostics = t
                    .params
                    .iter()
                    .filter_map(|p| property_param_unsupported(&p.ty, p.ty_span))
                    .collect();
                let mut function = Func::implicit_run(std::mem::take(&mut t.body), t.span);
                function.name = format!("__test_{test_name}");
                function.name_span = t.name_span;
                function.params = t.params.clone();
                jobs.push(BodyJob {
                    owner: None,
                    raw_protocol_return: false,
                    kind: BodyJobKind::Test {
                        test: t,
                        function,
                        param_diagnostics,
                    },
                });
            }
            Item::CodeModule(cm) => {
                // Type-check inline-module function bodies. Sibling calls were
                // already rewritten to mangled names by `mangle_inline_sibling_calls`,
                // and the mangled signatures are registered in `st.funcs`.
                let module = cm.name.clone();
                if let Some(body) = cm.body.as_mut() {
                    for inner in body.iter_mut() {
                        if let Item::Func(f) = inner {
                            jobs.push(BodyJob {
                                owner: None,
                                raw_protocol_return: false,
                                kind: BodyJobKind::InlineModule {
                                    function: f,
                                    module: module.clone(),
                                },
                            });
                        }
                    }
                }
            }
            Item::ErrorConv(ec) => {
                let function = error_conversion_function(ec);
                jobs.push(BodyJob {
                    owner: Some(ec.from_ty.clone()),
                    raw_protocol_return: false,
                    kind: BodyJobKind::ErrorConv {
                        conversion: ec,
                        function,
                    },
                });
            }
            _ => {}
        }
    }
    jobs
}

/// The method jobs of one struct or enum: its own methods, then the methods
/// of its nested trait impls. Trait impls nested in a type are real method
/// bodies too. They inherit the type's generic parameters, just as the Rust
/// impl emitted for them does, so those parameters are exposed to the body
/// checker while the parsed method signature is kept for codegen.
fn push_owner_method_jobs<'m>(
    jobs: &mut Vec<BodyJob<'m>>,
    module_key: &str,
    kind: &str,
    owner: &str,
    owner_params: &[crate::AST::TypeParam],
    methods: &'m mut [Func],
    trait_impls: &'m mut [crate::AST::TraitImplBlock],
) {
    for m in methods.iter_mut() {
        let own_params = std::mem::take(&mut m.type_params);
        if own_params.is_empty() {
            m.type_params = owner_params.to_vec();
        }
        jobs.push(BodyJob {
            owner: Some(owner.to_string()),
            raw_protocol_return: false,
            kind: BodyJobKind::Function {
                cache_key: format!("{module_key}::{kind}:{owner}::method:{}", m.name),
                ct_keys: vec![format!("{owner}::{}", m.name), m.name.clone()],
                restore_type_params: Some(own_params),
                generated_anchor: None,
                function: m,
            },
        });
    }
    for block in trait_impls.iter_mut() {
        let trait_name = block.trait_name.clone();
        let compiler_generated = block.compiler_generated;
        let generated_anchor = compiler_generated.then_some(block.trait_span);
        for m in block.methods.iter_mut() {
            let own_params = std::mem::take(&mut m.type_params);
            m.type_params = if own_params.is_empty() {
                owner_params.to_vec()
            } else {
                own_params.clone()
            };
            // Generated serde methods temporarily carry inherited, inferred
            // bounds solely for sema. Their Rust generics belong on the
            // enclosing impl, not on the method.
            let restore_type_params = if matches!(
                trait_name.as_str(),
                crate::Generics::ENCODE | crate::Generics::DECODE
            ) {
                Vec::new()
            } else {
                own_params
            };
            let raw_protocol_return =
                uses_raw_protocol_function_return(Some(&trait_name), compiler_generated, m);
            jobs.push(BodyJob {
                owner: Some(owner.to_string()),
                raw_protocol_return,
                kind: BodyJobKind::Function {
                    cache_key: format!(
                        "{module_key}::{kind}:{owner}::trait:{trait_name}::method:{}",
                        m.name
                    ),
                    ct_keys: vec![format!("{owner}::{}", m.name), m.name.clone()],
                    restore_type_params: Some(restore_type_params),
                    generated_anchor,
                    function: m,
                },
            });
        }
    }
}

/// What one body check produced, before it is merged.
struct BodyOutcome {
    diagnostics: Vec<Diagnostic>,
    products: BodyProducts,
    ledger: jet_foundation::Names::NameLedger,
}

/// Check one job's body into fresh products and a fresh ledger snapshot.
fn check_body_job(
    cx: &BodyContext<'_>,
    job: &mut BodyJob<'_>,
    checked_ct_funcs: &HashMap<String, Func>,
    name_ledger: &jet_foundation::Names::NameLedger,
    cache: Option<&mut IncrementalSemaCache>,
    cache_allowed: bool,
) -> BodyOutcome {
    let mut ledger = name_ledger.body_snapshot();
    let mut products = BodyProducts::default();
    let owner = job.owner.as_deref();
    let raw_protocol_return = job.raw_protocol_return;
    let diagnostics = match &mut job.kind {
        BodyJobKind::Function {
            function,
            cache_key,
            ..
        } => {
            // #2517 S3R: inside a package-record session, a body whose item
            // key has a record installs the stored checked body and replays
            // its outputs instead of being checked. A module with
            // view-returning callables keeps checking every body, as the
            // incremental cache does: their view pre-pass reads sibling bodies.
            let tracked = cache_allowed && super::ItemReuse::tracks(cx.module_idx);
            if tracked
                && super::ItemReuse::reuse(
                    cx.module_idx,
                    cache_key,
                    &mut **function,
                    owner,
                    raw_protocol_return,
                    &mut ledger,
                    &mut products,
                )
            {
                return BodyOutcome {
                    diagnostics: Vec::new(),
                    products,
                    ledger,
                };
            }
            let pristine = tracked.then(|| (**function).clone());
            let diagnostics = match cache {
                Some(cache) => check_func_body_cached(
                    cx,
                    cache_key.clone(),
                    &mut **function,
                    owner,
                    raw_protocol_return,
                    checked_ct_funcs,
                    &mut ledger,
                    &mut products,
                    cache,
                    cache_allowed,
                ),
                None => check_func_body(
                    cx,
                    &mut **function,
                    owner,
                    raw_protocol_return,
                    None,
                    Some(checked_ct_funcs),
                    &mut ledger,
                    &mut products,
                ),
            };
            if let Some(pristine) = pristine {
                super::ItemReuse::record(
                    cx.module_idx,
                    cache_key,
                    pristine,
                    &**function,
                    owner,
                    raw_protocol_return,
                    &diagnostics,
                    &products,
                    &ledger,
                );
            }
            diagnostics
        }
        BodyJobKind::Test { function, .. } => check_func_body(
            cx,
            function,
            None,
            false,
            None,
            Some(checked_ct_funcs),
            &mut ledger,
            &mut products,
        ),
        BodyJobKind::InlineModule { function, module } => check_func_body(
            cx,
            &mut **function,
            None,
            false,
            Some(module.as_str()),
            Some(checked_ct_funcs),
            &mut ledger,
            &mut products,
        ),
        BodyJobKind::ErrorConv { function, .. } => {
            let cx = BodyContext {
                no_os: false,
                gates: crate::Policy::GateSet::default(),
                ..*cx
            };
            check_func_body(
                &cx,
                function,
                owner,
                false,
                None,
                Some(checked_ct_funcs),
                &mut ledger,
                &mut products,
            )
        }
    };
    BodyOutcome {
        diagnostics,
        products,
        ledger,
    }
}

/// The main pass's shared sinks. Jobs are checked, in parallel when the pass
/// has no incremental cache, and merged into these in source order.
struct MainBodyPass<'p, 'c> {
    cx: &'p BodyContext<'c>,
    checked_ct_funcs: &'p mut HashMap<String, Func>,
    name_ledger: &'p mut jet_foundation::Names::NameLedger,
    products: &'p mut BodyProducts,
    diags: &'p mut Vec<Diagnostic>,
    incremental: Option<&'p mut IncrementalSemaCache>,
    cache_allowed: bool,
}

impl MainBodyPass<'_, '_> {
    fn run(&mut self, jobs: Vec<BodyJob<'_>>) {
        let workers = if self.incremental.is_some() {
            1
        } else {
            super::Parallel::check_worker_count(jobs.len())
        };
        if workers <= 1 {
            // Serial: each body sees every body checked before it.
            for mut job in jobs {
                let outcome = self.check(&mut job);
                self.merge(job, outcome);
            }
            return;
        }
        // Parallel: every body is checked once against the staged table. A
        // body whose check ran the compile-time evaluator depends on the
        // bodies before it (their checked forms join `checked_ct_funcs` as
        // they are merged), so the worker puts its pristine form back and
        // drops that outcome at once; it is checked again below, in source
        // order, against exactly the table a serial pass gives it. Holding
        // only one form of each body keeps the peak near a serial pass's.
        let checked = {
            let cx = self.cx;
            let staged = &*self.checked_ct_funcs;
            let name_ledger = &*self.name_ledger;
            let cache_allowed = self.cache_allowed;
            super::Parallel::map_checked(jobs, |mut job| {
                let pristine = job.function().clone();
                let outcome =
                    check_body_job(cx, &mut job, staged, name_ledger, None, cache_allowed);
                if outcome.products.ran_ct_evaluator {
                    *job.function_mut() = pristine;
                    (job, None)
                } else {
                    (job, Some(outcome))
                }
            })
        };
        for (mut job, outcome) in checked {
            let outcome = match outcome {
                Some(outcome) => outcome,
                None => self.check(&mut job),
            };
            self.merge(job, outcome);
        }
    }

    fn check(&mut self, job: &mut BodyJob<'_>) -> BodyOutcome {
        check_body_job(
            self.cx,
            job,
            &*self.checked_ct_funcs,
            &*self.name_ledger,
            self.incremental.as_deref_mut(),
            self.cache_allowed,
        )
    }

    fn merge(&mut self, job: BodyJob<'_>, outcome: BodyOutcome) {
        let BodyOutcome {
            diagnostics,
            products: mut body,
            ledger,
        } = outcome;
        if body.uses_exact_int {
            self.cx.states[self.cx.module_idx]
                .exact_int_reachable
                .set(true);
        }
        match job.kind {
            BodyJobKind::Function {
                function,
                ct_keys,
                restore_type_params,
                generated_anchor,
                ..
            } => {
                self.diags
                    .extend(author_facing_diagnostics(generated_anchor, diagnostics));
                self.merge_ledger(&ledger);
                self.products.absorb(body);
                for key in ct_keys {
                    self.checked_ct_funcs.insert(key, function.clone());
                }
                if let Some(type_params) = restore_type_params {
                    function.type_params = type_params;
                }
            }
            BodyJobKind::Test {
                test,
                function,
                param_diagnostics,
            } => {
                self.diags.extend(param_diagnostics);
                self.diags.extend(diagnostics);
                self.merge_ledger(&ledger);
                self.products.absorb(body);
                test.body = function.body;
            }
            BodyJobKind::InlineModule { function, module } => {
                self.diags.extend(diagnostics);
                self.merge_ledger(&ledger);
                // Inline-module calls use their registered mangled identity
                // (`__jet_module__fn`): the body's summary moves to that key and
                // any top-level same-name summary stays in place.
                let summary = body.summaries.remove(&function.name);
                let function_key = jet_foundation::Names::member_name(&module, &function.name);
                for pending in &mut body.pending_diagnostics {
                    pending.function_key = function_key.clone();
                }
                self.products.absorb(body);
                if let Some(summary) = summary {
                    self.products.summaries.insert(
                        crate::Sema::inline_effect_key(&module, &function.name),
                        summary,
                    );
                }
            }
            BodyJobKind::ErrorConv {
                conversion,
                function,
            } => {
                // Error-conversion bodies are checked like functions, but they
                // are not functions: their synthetic names and local analysis
                // artifacts stay out of the program-wide accumulators.
                self.diags.extend(diagnostics);
                self.products
                    .pending_diagnostics
                    .extend(body.pending_diagnostics);
                self.products
                    .devtools_publications
                    .extend(body.devtools_publications);
                conversion.body = function.body;
            }
        }
    }

    fn merge_ledger(&mut self, ledger: &jet_foundation::Names::NameLedger) {
        self.name_ledger.merge_references(ledger);
        self.name_ledger.merge_structure_facts(ledger);
    }
}

/// Check every body of one module. Bodies are independent jobs checked on a
/// bounded worker pool (see `Parallel`) against the module's final tables;
/// their products are merged in source order, so the checked module,
/// diagnostics and facts equal a serial run's. `JET_CHECK_THREADS=1` runs the
/// same jobs one by one.
#[allow(clippy::too_many_arguments)]
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
    name_ledger: &mut jet_foundation::Names::NameLedger,
    products: &mut BodyProducts,
    mut incremental: Option<&mut IncrementalSemaCache>,
) -> Vec<Diagnostic> {
    let st = &states[module_idx];
    // D-COMPILE-SPEED1 (#3661): registration is over and `states` stays shared
    // for every body below, so structural type answers (clone, send, view,
    // heap) are remembered across bindings and functions of this module.
    let _nominal_memo = st.registry.open_nominal_memo();
    let mut diags = Vec::new();
    let (ct_funcs, ct_externs, ct_globals) = comptime_context_from_items(&module.items);
    let invalid_serde_impls = invalid_serde_derive_impls(&module.items, &st.trait_reg);
    let ct_base_dir = module
        .path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let cx = BodyContext {
        module_idx,
        states,
        plugin_interfaces,
        devtools_registry,
        effect_facts,
        ct_funcs: &ct_funcs,
        ct_externs: &ct_externs,
        ct_base_dir: &ct_base_dir,
        ct_globals: &ct_globals,
        no_os,
        gates,
        no_prelude: module.no_prelude,
    };
    let checked_ct_funcs = stage_comptime_functions(&cx, &module.items, &ct_funcs, name_ledger);
    // D-MEM-VIEWRET1=B: resolve callable view summaries before the real body
    // pass so declaration order cannot affect a public owner contract. Each
    // iteration checks pristine clones and publishes only the canonical fact;
    // diagnostics and other analysis products are discarded. Tentative facts
    // let mutually recursive SCCs converge; the real pass below still rejects
    // any path that ultimately conflicts or cannot stabilize.
    // Jobs borrow their functions: most are dropped by the view filter below,
    // and each round clones the kept ones afresh anyway (#3661).
    struct ViewSummaryJob<'m> {
        key: String,
        owner: Option<String>,
        trait_name: Option<String>,
        raw_protocol_return: bool,
        function: &'m Func,
    }
    let mut view_jobs = Vec::new();
    for item in &module.items {
        match item {
            Item::Func(function) => view_jobs.push(ViewSummaryJob {
                key: function.name.clone(),
                owner: None,
                trait_name: None,
                raw_protocol_return: false,
                function,
            }),
            Item::Struct(definition) => {
                for function in &definition.methods {
                    view_jobs.push(ViewSummaryJob {
                        key: format!("{}::{}", definition.name, function.name),
                        owner: Some(definition.name.clone()),
                        trait_name: None,
                        raw_protocol_return: false,
                        function,
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
                            raw_protocol_return: uses_raw_protocol_function_return(
                                Some(implementation.trait_name.as_str()),
                                implementation.compiler_generated,
                                function,
                            ),
                            function,
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
                        function,
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
                            raw_protocol_return: uses_raw_protocol_function_return(
                                Some(implementation.trait_name.as_str()),
                                implementation.compiler_generated,
                                function,
                            ),
                            function,
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
                        raw_protocol_return: uses_raw_protocol_function_return(
                            implementation.trait_name.as_deref(),
                            false,
                            function,
                        ),
                        function,
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
    // Each round publishes every job's `return_view_provenance` (BodyCheck)
    // and each complete trait contract. When a round yields exactly the
    // previous round's per-job results, it published the same values, so the
    // next round would start from the same state and repeat itself: that is
    // the fixed point. `view_jobs.len() + 1` rounds stays the upper bound;
    // running all of them after convergence made this pass quadratic in the
    // number of view-returning functions.
    let mut previous_round: Option<Vec<Option<crate::AST::ViewProvenanceMap>>> = None;
    for _ in 0..=view_jobs.len() {
        let mut trait_candidates =
            HashMap::<(String, String), Vec<crate::AST::ViewProvenanceMap>>::new();
        let mut round = Vec::with_capacity(view_jobs.len());
        for job in &view_jobs {
            let mut function = job.function.clone();
            let mut scratch_ledger = name_ledger.body_snapshot();
            let mut scratch = BodyProducts::default();
            let _ = check_func_body(
                &cx,
                &mut function,
                job.owner.as_deref(),
                job.raw_protocol_return,
                None,
                Some(&checked_ct_funcs),
                &mut scratch_ledger,
                &mut scratch,
            );
            if scratch.uses_exact_int {
                st.exact_int_reachable.set(true);
            }
            round.push(function.return_view_provenance.clone());
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
        if previous_round.as_ref() == Some(&round) {
            break;
        }
        previous_round = Some(round);
    }
    let cache_allowed = view_jobs.is_empty();
    let module_key = module.display.clone();
    let mut checked_ct_funcs = checked_ct_funcs;
    let mut pass = MainBodyPass {
        cx: &cx,
        checked_ct_funcs: &mut checked_ct_funcs,
        name_ledger: &mut *name_ledger,
        products: &mut *products,
        diags: &mut diags,
        incremental: incremental.as_deref_mut(),
        cache_allowed,
    };
    // D-FAIL-CONV2=A: the shipped `impl <CoreError> -> Err` conversions are
    // demand-driven — which ones a module needs is only known once every body
    // in it has recorded its `TryConvert::Typed` facts. So they are injected
    // after the module's own bodies are checked, and each appended body then
    // goes through the `Item::ErrorConv` job exactly like a user-declared
    // conversion. Injecting after this function returns instead left the
    // shipped bodies unchecked: `Err("{self}")` stayed an ordinary
    // `Expr::Call`, never normalized into the default-error `Err` struct
    // literal, and codegen's TIR gate refused it as an uncovered construct
    // (an I2 abort, not a user diagnostic).
    let declared_items = module.items.len();
    pass.run(collect_body_jobs(
        &mut module.items,
        st,
        &module_key,
        mode,
        &invalid_serde_impls,
    ));
    pass.diags
        .extend(super::super::Prelude::inject_exercised_error_conversions(module));
    pass.run(collect_body_jobs(
        &mut module.items[declared_items..],
        st,
        &module_key,
        mode,
        &invalid_serde_impls,
    ));
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
            let mut wrapper_ledger = name_ledger.body_snapshot();
            let mut wrapper_products = BodyProducts::default();
            diags.extend(check_func_body(
                &cx,
                &mut wrapper,
                None,
                false,
                None,
                Some(&checked_ct_funcs),
                &mut wrapper_ledger,
                &mut wrapper_products,
            ));
            if wrapper_products.uses_exact_int {
                st.exact_int_reachable.set(true);
            }
            for pending in &mut wrapper_products.pending_diagnostics {
                pending.function_key = format!("policy:{}::{}", policy_name, target.name);
            }
            products
                .pending_diagnostics
                .extend(wrapper_products.pending_diagnostics);
            products
                .devtools_publications
                .extend(wrapper_products.devtools_publications);
            if generated_policy_wrapper_names.insert(wrapper.name.clone()) {
                generated_policy_wrappers.push(crate::AST::Item::Func(wrapper));
            }
        }
    }
    module.items.extend(generated_policy_wrappers);
    let _ = st;
    diags
}
