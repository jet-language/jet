use super::*;

pub(super) fn merge_view_provenance(
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn checker_for_module<'a>(
    module_idx: usize,
    states: &'a [ModuleState],
    plugin_interfaces: &'a PluginInterfaceRegistry,
    devtools_registry: &'a jet_foundation::AST::DevtoolsRegistry,
    effect_facts: &'a jet_foundation::Facts::FactRegistry,
    ct_funcs: &'a HashMap<String, Func>,
    ct_checked_funcs: &'a HashMap<String, Func>,
    ct_items: &'a [crate::AST::Item],
    ct_externs: &'a HashSet<String>,
    ct_base_dir: &'a std::path::Path,
    ct_globals: &'a HashMap<String, crate::Comptime::CtValue>,
    no_os: bool,
    gates: crate::Policy::GateSet,
    no_prelude: bool,
    name_ledger: &'a mut jet_foundation::Names::NameLedger,
    function: Option<&Func>,
    raw_protocol_return: bool,
    defer_ct_evaluation: bool,
) -> Checker<'a> {
    let st = &states[module_idx];
    let current_function_span = function.map_or_else(|| Span::new(0, 0), |f| f.span);
    let in_unsafe = function.is_some_and(|f| f.is_unsafe);
    let in_pure = function.is_some_and(|f| f.is_pure || f.is_comptime);
    let in_comptime = function.is_some_and(|f| f.is_comptime);
    let compiler_api_allowed =
        function.is_some_and(|f| st.allow_compiler_api && super::is_build_entry(f));
    let ret = function.and_then(|f| crate::Sema::checked_body_return_type(f, raw_protocol_return));
    let fn_name = function.map_or_else(String::new, |f| f.name.clone());
    let compiler_generated = function.is_some_and(|f| f.compiler_generated);
    let declared_return_type = function.and_then(|f| f.return_type.clone());
    let current_return_type_span = function.and_then(|f| f.return_type_span);
    let current_param_names: Vec<String> = function
        .map(|f| {
            f.params
                .iter()
                .filter(|param| param.name != crate::Syntax::KW_SELF)
                .map(|param| param.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let type_param_scope = function
        .map(|f| f.type_params.clone())
        .unwrap_or_default();

    Checker {
        funcs: &st.funcs,
        diverging_functions: &st.diverging_functions,
        plugin_interfaces,
        registry: &st.registry,
        devtools_registry,
        effect_facts,
        consts: &st.consts,
        modules: Some(states),
        nominal_owner_cache: std::cell::RefCell::new(HashMap::new()),
        items: &st.items,
        module_idx,
        imports: &st.imports,
        core_imports: &st.core_imports,
        code_modules: &st.code_modules,
        code_module_identities: &st.code_module_identities,
        unqualified: &st.unqualified,
        unqualified_file: &st.unqualified_file,
        core_item_imports: &st.core_item_imports,
        model_outputs: &st.model_outputs,
        inline_unqualified: &st.inline_unqualified,
        inline_unqualified_file: &st.inline_unqualified_file,
        inline_module: None,
        inline_reexport_inline: &st.inline_reexport_inline,
        inline_reexport_file: &st.inline_reexport_file,
        inline_reexport_core: &st.inline_reexport_core,
        inline_reexport_foreign: &st.inline_reexport_foreign,
        module_path: &st.module_path,
        source: &st.source,
        package_scope: &st.package_scope,
        policy_declarations: &st.policy_declarations,
        callable_policy_declarations: &st.callable_policy_declarations,
        rule_facts: st.rule_facts.clone(),
        frame_schedule_systems: Vec::new(),
        current_function_span,
        name_ledger,
        diags: Vec::new(),
        statement_lint_allows: Vec::new(),
        stdlib_lint_candidates: HashMap::new(),
        unused_bindings: Vec::new(),
        unused_binding_refs: HashSet::new(),
        flow: crate::Sema::FlowFacts::FlowFacts {
            depth: 1,
            ..Default::default()
        },
        concrete_unit_values: vec![HashMap::new()],
        suppress_partial_move_root_read: false,
        loop_depth: 0,
        implicit_loop_subject_depth: 0,
        subject_shorthand_depth: 0,
        source_nesting: 0,
        statement_expr_root_depth: None,
        loop_labels: Vec::new(),
        collect_item_types: Vec::new(),
        loop_value_frames: Vec::new(),
        loop_break_flows: Vec::new(),
        pending_loop_value: None,
        arrow_loop_body: false,
        last_loop_result_type: None,
        fx_direct: std::collections::BTreeSet::new(),
        fx_direct_spans: HashMap::new(),
        lambda_effect_stack: Vec::new(),
        fx_edges: std::collections::BTreeSet::new(),
        fx_maximal: false,
        fx_maximal_span: None,
        region_stack: Vec::new(),
        fx_regions: Vec::new(),
        fx_authority_delegations: Vec::new(),
        fx_callback_obligations: Vec::new(),
        fx_autodiff_obligations: Vec::new(),
        fx_compute_calls: Vec::new(),
        fx_autodiff_safe_panic: false,
        fx_autodiff_unsafe_panic: false,
        autodiff_safe_panic_context: false,
        fx_pending_diagnostics: Vec::new(),
        fx_memory_events: Vec::new(),
        fx_memory_open: Vec::new(),
        memory_policy_stack: Vec::new(),
        arithmetic_policy_stack: Vec::new(),
        fx_memory_regions: Vec::new(),
        fx_memory_unbounded_control: Vec::new(),
        fx_memory_calls: Vec::new(),
        memory_control_multiplier: Some(1),
        txn_depth: 0,
        txn_wall_depth: 0,
        deterministic_world_depth: 0,
        det_suppress: 0,
        context_depth: 0,
        context_allocator_active: false,
        in_unsafe,
        suppress_must_use: false,
        in_pure,
        no_prelude,
        in_pre_clause: false,
        fallback_has_err: None,
        failure_auto_root_suppression: 0,
        failure_auto_depth: 0,
        fallback_is_shape_miss: false,
        in_comptime,
        compiler_api_allowed,
        ret,
        fn_name,
        compiler_generated,
        raw_protocol_return,
        declared_return_type,
        current_return_type_span,
        current_param_names,
        binder_ref_types: HashMap::new(),
        expected_type: None,
        uses_exact_int: false,
        knowledge_gate: None,
        iter_borrowed: HashSet::new(),
        noelse_chains_checked: HashSet::new(),
        result_handler_subject_types: HashMap::new(),
        lending_view_loop_vars: HashSet::new(),
        return_view_provenance: None,
        views_used_in_stmt: Default::default(),
        scoped_loan_read_reported: false,
        call_access_frames: Vec::new(),
        borrow_ctx: false,
        owning_if_value_depth: 0,
        allow_fixed_constructor: false,
        allow_string_view_read: false,
        lambda_escapes: true,
        in_lambda_body: false,
        inferred_lambda_mut_captures: HashSet::new(),
        lambda_params_are_lending_views: false,
        is_task_spawn: false,
        task_body_propagates: false,
        failure_carrier_inference: false,
        failure_carrier: None,
        ordinary_binding_root_depth: None,
        statement_expr_inference: false,
        http_handler_depth: 0,
        interrupt_callback_depth: 0,
        lambda_param_mutable: false,
        lambda_param_is_secret_loan: false,
        view_capture_tasks: HashSet::new(),
        reactive_upgrades: Vec::new(),
        reactive_upgrade_names: HashSet::new(),
        view_borrow_escape_tasks: HashSet::new(),
        current_binding_name: None,
        task_spawn_binding_name: None,
        lambda_binding: None,
        lambda_mut_borrow_stack: vec![HashSet::new()],
        trait_reg: &st.trait_reg,
        ct_funcs,
        ct_checked_funcs,
        ct_items,
        ct_externs,
        ct_base_dir,
        ct_globals,
        ct_scopes: vec![HashMap::new()],
        defer_ct_evaluation,
        type_param_scope,
        no_os,
        gates,
        ct_impure_depth: 0,
        ct_embed_inputs: Vec::new(),
        in_dropped_comptime_arm: false,
        stmt_tail_ptr: std::ptr::null(),
        stmt_tail_len: 0,
        liveness_frames: Vec::new(),
        taskgroup_stack: Vec::new(),
        in_taskgroup_spawn: false,
        inline_addr_taken: HashSet::new(),
    }
}


#[allow(clippy::too_many_arguments)]
pub(super) fn check_func_body_bundle_with_usage_checked(
    f: &mut Func,
    module_idx: usize,
    states: &[ModuleState],
    plugin_interfaces: &PluginInterfaceRegistry,
    devtools_registry: &jet_foundation::AST::DevtoolsRegistry,
    effect_facts: &jet_foundation::Facts::FactRegistry,
    owner_type: Option<&str>,
    raw_protocol_return: bool,
    ct_funcs: &HashMap<String, Func>,
    ct_checked_funcs: &HashMap<String, Func>,
    ct_items: &[crate::AST::Item],
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
) -> (Vec<Diagnostic>, bool) {
    check_func_body_bundle_scoped_with_mode(
        f,
        module_idx,
        states,
        plugin_interfaces,
        devtools_registry,
        effect_facts,
        owner_type,
        raw_protocol_return,
        ct_funcs,
        ct_checked_funcs,
        ct_items,
        false,
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
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn check_func_body_bundle_deferred(
    f: &mut Func,
    module_idx: usize,
    states: &[ModuleState],
    plugin_interfaces: &PluginInterfaceRegistry,
    devtools_registry: &jet_foundation::AST::DevtoolsRegistry,
    effect_facts: &jet_foundation::Facts::FactRegistry,
    owner_type: Option<&str>,
    raw_protocol_return: bool,
    ct_funcs: &HashMap<String, Func>,
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
) -> (Vec<Diagnostic>, bool) {
    let ct_checked_funcs = HashMap::new();
    let ct_items = states[module_idx].items.as_slice();
    check_func_body_bundle_scoped_with_mode(
        f,
        module_idx,
        states,
        plugin_interfaces,
        devtools_registry,
        effect_facts,
        owner_type,
        raw_protocol_return,
        ct_funcs,
        &ct_checked_funcs,
        ct_items,
        true,
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
        None,
    )
}


#[allow(clippy::too_many_arguments)]
pub(super) fn check_func_body_bundle_checked(
    f: &mut Func,
    module_idx: usize,
    states: &[ModuleState],
    plugin_interfaces: &PluginInterfaceRegistry,
    devtools_registry: &jet_foundation::AST::DevtoolsRegistry,
    effect_facts: &jet_foundation::Facts::FactRegistry,
    owner_type: Option<&str>,
    raw_protocol_return: bool,
    ct_funcs: &HashMap<String, Func>,
    ct_checked_funcs: &HashMap<String, Func>,
    ct_items: &[crate::AST::Item],
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
) -> Vec<Diagnostic> {
    check_func_body_bundle_with_usage_checked(
        f,
        module_idx,
        states,
        plugin_interfaces,
        devtools_registry,
        effect_facts,
        owner_type,
        raw_protocol_return,
        ct_funcs,
        ct_checked_funcs,
        ct_items,
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
    )
    .0
}


#[allow(clippy::too_many_arguments)]
pub(super) fn check_func_body_bundle_scoped_checked(
    f: &mut Func,
    module_idx: usize,
    states: &[ModuleState],
    plugin_interfaces: &PluginInterfaceRegistry,
    devtools_registry: &jet_foundation::AST::DevtoolsRegistry,
    effect_facts: &jet_foundation::Facts::FactRegistry,
    owner_type: Option<&str>,
    raw_protocol_return: bool,
    ct_funcs: &HashMap<String, Func>,
    ct_checked_funcs: &HashMap<String, Func>,
    ct_items: &[crate::AST::Item],
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
    inline_module: Option<&str>,
) -> (Vec<Diagnostic>, bool) {
    check_func_body_bundle_scoped_with_mode(
        f,
        module_idx,
        states,
        plugin_interfaces,
        devtools_registry,
        effect_facts,
        owner_type,
        raw_protocol_return,
        ct_funcs,
        ct_checked_funcs,
        ct_items,
        false,
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
        inline_module,
    )
}

#[allow(clippy::too_many_arguments)]
fn check_func_body_bundle_scoped_with_mode(
    f: &mut Func,
    module_idx: usize,
    states: &[ModuleState],
    plugin_interfaces: &PluginInterfaceRegistry,
    devtools_registry: &jet_foundation::AST::DevtoolsRegistry,
    effect_facts: &jet_foundation::Facts::FactRegistry,
    owner_type: Option<&str>,
    raw_protocol_return: bool,
    ct_funcs: &HashMap<String, Func>,
    ct_checked_funcs: &HashMap<String, Func>,
    ct_items: &[crate::AST::Item],
    defer_ct_evaluation: bool,
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
    inline_module: Option<&str>,
) -> (Vec<Diagnostic>, bool) {
    let st = &states[module_idx];
    let mut scoped_imports = st.imports.clone();
    let mut scoped_core_imports = st.core_imports.clone();
    let mut scoped_unqualified = st.unqualified.clone();
    let mut scoped_unqualified_file = st.unqualified_file.clone();
    let mut scoped_core_item_imports = st.core_item_imports.clone();
    if let Some(inline_module) = inline_module {
        for ((scope, name), target) in &st.inline_foreign_imports {
            if scope == inline_module {
                scoped_imports.insert(name.clone(), *target);
            }
        }
        for ((scope, name), module) in &st.inline_core_imports {
            if scope == inline_module {
                scoped_core_imports.insert(name.clone(), module.clone());
            }
        }
        for ((scope, name), item) in &st.inline_core_items {
            if scope == inline_module {
                scoped_core_item_imports.insert(name.clone(), item.clone());
            }
        }
        for ((scope, name), mangled) in &st.inline_unqualified {
            if scope == inline_module {
                scoped_unqualified.insert(name.clone(), mangled.clone());
            }
        }
        for ((scope, name), target) in &st.inline_unqualified_file {
            if scope == inline_module {
                scoped_unqualified_file.insert(name.clone(), target.clone());
            }
        }
    }
    let mut ck = checker_for_module(
        module_idx,
        states,
        plugin_interfaces,
        devtools_registry,
        effect_facts,
        ct_funcs,
        ct_checked_funcs,
        ct_items,
        ct_externs,
        ct_base_dir,
        ct_globals,
        no_os,
        gates,
        no_prelude,
        name_ledger,
        Some(f),
        raw_protocol_return,
        defer_ct_evaluation,
    );
    ck.imports = &scoped_imports;
    ck.core_imports = &scoped_core_imports;
    ck.unqualified = &scoped_unqualified;
    ck.unqualified_file = &scoped_unqualified_file;
    ck.core_item_imports = &scoped_core_item_imports;
    ck.inline_module = inline_module.map(str::to_owned);
    // Canonicalize the declaration before the shared body checker projects
    // the implicit failure carrier. This keeps alias-backed returns from
    // becoming nested `Result<Result<...>, Err>` values in later seams.
    ck.canonicalize_function_return_type(f);
    ck.ret = crate::Sema::checked_body_return_type(f, raw_protocol_return);
    for (active, name, span) in [
        (f.is_pure, crate::Syntax::KW_PURE, f.name_span),
        (f.is_sanitizer, crate::Syntax::KW_SANITIZER, f.name_span),
        (
            f.is_unsafe,
            crate::Syntax::KW_UNSAFE,
            f.unsafe_span.unwrap_or(f.name_span),
        ),
        (
            f.is_replayable,
            crate::Syntax::MARKER_REPLAYABLE,
            f.replayable_span.unwrap_or(f.name_span),
        ),
        (
            f.is_must_use,
            crate::Syntax::MARKER_MUST_USE,
            f.must_use_span.unwrap_or(f.name_span),
        ),
        (
            f.is_inline,
            crate::Syntax::MARKER_INLINE,
            f.inline_span.unwrap_or(f.name_span),
        ),
        (
            f.is_inline_always,
            crate::Syntax::MARKER_INLINE,
            f.inline_span.unwrap_or(f.name_span),
        ),
        (f.is_reactive, crate::Syntax::KW_REACTIVE, f.name_span),
    ] {
        if active && !crate::Policy::rule_allows(name, crate::Policy::RuleSite::Function) {
            ck.diags.push(Diagnostic::error("E0355", format!("`#{name}` cannot attach to a function"), "the compiler-owned applicability registry is shared by parser, sema, formatter, semantic index, and explain".to_string(), "move the rule to one of its registered sites".to_string(), Some(span)));
        }
    }
    ck.check_params_and_body(f, owner_type);
    // D-NEVER2=B: the declared bottom contract must have no normal exit.
    // The shared flow proof already understands loops, panic, and known
    // diverging calls, so report the actual fallthrough boundary here.
    if f.return_type
        .as_ref()
        .is_some_and(Type::has_never_success)
        && body_flow(&f.body, &st.diverging_functions, st).normal
    {
        let span = f
            .body
            .last()
            .map(|stmt| stmt.span())
            .unwrap_or(f.return_type_span.unwrap_or(f.name_span));
        ck.diags.push(Diagnostic::from_row(
            "E2423",
            &[("function", f.name.as_str())],
            Some(span),
        ));
    }
    // D-NEVER2=B: preserve the inferred D-NEVER1 fact for internal callers,
    // while making an always-diverging public API explicit for downstream
    // users and API snapshots.
    if f.is_pub && f.return_type.is_none() && f.diverges {
        ck.diags.push(Diagnostic::from_row(
            "L2421",
            &[("function", f.name.as_str())],
            Some(f.name_span),
        ));
    }
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
                let Some(declared_prov) = declared.get(slot).or_else(|| declared.get(&Vec::new()))
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
                    declared_prov.mutable = leaves
                        .iter()
                        .any(|(path, access)| path == slot && *access == ViewAccess::Write);
                }
            }
            f.return_view_provenance = Some(merged);
        } else {
            f.return_view_provenance = Some(merged);
        }
    }
    if let Some(owner) = owner_type {
        if let (Some(signature), Some(provenance)) = (
            st.registry.method(owner, &f.name),
            f.return_view_provenance.clone(),
        ) {
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
    // inferred-pure callee need not repeat `-[]>`.
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
    ck.diags.extend(check_every_marker(f, &st.registry));
    global_addr_taken.extend(std::mem::take(&mut ck.inline_addr_taken));
    // D-EXPANDCLI1 (card #183): roll this function's resolved ref-owner facts
    // into the whole-bundle accumulator for `jet inspect expand --facts refs`.
    // D-CTEFFECT1 Tier-1: drain embed inputs into the caller's accumulator.
    embed_inputs_out.extend(std::mem::take(&mut ck.ct_embed_inputs));
    // D-EFFECT-OMIT1/D-EFF3: an explicit row is an upper bound, not an effect
    // declaration. Static calls propagate the implementation's inferred body
    // row; dynamic trait calls use the trait method bound separately.
    let mut direct = std::mem::take(&mut ck.fx_direct);
    let mut direct_spans = std::mem::take(&mut ck.fx_direct_spans);
    let memory_events = std::mem::take(&mut ck.fx_memory_events);
    // D-AUTHORITY-MEM1: memory operations publish the same rights-tree names
    // as manifest `authority.holds.deny`. The detailed event stream remains the
    // source for bounded proofs and diagnostics.
    for event in &memory_events {
        let right = match event.kind {
            super::super::MemoryEventKind::Allocation
            | super::super::MemoryEventKind::ArenaBytes(_) => "Mem.Alloc",
            super::super::MemoryEventKind::RetainRelease => "Mem.Rc",
        };
        direct.insert(right.to_string());
        direct_spans.entry(right.to_string()).or_insert(event.span);
    }
    let mut memory_events = memory_events;
    for event in &mut memory_events {
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
            direct_spans,
            edges: std::mem::take(&mut ck.fx_edges),
            maximal: ck.fx_maximal,
            maximal_span: ck.fx_maximal_span,
            unbounded_trait_dispatch: false,
            regions: std::mem::take(&mut ck.fx_regions),
            authority_delegations: std::mem::take(&mut ck.fx_authority_delegations),
            callback_obligations: std::mem::take(&mut ck.fx_callback_obligations),
            autodiff_obligations: std::mem::take(&mut ck.fx_autodiff_obligations),
            compute_calls: std::mem::take(&mut ck.fx_compute_calls),
            autodiff_safe_panic: ck.fx_autodiff_safe_panic,
            autodiff_unsafe_panic: ck.fx_autodiff_unsafe_panic,
            memory: super::MemoryFacts::MemorySummary {
                events: memory_events,
                open_dispatches: std::mem::take(&mut ck.fx_memory_open),
                regions: std::mem::take(&mut ck.fx_memory_regions),
                unbounded_control: std::mem::take(&mut ck.fx_memory_unbounded_control),
                calls: std::mem::take(&mut ck.fx_memory_calls),
            },
        },
    );
    let uses_exact_int = ck.uses_exact_int;
    if uses_exact_int {
        st.exact_int_reachable.set(true);
    }
    (ck.diags, uses_exact_int)
}

/// D-DATARACE1=C: mark reactive bindings that crossed a concurrency boundary so
/// codegen can emit the upgrade report comments.
pub(super) fn apply_reactive_upgrade_flags(stmts: &mut [Stmt], names: &std::collections::HashSet<String>) {
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
                | Stmt::AuthorityScope { body, .. }
                | Stmt::ComptimeBlock { body, .. }
                | Stmt::ContextBlock { body, .. }
                | Stmt::Live { body, .. }
                | Stmt::AssumeDet { body, .. }
                | Stmt::Transact { body, .. }
                | Stmt::Switched { body, .. } => walk(body, names),
                Stmt::Switch {
                    arms, else_body, ..
                }
                | Stmt::ComptimeSwitch {
                    arms, else_body, ..
                } => {
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
 

/// Project the callable contract into a function value's type without
/// promoting declaration-local names to public labels. Keep an entry for
/// every parameter so positional-only and label-only zones remain visible;
/// `call_metadata` retains the local names for diagnostics and calls.
pub(super) fn function_value_param_contract(sig: &FuncSig) -> Option<Vec<(String, crate::AST::ParamZone)>> {
    let mut contract = Vec::with_capacity(sig.param_call.len());
    for (index, (label, zone)) in sig.param_call.iter().enumerate() {
        let implicit_local_label = *zone == crate::AST::ParamZone::Either
            && sig
                .param_info
                .get(index)
                .is_some_and(|(name, _)| name == label);
        let label = match zone {
            crate::AST::ParamZone::PositionalOnly => String::new(),
            crate::AST::ParamZone::Either if implicit_local_label => String::new(),
            _ => label.clone(),
        };
        contract.push((label, *zone));
    }
    contract
        .iter()
        .any(|(label, zone)| !label.is_empty() || *zone != crate::AST::ParamZone::Either)
        .then_some(contract)
}

pub(crate) fn func_sig_to_fn_type(sig: &FuncSig) -> Type {
    Type::Fn {
        params: sig
            .params
            .iter()
            .enumerate()
            .map(|(index, (_, ty))| {
                if sig.param_variadic.get(index).copied().unwrap_or(false) {
                    Type::List(Box::new(ty.clone()))
                } else {
                    ty.clone()
                }
            })
            .collect(),
        ret: Some(Box::new(sig.effective_return_type())),
        // D-CABI-CALLBACK1 / D-EFF2: a function sema proved effect-free
        // (`pure fn`, or an allocation-free panic-free scalar body) publishes
        // the empty effect bound, so its value satisfies `-[]>` callable
        // positions without a second policing mechanism.
        effect_bound: (sig.is_pure || sig.is_foreign_thread_safe).then(Vec::new),
        param_contract: function_value_param_contract(sig),
        call_metadata: Some(crate::AST::FunctionCallMetadata {
            names: sig
                .param_info
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            defaults: sig.defaults.clone(),
            variadic: sig.param_variadic.clone(),
            conventions: sig
                .params
                .iter()
                .map(|(convention, _)| *convention)
                .collect(),
            policies: sig.callable_policies.clone(),
        }),
        return_view_provenance: sig.return_view_provenance.get(),
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn checked_func_sig_to_fn_type(&mut self, sig: &FuncSig) -> Type {
        let mut function_type = func_sig_to_fn_type(sig);
        if let Type::Fn {
            ret: Some(ret), ..
        } = &mut function_type
        {
            let (_, effective) = self.checked_return_types(sig.return_type.clone(), sig.is_extern);
            *ret = Box::new(effective);
        }
        function_type
    }
}

pub(crate) fn fn_types_compatible(want: &Type, got: &Type) -> bool {
    let want = want.with_effective_fn_returns();
    let got = got.with_effective_fn_returns();

    fn carrier_compatible(want: &Type, got: &Type) -> bool {
        let mut compatible = true;
        let shape = Type::for_each_composite_pair(want, got, &mut |want, got| {
            if compatible {
                compatible = match (want, got) {
                    (Type::List(_), Type::List(_))
                    | (Type::Option(_), Type::Option(_))
                    | (Type::Result { .. }, Type::Result { .. }) => true,
                    (
                        Type::Apply {
                            name: want_name, ..
                        },
                        Type::Apply { name: got_name, .. },
                    ) => want_name == got_name,
                    (Type::Fn { .. }, Type::Fn { .. }) => fn_types_compatible(want, got),
                    _ => want == got,
                };
            }
        });
        shape.is_ok() && compatible
    }
    fn effect_bound_compatible(
        want: Option<&Vec<(String, Span)>>,
        got: Option<&Vec<(String, Span)>>,
    ) -> bool {
        match want {
            None => true,
            Some(want) => got.is_some_and(|got| {
                got.iter()
                    .all(|(effect, _)| want.iter().any(|(required, _)| required == effect))
            }),
        }
    }
    let (
        Type::Fn {
            params: wp,
            ret: wr,
            effect_bound: we,
            ..
        },
        Type::Fn {
            params: gp,
            ret: gr,
            effect_bound: ge,
            ..
        },
    ) = (&want, &got)
    else {
        return false;
    };
    if wp.len() != gp.len() {
        return false;
    }
    for (a, b) in wp.iter().zip(gp.iter()) {
        if !carrier_compatible(a, b) {
            return false;
        }
    }
    let return_compatible = match (wr, gr) {
        (None, None) => true,
        (Some(a), Some(b)) => carrier_compatible(a, b),
        (None, Some(b)) | (Some(b), None) => crate::AST::is_unit_callable_return(b),
    };
    return_compatible && effect_bound_compatible(we.as_ref(), ge.as_ref())
}

#[cfg(test)]
mod callable_contract_tests {
    use super::fn_types_compatible;
    use crate::AST::{ParamZone, Type};

    fn callable(contract: Option<Vec<(&str, ParamZone)>>) -> Type {
        Type::Fn {
            params: vec![Type::Bool],
            ret: Some(Box::new(Type::Int)),
            effect_bound: None,
            param_contract: contract.map(|entries| {
                entries
                    .into_iter()
                    .map(|(label, zone)| (label.to_string(), zone))
                    .collect()
            }),
            call_metadata: None,
            return_view_provenance: None,
        }
    }

    #[test]
    fn function_contract_metadata_does_not_affect_assignability() {
        let bare = callable(None);
        let labelled = callable(Some(vec![("force", ParamZone::LabelOnly)]));
        let either = callable(Some(vec![("force", ParamZone::Either)]));

        assert!(fn_types_compatible(&bare, &labelled));
        assert!(fn_types_compatible(&labelled, &bare));
        assert!(fn_types_compatible(&labelled, &either));
        assert!(fn_types_compatible(&either, &labelled));
    }

    #[test]
    fn composite_callable_assignability_walks_nested_carriers() {
        fn nested(zone: ParamZone) -> Type {
            Type::Fn {
                params: vec![Type::Bool],
                ret: Some(Box::new(Type::Int)),
                effect_bound: None,
                param_contract: Some(vec![("force".to_string(), zone)]),
                call_metadata: None,
                return_view_provenance: None,
            }
        }
        fn outer(name: &str, inner: Type) -> Type {
            Type::Fn {
                params: vec![Type::Apply {
                    name: name.to_string(),
                    args: vec![
                        Type::List(Box::new(inner)),
                        Type::Option(Box::new(Type::Result {
                            ok: Box::new(Type::Int),
                            err: Box::new(Type::String),
                        })),
                    ],
                }],
                ret: None,
                effect_bound: None,
                param_contract: None,
                call_metadata: None,
                return_view_provenance: None,
            }
        }

        assert!(fn_types_compatible(
            &outer("SyntheticCarrier", nested(ParamZone::PositionalOnly)),
            &outer("SyntheticCarrier", nested(ParamZone::Either)),
        ));
        assert!(fn_types_compatible(
            &outer("SyntheticCarrier", nested(ParamZone::LabelOnly)),
            &outer("SyntheticCarrier", nested(ParamZone::PositionalOnly)),
        ));
        assert!(!fn_types_compatible(
            &outer("SyntheticCarrier", nested(ParamZone::PositionalOnly)),
            &outer("OtherCarrier", nested(ParamZone::PositionalOnly)),
        ));
    }
}

/// D-TEST1: which parameter types the property-test runner can synthesize inputs
/// for. The generator (codegen) covers the scalar value types plus `[T]` and
/// `T?` of a generatable element. Anything else (user structs/enums, `Map`,
/// functions, trait objects) has no automatic generator yet, so reject it with a
/// clear error rather than miscompile (I3 — checking lives in sema).
pub(super) fn property_param_generatable(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::Float32
        | Type::IntN { .. }
        | Type::InlineRange { .. } => true,
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
/// D-NEVER1=C: compute one bundle-wide fixed point of user functions whose
/// execution has no returning path. The result is a sema fact only; it is
/// copied into each module state before body checking and never becomes a type.
pub(crate) fn collect_diverging_functions(
    states: &[ModuleState],
) -> std::collections::HashSet<String> {
    let mut diverging = std::collections::HashSet::new();
    // D-NEVER2=B: declared `Never` success contracts are bottom facts before
    // the fixed point starts. Their `Err` route remains a real fallible exit,
    // but a direct call has no successful continuation.
    for state in states {
        for item in &state.items {
            match item {
                Item::Func(function)
                    if function
                        .return_type
                        .as_ref()
                        .is_some_and(Type::has_never_success) =>
                {
                    diverging.insert(function.name.clone());
                }
                Item::CodeModule(module) => {
                    let Some(body) = &module.body else {
                        continue;
                    };
                    for inner in body {
                        let Item::Func(function) = inner else {
                            continue;
                        };
                        if function
                            .return_type
                            .as_ref()
                            .is_some_and(Type::has_never_success)
                        {
                            diverging.insert(jet_foundation::Names::member_name(
                                &module.name,
                                &function.name,
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    loop {
        let mut changed = false;
        for state in states {
            for item in &state.items {
                match item {
                    Item::Func(function) => {
                        if body_definitely_diverges(&function.body, &diverging, state)
                            && diverging.insert(function.name.clone())
                        {
                            changed = true;
                        }
                    }
                    Item::CodeModule(module) => {
                        let Some(body) = &module.body else {
                            continue;
                        };
                        for inner in body {
                            let Item::Func(function) = inner else {
                                continue;
                            };
                            if body_definitely_diverges(&function.body, &diverging, state) {
                                let name = jet_foundation::Names::member_name(
                                    &module.name,
                                    &function.name,
                                );
                                if diverging.insert(name) {
                                    changed = true;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if !changed {
            return diverging;
        }
    }
}
/// Publish the fixed-point result on bundle functions for downstream lowering.
///
/// A function is projected as diverging only when its body has no reachable
/// normal, returning, or loop-control path; an E0307 `NoElse` marker is an
/// absent fallthrough, not a diverging result, and every arm/fallback/loop
/// path must independently diverge.
///
/// This is the sole semantic writer of `Func::diverges`; parser and synthetic
/// constructors only initialize the compiler-metadata bit to `false`.
pub(crate) fn project_divergence_facts(
    bundle: &mut crate::AST::ProgramBundle,
    diverging: &std::collections::HashSet<String>,
) {
    for module in &mut bundle.modules {
        for item in &mut module.items {
            match item {
                Item::Func(function) => {
                    function.diverges = diverging.contains(&function.name);
                }
                Item::CodeModule(code_module) => {
                    let Some(body) = &mut code_module.body else {
                        continue;
                    };
                    for inner in body {
                        let Item::Func(function) = inner else {
                            continue;
                        };
                        let name =
                            jet_foundation::Names::member_name(&code_module.name, &function.name);
                        function.diverges = diverging.contains(&name);
                    }
                }
                _ => {}
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct DivergenceFlow {
    normal: bool,
    returned: bool,
    diverged: bool,
    broke: bool,
    continued: bool,
}

impl DivergenceFlow {
    fn normal() -> Self {
        Self {
            normal: true,
            ..Self::default()
        }
    }

    fn none() -> Self {
        Self::default()
    }

    fn union(self, other: Self) -> Self {
        Self {
            normal: self.normal || other.normal,
            returned: self.returned || other.returned,
            diverged: self.diverged || other.diverged,
            broke: self.broke || other.broke,
            continued: self.continued || other.continued,
        }
    }

    /// Sequence `next` after every path in `self` that still reaches a
    /// statement. Terminal paths remain terminal and never reach `next`.
    fn then(self, next: Self) -> Self {
        if !self.normal {
            return self;
        }
        Self {
            normal: false,
            returned: self.returned,
            diverged: self.diverged,
            broke: self.broke,
            continued: self.continued,
        }
        .union(next)
    }

    fn return_to_caller(self) -> Self {
        Self {
            normal: false,
            returned: self.returned || self.normal,
            diverged: self.diverged,
            broke: self.broke,
            continued: self.continued,
        }
    }

    fn break_to_loop(self) -> Self {
        Self {
            normal: false,
            returned: self.returned,
            diverged: self.diverged,
            broke: self.broke || self.normal,
            continued: self.continued,
        }
    }

    fn definitely_diverges(self) -> bool {
        self.diverged && !self.normal && !self.returned && !self.broke && !self.continued
    }
}

pub(super) fn body_flow(
    body: &[Stmt],
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> DivergenceFlow {
    let mut flow = DivergenceFlow::normal();
    for stmt in body {
        flow = flow.then(stmt_flow(stmt, diverging, state));
    }
    flow
}

pub(super) fn body_definitely_diverges(
    body: &[Stmt],
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> bool {
    body_flow(body, diverging, state).definitely_diverges()
}

pub(super) fn expr_list_flow<'a, I>(
    expressions: I,
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> DivergenceFlow
where
    I: IntoIterator<Item = &'a Expr>,
{
    let mut flow = DivergenceFlow::normal();
    for expression in expressions {
        flow = flow.then(expr_flow(expression, diverging, state));
    }
    flow
}

pub(super) fn call_args_flow(
    args: &[crate::AST::CallArg],
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> DivergenceFlow {
    expr_list_flow(args.iter().map(|arg| &arg.expr), diverging, state)
}

pub(super) fn lvalue_flow(
    target: &LValue,
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> DivergenceFlow {
    match target {
        LValue::Local { .. } => DivergenceFlow::normal(),
        LValue::Index { base, index, .. } => {
            expr_flow(base, diverging, state).then(expr_flow(index, diverging, state))
        }
        LValue::Field { base, .. } => expr_flow(base, diverging, state),
    }
}

pub(super) fn for_kind_flow(
    kind: &ForKind,
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> DivergenceFlow {
    match kind {
        ForKind::Range {
            start, end, step, ..
        } => expr_flow(start, diverging, state)
            .then(expr_flow(end, diverging, state))
            .then(
                step.as_ref()
                    .map(|step| expr_flow(step, diverging, state))
                    .unwrap_or_else(DivergenceFlow::normal),
            ),
        ForKind::In { collection, step } => expr_flow(collection, diverging, state).then(
            step.as_ref()
                .map(|step| expr_flow(step, diverging, state))
                .unwrap_or_else(DivergenceFlow::normal),
        ),
    }
}

pub(super) fn finite_loop_flow(body: DivergenceFlow) -> DivergenceFlow {
    // A finite or conditionally entered loop can always exhaust without an
    // iteration. Return and divergence paths from an iteration still matter.
    DivergenceFlow {
        normal: true,
        returned: body.returned,
        diverged: body.diverged,
        broke: false,
        continued: false,
    }
}

pub(super) fn infinite_loop_flow(body: DivergenceFlow) -> DivergenceFlow {
    // Normal/continue paths start another iteration. A break is the loop's
    // normal exit; return/divergence escape the enclosing function.
    DivergenceFlow {
        normal: body.broke,
        returned: body.returned,
        diverged: body.diverged || body.normal || body.continued,
        broke: false,
        continued: false,
    }
}

pub(super) fn branch_flow(
    body: &[Stmt],
    value: &Expr,
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> DivergenceFlow {
    body_flow(body, diverging, state).then(expr_flow(value, diverging, state))
}

pub(super) fn switch_flow(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: Option<&[Stmt]>,
    span: Span,
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> DivergenceFlow {
    if arms.is_empty() {
        return expr_flow(subject, diverging, state).then(
            else_body
                .map(|body| body_flow(body, diverging, state))
                .unwrap_or_else(DivergenceFlow::normal),
        );
    }
    let mut branches = DivergenceFlow::none();
    for arm in arms {
        branches = branches.union(
            expr_flow(&arm.cond, diverging, state).then(body_flow(&arm.body, diverging, state)),
        );
    }
    let fallback = match else_body {
        Some(body) => body_flow(body, diverging, state),
        None if crate::AST::is_subjectless_guard(subject, span) => DivergenceFlow::normal(),
        // A checked exhaustive dispatch has no runtime miss path. Its
        // synthesized Expr::NoElse is represented by `none`, not Diverged.
        None => DivergenceFlow::none(),
    };
    expr_flow(subject, diverging, state).then(branches.union(fallback))
}

pub(super) fn stmt_flow(
    stmt: &Stmt,
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> DivergenceFlow {
    match stmt {
        Stmt::Expr(expr) => expr_flow(expr, diverging, state),
        Stmt::Val(binding) => expr_flow(&binding.init, diverging, state),
        Stmt::Assign { target, value, .. } => {
            lvalue_flow(target, diverging, state).then(expr_flow(value, diverging, state))
        }
        Stmt::Return(value, ..) => value
            .as_ref()
            .map(|value| expr_flow(value, diverging, state))
            .unwrap_or_else(DivergenceFlow::normal)
            .return_to_caller(),
        Stmt::Break(_) | Stmt::BreakLabel(_, _) => DivergenceFlow {
            broke: true,
            ..DivergenceFlow::none()
        },
        Stmt::BreakValue(value, _) | Stmt::BreakLabelValue(_, _, value, _) => {
            expr_flow(value, diverging, state).break_to_loop()
        }
        Stmt::Continue(_) | Stmt::ContinueLabel(_, _) => DivergenceFlow {
            continued: true,
            ..DivergenceFlow::none()
        },
        Stmt::Loop { body, .. } => infinite_loop_flow(body_flow(body, diverging, state)),
        Stmt::While { cond, body, .. } => {
            let loop_body = body_flow(body, diverging, state);
            let loop_flow = if matches!(cond.without_parens(), Expr::Bool(true, _)) {
                infinite_loop_flow(loop_body)
            } else {
                finite_loop_flow(loop_body)
            };
            expr_flow(cond, diverging, state).then(loop_flow)
        }
        Stmt::For { kind, body, .. } => for_kind_flow(kind, diverging, state)
            .then(finite_loop_flow(body_flow(body, diverging, state))),
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            let iteration = body_flow(body, diverging, state).then(
                step.as_deref()
                    .map(|step| stmt_flow(step, diverging, state))
                    .unwrap_or_else(DivergenceFlow::normal),
            );
            let loop_flow = if matches!(cond.without_parens(), Expr::Bool(true, _)) {
                infinite_loop_flow(iteration)
            } else {
                finite_loop_flow(iteration)
            };
            expr_flow(&init.init, diverging, state)
                .then(expr_flow(cond, diverging, state))
                .then(loop_flow)
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            span,
        }
        | Stmt::ComptimeSwitch {
            subject,
            arms,
            else_body,
            span,
        } => switch_flow(subject, arms, else_body.as_deref(), *span, diverging, state),
        Stmt::Unsafe {
            audit_expr, body, ..
        }
        | Stmt::Impure {
            reason_expr: audit_expr,
            body,
            ..
        } => audit_expr
            .as_ref()
            .map(|expr| expr_flow(expr, diverging, state))
            .unwrap_or_else(DivergenceFlow::normal)
            .then(body_flow(body, diverging, state)),
        Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::AuthorityScope { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::Transact { body, .. } => body_flow(body, diverging, state),
        Stmt::TaskGroup { limit, body, .. } => limit
            .as_ref()
            .map(|expr| expr_flow(expr, diverging, state))
            .unwrap_or_else(DivergenceFlow::normal)
            .then(body_flow(body, diverging, state)),
        Stmt::ComptimeIf {
            then_body,
            else_body,
            selected_then,
            ..
        } => match selected_then {
            Some(true) => body_flow(then_body, diverging, state),
            Some(false) => else_body
                .as_deref()
                .map(|body| body_flow(body, diverging, state))
                .unwrap_or_else(DivergenceFlow::normal),
            None => body_flow(then_body, diverging, state).union(
                else_body
                    .as_deref()
                    .map(|body| body_flow(body, diverging, state))
                    .unwrap_or_else(DivergenceFlow::normal),
            ),
        },
        Stmt::ContextBlock { fields, body, .. } => {
            let fields = expr_list_flow(fields.iter().map(|(_, value, _)| value), diverging, state);
            fields.then(body_flow(body, diverging, state))
        }
        Stmt::AssumeDet {
            reason_expr, body, ..
        } => expr_flow(reason_expr, diverging, state).then(body_flow(body, diverging, state)),
        Stmt::Yield(value, _) => expr_flow(value, diverging, state),
        Stmt::ScopeMember { args, body, .. } => {
            expr_list_flow(args.iter(), diverging, state).then(body_flow(body, diverging, state))
        }
        Stmt::DeferClose { close, .. } => expr_flow(close, diverging, state),
    }
}

pub(super) fn typed_lit_flow(
    body: &crate::AST::TypedLitBody,
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> DivergenceFlow {
    match body {
        crate::AST::TypedLitBody::Fields(fields) => {
            expr_list_flow(fields.iter().map(|(_, _, value)| value), diverging, state)
        }
        crate::AST::TypedLitBody::Elements(elements) => {
            expr_list_flow(elements.iter(), diverging, state)
        }
        crate::AST::TypedLitBody::Entries(entries) => expr_list_flow(
            entries.iter().flat_map(|(key, value)| [key, value]),
            diverging,
            state,
        ),
        crate::AST::TypedLitBody::Value(value) => expr_flow(value, diverging, state),
        crate::AST::TypedLitBody::ByteText(_) | crate::AST::TypedLitBody::Empty => {
            DivergenceFlow::normal()
        }
    }
}

pub(super) fn expr_flow(
    expr: &Expr,
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> DivergenceFlow {
    match expr.without_parens() {
        Expr::Str(parts, _) => {
            let mut flow = DivergenceFlow::normal();
            for part in parts {
                if let StrPart::Interp(value, _) = part {
                    flow = flow.then(expr_flow(value, diverging, state));
                }
            }
            flow
        }
        Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _)
        | Expr::Int(_, _, _, _)
        | Expr::Float(_, _, _, _)
        | Expr::Bool(_, _)
        | Expr::Unit(_)
        | Expr::Char(_, _)
        | Expr::Ident(_, _)
        | Expr::Absent(_)
        | Expr::ReduceMarker(_, _)
        | Expr::ComptimeName { .. }
        | Expr::Lambda(_) => DivergenceFlow::normal(),
        Expr::ListLit(items, _) => expr_list_flow(items.iter(), diverging, state),
        Expr::MemberSpread { base, .. } | Expr::Spread(base, _) => {
            expr_flow(base, diverging, state)
        }
        Expr::MapLit(items, _) => expr_list_flow(
            items.iter().flat_map(|(key, value)| [key, value]),
            diverging,
            state,
        ),
        Expr::Index { base, index, .. }
        | Expr::Range {
            start: base,
            end: index,
            ..
        } => expr_flow(base, diverging, state).then(expr_flow(index, diverging, state)),
        Expr::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            let mut flow = expr_flow(base, diverging, state)
                .then(expr_flow(start, diverging, state))
                .then(expr_flow(end, diverging, state));
            if let Some(range) = range {
                flow = flow.then(expr_flow(range, diverging, state));
            }
            flow
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            let flow =
                expr_flow(receiver, diverging, state).then(call_args_flow(args, diverging, state));
            if flow.normal && core_call_diverges(state, receiver, method) {
                DivergenceFlow {
                    normal: false,
                    diverged: true,
                    ..flow
                }
            } else {
                flow
            }
        }
        Expr::UnitLit { .. } => DivergenceFlow::normal(),
        Expr::Call(call) => {
            let args = call_args_flow(&call.args, diverging, state);
            let call_diverges =
                call.name == Syntax::BUILTIN_PANIC || diverging.contains(&call.name);
            if !call_diverges {
                args
            } else if args.normal {
                DivergenceFlow {
                    normal: false,
                    returned: args.returned,
                    diverged: true,
                    broke: args.broke,
                    continued: args.continued,
                }
            } else {
                args
            }
        }
        Expr::Unary(_, inner, _) | Expr::IncDec { operand: inner, .. } => {
            expr_flow(inner, diverging, state)
        }
        Expr::Binary(op, left, right, _) => {
            let left = expr_flow(left, diverging, state);
            let right = expr_flow(right, diverging, state);
            if matches!(op, crate::AST::BinOp::And | crate::AST::BinOp::Or) {
                if !left.normal {
                    left
                } else {
                    DivergenceFlow {
                        normal: true,
                        returned: left.returned || right.returned,
                        diverged: left.diverged || right.diverged,
                        broke: left.broke || right.broke,
                        continued: left.continued || right.continued,
                    }
                }
            } else {
                left.then(right)
            }
        }
        Expr::CompareChain { operands, .. } => {
            let mut flow = DivergenceFlow::normal();
            for (index, operand) in operands.iter().enumerate() {
                let prior = flow;
                flow = flow.then(expr_flow(operand, diverging, state));
                // A comparison can stop the chain before a later operand.
                if index > 0 && prior.normal {
                    flow.normal = true;
                }
            }
            flow
        }
        Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Field(inner, _, _)
        | Expr::OptField { base: inner, .. }
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Paren(inner, _) => expr_flow(inner, diverging, state),
        Expr::StructLit { fields, .. } => {
            expr_list_flow(fields.iter().map(|(_, _, value)| value), diverging, state)
        }
        Expr::TypedLit { body, .. } => typed_lit_flow(body, diverging, state),
        Expr::EnumLit { args, .. } => expr_list_flow(
            args.iter().filter_map(|arg| match arg {
                EnumLitArg::Positional(value) | EnumLitArg::Named { expr: value, .. } => {
                    Some(value)
                }
            }),
            diverging,
            state,
        ),
        Expr::Tainted(inner, ..) => expr_flow(inner, diverging, state),
        Expr::Todo { .. } => DivergenceFlow {
            diverged: true,
            ..DivergenceFlow::none()
        },
        // This is the parser's omitted exhaustive fallthrough. It has no
        // runtime outcome; treating it as Diverged makes every value dispatch
        // appear to be a Never-producing call.
        Expr::NoElse(_) => DivergenceFlow::none(),
        Expr::PatternTest { subject, .. } => expr_flow(subject, diverging, state),
        Expr::Try(inner, _, _, note) => {
            let mut flow = expr_flow(inner, diverging, state);
            if let Some(note) = note {
                flow = flow.union(expr_flow(note, diverging, state));
            }
            flow
        }
        Expr::OrFallback {
            value, fallback, ..
        } => {
            let value_flow = expr_flow(value, diverging, state);
            if value_flow.normal {
                value_flow.union(or_fallback_flow(fallback, diverging, state))
            } else {
                value_flow
            }
        }
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            let branches = branch_flow(then_body, then_value, diverging, state)
                .union(branch_flow(else_body, else_value, diverging, state));
            expr_flow(cond, diverging, state).then(branches)
        }
        Expr::TupleLit(fields, _, _) => {
            expr_list_flow(fields.iter().map(|(_, value)| value), diverging, state)
        }
        Expr::CallValue { callee, args, .. } => {
            expr_flow(callee, diverging, state).then(call_args_flow(args, diverging, state))
        }
        Expr::PtrFromAddr { addr, .. } => expr_flow(addr, diverging, state),
    }
}

pub(super) fn or_fallback_flow(
    fallback: &OrFallback,
    diverging: &std::collections::HashSet<String>,
    state: &ModuleState,
) -> DivergenceFlow {
    match fallback {
        OrFallback::Value(value) => expr_flow(value, diverging, state),
        OrFallback::Block { body, value, .. } => body_flow(body, diverging, state).then(
            value
                .as_deref()
                .map(|value| expr_flow(value, diverging, state))
                .unwrap_or_else(DivergenceFlow::normal),
        ),
        OrFallback::Return(value, _) => value
            .as_deref()
            .map(|value| expr_flow(value, diverging, state))
            .unwrap_or_else(DivergenceFlow::normal)
            .return_to_caller(),
        OrFallback::Panic { args, .. } => {
            let args = call_args_flow(args, diverging, state);
            if args.normal {
                DivergenceFlow {
                    normal: false,
                    returned: args.returned,
                    diverged: true,
                    broke: args.broke,
                    continued: args.continued,
                }
            } else {
                args
            }
        }
        OrFallback::Break(_) | OrFallback::BreakLabel(_, _) => DivergenceFlow {
            broke: true,
            ..DivergenceFlow::none()
        },
        OrFallback::Continue(_) | OrFallback::ContinueLabel(_, _) => DivergenceFlow {
            continued: true,
            ..DivergenceFlow::none()
        },
    }
}

pub(super) fn core_call_diverges(state: &ModuleState, receiver: &Expr, method: &str) -> bool {
    let Expr::Ident(alias, _) = receiver.without_parens() else {
        return false;
    };
    let Some(module) = state.core_imports.get(alias) else {
        return false;
    };
    crate::Sema::CheckerCoreLib::core_call_signature(module, method)
        .and_then(|(_, ret)| ret)
        .is_some_and(|ret| matches!(ret, Type::Named(name) if name == Syntax::TYPE_NEVER))
}
