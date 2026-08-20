use super::*;

mod Completion;
mod InlineImports;
use Completion::complete_bundle_check;
use InlineImports::resolve_inline_module_imports;

fn register_generated_union_enums(
    items: &[Item],
    state: &mut crate::Sema::ModuleState,
    diags: &mut Vec<Diagnostic>,
) {
    for item in items {
        match item {
            Item::Enum(definition)
                if definition.name.starts_with("__JetUnion_")
                    && !state.registry.contains(&definition.name) =>
            {
                register_enum(
                    definition,
                    &mut state.registry,
                    diags,
                    &state.funcs,
                    &state.consts,
                );
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    register_generated_union_enums(body, state, diags);
                }
            }
            _ => {}
        }
    }
}

fn existing_member_span(items: &[crate::AST::Item], type_name: &str, member: &str) -> Option<Span> {
    for item in items {
        match item {
            Item::Struct(def) if def.name == type_name => {
                if let Some(field) = def.fields.iter().find(|field| field.name == member) {
                    return Some(field.name_span);
                }
                if let Some(method) = def.methods.iter().find(|method| method.name == member) {
                    return Some(method.name_span);
                }
            }
            Item::Enum(def) if def.name == type_name => {
                if let Some(method) = def.methods.iter().find(|method| method.name == member) {
                    return Some(method.name_span);
                }
            }
            Item::Impl(def) if def.type_name == type_name => {
                if let Some(method) = def.methods.iter().find(|method| method.name == member) {
                    return Some(method.name_span);
                }
            }
            _ => {}
        }
    }
    None
}

fn block_source(
    source: &str,
    body: &[crate::AST::Stmt],
    block_spans: &[Span],
    outer: Span,
) -> String {
    let candidate = if let (Some(first), Some(last)) = (body.first(), body.last()) {
        block_spans
            .iter()
            .filter(|span| {
                span.start > outer.start
                    && span.end < outer.end
                    && span.start <= first.span().start
                    && span.end >= last.span().end
            })
            .max_by_key(|span| span.end.saturating_sub(span.start))
    } else {
        block_spans
            .iter()
            .filter(|span| span.start > outer.start && span.end < outer.end)
            .min_by_key(|span| span.start)
    };
    candidate
        .and_then(|span| source.get(span.start..span.end))
        .unwrap_or_default()
        .to_string()
}

fn collect_declared_text_blocks(
    statements: &[crate::AST::Stmt],
    source: &str,
    block_spans: &[Span],
    blocks: &mut Vec<(String, String, Span)>,
) {
    for statement in statements {
        if let crate::AST::Stmt::ScopeMember {
            name,
            body,
            dsl: true,
            span,
            ..
        } = statement
        {
            blocks.push((
                name.clone(),
                block_source(source, body, block_spans, *span),
                *span,
            ));
        }
        for child in super::super::ScopeMembers::statement_bodies(statement) {
            collect_declared_text_blocks(child, source, block_spans, blocks);
        }
    }
}

fn collect_item_declared_text_blocks(
    item: &crate::AST::Item,
    source: &str,
    block_spans: &[Span],
    blocks: &mut Vec<(String, String, Span)>,
) {
    match item {
        Item::Func(function) => {
            collect_declared_text_blocks(&function.body, source, block_spans, blocks)
        }
        Item::Test(test) => collect_declared_text_blocks(&test.body, source, block_spans, blocks),
        Item::Bench(bench) => collect_declared_text_blocks(&bench.body, source, block_spans, blocks),
        Item::Impl(implementation) => {
            for method in &implementation.methods {
                collect_declared_text_blocks(&method.body, source, block_spans, blocks);
            }
        }
        Item::Struct(definition) => {
            for method in &definition.methods {
                collect_declared_text_blocks(&method.body, source, block_spans, blocks);
            }
            for implementation in &definition.trait_impls {
                for method in &implementation.methods {
                    collect_declared_text_blocks(&method.body, source, block_spans, blocks);
                }
            }
        }
        Item::Enum(definition) => {
            for method in &definition.methods {
                collect_declared_text_blocks(&method.body, source, block_spans, blocks);
            }
            for implementation in &definition.trait_impls {
                for method in &implementation.methods {
                    collect_declared_text_blocks(&method.body, source, block_spans, blocks);
                }
            }
        }
        Item::CodeModule(module) => {
            if let Some(body) = &module.body {
                for item in body {
                    collect_item_declared_text_blocks(item, source, block_spans, blocks);
                }
            }
        }
        _ => {}
    }
}

fn derive_member_collision(
    derive_name: &str,
    type_name: &str,
    method: &crate::AST::Func,
    existing_site: &str,
    existing_span: Span,
) -> Diagnostic {
    Diagnostic::error(
        "E0105",
        format!(
            "generated method `{}` from `derive T.{}` collides with {}",
            method.name, derive_name, existing_site
        ),
        format!(
            "`derive T.{}` and {} both define a member named `{}` on `{}`",
            derive_name,
            existing_site,
            method.name,
            type_name
        ),
        format!(
            "rename the generated member in `derive T.{}`, or rename the colliding member",
            derive_name
        ),
        Some(method.name_span),
    )
    .with_detail(format!(
        "generated member `{}` from `derive T.{}` at span {}..{}\n{} at span {}..{}",
        method.name,
        derive_name,
        method.name_span.start,
        method.name_span.end,
        existing_site,
        existing_span.start,
        existing_span.end,
    ))
}

fn validate_foreign_imports(bundle: &ProgramBundle) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen = HashSet::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for (_, import) in crate::AST::walk_imports(module) {
            if !seen.insert((module_idx, import.span)) {
                continue;
            }
            if let Err(error) = import.foreign_imports() {
                diagnostics.push(error.diagnostic());
            }
        }
    }
    diagnostics
}

fn foreign_imports_after_validation(
    import: &crate::AST::ImportDecl,
) -> Vec<(crate::AST::ForeignNamespace, String)> {
    import.foreign_imports().unwrap_or_else(|error| {
        unreachable!(
            "invalid foreign import reached sema after validation: {}",
            error.path
        )
    })
}

fn is_c_import_after_validation(import: &crate::AST::ImportDecl) -> bool {
    import.is_c_import().unwrap_or_else(|error| {
        unreachable!(
            "invalid foreign import reached sema after validation: {}",
            error.path
        )
    })
}

/// D-BOUND-SINK1=A: keep each declared text head's compile-time contract
/// attached to the module that authored it. The first pass publishes every
/// head name before module checking; the per-module refresh below captures
/// comptime constants after they have been evaluated.
fn register_text_head_contracts(
    state: &mut crate::Sema::ModuleState,
    module: &crate::AST::LoadedModule,
    core_imports: &HashMap<String, String>,
) {
    let (funcs, _externs, globals) =
        super::super::Registration::comptime_context_from_items(&module.items);
    let sigs: HashMap<String, crate::AST::FuncSig> = funcs
        .iter()
        .map(|(name, function)| (name.clone(), super::super::func_to_sig(function)))
        .collect();
    let type_params: HashMap<String, Vec<crate::AST::TypeParam>> = funcs
        .iter()
        .map(|(name, function)| (name.clone(), function.type_params.clone()))
        .collect();
    let base_dir = module
        .path
        .parent()
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    for declaration in module.items.iter().filter_map(|item| match item {
        Item::MarkerDecl(declaration) => declaration.text.as_ref().map(|text| {
            (
                declaration.name.clone(),
                crate::Sema::TextHeadContract {
                    declaration: text.clone(),
                    funcs: funcs.clone(),
                    sigs: sigs.clone(),
                    type_params: type_params.clone(),
                    globals: globals.clone(),
                    core_imports: core_imports.clone(),
                    base_dir: base_dir.clone(),
                },
            )
        }),
        _ => None,
    }) {
        state.registry.register_text_head(declaration.0, declaration.1);
    }
}

pub(super) fn check_bundle_opts_for_output(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    explicit_output: Option<&str>,
    incremental: Option<&mut IncrementalSemaCache>,
) -> (Vec<Diagnostic>, super::super::Effects::SemIndexEffectFacts) {
    check_bundle_opts_for_output_with_context(
        bundle,
        mode,
        freestanding,
        gates,
        explicit_output,
        incremental,
        false,
    )
}

/// Checking is an unbounded-depth recursive descent over user syntax, so the
/// frame requirement is per source-nesting level, not per program size. This
/// is the narrowest point every public `check_bundle*` shares, so the sized
/// stack is installed here instead of being chased caller by caller:
/// `Sema::check_bundle` and its siblings are public API, and an embedder
/// holding its own bundle — or a 2 MiB libtest worker — would otherwise run
/// the descent on whatever stack it happens to have, aborting the process on
/// overflow.
///
/// The re-entrancy flag is shared in `jet-foundation`, so an outer boundary
/// already on the worker (a driver funnel, a JIT public entry, the loader)
/// makes this run inline: the check comes *first*, so the inline path does no
/// capture and no spawn.
///
/// Thread-locals across the spawn. `PACKAGE_EDITION` is established *inside*
/// the worker from `bundle.edition`, so `with_package_edition` stays under the
/// boundary rather than over it. The comptime ambient hooks are the one piece
/// of caller-established state the check reads back (derive/comptime folding
/// reaches them through `TirBridge`), so they are carried across explicitly —
/// the same carry `jet_driver::run_compiler_work` performs. `TirBridge`'s own
/// hooks are a process-global `OnceLock`, not thread-local, so they need no
/// carry.
pub(super) fn check_bundle_opts_for_output_with_context(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    explicit_output: Option<&str>,
    incremental: Option<&mut IncrementalSemaCache>,
    allow_compiler_api: bool,
) -> (Vec<Diagnostic>, super::super::Effects::SemIndexEffectFacts) {
    if jet_foundation::CompilerStack::on_compiler_worker() {
        return check_bundle_opts_for_output_on_stack(
            bundle,
            mode,
            freestanding,
            gates,
            explicit_output,
            incremental,
            allow_compiler_api,
        );
    }
    let (ambient_core_call, ambient_handle) = crate::Comptime::ambient_hooks();
    jet_foundation::CompilerStack::run_on_compiler_stack(move || {
        crate::Comptime::with_ambient(ambient_core_call, ambient_handle, || {
            check_bundle_opts_for_output_on_stack(
                bundle,
                mode,
                freestanding,
                gates,
                explicit_output,
                incremental,
                allow_compiler_api,
            )
        })
    })
}

fn check_bundle_opts_for_output_on_stack(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    explicit_output: Option<&str>,
    incremental: Option<&mut IncrementalSemaCache>,
    allow_compiler_api: bool,
) -> (Vec<Diagnostic>, super::super::Effects::SemIndexEffectFacts) {
    let edition = bundle.edition.clone();
    super::super::Edition::with_package_edition(&edition, || {
        check_bundle_opts_for_output_inner(
            bundle,
            mode,
            freestanding,
            gates,
            explicit_output,
            incremental,
            allow_compiler_api,
        )
    })
}

fn check_bundle_opts_for_output_inner(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    explicit_output: Option<&str>,
    incremental: Option<&mut IncrementalSemaCache>,
    allow_compiler_api: bool,
) -> (Vec<Diagnostic>, super::super::Effects::SemIndexEffectFacts) {
    let foreign_diags = validate_foreign_imports(bundle);
    if !foreign_diags.is_empty() {
        return (
            foreign_diags,
            super::super::Effects::SemIndexEffectFacts::default(),
        );
    }
    let mut diags = Vec::new();
    diags.extend(validate_script_entries(bundle));
    default_entry_return(bundle);
    diags.extend(inject_units_prelude(bundle));
    super::super::Prelude::inject(bundle);
    diags.extend(super::super::Casing::validate_bundle(bundle));
    diags.extend(resolve_unit_dimensions(bundle));
    // D-OSTARGET2=B (ratified 2026-07-03): fold every `@if @build.os == {
    // … }` switch to the arm matching this build's active OS *before* any other
    // pass sees a body — so OS-gating checks, the type-checker, and codegen only
    // meet the taken arm. Rewrites into an `@if` chain (reuses D-WHEN1).
    diags.extend(super::super::desugar_os_switches(bundle));
    // D-MIGRATE4: desugar each `change … via { (old) => … }` converter on a
    // decodable `#PublishedSchema` type into a synthetic top-level converter
    // function, so the runtime migration step (codegen) can call it. Runs before
    // registration/checking so those synthetic functions are type-checked and
    // lowered through the normal pipeline. Sets `conv_fn` on the `change` op.
    super::super::desugar_migrations(bundle);
    // D-SPREAD1=A: expand `prefix.[a, b]` to field lists (spliced in list
    // position) before inference sees bodies.
    super::super::desugar_member_spreads(bundle);
    // D-CONF-GENSPELL1=A: expand module aliases into concrete CodeModules before any
    // sibling-call mangling or registration sees the items.
    expand_generic_module_aliases(bundle, &mut diags);
    // D-CHOOSE-HEADS1=A: fold ordered multi-head declarations into one
    // ordinary enum pattern table before registration and body checking.
    desugar_multi_head_functions(bundle, &mut diags);
    // D-MOD2/D-MOD3: lift each inline module's member TYPES to their mangled
    // member identity beside the module, so registration, checking and every
    // engine see them the same way they see a generic module's members
    // (card #2054). Runs after generic expansion (instances are already
    // lifted) and before sibling-call mangling and registration.
    hoist_inline_module_member_types(bundle);
    // D-MOD2: rewrite inline-module sibling calls to their mangled names before any
    // registration/checking/codegen sees the bodies.
    mangle_inline_sibling_calls(bundle);
    // D-UNSAFE-OBLIG1=A: run after compile-time branch selection and generic
    // module expansion, but before registration/TIR. Assertions are checked and
    // erased here so no generated or untaken body bypasses the policy.
    diags.extend(super::super::UnsafeObligations::check_and_strip_with_gates(bundle, gates));
    let mut states: Vec<ModuleState> = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(module_idx, m)| ModuleState {
            module_path: m.display.clone(),
            module_alias: m.alias.clone(),
            items: m.items.clone(),
            build_facts: bundle.build_facts.clone(),
            allow_compiler_api: allow_compiler_api && module_idx == bundle.entry,
            exact_int_reachable: std::cell::Cell::new(false),
            funcs: HashMap::new(),
            registry: builtin_type_registry(),
            consts: HashMap::new(),
            imports: HashMap::new(),
            inline_foreign_imports: HashMap::new(),
            inline_reexport_foreign: HashMap::new(),
            core_imports: HashMap::new(),
            tests: HashMap::new(),
            trait_reg: TraitRegistry::default(),
            declared_states: m
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::StateDecl(state) => Some((
                        state.type_name.clone(),
                        state
                            .states
                            .iter()
                            .map(|(name, _)| name.clone())
                            .collect(),
                    )),
                    _ => None,
                })
                .collect(),
            policy_declarations: m.policy_declarations.clone(),
            rule_facts: m.rule_facts.clone(),
            code_modules: HashMap::new(),
            code_module_identities: HashMap::new(),
            unqualified: HashMap::new(),
            unqualified_file: HashMap::new(),
            core_item_imports: HashMap::new(),
            reexports: HashMap::new(),
            inline_unqualified: HashMap::new(),
            inline_unqualified_file: HashMap::new(),
            inline_core_imports: HashMap::new(),
            inline_core_items: HashMap::new(),
            inline_reexport_inline: HashMap::new(),
            inline_reexport_file: HashMap::new(),
            inline_reexport_core: HashMap::new(),
        })
        .collect();
    let mut name_ledger = std::mem::take(&mut bundle.name_ledger);
    name_ledger.clear_sema_facts();

    // Generic-instance declarations have one AST/codegen owner, while every
    // consumer registry receives the same nominal metadata. This is not a
    // declaration clone: generated Rust/TIR still sees the owner item once.
    let shared_instance_nominals: Vec<(usize, Item)> = bundle.modules.iter().enumerate().flat_map(|(owner, module)| {
        let prefixes: Vec<String> = module.items.iter().filter_map(|item| match item {
            Item::CodeModule(cm) if cm.instance_identity.is_some() =>
                Some(GenericModules::module_type_prefix(&cm.name)),
            _ => None,
        }).collect();
        module.items.iter().filter_map(move |item| match item {
            Item::Struct(def) if prefixes.iter().any(|prefix| def.name.starts_with(prefix)) => Some((owner, Item::Struct(clone_struct(def)))),
            Item::Enum(def) if prefixes.iter().any(|prefix| def.name.starts_with(prefix)) => Some((owner, Item::Enum(clone_enum(def)))),
            _ => None,
        })
    }).collect();
    for (owner, item) in &shared_instance_nominals {
        for (consumer, st) in states.iter_mut().enumerate() {
            if consumer == *owner { continue; }
            match item {
                Item::Struct(def) => {
                    register_struct(def, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                }
                Item::Enum(def) => {
                    register_enum(def, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                }
                _ => unreachable!(),
            }
            let module_alias = bundle.modules[consumer].alias.clone();
            declare_item_names(&mut name_ledger, consumer, &module_alias, item);
            let generated_name = match item {
                Item::Struct(definition) => Some(definition.name.as_str()),
                Item::Enum(definition) => Some(definition.name.as_str()),
                _ => None,
            };
            if let Some(generated_name) = generated_name {
                if let Some(display) = bundle.modules[*owner].items.iter().find_map(|item| {
                    let Item::CodeModule(instance) = item else { return None };
                    if instance.instance_identity.is_none() {
                        return None;
                    }
                    GenericModules::top_level_instance_display_paths(
                        instance,
                        &bundle.modules[*owner].items,
                    )
                    .into_iter()
                    .find_map(|(internal, display)| (internal == generated_name).then_some(display))
                }) {
                    name_ledger.record_display_path(
                        consumer,
                        format!("{}.{}", module_alias, generated_name),
                        display,
                    );
                }
            }
        }
    }

    // Derive bodies receive the same canonical path that later reflection and
    // tooling projections read. The final population pass remains below for
    // aliases and references discovered during registration.
    populate_name_ledger(bundle, &states, &mut name_ledger);

    // D-METADERIVE1=A orphan law needs a bundle-wide provider view: a derive
    // may be supplied by the entry module for an imported type, or imported
    // for an entry-local type.  Clone provider bodies/helpers before mutating
    // modules so expansion can attach generated items beside the target type.
    let derive_providers: Vec<(
        usize,
        String,
        String,
        Vec<crate::AST::DeriveBodyItem>,
        HashMap<String, Func>,
    )> = bundle
        .modules
        .iter()
        .enumerate()
        .flat_map(|(origin, module)| {
            let helpers: HashMap<String, Func> = module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Func(f) => Some((f.name.clone(), f.clone())),
                    _ => None,
                })
                .collect();
            module.items.iter().filter_map(move |item| match item {
                Item::UserDerive(d) => Some((
                    origin,
                    d.trait_name.clone(),
                    d.type_param.clone(),
                    d.body.clone(),
                    helpers.clone(),
                )),
                _ => None,
            })
        })
        .collect();

    // D-META-REG1=A / D-META-USER1=A: source marker declarations join the
    // same bundle-local registry as compiler rows and derive providers. Keep
    // the first declaration for lookup, but report every duplicate with both
    // source spans before any body can run.
    let mut marker_declarations = Vec::new();
    let mut marker_declaration_spans = HashMap::<String, (usize, Span)>::new();
    let mut fact_declaration_spans = HashMap::<String, (usize, Span)>::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            match item {
                Item::MarkerDecl(declaration) => {
                    if let Some((first_module, first_span)) =
                        marker_declaration_spans.insert(
                            declaration.name.clone(),
                            (module_idx, declaration.name_span),
                        )
                    {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!(
                                "marker `{}` is declared twice (spans {}..{} and {}..{})",
                                declaration.name,
                                first_span.start,
                                first_span.end,
                                declaration.name_span.start,
                                declaration.name_span.end,
                            ),
                            "one rule name must resolve to one declaration in the loaded bundle"
                                .to_string(),
                            "rename or remove one of the marker declarations".to_string(),
                            Some(declaration.name_span),
                        ).with_detail(format!(
                            "first declaration: module {first_module}, span {}..{}\nsecond declaration: module {module_idx}, span {}..{}",
                            first_span.start,
                            first_span.end,
                            declaration.name_span.start,
                            declaration.name_span.end,
                        )));
                    } else {
                        marker_declarations.push(declaration.clone());
                    }
                }
                Item::FactDecl(declaration) => {
                    if let Some((first_module, first_span)) = fact_declaration_spans.insert(
                        declaration.name.clone(),
                        (module_idx, declaration.name_span),
                    ) {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!(
                                "fact `{}` is declared twice (spans {}..{} and {}..{})",
                                declaration.name,
                                first_span.start,
                                first_span.end,
                                declaration.name_span.start,
                                declaration.name_span.end,
                            ),
                            "one fact name must resolve to one declaration in the loaded bundle"
                                .to_string(),
                            "rename or remove one of the fact declarations".to_string(),
                            Some(declaration.name_span),
                        ).with_detail(format!(
                            "first declaration: module {first_module}, span {}..{}\nsecond declaration: module {module_idx}, span {}..{}",
                            first_span.start,
                            first_span.end,
                            declaration.name_span.start,
                            declaration.name_span.end,
                        )));
                    }
                }
                _ => {}
            }
        }
    }
    let marker_vocabulary = jet_foundation::Policy::MarkerVocabulary::with_derives_and_declarations(
        derive_providers.iter().map(|(_, name, _, _, _)| name.clone()),
        marker_declarations,
    );
    let ct_core_imports: Vec<HashMap<String, String>> = bundle
        .modules
        .iter()
        .map(|module| crate::AST::core_import_maps(&module.imports).0)
        .collect();
    let ct_core_item_imports: Vec<HashMap<String, String>> = bundle
        .modules
        .iter()
        .map(|module| crate::AST::core_import_maps(&module.imports).1)
        .collect();
    for (state, (module, core_imports)) in states
        .iter_mut()
        .zip(bundle.modules.iter().zip(&ct_core_imports))
    {
        register_text_head_contracts(state, module, core_imports);
    }
    let mut top_level_embed_inputs = Vec::new();

    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        super::super::Protocol::expand_module_protocols(&mut module.items, &mut diags);
        // D-DOTSCOPE1: validate contextual `.member { … }` scope statements
        // against each marker's declared vocabulary (E0614/E0615/E0616/E0617/E0618).
        diags.extend(super::super::ScopeMembers::check(
            &module.items,
            &marker_vocabulary,
        ));
        // D-FIELDPOL1: computed-field cycle check (E0338) + `self.field`
        // rewrite + synthesized getter methods, before anything else.
        process_computed_fields(&mut module.items, &mut diags);
        // D-VALIDATE1 (card #506): `validate { … }` block shape check +
        // synthesized `Type.validate(value)`, same pre-registration timing.
        process_validate_blocks(&mut module.items, &mut diags);
        // D-PATCH1: synthetic `T.Patch` before struct registration.
        inject_patchable_types(&mut module.items, &mut diags);
        let base = module
            .path
            .parent()
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let mut comptime_types = HashMap::new();
        eval_comptime_items(
            &mut module.items,
            &mut comptime_types,
            &base,
            &mut diags,
            &ct_core_imports[idx],
            &ct_core_item_imports[idx],
            &bundle.build_facts,
            Some(&mut top_level_embed_inputs),
        );
        register_text_head_contracts(&mut states[idx], module, &ct_core_imports[idx]);
        super::super::Registration::resolve_comptime_declaration_values(
            &mut module.items,
            &base,
            &ct_core_imports[idx],
            &mut diags,
        );
        super::super::CheckerMarkers::resolve_static_rule_products(
            module,
            &base,
            &ct_core_imports[idx],
            &mut diags,
        );
        // Card #436: `CFFI::assemble` (jetpack crate) drains every
        // `#Extern`/`#Bindgen module` out of its declaring file and re-homes
        // it in a synthetic per-lib module (`<c.lib>`) with an empty
        // registry of its own — so a struct/enum/distinct declared in an
        // ordinary file was NEVER visible to `is_c_abi_type`'s `Type::Named`
        // lookup (`c_named_type_ok`, Sema/FFI.rs), and every named type was
        // silently rejected at the C boundary regardless of its shape. Real
        // modules are always processed before any synthetic one (assemble
        // only appends), so by this iteration every preceding module's
        // registry is already fully populated; merge them once here so a
        // same-project named type resolves. Type names are unique
        // program-wide (a duplicate definition is its own error elsewhere),
        // so this union is sound.
        let ffi_named_types: Option<HashMap<String, TypeDef>> = if module
            .items
            .iter()
            .any(|i| matches!(i, Item::CModule(_)))
        {
            Some(
                states[..idx]
                    .iter()
                    .flat_map(|s| s.registry.types.iter())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        } else {
            None
        };
        let st = &mut states[idx];
        for item in module
            .items
            .iter()
            .filter(|item| !matches!(item, Item::MarkerDecl(_) | Item::FactDecl(_)))
        {
            match item {
                Item::Func(f) => register_func_item(f, st, &mut diags, !module.no_prelude),
                Item::Struct(s) => {
                    register_struct(
                        s,
                        &mut st.registry,
                        &mut diags,
                        &st.funcs,
                        &st.consts,
                    );
                }
                Item::Enum(e) => {
                    register_enum(e, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                }
                Item::Impl(i) => {
                    if !i.type_name.contains('.') && !st.registry.contains(&i.type_name) {
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
                Item::Const(c) => {
                    if let Some(meta) = &c.meta {
                        diags.extend(CheckerCore::check_meta_attr_fields(meta));
                    }
                    register_const(c, &mut st.consts, &mut diags, &st.funcs, &st.registry)
                }
                Item::Distinct(d) => {
                    register_distinct(d, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                }
                Item::TypeAlias(a) => {
                    register_type_alias(a, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                }
                Item::Tag(_) => {}
                // D-QUAL3: a unit family lowers to one `#Numeric` distinct type
                // per member, each erasing to `Float`.
                Item::UnitFamily(uf) => {
                    let dimension = uf.resolved_dimension.clone();
                    for d in uf.distinct_defs() {
                        register_distinct(&d, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                        st.registry.unit_types.insert(d.name.clone());
                        // D-DIMENSION-OPEN1=D: a family that names a base
                        // unit relates its members by scale, dimension or not.
                        // Without a base — currency, plain tags — members stay
                        // unrelated nominal types with no conversion.
                        if let Some(owner) = uf
                            .resolved_owner
                            .as_deref()
                            .filter(|_| uf.base.is_some() || dimension.is_some())
                        {
                            if let Some(fact) = unit_fact(
                                uf,
                                &d.name,
                                dimension.clone(),
                                PathBuf::from(owner),
                            ) {
                                st.registry.unit_facts.insert(d.name.clone(), fact);
                            }
                        }
                    }
                    // D-TYPE2-TIME1=A / D-TYPE2-PLANE1=A: the canonical Time
                    // family owns these literal facts. Register them only
                    // after Prelude injection has selected the family, so
                    // `#NoPrelude` remains genuinely unscoped.
                    if uf.is_canonical_time() {
                        for member in &uf.members {
                            st.registry.literal_facts.insert(
                                format!("{}::{}", uf.family, member.name),
                                UnitFact {
                                    package: PathBuf::from(
                                        uf.resolved_owner
                                            .as_deref()
                                            .unwrap_or("core.units"),
                                    ),
                                    family: uf.family.clone(),
                                    member: member.name.clone(),
                                    dimension: dimension.clone(),
                                    scale: member.scale.clone(),
                                    scale_provenance: member.scale_provenance.clone(),
                                    offset: crate::AST::UnitRatio::zero(),
                                    kind: crate::AST::QuantityKind::Delta,
                                },
                            );
                        }
                    }
                }
                Item::Test(t) => {
                    let Some(name) = &t.name else {
                        continue;
                    };
                    if name_defined(name, &st.funcs, &st.registry, &st.consts)
                        || st.tests.contains_key(name)
                    {
                        diags.push(defined_twice(
                            name,
                            "every test needs a unique name so failures are easy to find",
                            t.name_span,
                        ));
                    } else {
                        st.tests.insert(name.clone(), t.name_span);
                    }
                }
                // D-BENCH1: `#Bench` blocks define no referenceable name; codegen
                // discovers them straight from the AST, so registration is a no-op.
                Item::Bench(_) => {}
                Item::ExternRust(block) => {
                    if check_extern_block(block, &st.registry, &mut diags) {
                        for ef in &block.functions {
                            register_extern_fn(
                                ef,
                                &mut st.funcs,
                                &st.registry,
                                &st.consts,
                                &mut diags,
                                false,
                                !module.no_prelude,
                            );
                        }
                    }
                }
                Item::CModule(cm) => {
                    // Card #436: check named C-ABI types (struct/enum/distinct)
                    // against the merged cross-file view built above, not the
                    // synthetic module's own (empty) registry. See the comment
                    // at `ffi_named_types`'s construction.
                    let merged_registry = ffi_named_types.as_ref().map(|extra| {
                        let mut types = st.registry.types.clone();
                        for (k, v) in extra {
                            types.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                        TypeRegistry {
                            types,
                            text_heads: st.registry.text_heads.clone(),
                            unit_types: st.registry.unit_types.clone(),
                            unit_facts: st.registry.unit_facts.clone(),
                            literal_facts: st.registry.literal_facts.clone(),
                            computed_fields: st.registry.computed_fields.clone(),
                            field_defaults: st.registry.field_defaults.clone(),
                        }
                    });
                    let check_registry = merged_registry.as_ref().unwrap_or(&st.registry);
                    if check_c_module(cm, check_registry, &mut diags) {
                        for ef in &cm.functions {
                            register_extern_fn(
                                ef,
                                &mut st.funcs,
                                check_registry,
                                &st.consts,
                                &mut diags,
                                true,
                                !module.no_prelude,
                            );
                        }
                    }
                }
                Item::Trait(_) => {
                }
                Item::Module(_) => {}
                Item::CodeModule(cm) => {
                    if let Some(body) = &cm.body {
                        // D-MOD2: register inline module functions under mangled names
                        // (`__jet_math__double`) so call-site sema can check them.
                        st.code_modules.insert(cm.name.clone(), cm.name.clone());
                        st.code_module_identities.insert(
                            cm.name.clone(),
                            cm.instance_identity.as_ref()
                                .map(|identity| format!("instance:{}", identity.fingerprint))
                                .unwrap_or_else(|| format!("module:{}::{}", st.module_path, cm.name)),
                        );
                        for inner in body {
                            if let Item::Func(f) = inner {
                                let mangled = jet_foundation::Names::member_name(&cm.name, &f.name);
                                st.funcs.insert(mangled.clone(), func_to_sig(f));
                                if !f.type_params.is_empty() {
                                    st.trait_reg
                                        .fn_params
                                        .insert(mangled.clone(), f.type_params.clone());
                                }
                            }
                        }
                    }
                }
                Item::ErrorConv(_) => {}
                // D-MIGRATE1: migration decls are handled by the schema diff pass; no registration needed.
                Item::Migration(_) => {}
                // D-STATE-DECL: state-set decls are sema-only (I3); no type to register.
                Item::StateDecl(_) => {}
                // D-PROTO1/D-PROTO2: expanded before registration; declaration erases.
                Item::ProtocolDecl(_) => {}
                // D-METADERIVE1=A: user-authored derive blocks are expanded below; skip here.
                Item::UserDerive(_) => {}
                // D-META-USER1=A: declaration rows were consumed by the
                // bundle-local marker registry before ordinary registration.
                Item::EffectDecl(_)
                | Item::GenericModule(_)
                | Item::ModuleAlias(_) => {}
                Item::MarkerDecl(_) | Item::FactDecl(_) => {
                    unreachable!("declaration items are consumed by the bundle registry")
                }
            }
        }
        // D-METADERIVE1=A: user-derive expansion — run after struct/func registration so
        // derive bodies can call helper functions and access TypeInfo. The expanded typed
        // items are appended to the ordinary sema stream; no source string is re-lexed.
        {
            if !derive_providers.is_empty() {
                let struct_infos: Vec<&crate::AST::StructDef> = module
                    .items
                    .iter()
                    .filter_map(|i| {
                        if let Item::Struct(s) = i {
                            Some(s)
                        } else {
                            None
                        }
                    })
                    .collect();

                let mut new_items: Vec<Item> = Vec::new();

                for s in &struct_infos {
                    let mut existing_member_sites: HashMap<String, (String, Span)> =
                        HashMap::new();
                    for item in &module.items {
                        match item {
                            Item::Struct(candidate) if candidate.name == s.name => {
                                for field in &candidate.fields {
                                    existing_member_sites.insert(
                                        field.name.clone(),
                                        (
                                            format!("existing field `{}.{}`", s.name, field.name),
                                            field.name_span,
                                        ),
                                    );
                                }
                                for method in &candidate.methods {
                                    existing_member_sites.insert(
                                        method.name.clone(),
                                        (
                                            format!("existing method `{}.{}`", s.name, method.name),
                                            method.name_span,
                                        ),
                                    );
                                }
                            }
                            Item::Impl(candidate) if candidate.type_name == s.name => {
                                for method in &candidate.methods {
                                    existing_member_sites.insert(
                                        method.name.clone(),
                                        (
                                            format!("existing method `{}.{}`", s.name, method.name),
                                            method.name_span,
                                        ),
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    let mut generated_method_spans: HashMap<String, (String, Span)> =
                        HashMap::new();
                    for (derive_name, _derive_span) in &s.derives {
                        // Prefer an entry-local provider, then one beside the target.
                        // Remaining imported/imported pairs violate the orphan law:
                        // either provider or target must be entry-local.
                        let provider = derive_providers
                            .iter()
                            .filter(|(_, name, _, _, _)| name == derive_name)
                            .min_by_key(|(origin, _, _, _, _)| {
                                if *origin == 0 {
                                    0
                                } else if *origin == idx {
                                    1
                                } else {
                                    2
                                }
                            });
                        let Some((provider_idx, _, type_param, body, helper_funcs)) = provider else {
                            continue;
                        };
                        if idx > 0 && *provider_idx > 0 {
                            diags.push(Diagnostic::error(
                                "E2711",
                                format!(
                                    "derive orphan rule: neither `derive T.{}` nor `{}` is local",
                                    derive_name, s.name
                                ),
                                "a generated implementation is owned locally only when the derive provider or target type lives in the entry module".to_string(),
                                format!(
                                    "define `derive T.{}` or `{}` in the entry module",
                                    derive_name, s.name
                                ),
                                // The violating marker belongs to an imported source file;
                                // the bundled diagnostic currently renders against the entry
                                // file, so omit a misleading entry-file caret.
                                None,
                            ));
                            continue;
                        }
                        let actual_funcs: HashMap<String, &Func> = helper_funcs
                            .iter()
                            .map(|(name, func)| (name.clone(), func))
                            .collect();
                        let states = module
                            .items
                            .iter()
                            .find_map(|item| match item {
                                Item::StateDecl(state) if state.type_name == s.name => Some(
                                    state
                                        .states
                                        .iter()
                                        .map(|(name, _)| name.clone())
                                        .collect::<Vec<_>>(),
                                ),
                                _ => None,
                            })
                            .unwrap_or_default();
                        let type_path = name_ledger
                            .canonical_path(idx, &s.name)
                            .expect("derive target missing from the name ledger");
                        let type_info = crate::Comptime::build_struct_type_info_with_path(
                            s,
                            &states,
                            &type_path,
                        );

                        match crate::Comptime::expand_derive_body(
                            body,
                            type_param,
                            type_info,
                            &actual_funcs,
                            &bundle.project_root,
                        ) {
                                Ok(expanded) => {
                                    let mut methods = Vec::new();
                                    for item in expanded {
                                        match item {
                                            Item::Func(function) => {
                                                if let Some((existing_site, existing_span)) =
                                                    existing_member_sites.get(&function.name)
                                                {
                                                    diags.push(derive_member_collision(
                                                        derive_name,
                                                        &s.name,
                                                        &function,
                                                        existing_site,
                                                        *existing_span,
                                                    ));
                                                    continue;
                                                }
                                                if let Some((generated_derive, generated_span)) =
                                                    generated_method_spans.get(&function.name)
                                                {
                                                    let existing_site = format!(
                                                        "generated member `{}` from `derive T.{}`",
                                                        function.name, generated_derive
                                                    );
                                                    diags.push(derive_member_collision(
                                                        derive_name,
                                                        &s.name,
                                                        &function,
                                                        &existing_site,
                                                        *generated_span,
                                                    ));
                                                    continue;
                                                }
                                                generated_method_spans.insert(
                                                    function.name.clone(),
                                                    (derive_name.clone(), function.name_span),
                                                );
                                                methods.push(function)
                                            }
                                            other => new_items.push(other),
                                        }
                                    }
                                    if !methods.is_empty() {
                                        new_items.push(Item::Impl(crate::AST::ImplDef {
                                            span: s.span,
                                            type_name: s.name.clone(),
                                            type_span: s.name_span,
                                            // A derive provider names the
                                            // capability that selects it; its
                                            // body supplies ordinary members
                                            // on the target. The provider name
                                            // is not a Rust/Jet trait
                                            // conformance requirement.
                                            trait_name: None,
                                            trait_span: None,
                                            methods,
                                            delegation_field: None,
                                            assoc_type_impls: Vec::new(),
                                            is_generated_serde: false,
                                            os_target: None,
                                        }));
                                    }
                                }
                            Err(inner) => {
                                diags.push(inner);
                            }
                        }
                    }
                }

                // Register new items before synthesis so they go through
                // the normal sema pipeline.
                for item in &new_items {
                    match item {
                        Item::Func(f) => register_func_item(f, st, &mut diags, !module.no_prelude),
                        Item::Struct(s) => {
                            register_struct(
                                s,
                                &mut st.registry,
                                &mut diags,
                                &st.funcs,
                                &st.consts,
                            );
                        }
                        Item::Enum(e) => {
                            register_enum(
                                e,
                                &mut st.registry,
                                &mut diags,
                                &st.funcs,
                                &st.consts,
                            );
                        }
                        Item::Tag(_) => {}
                        Item::Impl(_) => {}
                        _ => {}
                    }
                }
                module.items.extend(new_items);
            }
        }

        // D-META-USER1=A: a declared marker body uses the same typed
        // comptime expansion path as a derive body. The target TypeInfo is
        // immutable input; generated items re-enter ordinary registration.
        {
            let declared_targets: Vec<(
                crate::AST::StructDef,
                crate::AST::Marker,
                crate::AST::MarkerDecl,
            )> = module
                .items
                .iter()
                .flat_map(|item| {
                    let Item::Struct(def) = item else { return None };
                    Some(def.type_markers.iter().filter_map(|marker| {
                        let declaration = marker_vocabulary.declaration(&marker.name)?.clone();
                        declaration.body.as_ref()?;
                        Some((def.clone(), marker.clone(), declaration))
                    }))
                })
                .flatten()
                .collect();
            let helper_funcs_owned: HashMap<String, Func> = module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Func(function) => Some((function.name.clone(), function.clone())),
                    _ => None,
                })
                .collect();
            let actual_funcs: HashMap<String, &Func> = helper_funcs_owned
                .iter()
                .map(|(name, function)| (name.clone(), function))
                .collect();
            let mut new_items = Vec::new();

            for (target, marker, declaration) in declared_targets {
                if marker.negated {
                    continue;
                }
                let Some(body) = declaration.body.as_ref() else {
                    continue;
                };
                let states = module
                    .items
                    .iter()
                    .find_map(|item| match item {
                        Item::StateDecl(state) if state.type_name == target.name => Some(
                            state
                                .states
                                .iter()
                                .map(|(name, _)| name.clone())
                                .collect::<Vec<_>>(),
                        ),
                        _ => None,
                    })
                    .unwrap_or_default();
                let type_path = name_ledger
                    .canonical_path(idx, &target.name)
                    .expect("declared rule target missing from the name ledger");
                let type_info = crate::Comptime::build_struct_type_info_with_path_and_vocabulary(
                    &target,
                    &states,
                    &type_path,
                    Some(&marker_vocabulary),
                );
                match crate::Comptime::expand_derive_body(
                    body,
                    "target",
                    type_info,
                    &actual_funcs,
                    &bundle.project_root,
                ) {
                    Ok(expanded) => {
                        for item in expanded {
                            let Item::Impl(implementation) = &item else {
                                new_items.push(item);
                                continue;
                            };
                            let mut accepted = implementation.clone();
                            accepted.methods.retain(|method| {
                                let existing = existing_member_span(
                                    &module.items,
                                    &accepted.type_name,
                                    &method.name,
                                )
                                .or_else(|| {
                                    existing_member_span(
                                        &new_items,
                                        &accepted.type_name,
                                        &method.name,
                                    )
                                });
                                let Some(existing) = existing else {
                                    return true;
                                };
                                diags.push(
                                    Diagnostic::error(
                                        "E0105",
                                        format!(
                                            "generated member `{}` would change or shadow `{}` on `{}`",
                                            method.name, method.name, accepted.type_name
                                        ),
                                        "a rule may add a member but cannot change or shadow a written member".to_string(),
                                        "rename the generated member or remove the existing member".to_string(),
                                        Some(method.name_span),
                                    )
                                    .with_detail(format!(
                                        "generated member `{}` from rule `{}` at span {}..{}\nexisting member `{}.{}` at span {}..{}",
                                        method.name,
                                        declaration.name,
                                        method.name_span.start,
                                        method.name_span.end,
                                        accepted.type_name,
                                        method.name,
                                        existing.start,
                                        existing.end,
                                    )),
                                );
                                false
                            });
                            if !accepted.methods.is_empty() {
                                new_items.push(Item::Impl(accepted));
                            }
                        }
                    }
                    Err(inner) => diags.push(inner),
                }
            }

            // Function-site rules use the same typed body expansion. Their
            // target projection is a FunctionInfo, and a body may reject the
            // declaration or emit ordinary top-level Jet items.
            let declared_function_targets: Vec<(
                crate::AST::Func,
                crate::AST::Marker,
                crate::AST::MarkerDecl,
            )> = module
                .items
                .iter()
                .flat_map(|item| {
                    let Item::Func(function) = item else { return None };
                    Some(function.markers.iter().filter_map(|marker| {
                        let declaration = marker_vocabulary.declaration(&marker.name)?.clone();
                        declaration.body.as_ref()?;
                        Some((function.clone(), marker.clone(), declaration))
                    }))
                })
                .flatten()
                .collect();

            for (target, marker, declaration) in declared_function_targets {
                if marker.negated {
                    continue;
                }
                let Some(body) = declaration.body.as_ref() else {
                    continue;
                };
                let target_info = crate::Comptime::build_function_type_info(&target);
                match crate::Comptime::expand_derive_body(
                    body,
                    "target",
                    target_info,
                    &actual_funcs,
                    &bundle.project_root,
                ) {
                    Ok(expanded) => new_items.extend(expanded),
                    Err(inner) => diags.push(inner),
                }
            }

            // D-META-DSL1=A: a `.Block` rule receives the text inside Jet's
            // closed braces as its `target` value. The library body can check
            // that text and emit ordinary Jet items through the same typed
            // path as a type or function rule.
            let mut declared_text_blocks = Vec::new();
            for item in &module.items {
                collect_item_declared_text_blocks(
                    item,
                    &module.source,
                    &module.block_spans,
                    &mut declared_text_blocks,
                );
            }
            for (block_name, block_text, _block_span) in declared_text_blocks {
                let Some(declaration) = marker_vocabulary.declaration(&block_name) else {
                    continue;
                };
                let Some(body) = declaration.body.as_ref() else {
                    continue;
                };
                match crate::Comptime::expand_derive_body(
                    body,
                    "target",
                    crate::AST::CtValue::Str(block_text),
                    &actual_funcs,
                    &bundle.project_root,
                ) {
                    Ok(expanded) => new_items.extend(expanded),
                    Err(inner) => diags.push(inner),
                }
            }

            for item in &new_items {
                match item {
                    Item::Func(function) => {
                        register_func_item(function, st, &mut diags, !module.no_prelude)
                    }
                    Item::Struct(def) => register_struct(
                        def,
                        &mut st.registry,
                        &mut diags,
                        &st.funcs,
                        &st.consts,
                    ),
                    Item::Enum(def) => register_enum(
                        def,
                        &mut st.registry,
                        &mut diags,
                        &st.funcs,
                        &st.consts,
                    ),
                    Item::Tag(_) => {}
                    Item::Impl(_) => {}
                    _ => {}
                }
            }
            module.items.extend(new_items);
        }

        st.consts.extend(comptime_types);
        // D-UNIONTYPE1=A / R11: materialize every anonymous union seen after
        // typed derive/marker expansion as a sema-owned hidden enum. Its
        // representation remains backend sugar; its codec members are normal
        // checked Jet impl items, just like named serde types.
        let union_item_start = module.items.len();
        super::super::Registration::inject_anonymous_union_items(&mut module.items);
        for item in module.items.iter().skip(union_item_start) {
            if let Item::Enum(definition) = item {
                register_enum(
                    definition,
                    &mut st.registry,
                    &mut diags,
                    &st.funcs,
                    &st.consts,
                );
            }
        }
        // D-ONCE-DERIVE1=A / I3: built-in capability requests re-enter as
        // ordinary Jet impl blocks before the final trait registration pass.
        let known_functions: HashSet<String> = module
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Func(function) => Some(function.name.clone()),
                _ => None,
            })
            .collect();
        super::super::Registration::expand_builtin_derive_items(&mut module.items, &mut diags);
        for item in &module.items {
            if let Item::Func(function) = item {
                if !known_functions.contains(&function.name) {
                    register_func_item(function, st, &mut diags, !module.no_prelude);
                }
            }
        }
        // D-SERDE2=A/R11: built-in codecs re-enter as ordinary Jet source in
        // bundle builds too; this is the production multi-file path.
        super::super::Registration::expand_builtin_serde_items(&mut module.items, &mut diags);
        // Anonymous union declarations are synthesized by the same serde pass
        // as their typed codec impls. Register the new enum names before any
        // generated method body is checked.
        register_generated_union_enums(&module.items, st, &mut diags);

        // S62 + D-LIB2: synthesis must happen before register_impl_methods
        // so the synthesised Func nodes appear in the type registry.
        synthesize_impls(&mut module.items);
        register_type_methods(&module.items, &mut st.registry, &mut diags);
        register_patchable_methods(&module.items, &mut st.registry);
        register_impl_methods(&module.items, &mut st.registry, &mut diags);
        // D-TXN-ROLLBACK layer 2: ensure Rollback is known before user impl blocks.
        st.trait_reg.register_synthetic_rollback();
        st.trait_reg.register_synthetic_display_debug();
        st.trait_reg.register_synthetic_close();
        st.trait_reg.register_synthetic_operators();
        st.trait_reg.register_synthetic_iter_index();
        st.trait_reg.register_synthetic_io();
        st.trait_reg.register_synthetic_driver();
        st.trait_reg.register_items(&module.items, &mut diags);
        for type_name in &st.registry.unit_types {
            st.trait_reg
                .trait_impls
                .insert((type_name.clone(), crate::Generics::DISPLAY.to_string()));
        }
        for (type_name, fact) in &st.registry.unit_facts {
            {
                st.trait_reg.trait_impls.insert((
                    type_name.clone(),
                    crate::Generics::quantity_bound(&fact.family, fact.kind.name()),
                ));
                for capability in [crate::Generics::ENCODE, crate::Generics::DECODE] {
                    st.trait_reg
                        .derives
                        .entry(type_name.clone())
                        .or_default()
                        .insert(capability.to_string());
                }
            }
        }
        // D-SERDE: validate `#[Codable]`/`#[Encode]`/`#[Decode]` markers (E2407–E2412)
        // now that the trait registry resolves field/variant types — keeps the emitted
        // `impl`s rustc-clean (I2).
        diags.extend(validate_serde_items(&module.items, &st.trait_reg));
        // D-MARK-VOCAB1 (card #518): a marker name outside the registered
        // `@`/`#` plane vocabulary is E0927, instead of silently doing
        // nothing (the parser accepts any PascalCase name structurally).
        diags.extend(check_marker_vocabulary(
            &module.items,
            &module.rule_facts,
            &marker_vocabulary,
        ));
        diags.extend(check_declared_rule_facts(
            &module.rule_facts,
            &marker_vocabulary,
        ));
        // D-CLIFLAG1: validate `#[CLI]`-derived structs (E1305/E1306), same
        // timing as the serde pass above (trait registry must be built so
        // `CLI` is visible on `s.derives`).
        diags.extend(validate_cli_items(&module.items, &st.trait_reg));
        // D-MIGRATE1: schema diff pass (E0910) — runs after struct registration (I3).
        diags.extend(check_schema_migrations(
            &module.items,
            &bundle.project_root,
            &st.trait_reg,
        ));
    }
    // D-BOUND-UNDO1=A: an inverse belongs to the module that owns the foreign
    // binding. CFFI re-homes C declarations into a shared synthetic module, so
    // use the import links to recover that owner rather than consulting a
    // bundle-wide bare-name table.
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        let importers = bundle
            .cffi
            .import_links
            .iter()
            .filter(|link| link.target_idx == module_idx)
            .map(|link| link.importing_idx)
            .collect::<std::collections::BTreeSet<_>>();
        if importers.is_empty() {
            validate_foreign_undo_contracts(
                &module.items,
                &states[module_idx].funcs,
                &mut diags,
            );
            continue;
        }

        for contract in foreign_undo_contracts(&module.items) {
            let owners = importers
                .iter()
                .copied()
                .filter(|owner| states[*owner].funcs.contains_key(contract.inverse))
                .collect::<Vec<_>>();
            match owners.as_slice() {
                [owner] => validate_foreign_undo_contract(
                    &contract,
                    &states[*owner].funcs,
                    &mut diags,
                ),
                [] => validate_foreign_undo_contract(
                    &contract,
                    &states[module_idx].funcs,
                    &mut diags,
                ),
                _ => {
                    let modules = owners
                        .iter()
                        .map(|owner| bundle.modules[*owner].display.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    diags.push(Diagnostic::error(
                        "E0105",
                        format!(
                            "undo function `{}` is ambiguous for foreign binding `{}`",
                            contract.inverse, contract.forward_name
                        ),
                        format!(
                            "the shared C binding is imported by more than one module that defines `{}`: {modules}",
                            contract.inverse
                        ),
                        "give the foreign binding one uniquely named inverse in one defining module"
                            .to_string(),
                        Some(contract.inverse_span),
                    ));
                }
            }
        }
    }
    // D-META-AUTO1: the bundle projection is the canonical answer for local
    // shapes that contain imported nominal types. The early item-local pass
    // handles explicit requests and declarations produced while processing one
    // module; complete any missing structural implementations from this shared
    // fixed point before body checking and codegen consume the finished AST.
    let bundle_auto_derives = TraitRegistry::bundle_auto_derives(bundle, &name_ledger);
    for idx in 0..states.len() {
        let auto_derives = &bundle_auto_derives[idx];
        states[idx].trait_reg.merge_auto_derives(auto_derives);

        let module = &mut bundle.modules[idx];
        super::super::Registration::expand_builtin_derive_items_with_auto(
            &mut module.items,
            auto_derives,
            &mut diags,
        );
        for item in &module.items {
            if let Item::Func(function) = item {
                if !states[idx].funcs.contains_key(&function.name) {
                    register_func_item(
                        function,
                        &mut states[idx],
                        &mut diags,
                        !module.no_prelude,
                    );
                }
            }
        }

        let serde_item_start = module.items.len();
        super::super::Registration::expand_builtin_serde_items_with_auto(
            &mut module.items,
            auto_derives,
            &mut diags,
        );
        register_generated_union_enums(&module.items, &mut states[idx], &mut diags);
        register_impl_methods(
            &module.items[serde_item_start..],
            &mut states[idx].registry,
            &mut diags,
        );
    }
    bundle.comptime_inputs.extend(top_level_embed_inputs);
    diags.extend(super::super::BudgetSpecs::validate_bundle(bundle));

    // S62 E2401: delegation validation — check field exists and implements trait.
    // Runs after all m9 registrations so implements_trait is populated.
    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &states[idx];
        for item in &module.items {
            if let Item::Impl(i) = item {
                if let (Some(trait_name), Some(field_name)) = (&i.trait_name, &i.delegation_field) {
                    if let Some(fields) = st.registry.struct_fields(&i.type_name) {
                        if let Some((_, _, field_ty)) =
                            fields.iter().find(|(n, _, _)| n == field_name)
                        {
                            let field_type_name = field_ty.name();
                            if !st.trait_reg.implements_trait(&field_type_name, trait_name) {
                                diags.push(Diagnostic::error(
                                    "E2401",
                                    format!(
                                        "`{}` doesn't implement `{}`, so it can't delegate",
                                        field_type_name, trait_name
                                    ),
                                    format!(
                                        "`impl {}.{} using {}` forwards `{}` methods to the `{}` field, but `{}` doesn't implement `{}`",
                                        i.type_name, trait_name, field_name,
                                        trait_name, field_name,
                                        field_type_name, trait_name
                                    ),
                                    format!(
                                        "implement `impl {}: {}` on the field's type, or choose a different field",
                                        field_type_name, trait_name
                                    ),
                                    Some(i.type_span),
                                ));
                            }
                        } else {
                            diags.push(Diagnostic::error(
                                "E2401",
                                format!("`{}` has no field `{}`", i.type_name, field_name),
                                format!(
                                    "`impl {}.{} using {}` needs `{}` to have a field named `{}`",
                                    i.type_name, trait_name, field_name, i.type_name, field_name
                                ),
                                format!("add `{}: Type` to `struct {}`", field_name, i.type_name),
                                Some(i.type_span),
                            ));
                        }
                    }
                }
            }
        }
    }

    // D-NAME-TREE1=A: registration is complete, so publish declarations and
    // visibility before any import or body pass consults them. The later
    // unqualified-import pass adds alias rows to this same ledger.
    populate_name_ledger(bundle, &states, &mut name_ledger);

    // D-MOD3/4: Unqualified imports (`use alias.Item`) are processed in a
    // dedicated pass *after* file-module aliases land in `st.imports` below.

    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &mut states[idx];
        for imp in &module.imports {
            // Foreign member lists retain the ordinary unqualified AST shape,
            // but bind mounted namespaces here as one transactional group.
            if matches!(&imp.kind, ImportKind::Unqualified { .. }) {
                let foreign = foreign_imports_after_validation(imp);
                if foreign.is_empty() {
                    continue;
                }
                let mut group_failed = false;
                let mut resolved = Vec::new();
                let mut names = HashSet::new();
                for (namespace, alias) in foreign {
                    if st.imports.contains_key(&alias)
                        || st.core_imports.contains_key(&alias)
                        || !names.insert(alias.clone())
                    {
                        group_failed = true;
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{alias}` is used twice"),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                        continue;
                    }
                    let target = if namespace.language == crate::AST::ForeignLanguage::C {
                        bundle.cffi.target_for(idx, &alias)
                    } else {
                        bundle
                            .modules
                            .iter()
                            .position(|candidate| candidate.display == namespace.display())
                    };
                    if let Some(target) = target {
                        resolved.push((alias, target));
                    } else {
                        group_failed = true;
                        let root = namespace.language.root();
                        diags.push(Diagnostic::error(
                            "E1002",
                            format!("`{root}` is reserved for first-party or foreign packages"),
                            "a foreign namespace must resolve to a mounted library before it can be imported".to_string(),
                            format!("make the `{root}` binding available or rename the import"),
                            Some(imp.alias_span),
                        ));
                    }
                }
                if !group_failed {
                    for (alias, target) in resolved {
                        st.imports.insert(alias, target);
                    }
                }
                continue;
            }
            let alias = imp.import_alias();
            if st.imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", Syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if st.core_imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", Syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if let ImportKind::Module(name, _) = &imp.kind {
                if crate::Syntax::is_legacy_std_import(name) {
                    diags.push(Diagnostic::error(
                        "E0019",
                        format!("`{name}` is the old standard-library import spelling"),
                        "the standard library module was renamed to `core`".to_string(),
                        format!(
                            "use `import {}` or `import {}.fs as fs`",
                            Syntax::CORE_SHORT,
                            Syntax::CORE_SHORT
                        ),
                        Some(imp.span),
                    ));
                    continue;
                }
                // D-CORENS1 / E0341: old `jet.<ring>` spelling → teach the new `core.<ring>`.
                if let Some(ring) = name.strip_prefix("jet.") {
                    if crate::Syntax::is_ring_module(ring) {
                        diags.push(Diagnostic::error(
                            "E0341",
                            format!("`use jet.{ring}` is the old first-party library spelling"),
                            "first-party libraries moved to the `core.*` namespace (D-CORENS1)"
                                .to_string(),
                            format!(
                                "write `use {}` instead",
                                crate::Syntax::canonical_ring_module(ring)
                            ),
                            Some(imp.span),
                        ));
                        continue;
                    }
                }
            }
            if let Some(module) = imp.core_module_path() {
                if !crate::Syntax::is_known_core_module(&module) {
                    diags.push(crate::Sema::CheckerCoreLib::unknown_core_module(
                        &module,
                        imp.span,
                    ));
                    continue;
                }
                // D-RINGLAYER1=A: infer minimum layer and enforce optional ceiling.
                if let Some(mod_layer) = crate::Syntax::core_module_layer(&module) {
                    if let Some(ceiling) = bundle.layer_ceiling {
                        if mod_layer > ceiling {
                            diags.push(crate::Syntax::layer_ceiling_exceeded(
                                &module,
                                mod_layer,
                                ceiling,
                                Some(imp.span),
                                Some(&format!("`use {module}`")),
                            ));
                            continue;
                        }
                    }
                    if mod_layer > bundle.inferred_layer {
                        bundle.inferred_layer = mod_layer;
                    }
                }
                st.core_imports.insert(alias, module);
                continue;
            }
            if matches!(&imp.kind, ImportKind::File(path, _) if path.ends_with(".h")) {
                let target = bundle.cffi.target_for(idx, &alias).unwrap_or_else(|| {
                    unreachable!(
                        "C header import target missing after CFFI assembly: module={} alias={alias}",
                        idx
                    )
                });
                st.imports.insert(alias.clone(), target);
                name_ledger.record_import_target(idx, imp.span, target);
                continue;
            }
            let foreign = foreign_imports_after_validation(imp);
            if !foreign.is_empty() {
                let mut group_failed = false;
                let mut resolved = Vec::new();
                for (namespace, foreign_alias) in foreign {
                    let target = if namespace.language == crate::AST::ForeignLanguage::C {
                        bundle.cffi.target_for(idx, &foreign_alias)
                    } else {
                        bundle
                            .modules
                            .iter()
                            .position(|candidate| candidate.display == namespace.display())
                    };
                    if let Some(target) = target {
                        resolved.push((foreign_alias, target));
                    } else {
                        group_failed = true;
                        let root = namespace.language.root();
                        diags.push(Diagnostic::error(
                            "E1002",
                            format!("`{root}` is reserved for first-party or foreign packages"),
                            "a foreign namespace must resolve to a mounted library before it can be imported".to_string(),
                            format!("make the `{root}` binding available or rename the import"),
                            Some(imp.alias_span),
                        ));
                    }
                }
                if !group_failed {
                    for (foreign_alias, target) in resolved {
                        st.imports.insert(foreign_alias, target);
                    }
                    if let Some(target) = st.imports.get(&alias).copied() {
                        name_ledger.record_import_target(idx, imp.span, target);
                    }
                }
                continue;
            }
            // S59 (E2-M14): C `use` forms bind to a synthetic merged module
            // resolved by `CFFI::assemble` (E3204 already reported there).
            if is_c_import_after_validation(imp) {
                if let Some(target) = bundle.cffi.target_for(idx, &alias) {
                    st.imports.insert(alias.clone(), target);
                    name_ledger.record_import_target(idx, imp.span, target);
                }
                continue;
            }
            if let Some(target) = name_ledger.import_target(idx, imp.span) {
                st.imports.insert(alias, target);
            }
        }
    }

    // D-MOD3/4: process `use alias.Item` unqualified imports now that file-module
    // aliases are registered in `st.imports`. `pub use` additionally re-exports the
    // item onto this module's public surface (`reexports`).
    for (idx, module) in bundle.modules.iter().enumerate() {
        for imp in &module.imports {
            let ImportKind::Unqualified {
                module_alias,
                module_alias_span,
                ..
            } = &imp.kind
            else {
                continue;
            };
            if !foreign_imports_after_validation(imp).is_empty() {
                continue;
            }
            let bindings = imp.walk_bindings();
            let group_diagnostics = diags.len();
            let mut inserted_unqualified = Vec::new();
            let mut inserted_reexports = Vec::new();
            let mut inserted_core = Vec::new();
            let mut inserted_core_items = Vec::new();
            let mut inserted_file = Vec::new();
            let mut inserted_imports = Vec::new();
            if let Some(canonical) = states[idx].code_modules.get(module_alias.as_str()).cloned() {
                // Inline module: items are mangled as `__jet_{alias}__{item}`.
                let st = &mut states[idx];
                for binding in &bindings {
                    let orig = binding
                        .original
                        .expect("member walker returned a binding without a member");
                    let local = binding.local.clone();
                    let mangled = jet_foundation::Names::member_name(&canonical, orig);
                    if st.unqualified.contains_key(&local)
                        || st.unqualified_file.contains_key(&local)
                        || st.core_imports.contains_key(&local)
                    {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{local}` is used twice"),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                    } else if !st.funcs.contains_key(&mangled) {
                        diags.push(Diagnostic::error(
                            "E0611",
                            format!("`{}` is not defined in module `{}`", orig, module_alias),
                            "check the module body for the item you're importing".to_string(),
                            "make sure the name is spelled correctly".to_string(),
                            Some(*module_alias_span),
                        ));
                    } else if !name_ledger.exported(idx, &mangled) {
                        diags.push(Diagnostic::error(
                            "E0609",
                            format!("`{}` is private in module `{}`", orig, module_alias),
                            "only `pub` items can be brought into scope with `use`".to_string(),
                            format!(
                                "add `pub` before `fn {}` in module `{}`",
                                orig, module_alias
                            ),
                            Some(*module_alias_span),
                        ));
                    } else {
                        st.unqualified.insert(local.clone(), mangled.clone());
                        inserted_unqualified.push(local.clone());
                        if imp.is_pub {
                            st.reexports.insert(local.clone(), (mangled, idx));
                            inserted_reexports.push(local);
                        }
                    }
                }
            } else if let Some(core_prefix) = crate::AST::core_list_prefix(module_alias) {
                // D-CORE-USELIST1=A: a list member may name either a Core
                // submodule (`core.encoding.[json]`) or an item in the
                // longest known module prefix (`core.math.[abs]`).
                let st = &mut states[idx];
                for binding in &bindings {
                    let orig = binding
                        .original
                        .expect("member walker returned a binding without a member");
                    let local = binding.local.clone();
                    let full = format!("{core_prefix}.{orig}");
                    let target = match crate::AST::core_list_path(module_alias, orig) {
                        Some(crate::AST::CoreListPath::Module(module)) => Some((module, None)),
                        Some(crate::AST::CoreListPath::Item { module, item })
                            if crate::Sema::CheckerCoreLib::core_module_items(&module)
                                .iter()
                                .any(|known| known == &item) =>
                        {
                            Some((module, Some(item)))
                        }
                        _ => None,
                    };
                    let Some((module, item)) = target else {
                        if crate::Syntax::is_known_core_module(&core_prefix) {
                            diags.push(crate::Sema::CheckerCoreLib::unknown_core_item(
                                &core_prefix,
                                orig,
                                *module_alias_span,
                            ));
                        } else {
                            diags.push(crate::Sema::CheckerCoreLib::unknown_core_module(
                                &full,
                                *module_alias_span,
                            ));
                        }
                        continue;
                    };
                    if st.core_imports.contains_key(&local)
                        || st.unqualified.contains_key(&local)
                        || st.unqualified_file.contains_key(&local)
                    {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{}` is used twice", local),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                    } else {
                        // D-RINGLAYER1=A M2: unqualified `use core.X` obeys the same layer rules.
                        if let Some(mod_layer) = crate::Syntax::core_module_layer(&module) {
                            if let Some(ceiling) = bundle.layer_ceiling {
                                if mod_layer > ceiling {
                                    diags.push(crate::Syntax::layer_ceiling_exceeded(
                                        &module,
                                        mod_layer,
                                        ceiling,
                                        Some(*module_alias_span),
                                        Some(&format!("`use {core_prefix}.{orig}`")),
                                    ));
                                    continue;
                                }
                            }
                            if mod_layer > bundle.inferred_layer {
                                bundle.inferred_layer = mod_layer;
                            }
                        }
                        st.core_imports.insert(local.clone(), module);
                        inserted_core.push(local.clone());
                        if let Some(item) = item {
                            st.core_item_imports.insert(local.clone(), item);
                            inserted_core_items.push(local);
                        }
                    }
                }
            } else if states[idx].imports.contains_key(module_alias.as_str()) {
                // File module: look up items in the target module's state.
                let target_idx = states[idx].imports[module_alias.as_str()];
                let is_reexport = imp.is_pub;
                for binding in &bindings {
                    let orig = binding
                        .original
                        .expect("member walker returned a binding without a member");
                    let local = binding.local.clone();
                    let is_pub = name_ledger.visible(idx, target_idx, orig);
                    let file_module_target = states[target_idx]
                        .imports
                        .get(orig)
                        .copied()
                        .filter(|_| {
                            name_ledger
                                .declaration(target_idx, orig)
                                .is_some_and(|declaration| declaration.kind == "file_module")
                        });
                    if states[idx].unqualified.contains_key(&local)
                        || states[idx].unqualified_file.contains_key(&local)
                        || states[idx].core_imports.contains_key(&local)
                    {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{local}` is used twice"),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                    } else if let Some(file_module_target) = file_module_target {
                        if !is_pub {
                            diags.push(Diagnostic::error(
                                "E0609",
                                format!("`{}` is private in module `{}`", orig, module_alias),
                                "only public modules can be brought into scope with `use`".to_string(),
                                format!("add `pub` before `module {}` in the imported file", orig),
                                Some(*module_alias_span),
                            ));
                        } else {
                            states[idx].imports.insert(local.clone(), file_module_target);
                            inserted_imports.push(local.clone());
                        }
                        continue;
                    }
                    let exists = states[target_idx].funcs.contains_key(orig);
                    if !exists {
                        diags.push(Diagnostic::error(
                            "E0611",
                            format!("`{}` is not defined in module `{}`", orig, module_alias),
                            "check the module for the item you're importing".to_string(),
                            "make sure the name is spelled correctly".to_string(),
                            Some(*module_alias_span),
                        ));
                    } else if !is_pub {
                        diags.push(Diagnostic::error(
                            "E0609",
                            format!("`{}` is private in module `{}`", orig, module_alias),
                            "only `pub` items can be brought into scope with `use`".to_string(),
                            format!("add `pub` before `fn {}` in the imported file", orig),
                            Some(*module_alias_span),
                        ));
                    } else {
                        states[idx]
                            .unqualified_file
                            .insert(local.clone(), (orig.to_string(), target_idx));
                        inserted_file.push(local.clone());
                        if is_reexport {
                            states[idx]
                                .reexports
                                .insert(local.clone(), (orig.to_string(), target_idx));
                            inserted_reexports.push(local);
                        }
                    }
                }
            } else {
                // Module alias not found — E0610.
                diags.push(Diagnostic::error(
                    "E0610",
                    format!("no module named `{}` in scope", module_alias),
                    "the alias must refer to a module imported earlier in this file".to_string(),
                    format!("add `import … as {}`  before this `use`", module_alias),
                    Some(*module_alias_span),
                ));
            }
            if diags.len() != group_diagnostics {
                let st = &mut states[idx];
                for name in inserted_unqualified {
                    st.unqualified.remove(&name);
                }
                for name in inserted_reexports {
                    st.reexports.remove(&name);
                }
                for name in inserted_core {
                    st.core_imports.remove(&name);
                }
                for name in inserted_core_items {
                    st.core_item_imports.remove(&name);
                }
                for name in inserted_file {
                    st.unqualified_file.remove(&name);
                }
                for name in inserted_imports {
                    st.imports.remove(&name);
                }
            }
        }
    }

    resolve_inline_module_imports(bundle, &mut states, &mut name_ledger, &mut diags);

    complete_bundle_check(
        bundle,
        &states,
        mode,
        freestanding,
        gates,
        explicit_output,
        incremental,
        allow_compiler_api,
        name_ledger,
        diags,
    )
}

/// D-FAIL-EXIT1=A: every explicit `fn run` gets the default fallible entry
/// carrier before registration and body inference. The source may omit the
/// return clause; the checked AST still carries one canonical `Result<(), Err>`
/// contract through sema, TIR, AOT, JIT, and the interpreter.
fn default_entry_return(bundle: &mut ProgramBundle) {
    let Some(module) = bundle.modules.get_mut(bundle.entry) else {
        return;
    };
    let Some(run) = module.items.iter_mut().find_map(|item| match item {
        Item::Func(function) if function.name == "run" => Some(function),
        _ => None,
    }) else {
        return;
    };
    if run.return_type.is_none() {
        run.return_type = Some(Type::Result {
            ok: Box::new(Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())),
            err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
        });
    }
}
