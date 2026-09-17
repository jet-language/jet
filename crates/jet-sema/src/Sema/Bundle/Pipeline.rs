use super::*;
use jet_foundation::CanonicalPass;
use super::super::CheckerValidate::process_validate_blocks;

mod Completion;
mod InlineImports;
use Completion::complete_bundle_check;
use InlineImports::resolve_inline_module_imports;

/// D-STRUCT-ONCE1=A: expand root declaration loops through the same typed
/// comptime body used by derives and marker declarations. The expansion is
/// deliberately before registration, so every generated impl, conversion,
/// and test is an ordinary item for the rest of sema.
fn expand_item_template_loops(
    items: &mut Vec<Item>,
    base_dir: &std::path::Path,
    diags: &mut Vec<Diagnostic>,
) {
    if !items
        .iter()
        .any(|item| matches!(item, Item::TemplateLoop(_)))
    {
        return;
    }
    let (funcs_owned, _externs, globals) =
        super::super::Registration::comptime_context_from_items(items);
    let funcs = funcs_owned
        .iter()
        .map(|(name, function)| (name.clone(), function))
        .collect::<HashMap<_, _>>();
    let source_items = std::mem::take(items);
    let mut expanded_items = Vec::with_capacity(source_items.len());
    for item in source_items {
        let Item::TemplateLoop(loop_item) = item else {
            expanded_items.push(item);
            continue;
        };
        let template = crate::AST::DeriveBodyItem::Loop {
            var: loop_item.var,
            var_span: loop_item.var_span,
            source: loop_item.source,
            body: loop_item.body,
            span: loop_item.span,
        };
        match crate::Comptime::expand_template_body(
            std::slice::from_ref(&template),
            &globals,
            &funcs,
            base_dir,
        ) {
            Ok(generated) => expanded_items.extend(generated),
            Err(diagnostic) => diags.push(diagnostic),
        }
    }
    *items = expanded_items;
}

/// E0301: an `impl` names a type this program cannot extend. The header echoes
/// what the user wrote (`impl Money.Mul`, not `impl Money`); the Why and Fix
/// separate a built-in target, which never takes user methods (D-OPDEF1; an
/// operator hook on a built-in type belongs to the package that declares the
/// other operand type, D-OPMIX1), from a name that is simply not declared.
fn e0301_impl_target(i: &crate::AST::ImplDef) -> Diagnostic {
    let type_name = i.type_name.as_str();
    let header = match &i.trait_name {
        Some(trait_name) => format!("{type_name}.{trait_name}"),
        None => type_name.to_string(),
    };
    let (leaf_ns, leaf) = type_name
        .rsplit_once('.')
        .map_or((None, type_name), |(ns, leaf)| (Some(ns), leaf));
    let built_in = leaf_ns.is_none()
        && (crate::Sema::Diagnostics::builtin_type_from_ident(leaf).is_some()
            || crate::Sema::CheckerCoreLib::core_type_known(leaf)
            || crate::Sema::CheckerCoreLib::is_math_type(leaf)
            || crate::Collections::is_reserved_type(leaf));
    let (why, fix) = if built_in {
        (
            format!(
                "`{type_name}` is built in, and an `impl` only extends a struct, enum, or distinct type declared by your program"
            ),
            match i.trait_name.as_deref() {
                Some(
                    trait_name @ (crate::Syntax::TRAIT_ADD
                    | crate::Syntax::TRAIT_SUB
                    | crate::Syntax::TRAIT_MUL
                    | crate::Syntax::TRAIT_DIV
                    | crate::Syntax::TRAIT_EQUATABLE
                    | crate::Syntax::TRAIT_COMPARABLE),
                ) => format!(
                    "call a named method, or implement `{trait_name}` on a struct or distinct type of your own (an operator hook on `{type_name}` belongs to the package that declares the other operand type, D-OPMIX1)"
                ),
                Some(trait_name) => format!(
                    "implement `{trait_name}` on a struct or distinct type of your own, or wrap `{type_name}` in one"
                ),
                None => format!(
                    "call a named method, or wrap `{type_name}` in a struct or distinct type of your own and implement the methods there"
                ),
            },
        )
    } else {
        (
            format!(
                "no struct, enum, or distinct type named `{type_name}` is declared here, and an `impl` only extends a declared type"
            ),
            match leaf_ns {
                Some(ns) => format!(
                    "export `{leaf}` from `{ns}` as a struct, enum, or distinct type, or check the spelling"
                ),
                None => format!(
                    "declare `struct {leaf}`, `enum {leaf}`, or `{leaf} :: distinct …` first, or check the spelling"
                ),
            },
        )
    };
    Diagnostic::error(
        "E0301",
        format!("`impl {header}` needs a user-defined type"),
        why,
        fix,
        Some(i.type_span),
    )
}

/// Register one test after a template expansion has materialized its static
/// name. Root loops reach the ordinary item pass directly; marker and derive
/// bodies use this helper when their generated items are appended later.
fn register_test_item(
    test: &crate::AST::TestDef,
    state: &mut crate::Sema::ModuleState,
    diags: &mut Vec<Diagnostic>,
) {
    let Some(name) = &test.name else {
        return;
    };
    if name_defined(name, &state.funcs, &state.registry, &state.consts)
        || state.tests.contains_key(name)
    {
        diags.push(defined_twice(
            name,
            "every test needs a unique name so failures are easy to find",
            test.name_span,
        ));
    } else {
        state.tests.insert(name.clone(), test.name_span);
    }
}
/// Register one source fact declaration in the bundle-local fact ledger.
///
/// Every declaration maps to one effective fact identity (`@name` when it is
/// a plain string, otherwise its source name); duplicates remain diagnostics.
fn fact_identity(declaration: &crate::AST::FactDecl) -> String {
    declaration
        .params
        .iter()
        .find(|parameter| parameter.name == "@name")
        .and_then(|parameter| parameter.value.as_deref())
        .and_then(|value| match value {
            crate::AST::Expr::Str(parts, _) => {
                let mut identity = String::new();
                for part in parts {
                    match part {
                        crate::AST::StrPart::Lit(value) => identity.push_str(value),
                        crate::AST::StrPart::Interp(..) => return None,
                    }
                }
                Some(identity)
            }
            _ => None,
        })
        .unwrap_or_else(|| declaration.name.clone())
}

fn register_fact_declaration(
    declaration: &crate::AST::FactDecl,
    module_idx: usize,
    declarations: &mut HashMap<String, (usize, Span, String)>,
    diags: &mut Vec<Diagnostic>,
) {
    let identity = fact_identity(declaration);
    if let Some((first_module, first_span, first_source)) = declarations.get(&identity) {
        diags.push(
            Diagnostic::error(
                "E0105",
                format!(
                    "fact `{identity}` is declared twice (spans {}..{} and {}..{})",
                    first_span.start,
                    first_span.end,
                    declaration.name_span.start,
                    declaration.name_span.end,
                ),
                "one fact name must resolve to one declaration in the loaded bundle"
                    .to_string(),
                "rename or remove one of the fact declarations".to_string(),
                Some(declaration.name_span),
            )
            .with_detail(format!(
                "first declaration `{first_source}`: module {first_module}, span {}..{}\nsecond declaration `{}`: module {module_idx}, span {}..{}",
                first_span.start,
                first_span.end,
                declaration.name,
                declaration.name_span.start,
                declaration.name_span.end,
            )),
        );
        return;
    }
    declarations.insert(
        identity,
        (module_idx, declaration.name_span, declaration.name.clone()),
    );
}

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
            derive_name, existing_site, method.name, type_name
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
    let mut seen_aliases: HashMap<(usize, Option<String>), HashSet<String>> = HashMap::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for (scope, import) in crate::AST::walk_imports(module) {
            if !seen.insert((module_idx, import.span)) {
                continue;
            }
            let foreign = match import.foreign_imports() {
                Ok(foreign) => foreign,
                Err(error) => {
                    diagnostics.push(error.diagnostic());
                    continue;
                }
            };
            if foreign.is_empty() {
                continue;
            }
            let aliases = seen_aliases
                .entry((module_idx, scope.map(str::to_owned)))
                .or_default();
            for (_, alias) in foreign {
                if aliases.insert(alias.clone()) {
                    continue;
                }
                diagnostics.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{alias}` is used twice"),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", Syntax::KW_AS),
                    Some(import.alias_span),
                ));
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
    no_os: bool,
    gates: crate::Policy::GateSet,
    explicit_output: Option<&str>,
    incremental: Option<&mut IncrementalSemaCache>,
) -> (Vec<Diagnostic>, super::super::Effects::SemIndexEffectFacts) {
    check_bundle_opts_for_output_with_context(
        bundle,
        mode,
        no_os,
        gates,
        explicit_output,
        incremental,
        false,
    )
}

/// Checking is an unbounded-depth recursive descent over user syntax, so the
/// frame requirement is per source-nesting level, not per program size. This is
/// the narrowest point every public `check_bundle*` shares, so the sized
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
/// reaches them through `MirBridge`), so they are carried across explicitly —
/// the same carry `jet_driver::run_compiler_work` performs. `MirBridge`'s own
/// hooks are a process-global `OnceLock`, not thread-local, so they need no
/// carry.
pub(super) fn check_bundle_opts_for_output_with_context(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    no_os: bool,
    gates: crate::Policy::GateSet,
    explicit_output: Option<&str>,
    incremental: Option<&mut IncrementalSemaCache>,
    allow_compiler_api: bool,
) -> (Vec<Diagnostic>, super::super::Effects::SemIndexEffectFacts) {
    if jet_foundation::CompilerStack::on_compiler_worker() {
        return check_bundle_opts_for_output_on_stack(
            bundle,
            mode,
            no_os,
            gates,
            explicit_output,
            incremental,
            allow_compiler_api,
        );
    }
    let (ambient_core_call, ambient_handle, ambient_extern_call) = crate::Comptime::ambient_hooks();
    jet_foundation::CompilerStack::run_on_compiler_stack(move || {
        crate::Comptime::with_ambient(
            ambient_core_call,
            ambient_handle,
            ambient_extern_call,
            || {
                check_bundle_opts_for_output_on_stack(
                    bundle,
                    mode,
                    no_os,
                    gates,
                    explicit_output,
                    incremental,
                    allow_compiler_api,
                )
            },
        )
    })
}

fn check_bundle_opts_for_output_on_stack(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    no_os: bool,
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
            no_os,
            gates,
            explicit_output,
            incremental,
            allow_compiler_api,
        )
    })
}

mod CheckInner;
use CheckInner::check_bundle_opts_for_output_inner;
/// D-STRUCT-PLANE1=A / I2 / I3: the fact law is a front-end invariant. Keep
/// the registry guard in sema so no downstream engine can become the first
/// place that notices a structure row lost its direction or gate.
fn guard_fact_registry_law() {
    if let Some(violation) = jet_foundation::Registry::law_violations()
        .into_iter()
        .next()
    {
        jet_foundation::ice!(None, "fact registry law violation: {violation}");
    }
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
