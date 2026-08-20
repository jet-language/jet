use super::*;

pub(super) fn complete_bundle_check(
    bundle: &mut ProgramBundle,
    states: &[ModuleState],
    mode: CompileMode,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    explicit_output: Option<&str>,
    mut incremental: Option<&mut IncrementalSemaCache>,
    allow_compiler_api: bool,
    mut name_ledger: jet_foundation::Names::NameLedger,
    mut diags: Vec<Diagnostic>,
) -> (Vec<Diagnostic>, super::super::super::Effects::SemIndexEffectFacts) {
    populate_name_ledger(bundle, &states, &mut name_ledger);

    // D-SHARED-CYCLE1=C: run one graph/memo pass after registry and import
    // identities are complete, so a qualified field type resolves to its
    // owning module instead of a registry-order-dependent leaf name.
    check_strong_shared_cycles(&states, &name_ledger, &mut diags);

    for idx in 0..bundle.modules.len() {
        for item in &bundle.modules[idx].items {
            let Item::Impl(i) = item else { continue };
            if !i.type_name.contains('.') {
                continue;
            }
            if !impl_type_exists(
                &i.type_name,
                &states[idx].registry,
                &states[idx].imports,
                Some(&states),
            ) {
                diags.push(Diagnostic::error(
                    "E0301",
                    format!("`impl {}` names a type that doesn't exist", i.type_name),
                    format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                    format!(
                        "define `struct {}` or `enum {}` first",
                        i.type_name, i.type_name
                    ),
                    Some(i.type_span),
                ));
            }
        }
    }

    // D-SHAPE-OUTPUT-CALLABLE1: freeze every runnable Output to the ordinary
    // function it resolves to before entry selection or lowering can inspect it.
    resolve_outputs(
        bundle,
        &states,
        &name_ledger,
        mode,
        explicit_output,
        &mut diags,
    );

    // Parity with the single-file path: `@static` and address-taken consts
    // must lower to Rust `static` in bundle mode too.
    for module in bundle.modules.iter_mut() {
        let const_names: Vec<String> = module
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Const(c) => Some(c.name.clone()),
                _ => None,
            })
            .collect();
        let mut address_taken: HashSet<String> = HashSet::new();
        for item in &module.items {
            match item {
                Item::Func(f) => {
                    walk_stmts_for_const_refs(&f.body, &const_names, &mut address_taken)
                }
                Item::Struct(s) => {
                    for m in &s.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Enum(e) => {
                    for m in &e.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Impl(i) => {
                    for m in &i.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Test(t) => {
                    walk_stmts_for_const_refs(&t.body, &const_names, &mut address_taken)
                }
                Item::EffectDecl(_)
                | Item::MarkerDecl(_)
                | Item::FactDecl(_)
                | Item::Const(_)
                | Item::ExternRust(_)
                | Item::Trait(_)
                | Item::Tag(_) // D-QUAL2: tags erase
                | Item::Module(_)
                | Item::Distinct(_)
                | Item::TypeAlias(_) // D-TYPEALIAS1: erases
                | Item::UnitFamily(_) // D-QUAL3: lowered to distinct types
                | Item::CModule(_) | Item::CodeModule(_)
                | Item::ErrorConv(_)
                | Item::Migration(_) // D-MIGRATE1
                | Item::StateDecl(_) // D-STATE1: erases
                | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
                | Item::UserDerive(_) // D-METADERIVE1=A: already expanded above
                | Item::GenericModule(_) // D-CONF-GENSPELL1=A: template — erases
                | Item::ModuleAlias(_) => {} // D-CONF-GENSPELL1=A: alias — erases after expansion
            }
        }
        for item in &mut module.items {
            if let Item::Const(c) = item {
                let force_static = c.is_persist || c.attrs.contains(&ConstAttr::ForceStatic);
                c.rust_kind = if force_static || address_taken.contains(&c.name) {
                    RustConstKind::Static
                } else {
                    RustConstKind::Const
                };
            }
        }
    }

    // Each non-entry module becomes a Rust `mod __jet_<alias>`; a type in the
    // entry file with the same name would collide in the type namespace.
    for (idx, m) in bundle.modules.iter().enumerate() {
        if idx == bundle.entry {
            continue;
        }
        if states[bundle.entry].registry.contains(&m.alias) {
            diags.push(Diagnostic::error(
                "E0105",
                format!(
                    "the type `{}` clashes with the imported file `{}`",
                    m.alias, m.display
                ),
                "a type and an imported module can't share a name".to_string(),
                format!(
                    "rename the type, or import with `{} other_name`",
                    Syntax::KW_AS
                ),
                None,
            ));
        }
    }

    if mode == CompileMode::Run || mode == CompileMode::Eval {
        let entry_items = &bundle.modules[bundle.entry].items;
        let has_selected_output = entry_items.iter().any(|item| {
            matches!(item, Item::Const(value) if value.resolved_output.as_ref().is_some_and(|output| output.selected))
        });
        if has_selected_output {
            // The selected Output contract was checked before body checking.
        } else if let Some(run_fn) = entry_items.iter().find_map(|i| match i {
            Item::Func(f) if f.name == "run" => Some(f),
            _ => None,
        }) {
            // S12/D-CLIFLAG1: `run` is the only program entry name. Its
            // canonical CLI schema owns both direct scalar inputs and the
            // one typed CLI-spec parameter (`#[CLI]` program struct).
            let direct_cli = jet_foundation::CLISchema::is_direct_run_entry(entry_items);
            if direct_cli {
                // The canonical schema producer has already classified every
                // direct input and its default. Keep this gate in sema, but do
                // not reconstruct that policy here.
            } else if run_fn.params.len() == 1 {
                let param = &run_fn.params[0];
                let cli_module = jet_foundation::CLISchema::entry_type_module(bundle)
                    .unwrap_or(bundle.entry);
                match cli_entry_param_shape(
                    &bundle.modules[cli_module].items,
                    &param.ty,
                    &states[cli_module].trait_reg,
                ) {
                    CLIEntryShape::Struct => {}
                    CLIEntryShape::Invalid => diags.push(e1308(Some(param.ty_span))),
                }
            } else if run_fn.params.len() > 1 {
                diags.push(e1308(Some(run_fn.name_span)));
            }
        } else {
            diags.push(no_run_error());
        }
    }
    match mode {
        CompileMode::Test if !states.iter().any(|state| !state.tests.is_empty())
            && !bundle.modules[bundle.entry].items.iter().any(|item| {
                matches!(item, Item::Const(value) if value.resolved_output.as_ref().is_some_and(|output| output.selected && output.kind == crate::AST::OutputKind::Check))
            }) => {
            diags.push(Diagnostic::error(
                "E0601",
                format!("no `#{}` blocks found to run", Syntax::KW_TEST),
                format!(
                    "add at least one top-level block: #{}(\"describes what this checks\") {{ ... }}",
                    Syntax::KW_TEST
                ),
                format!(
                    "use `{}` and `{}` inside the block to check results",
                    Syntax::BUILTIN_ASSERT,
                    Syntax::BUILTIN_ASSERT_EQ
                ),
                None,
            ));
        }
        // `jet bench` checks the AST for `#Bench` blocks before entering Bench
        // mode and falls back to whole-program timing otherwise, so an empty
        // bench set is never an error here.
        CompileMode::Bench
        | CompileMode::Test
        | CompileMode::TestOverride
        | CompileMode::BenchOverride
        | CompileMode::Run
        | CompileMode::Check
        | CompileMode::Eval => {}
    }

    // D-EFF1: collect effect summaries across every module, then run the
    // shared reachability projection and enforce each `#(…)` bound once.
    let mut declared_effect_facts = jet_foundation::Facts::FactRegistry::default();
    register_effect_facts(bundle, &mut declared_effect_facts);
    diags.extend(validate_declared_effects(bundle, &declared_effect_facts));

    // D-CTEFFECT1 Tier-1: accumulate embed inputs from all module checks.
    // Use a temporary to avoid simultaneous &mut borrows of `bundle`.
    if mode == CompileMode::Check {
        if let Some(cache) = incremental.as_deref_mut() {
            cache.begin_bundle(bundle);
        }
    } else {
        incremental = None;
    }
    let mut embed_inputs = std::mem::take(&mut bundle.comptime_inputs);
    let mut effect_summaries: HashMap<String, EffectSummary> = HashMap::new();
    let mut module_effect_summaries: Vec<(String, HashMap<String, EffectSummary>)> = Vec::new();
    let mut module_pending_diagnostics = Vec::new();
    diags.extend(check_job_collisions(&bundle.modules));
    // D-METHODMACRO1=A: top-level function names whose address was taken
    // anywhere in the bundle, accumulated across every module below; the
    // `#Inline(Always)` address-taken pass (E0918) runs after the loop, once
    // this set is complete across the whole bundle.
    let mut global_addr_taken: HashSet<String> = HashSet::new();
    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        let mut local_summaries = HashMap::new();
        let mut local_pending_diagnostics = Vec::new();
        let mut module_diags = check_module_bodies(
            module,
            idx,
            &states,
            &declared_effect_facts,
            mode,
            freestanding,
            gates,
            &mut local_summaries,
            &mut embed_inputs,
            &mut global_addr_taken,
            &mut name_ledger,
            &mut local_pending_diagnostics,
            incremental.as_deref_mut(),
        );
        dedupe_unknown_names(&mut module_diags);
        dedupe_soft_public_lints(&mut module_diags);
        diags.extend(module_diags);
        for pending in &mut local_pending_diagnostics {
            pending.function_key = name_ledger
                .semantic_identity(idx, &pending.function_key)
                .unwrap_or_else(|| format!("{}::{}", module.alias, pending.function_key));
        }
        module_pending_diagnostics.push(local_pending_diagnostics);
        seed_trait_dispatch_effects(&module.items, &mut local_summaries);
        apply_effect_via(&module.items, &mut local_summaries, &mut Vec::new());
        effect_summaries.extend(local_summaries.clone());
        module_effect_summaries.push((
            name_ledger
                .module_alias(idx)
                .unwrap_or(&module.alias)
                .to_string(),
            local_summaries,
        ));
    }
    bundle.comptime_inputs = embed_inputs;
    // D-METHODMACRO1=A: E0918 (address-taken) needs every module's function
    // bodies checked first. Methods can't appear in `global_addr_taken`
    // (Jet's grammar has no way to read a method's bare name as a value), so
    // this only ever fires for top-level functions.
    let mut failed_diagnostic_phases = HashSet::new();
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let phase_diagnostic_start = diags.len();
        for item in &module.items {
            if let Item::Func(f) = item {
                if f.is_inline_always && global_addr_taken.contains(&f.name) {
                    diags.push(e0918_address_taken(
                        &f.name,
                        f.inline_span.unwrap_or(f.name_span),
                    ));
                }
            }
        }
        mark_failed_pending_functions(
            &diags[phase_diagnostic_start..],
            &module_pending_diagnostics[module_index],
            &mut failed_diagnostic_phases,
        );
    }
    // D-EFF2 (`#(via f)`): seed each via-fn's summary with its callback's bound
    // before projection, so its published effect set is a tight pass-through.
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let phase_diagnostic_start = diags.len();
        apply_effect_via(&module.items, &mut effect_summaries, &mut diags);
        mark_failed_pending_functions(
            &diags[phase_diagnostic_start..],
            &module_pending_diagnostics[module_index],
            &mut failed_diagnostic_phases,
        );
    }
    // File modules need qualified facts: bare top-level names overwrite one
    // another, while D-EFFECT-OMIT1 requires one cross-package solver answer.
    let mut taint_returns = HashMap::new();
    let mut return_types = HashMap::new();
    for module in &bundle.modules {
        super::super::super::Taint::collect_return_tag_facts(
            &module.items,
            &mut taint_returns,
            &mut return_types,
        );
    }
    let (public_summaries, public_reachability) =
        qualified_effect_facts(&module_effect_summaries, &taint_returns);
    let public_solved: HashMap<String, EffectSet> = public_summaries
        .keys()
        .filter_map(|key| {
            public_reachability
                .row("effects")
                .and_then(|row| row.get(key))
                .map(|effects| (key.clone(), effects.clone()))
        })
        .collect();
    if let Some(row) = public_reachability.row("taint") {
        for (key, tags) in row {
            if !tags.is_empty() {
                taint_returns.insert(key.clone(), tags.clone());
            }
        }
    }
    // The Output carries the same solved effect row used by diagnostics and
    // semantic-index consumers. Tooling never re-walks the callable body.
    for module in &mut bundle.modules {
        let display = module.display.clone();
        for item in &mut module.items {
            let Item::Const(value) = item else { continue };
            let Some(output) = &mut value.resolved_output else { continue };
            let alias = name_ledger
                .module_alias(output.module)
                .unwrap_or(&states[output.module].module_alias);
            let identity = name_ledger
                .semantic_identity(output.module, &output.semantic_name)
                .unwrap_or_else(|| format!("{alias}::{}", output.semantic_name));
            output.effects = public_solved
                .get(&identity)
                .map(|effects| effects.iter().cloned().collect())
                .unwrap_or_default();
            name_ledger.record_reference(
                display.clone(),
                output.reference.start,
                output.reference.end,
                jet_foundation::Names::NameReference {
                    module_path: output.source_path.clone(),
                    kind: "function".to_string(),
                    def_span: output.definition,
                    semantic_identity: Some(identity),
                },
            );
        }
    }
    // `public_summaries` also carries unique short aliases for tooling. Run
    // diagnostics only over canonical module-qualified nodes so each source
    // obligation is reported once.
    let module_aliases = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(module_idx, module)| {
            format!(
                "{}::",
                name_ledger
                    .module_alias(module_idx)
                    .unwrap_or(&module.alias)
            )
        })
        .collect::<Vec<_>>();
    let validation_summaries = public_summaries
        .iter()
        .filter(|(key, _)| module_aliases.iter().any(|prefix| key.starts_with(prefix)))
        .map(|(key, summary)| (key.clone(), summary.clone()))
        .collect::<HashMap<_, _>>();
    super::super::super::Effects::check_autodiff_purity(
        &validation_summaries,
        &public_solved,
        &mut diags,
    );
    // Candidate facts survive only when their entire function
    // remains error-free through the solved effect phases below.
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let phase_diagnostic_start = diags.len();
        let prefix = format!(
            "{}::",
            name_ledger
                .module_alias(module_index)
                .unwrap_or(&module.alias)
        );
        let local_solved = public_solved
            .iter()
            .filter_map(|(key, row)| key.strip_prefix(&prefix).map(|key| (key.to_string(), row.clone())))
            .collect::<HashMap<_, _>>();
        let local_summaries = validation_summaries
            .iter()
            .map(|(key, summary)| {
                if let Some(key) = key.strip_prefix(&prefix) {
                    let mut summary = summary.clone();
                    summary.edges = summary
                        .edges
                        .iter()
                        .map(|edge| edge.strip_prefix(&prefix).unwrap_or(edge).to_string())
                        .collect();
                    for call in &mut summary.memory.calls {
                        call.callee = call
                            .callee
                            .strip_prefix(&prefix)
                            .unwrap_or(&call.callee)
                            .to_string();
                    }
                    (key.to_string(), summary)
                } else {
                    (key.clone(), summary.clone())
                }
            })
            .collect::<HashMap<_, _>>();
        check_effect_boundaries(
            &module.items,
            &local_solved,
            &local_summaries,
            &mut diags,
        );
        let module_alias = name_ledger
            .module_alias(module_index)
            .unwrap_or(&module.alias);
        super::super::super::Effects::check_inferred_purity(
            &module.items,
            module_alias,
            &validation_summaries,
            &public_solved,
            &public_reachability,
            &mut diags,
        );
        check_replayable_effects(&module.items, &local_solved, &mut diags);
        check_secret_grants(
            &module.items,
            module_alias,
            &public_reachability.nodes_with("secret", Effect::Secret.name()),
            &mut diags,
        );
        mark_failed_pending_functions(
            &diags[phase_diagnostic_start..],
            &module_pending_diagnostics[module_index],
            &mut failed_diagnostic_phases,
        );
    }
    check_region_caps(&validation_summaries, &public_solved, &mut failed_diagnostic_phases, &mut diags);
    // D-EFF2: callback param effect bounds (E0747).
    check_callback_bounds(&validation_summaries, &public_solved, &mut failed_diagnostic_phases, &mut diags);
    for (module_index, pending_diagnostics) in module_pending_diagnostics.into_iter().enumerate() {
        let module_alias = name_ledger
            .module_alias(module_index)
            .unwrap_or(&bundle.modules[module_index].alias);
        let identity_prefix = name_ledger
            .module_identity(module_index)
            .map(|identity| format!("{identity}::"));
        for pending in pending_diagnostics {
            let loader_key = identity_prefix.as_deref().and_then(|prefix| {
                pending
                    .function_key
                    .strip_prefix(prefix)
                    .map(|local| format!("{module_alias}::{local}"))
            });
            if !failed_diagnostic_phases.contains(&pending.function_key)
                && !loader_key
                    .as_deref()
                    .is_some_and(|key| failed_diagnostic_phases.contains(key))
            {
                diags.push(pending.diagnostic);
            }
        }
    }

    // JS/WASM partition inference and boundary checks.
    // D-MEM-FACTS1: function effect-row denials and package manifest denials
    // are checked only after the same qualified, dependency-complete graph is projected.
    // #657 feeds the other scope levels and the two remaining fact values into
    // this declaration surface; reachability itself stays single-mechanism.
    let (memory_summaries, memory_declarations) =
        super::super::super::MemoryFacts::bundle_memory_inputs(bundle, &public_summaries);
    let memory_projections = memory_declarations
        .iter()
        .flat_map(|declaration| {
            declaration.roots.iter().map(|root| {
                (
                    (root.clone(), declaration.fact),
                    super::super::super::MemoryFacts::project_memory_fact(
                        declaration.fact,
                        root,
                        &memory_summaries,
                    ),
                )
            })
        })
        .collect::<HashMap<_, _>>();
    diags.extend(super::super::super::MemoryFacts::check_memory_facts(
        &memory_declarations,
        &memory_summaries,
    ));
    diags.extend(check_web_partition(
        bundle,
        &public_summaries,
        &public_solved,
    ));

    // D-WEBAPP1=D / D-WEBAUTHOR1=D (Tower #1274, #1703): one sema-known application graph.
    let (app_graph, app_diags) = super::super::super::App::extract_app_graph(bundle);
    diags.extend(app_diags);

    // D-OSTARGET1=A (ratified 2026-07-01, c134): native OS platform gating —
    // mixed-axis conflicts and unmatched cross-gate calls.
    diags.extend(check_os_target(bundle, freestanding));

    // D-FACTMODEL1=A: one erased fact model for tags, effects, and states.
    // Keep the pass in its own frame; this bundle checker already carries the
    // compiler's largest solved graphs.
    let fact_registry = check_fact_tags_and_states(
        bundle,
        &states,
        &taint_returns,
        &return_types,
        &mut diags,
    );

    let (mut used_core, usage_spans, ffi_callback_fns) = collect_used_core(bundle, &states);
    // D-CLIFLAG1: generated CLI specs/decoders and job argv dispatch call
    // straight into `core.args`'s `JetArgsSpec`/`JetParsedArgs`/argv Prelude
    // — but they're pure codegen text, not a Jet method call `collect_used_core`
    // can see by walking function bodies.
    // Force the same `CORELIB_PRELUDE` inclusion a hand-written
    // `use core.args` would trigger (any key works; the caller only checks
    // "is this set empty").
    if bundle.modules.iter().any(|m| {
        m.items.iter().any(|i| {
            matches!(i, Item::Struct(s) if s.derives.iter().any(|(t, _)| t == "CLI"))
                || matches!(i, Item::Func(f) if f.is_job)
        })
    }) {
        used_core.insert("core.args::spec".to_string());
    }
    // D-MEM1 S6: `Shared<T>`/`Pool<T>`/`Id<T>` need `CORELIB_PRELUDE`'s `jet_std`
    // module (`JetShared`/`JetPool`/`JetId`), but need no `use core.X` import to
    // reach them — `collect_used_core` only walks
    // import aliases, so it never sees them. Same forced-insert shape as
    // D-CLIFLAG1 above; a cheap source-text scan is deliberately over-eager (a
    // false positive just includes the prelude when it wasn't strictly needed —
    // harmless, `#![allow(warnings)]` covers the unused code).
    if bundle.modules.iter().any(|m| {
        m.source.contains("Pool<")
            || m.source.contains("Shared<")
            // D-CONC-SHARE1=A: `shared x` is the one construction spelling
            // (`Shared.new(` is retired, E1115), so the probe reads the word.
            || m.source.contains("shared ")
            || m.source.contains("Cell<")
            || m.source.contains("Cell.new(")
            || m.source.contains("Id<")
    }) {
        used_core.insert("core.mem::pool_shared".to_string());
    }
    // D-TYPE2-MEASURE1=A: `Vec<N>` and `Matrix<M, N>` are unqualified core
    // generics, so a program can name them — and lower `Matrix * Matrix` to
    // `core.compute::matmul` — without any `use core.compute` import for
    // `collect_used_core` to see. Same forced-insert shape as D-MEM1 S6 above,
    // and over-eager for the same harmless reason.
    if bundle
        .modules
        .iter()
        .any(|m| m.source.contains("Matrix<") || m.source.contains("Vec<"))
    {
        used_core.insert("core.compute::shape_alias".to_string());
    }
    // D-VALIDATE1 (card #506): a `validate { … }` block synthesizes
    // `Type.validate(value)`, which returns `[jet_std::FieldError]` — same
    // forced-insert shape as D-CLIFLAG1/D-MEM1 S6 above, since declaring the
    // block needs no `use core.X` import to reach `CORELIB_PRELUDE`.
    if bundle.modules.iter().any(|m| {
        m.items
            .iter()
            .any(|i| matches!(i, Item::Struct(s) if !s.validate_block.is_empty()))
    }) {
        used_core.insert("core.validate::field_error".to_string());
    }
    // D-EMAIL-SMTP-CONFIG1=A: sema canonicalizes `email.Limits.safe()` to a
    // static `Limits.safe()` call before this late usage walk. Preserve CoreLib
    // reachability for type-only SMTP policy programs.
    if bundle.modules.iter().zip(states.iter()).any(|(module, state)| {
        module.source.contains(".Limits")
            && state.core_imports.values().any(|path| path == "core.email")
    }) {
        used_core.insert("core.email::Limits.safe".to_string());
    }
    // D-CORE-SOURCE-AUTHORITY1=A: late sema-generated helpers join the same
    // source-owned package and audited ABI closure as explicit calls.
    expand_core_reachable_closure(&mut used_core);
    bundle.used_core = used_core;
    bundle.ffi_callback_fns = ffi_callback_fns;
    diags.extend(super::super::super::MemoryFacts::annotate_scoped_gc_promotions(bundle));
    apply_helper_layer_inference(bundle, &states, &usage_spans, &mut diags);
    bundle.name_ledger = name_ledger.clone();
    // D-BUILDENTRY1 / I2+I3+I9 (Tower card 2008): checking is finished, so the
    // build entry has had every diagnostic it is owed. It is not runtime code,
    // and `BuildContext`/`BuildPlan` have no runtime lowering, so leaving it in
    // hands codegen a function the typed IR cannot cover — an internal compiler
    // error raised by `emit_func`, from a program sema just accepted. Project it
    // out here, once, and every engine downstream sees the same program.
    //
    // Two checks keep their build entry. `allow_compiler_api` is the build
    // session's own check of the selected root, whose caller still holds an
    // index into the entry module's items and evaluates the entry next; the
    // Driver removes it after the build runs. `Check` is `jet check`/LSP, which
    // emits no code and reports `E3501` against the item.
    if !allow_compiler_api && mode != CompileMode::Check {
        super::super::strip_build_only_entries(bundle);
    }
    (
        diags,
        super::super::super::Effects::SemIndexEffectFacts {
            summaries: public_summaries,
            solved: public_solved,
            reachability: public_reachability,
            memory_declarations,
            memory_projections,
            name_ledger: name_ledger.clone(),
            web_app: app_graph,
            fact_registry,
        },
    )
}
