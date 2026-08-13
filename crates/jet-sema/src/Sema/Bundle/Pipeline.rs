use super::*;

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

pub(super) fn check_bundle_opts_for_output_with_context(
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
    mut incremental: Option<&mut IncrementalSemaCache>,
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
    // D-OSTARGET2=B (ratified 2026-07-03): fold every `@if build.os == {
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
    // D-GENMOD2=A: expand module aliases into concrete CodeModules before any
    // sibling-call mangling or registration sees the items.
    expand_generic_module_aliases(bundle, &mut diags);
    // D-CHOOSE-HEADS1=A: fold ordered multi-head declarations into one
    // ordinary enum pattern table before registration and body checking.
    desugar_multi_head_functions(bundle, &mut diags);
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
            allow_compiler_api: allow_compiler_api && module_idx == bundle.entry,
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
        Vec<crate::AST::Stmt>,
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
        .map(|module| {
            let mut imports = HashMap::new();
            for import in &module.imports {
                if let Some(core_module) = import.core_module_path() {
                    imports.insert(import.import_alias(), core_module);
                }
                let ImportKind::Unqualified {
                    module_alias,
                    ..
                } = &import.kind
                else {
                    continue;
                };
                let Some(core_prefix) = crate::AST::core_list_prefix(module_alias) else {
                    continue;
                };
                for binding in import.walk_bindings() {
                    let original = binding
                        .original
                        .expect("member walker returned a binding without a member");
                    let local = binding.local;
                    let full = format!("{core_prefix}.{original}");
                    if crate::Syntax::is_known_core_module(&full) {
                        imports.insert(local, full);
                    }
                }
            }
            imports
        })
        .collect();
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
            Some(&mut top_level_embed_inputs),
        );
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
                            unit_types: st.registry.unit_types.clone(),
                            unit_facts: st.registry.unit_facts.clone(),
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
        // derive bodies can call helper functions and access TypeInfo. Re-entry (D-CTCODEGEN1=A):
        // emitted fragments go through the full lexer→parser pipeline and are appended as items.
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
                    for (derive_name, derive_span) in &s.derives {
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

                        match crate::Comptime::evaluate_derive_body(
                            body,
                            type_param,
                            type_info,
                            &actual_funcs,
                            &bundle.project_root,
                        ) {
                                Ok(fragments) => {
                                    for fragment in fragments {
                                        let what = format!(
                                            "`derive T.{}` generated invalid Jet while expanding `#{}` on `{}`",
                                            derive_name, derive_name, s.name
                                        );
                                        if let Some(mut parsed) =
                                            super::super::Registration::parse_generated_fragment(
                                                &fragment,
                                                what,
                                                "fix the `derive` body so every emitted fragment is valid Jet source".to_string(),
                                                *derive_span,
                                                &mut diags,
                                            )
                                        {
                                            new_items.extend(parsed.drain(..));
                                        }
                                    }
                                }
                                // E2710: derive body failed at comptime. Wrap with context
                                // pointing at the #TraitName trigger on the struct.
                            Err(inner) => {
                                let layout_refusal =
                                    inner.code == "E0956" && inner.what.contains("D-LAYOUT-FACTS1=B");
                                let why = if layout_refusal {
                                    format!("{}; {}", inner.what, inner.why)
                                } else {
                                    inner.what.clone()
                                };
                                let fix = if layout_refusal {
                                    inner.fix.clone()
                                } else {
                                    "fix the `derive` body so it generates valid Jet at compile time"
                                        .to_string()
                                };
                                super::super::push_causal_report(&mut diags, Diagnostic::error(
                                    "E2710",
                                    format!(
                                        "`derive T.{}` body failed while expanding `#{}` on `{}`",
                                        derive_name, derive_name, s.name
                                    ),
                                    why,
                                    fix,
                                    Some(*derive_span),
                                ), inner);
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

        // D-META-USER1=A: a declared marker body uses the same checked
        // comptime fragment path as a derive body. The target TypeInfo is
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
                match crate::Comptime::evaluate_derive_body(
                    body,
                    "target",
                    type_info,
                    &actual_funcs,
                    &bundle.project_root,
                ) {
                    Ok(fragments) => {
                        for fragment in fragments {
                            let what = format!(
                                "rule `{}` generated invalid Jet while expanding `#{}` on `{}`",
                                declaration.name, declaration.name, target.name
                            );
                            let Some(mut parsed) = super::super::Registration::parse_generated_fragment(
                                &fragment,
                                what,
                                "fix the rule body so every emitted fragment is valid Jet source".to_string(),
                                marker.span,
                                &mut diags,
                            ) else {
                                continue;
                            };
                            for item in parsed.drain(..) {
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
                                            "generated rule `{}` at span {}..{}\nexisting member at span {}..{}",
                                            declaration.name,
                                            marker.span.start,
                                            marker.span.end,
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
                    }
                    Err(inner) => {
                        super::super::push_causal_report(
                            &mut diags,
                            Diagnostic::error(
                                "E2710",
                                format!(
                                    "rule `{}` body failed while expanding `#{}` on `{}`",
                                    declaration.name, declaration.name, target.name
                                ),
                                inner.what.clone(),
                                "fix the rule body so it succeeds at compile time".to_string(),
                                Some(marker.span),
                            ),
                            inner,
                        );
                    }
                }
            }

            // Function-site rules use the same body evaluator. Their target
            // projection is a FunctionInfo, and a body may reject the
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
                match crate::Comptime::evaluate_derive_body(
                    body,
                    "target",
                    target_info,
                    &actual_funcs,
                    &bundle.project_root,
                ) {
                    Ok(fragments) => {
                        for fragment in fragments {
                            let what = format!(
                                "rule `{}` generated invalid Jet while expanding `#{}` on `{}`",
                                declaration.name, declaration.name, target.name
                            );
                            if let Some(mut parsed) =
                                super::super::Registration::parse_generated_fragment(
                                    &fragment,
                                    what,
                                    "fix the rule body so every emitted fragment is valid Jet source".to_string(),
                                    marker.span,
                                    &mut diags,
                                )
                            {
                                new_items.extend(parsed.drain(..));
                            }
                        }
                    }
                    Err(inner) => {
                        super::super::push_causal_report(
                            &mut diags,
                            Diagnostic::error(
                                "E2710",
                                format!(
                                    "rule `{}` body failed while expanding `#{}` on `{}`",
                                    declaration.name, declaration.name, target.name
                                ),
                                inner.what.clone(),
                                "fix the rule body so it succeeds at compile time".to_string(),
                                Some(marker.span),
                            ),
                            inner,
                        );
                    }
                }
            }

            // D-META-DSL1=A: a `.Block` rule receives the text inside Jet's
            // closed braces as its `target` value. The library body can check
            // that text and emit ordinary Jet items through the same re-entry
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
            for (block_name, block_text, block_span) in declared_text_blocks {
                let Some(declaration) = marker_vocabulary.declaration(&block_name) else {
                    continue;
                };
                let Some(body) = declaration.body.as_ref() else {
                    continue;
                };
                match crate::Comptime::evaluate_derive_body(
                    body,
                    "target",
                    crate::AST::CtValue::Str(block_text),
                    &actual_funcs,
                    &bundle.project_root,
                ) {
                    Ok(fragments) => {
                        for fragment in fragments {
                            let what = format!(
                                "rule `{}` generated invalid Jet while expanding `#{}`",
                                declaration.name, block_name
                            );
                            if let Some(mut parsed) =
                                super::super::Registration::parse_generated_fragment(
                                    &fragment,
                                    what,
                                    "fix the rule body so every emitted fragment is valid Jet source".to_string(),
                                    block_span,
                                    &mut diags,
                                )
                            {
                                new_items.extend(parsed.drain(..));
                            }
                        }
                    }
                    Err(inner) => {
                        super::super::push_causal_report(
                            &mut diags,
                            Diagnostic::error(
                                "E2710",
                                format!(
                                    "rule `{}` body failed while checking `#{}`",
                                    declaration.name, block_name
                                ),
                                inner.what.clone(),
                                "fix the rule body so it succeeds at compile time".to_string(),
                                Some(block_span),
                            ),
                            inner,
                        );
                    }
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
        // D-SHARED-CYCLE1=C: strong Shared cycles are beginner-rejected (E0221);
        // expert cycles use Shared.Weak and are admitted.
        check_strong_shared_cycles(&st.registry, &mut diags);
    }
    // D-BOUND-UNDO1=A: CFFI may re-home a foreign declaration into a synthetic
    // module, while its Jet undo function remains in the source module. Validate
    // against the complete post-registration function view so that re-homing and
    // declaration order cannot change the rollback contract.
    let all_funcs: HashMap<String, FuncSig> = states
        .iter()
        .flat_map(|state| state.funcs.iter().map(|(name, sig)| (name.clone(), sig.clone())))
        .collect();
    for module in bundle.modules.iter() {
        validate_foreign_undo_contracts(&module.items, &all_funcs, &mut diags);
    }
    let bundle_auto_derives = TraitRegistry::bundle_auto_derives(bundle, &name_ledger);
    for (state, auto_derives) in states.iter_mut().zip(&bundle_auto_derives) {
        state.trait_reg.merge_auto_derives(auto_derives);
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
                            format!("write `use core.{ring}` instead"),
                            Some(imp.span),
                        ));
                        continue;
                    }
                }
            }
            if let Some(module) = imp.core_module_path() {
                if !crate::Syntax::is_known_core_module(&module) {
                    diags.push(Diagnostic::error(
                        "E1001",
                        format!("there is no core module `{}`", module),
                        "`core` is compiler-known in M10, and only the frozen core modules exist"
                            .to_string(),
                        format!("import one of: {}", crate::Syntax::core_modules_list()),
                        Some(imp.span),
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
                            diags.push(Diagnostic::error(
                                "E1001",
                                format!("there is no core module `{}`", full),
                                "`core` is compiler-known in M10, and only the frozen core modules exist".to_string(),
                                format!("import one of: {}", crate::Syntax::core_modules_list()),
                                Some(*module_alias_span),
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

    // D-NAME-WALK1=A: resolve use/pub use declared inside inline-module
    // bodies. These bindings inherit the enclosing file's module aliases, but
    // are keyed by inline module so they cannot leak into sibling or top-level
    // bodies. Generic module instances are ordinary CodeModules by this pass.
    for idx in 0..bundle.modules.len() {
        let inline_imports: Vec<(String, Vec<crate::AST::ImportDecl>)> = bundle.modules[idx]
            .items
            .iter()
            .filter_map(|item| match item {
                Item::CodeModule(cm) if cm.body.is_some() && !cm.imports.is_empty() => {
                    Some((cm.name.clone(), cm.imports.clone()))
                }
                _ => None,
            })
            .collect();
        for (inline_name, imports) in inline_imports {
            let mut inline_names = HashSet::new();
            for imp in imports {
                // Qualified Core imports use the enclosing file's Core
                // namespace, but their binding remains local to this inline
                // module body.
                if let Some(module) = imp.core_module_path() {
                    if !crate::Syntax::is_known_core_module(&module) {
                        diags.push(Diagnostic::error(
                            "E1001",
                            format!("there is no core module `{module}`"),
                            "`core` is compiler-known, and only the frozen core modules exist"
                                .to_string(),
                            format!("import one of: {}", crate::Syntax::core_modules_list()),
                            Some(imp.span),
                        ));
                        continue;
                    }
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
                    let local = imp
                        .walk_bindings()
                        .into_iter()
                        .next()
                        .map(|binding| binding.local)
                        .unwrap_or_else(|| imp.import_alias());
                    if !inline_names.insert(local.clone()) {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{local}` is used twice"),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                        continue;
                    }
                    states[idx]
                        .inline_core_imports
                        .insert((inline_name.clone(), local), module);
                    continue;
                }
                let foreign = foreign_imports_after_validation(&imp);
                if !foreign.is_empty() {
                    let mut group_failed = false;
                    let mut resolved = Vec::new();
                    let mut group_names = Vec::new();
                    for (namespace, alias) in foreign {
                        if !inline_names.insert(alias.clone()) {
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
                        group_names.push(alias.clone());
                        let target = if namespace.language == crate::AST::ForeignLanguage::C {
                            bundle
                                .cffi
                                .target_for_scope(idx, Some(&inline_name), &alias)
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
                            states[idx]
                                .inline_foreign_imports
                                .insert((inline_name.clone(), alias.clone()), target);
                            if imp.is_pub {
                                states[idx]
                                    .inline_reexport_foreign
                                    .insert((inline_name.clone(), alias), target);
                            }
                        }
                    } else {
                        for alias in group_names {
                            inline_names.remove(&alias);
                        }
                    }
                    continue;
                }
                let ImportKind::Unqualified {
                    module_alias,
                    module_alias_span,
                    ..
                } = &imp.kind
                else {
                    // Inline bodies inherit file/module aliases; a second
                    // module-loading declaration has no loader target.
                    continue;
                };
                let module_alias = module_alias.as_str();
                let module_alias_span = *module_alias_span;
                let bindings = imp.walk_bindings();
                let group_diagnostics = diags.len();
                let mut group_names = Vec::new();
                let mut inserted_inline = Vec::new();
                let mut inserted_file = Vec::new();
                let mut inserted_core = Vec::new();
                let mut inserted_core_items = Vec::new();
                let mut inserted_reexport_inline = Vec::new();
                let mut inserted_reexport_file = Vec::new();
                let mut inserted_reexport_core = Vec::new();
                for binding in bindings {
                    let orig = binding
                        .original
                        .expect("member walker returned a binding without a member");
                    let local = binding.local;
                    if !inline_names.insert(local.clone()) {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{local}` is used twice"),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                        continue;
                    }
                    group_names.push(local.clone());
                    enum Target {
                        Inline { alias: String, mangled: String },
                        File { name: String, module_idx: usize },
                        Core { module: String, item: Option<String> },
                    }
                    let resolved = {
                        let st = &states[idx];
                        if let Some(canonical) = st.code_modules.get(module_alias) {
                            let mangled =
                                jet_foundation::Names::member_name(canonical, &orig);
                            if !st.funcs.contains_key(&mangled) {
                                diags.push(Diagnostic::error(
                                    "E0611",
                                    format!("{orig} is not defined in module {module_alias}"),
                                    "check the module body for the item you are importing".to_string(),
                                    "make sure the name is spelled correctly".to_string(),
                                    Some(module_alias_span),
                                ));
                                None
                            } else {
                                if !name_ledger.exported(idx, &mangled) {
                                    diags.push(Diagnostic::error(
                                        "E0609",
                                        format!("{orig} is private in module {module_alias}"),
                                        "only public items can be brought into scope with use".to_string(),
                                        format!("add pub before fn {orig} in module {module_alias}"),
                                        Some(module_alias_span),
                                    ));
                                    None
                                } else {
                                    Some(Target::Inline {
                                        alias: module_alias.to_string(),
                                        mangled,
                                    })
                                }
                            }
                        } else if let Some(core_prefix) =
                            crate::AST::core_list_prefix(&module_alias)
                        {
                            let full = format!("{core_prefix}.{orig}");
                            let target = match crate::AST::core_list_path(&module_alias, &orig) {
                                Some(crate::AST::CoreListPath::Module(module)) => {
                                    Some((module, None))
                                }
                                Some(crate::AST::CoreListPath::Item { module, item })
                                    if crate::Sema::CheckerCoreLib::core_module_items(&module)
                                        .iter()
                                        .any(|known| known == &item) =>
                                {
                                    Some((module, Some(item)))
                                }
                                _ => None,
                            };
                            match target {
                                Some((module, item)) => Some(Target::Core { module, item }),
                                None => {
                                    if crate::Syntax::is_known_core_module(&core_prefix) {
                                        diags.push(crate::Sema::CheckerCoreLib::unknown_core_item(
                                            &core_prefix,
                                            &orig,
                                            module_alias_span,
                                        ));
                                    } else {
                                        diags.push(Diagnostic::error(
                                            "E1001",
                                            format!("there is no core module {full}"),
                                            "core is compiler-known, and only the frozen core modules exist".to_string(),
                                            format!("import one of: {}", crate::Syntax::core_modules_list()),
                                            Some(module_alias_span),
                                        ));
                                    }
                                    None
                                }
                            }
                        } else if let Some(&target_idx) = st.imports.get(module_alias) {
                            let target = &states[target_idx];
                            let visible = name_ledger.visible(idx, target_idx, &orig);
                            if !target.funcs.contains_key(orig) {
                                diags.push(Diagnostic::error(
                                    "E0611",
                                    format!("{orig} is not defined in module {module_alias}"),
                                    "check the module for the item you are importing".to_string(),
                                    "make sure the name is spelled correctly".to_string(),
                                    Some(module_alias_span),
                                ));
                                None
                            } else if !visible {
                                diags.push(Diagnostic::error(
                                    "E0609",
                                    format!("{orig} is private in module {module_alias}"),
                                    "only public items can be brought into scope with use".to_string(),
                                    format!("add pub before fn {orig} in the imported file"),
                                    Some(module_alias_span),
                                ));
                                None
                            } else {
                                Some(Target::File {
                                    name: orig.to_string(),
                                    module_idx: target_idx,
                                })
                            }
                        } else {
                            diags.push(Diagnostic::error(
                                "E0610",
                                format!("no module named {module_alias} in scope"),
                                "the alias must refer to a module in the enclosing file".to_string(),
                                format!("import a module as {module_alias} before this use"),
                                Some(module_alias_span),
                            ));
                            None
                        }
                    };
                    let Some(target) = resolved else { continue };
                    let st = &mut states[idx];
                    match target {
                        Target::Inline { alias, mangled } => {
                            st.inline_unqualified
                                .insert((inline_name.clone(), local.clone()), mangled.clone());
                            inserted_inline.push((inline_name.clone(), local.clone()));
                            if imp.is_pub {
                                st.inline_reexport_inline.insert(
                                    (inline_name.clone(), local.clone()),
                                    (alias, mangled),
                                );
                                inserted_reexport_inline.push((inline_name.clone(), local));
                            }
                        }
                        Target::File { name, module_idx } => {
                            st.inline_unqualified_file.insert(
                                (inline_name.clone(), local.clone()),
                                (name.clone(), module_idx),
                            );
                            inserted_file.push((inline_name.clone(), local.clone()));
                            if imp.is_pub {
                                st.inline_reexport_file
                                    .insert((inline_name.clone(), local.clone()), (name, module_idx));
                                inserted_reexport_file.push((inline_name.clone(), local));
                            }
                        }
                        Target::Core { module, item } => {
                            let key = (inline_name.clone(), local.clone());
                            st.inline_core_imports
                                .insert(key.clone(), module.clone());
                            inserted_core.push(key.clone());
                            if let Some(item) = item {
                                st.inline_core_items
                                    .insert(key.clone(), item.clone());
                                inserted_core_items.push(key.clone());
                                if imp.is_pub {
                                    st.inline_reexport_core
                                        .insert(key.clone(), (module, item));
                                    inserted_reexport_core.push(key);
                                }
                            }
                        }
                    }
                }
                if diags.len() != group_diagnostics {
                    let st = &mut states[idx];
                    for key in inserted_inline {
                        st.inline_unqualified.remove(&key);
                    }
                    for key in inserted_file {
                        st.inline_unqualified_file.remove(&key);
                    }
                    for key in inserted_core {
                        st.inline_core_imports.remove(&key);
                    }
                    for key in inserted_core_items {
                        st.inline_core_items.remove(&key);
                    }
                    for key in inserted_reexport_inline {
                        st.inline_reexport_inline.remove(&key);
                    }
                    for key in inserted_reexport_file {
                        st.inline_reexport_file.remove(&key);
                    }
                    for key in inserted_reexport_core {
                        st.inline_reexport_core.remove(&key);
                    }
                    for name in group_names {
                        inline_names.remove(&name);
                    }
                }
            }
        }
    }

    populate_name_ledger(bundle, &states, &mut name_ledger);

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
                Item::Bench(b) => {
                    walk_stmts_for_const_refs(&b.body, &const_names, &mut address_taken)
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
                | Item::StateDecl(_) // D-STATE-DECL: erases
                | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
                | Item::UserDerive(_) // D-METADERIVE1=A: already expanded above
                | Item::GenericModule(_) // D-GENMOD2=A: template — erases
                | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
            }
        }
        for item in &mut module.items {
            if let Item::Const(c) = item {
                let force_static = c.attrs.contains(&ConstAttr::ForceStatic);
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
            // S12/D-CLIFLAG1: `run` is the only program entry name. It is
            // zero-arg, or one typed CLI-spec parameter (`#[CLI]` struct / enum).
            if run_fn.params.len() == 1 {
                let param = &run_fn.params[0];
                let cli_module = jet_foundation::CLISchema::entry_type_module(bundle)
                    .unwrap_or(bundle.entry);
                match cli_entry_param_shape(
                    &bundle.modules[cli_module].items,
                    &param.ty,
                    &states[cli_module].trait_reg,
                ) {
                    CLIEntryShape::Struct | CLIEntryShape::Enum => {}
                    CLIEntryShape::EnumBadVariants(bad) => diags.extend(bad),
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
                    "add at least one top-level block: #{} \"describes what this checks\" {{ ... }}",
                    Syntax::KW_TEST
                ),
                format!(
                    "use `{}` and `{}` inside the block to check results",
                    Syntax::BUILTIN_REQUIRE,
                    Syntax::BUILTIN_REQUIRE_EQ
                ),
                None,
            ));
        }
        // `jet bench` checks the AST for `#Bench` blocks before entering Bench
        // mode and falls back to whole-program timing otherwise, so an empty
        // bench set is never an error here.
        CompileMode::Bench
        | CompileMode::Test
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
        super::super::Taint::collect_return_tag_facts(
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
    super::super::Effects::check_autodiff_purity(
        &validation_summaries,
        &public_solved,
        &mut diags,
    );
    // D-CRYPTO-DIAG1: candidate facts survive only when their entire function
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
        super::super::Effects::check_inferred_purity(
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

    // D-WASM1=A (c123 M1): JS/WASM partition inference and boundary checks.
    // D-MEM-FACTS1: module `#Policy(no_alloc)` declarations are checked only
    // after the same qualified, dependency-complete graph is projected.
    // #657 feeds the other scope levels and the two remaining fact values into
    // this declaration surface; reachability itself stays single-mechanism.
    let (memory_summaries, memory_declarations) =
        super::super::MemoryFacts::bundle_memory_inputs(bundle, &public_summaries);
    let memory_projections = memory_declarations
        .iter()
        .flat_map(|declaration| {
            declaration.roots.iter().map(|root| {
                (
                    (root.clone(), declaration.fact),
                    super::super::MemoryFacts::project_memory_fact(
                        declaration.fact,
                        root,
                        &memory_summaries,
                    ),
                )
            })
        })
        .collect::<HashMap<_, _>>();
    diags.extend(super::super::MemoryFacts::check_memory_facts(
        &memory_declarations,
        &memory_summaries,
    ));
    diags.extend(check_web_partition(
        bundle,
        &public_summaries,
        &public_solved,
    ));

    // D-WEBAPP1=D / D-WEBAUTHOR1=D (Tower #438): one sema-known application graph.
    let (web_app_graph, web_app_diags) = super::super::WebApp::extract_web_app_graph(bundle);
    diags.extend(web_app_diags);

    // D-OSTARGET1=A (ratified 2026-07-01, c134): native OS platform gating —
    // mixed-axis conflicts and unmatched cross-gate calls.
    diags.extend(check_os_target(bundle));

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
    // D-CLIFLAG1: a `#[CLI]`-derived struct's generated `__jet_cli_spec_*`/
    // `__jet_cli_decode_*` functions (and the synthesized `fn main` for a
    // typed `fn run`) call straight into `core.args`'s `JetArgsSpec`/
    // `JetParsedArgs` prelude — but they're pure codegen text, not a Jet
    // method call `collect_used_core` can see by walking function bodies.
    // Force the same `CORELIB_PRELUDE` inclusion a hand-written
    // `use core.args` would trigger (any key works; the caller only checks
    // "is this set empty").
    if bundle.modules.iter().any(|m| {
        m.items
            .iter()
            .any(|i| matches!(i, Item::Struct(s) if s.derives.iter().any(|(t, _)| t == "CLI")))
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
            || m.source.contains("Shared.new(")
            || m.source.contains("Cell<")
            || m.source.contains("Cell.new(")
            || m.source.contains("Id<")
    }) {
        used_core.insert("core.mem::pool_shared".to_string());
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
    diags.extend(super::super::MemoryFacts::annotate_scoped_gc_promotions(bundle));
    apply_helper_layer_inference(bundle, &states, &usage_spans, &mut diags);
    bundle.name_ledger = name_ledger.clone();
    (
        diags,
        super::super::Effects::SemIndexEffectFacts {
            summaries: public_summaries,
            solved: public_solved,
            reachability: public_reachability,
            memory_declarations,
            memory_projections,
            name_ledger: name_ledger.clone(),
            web_app: web_app_graph,
            fact_registry,
        },
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
