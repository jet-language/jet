use super::*;
use crate::Diagnostics::{Diagnostic, TextEdit};
use crate::Syntax;
use crate::Traits::TraitRegistry;
use crate::AST::{
    AccessConvention, CodeModule, ConstAttr, EnumLitArg, Expr, ForKind, Func, GenericModuleDef,
    GenericModuleParam, ImportKind, Item, LValue, LambdaBody, ModuleAliasDef, ModuleArg,
    OrFallback, Param, ParamZone, Pattern, ProgramBundle, RustConstKind, Stmt, StrPart, SwitchArm,
    Type, VariantPayload,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod Comptime;
mod Units;

mod GenericModules;
mod InlineCalls;
mod Liveness;
mod Outputs;
mod Pipeline;
mod Validation;

pub use Comptime::bundle_has_comptime_evaluation;
use Comptime::stmts_have_comptime_evaluation;
use Units::{inject_units_prelude, resolve_unit_dimensions};

pub(crate) use InlineCalls::{mangle_inline_sibling_calls, rewrite_inline_calls_stmts};

pub(crate) use GenericModules::expand_generic_module_aliases;
pub(crate) use GenericModules::hoist_inline_module_member_types;
pub(crate) use GenericModules::module_type_name;
pub use GenericModules::specialize_function_types;
use GenericModules::{clone_enum, clone_struct};
use Outputs::{cli_entry_param_shape, no_run_error, resolve_outputs, CLIEntryShape};
use Pipeline::{
    check_bundle_opts_for_output as pipeline_check_bundle_opts_for_output,
    check_bundle_opts_for_output_with_context as pipeline_check_bundle_opts_for_output_with_context,
};
use Validation::{apply_helper_layer_inference, qualified_effect_facts, taint_check_item};
#[allow(unused_imports)]
pub(crate) use Validation::{
    check_func_body_bundle, check_module_bodies, collect_core_expr, collect_core_lvalue,
    collect_core_stmts, collect_used_core, expand_core_reachable_closure, fn_types_compatible,
    func_sig_to_fn_type, register_func_item,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IncrementalSemaStats {
    pub hits: u64,
    pub recomputes: u64,
    pub live_items: usize,
    pub live_item_bytes: usize,
    pub recomputed_items: Vec<String>,
}

#[derive(Clone)]
pub(super) struct CachedFunctionBody {
    pub input: Vec<u8>,
    pub function: Func,
    pub diagnostics: Vec<Diagnostic>,
    pub summaries: HashMap<String, EffectSummary>,
    pub comptime_inputs: Vec<crate::AST::ComptimeInput>,
    pub address_taken: HashSet<String>,
    pub name_ledger: jet_foundation::Names::NameLedger,
    pub pending_diagnostics: Vec<PendingFunctionDiagnostic>,
    /// D-INTBIG1: replay the typed exact-Int reachability fact on cache hits.
    pub uses_exact_int: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingFunctionDiagnostic {
    pub function_key: String,
    pub function_span: Span,
    pub diagnostic: Diagnostic,
}

fn mark_failed_pending_functions(
    diagnostics: &[Diagnostic],
    pending: &[PendingFunctionDiagnostic],
    failed: &mut HashSet<String>,
) {
    for diagnostic in diagnostics {
        if !matches!(diagnostic.severity, crate::Diagnostics::Severity::Error) {
            continue;
        }
        let Some(span) = diagnostic.span else {
            continue;
        };
        for candidate in pending {
            if span.start >= candidate.function_span.start
                && span.end <= candidate.function_span.end
            {
                failed.insert(candidate.function_key.clone());
            }
        }
    }
}

/// D-CHOOSE-HEADS1=A / S83: keep the authored head order, then lower all
/// heads with one function name into one ordinary enum pattern table. The
/// regular registration, checker, TIR, AOT, JIT, and interpreter paths then
/// see exactly one function and one proof site.
fn desugar_multi_head_functions(bundle: &mut ProgramBundle, diags: &mut Vec<Diagnostic>) {
    let mut variants: HashMap<String, Vec<(String, VariantPayload)>> = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            let Item::Enum(def) = item else { continue };
            for variant in &def.variants {
                variants
                    .entry(variant.name.clone())
                    .or_default()
                    .push((def.name.clone(), variant.payload.clone()));
            }
        }
    }

    enum OrderedItem {
        Item(Item),
        Heads(String),
    }

    for module in &mut bundle.modules {
        let items = std::mem::take(&mut module.items);
        let mut grouped: HashMap<String, Vec<Func>> = HashMap::new();
        let mut ordered = Vec::with_capacity(items.len());
        for item in items {
            match item {
                Item::Func(function) if function.head_pattern.is_some() => {
                    let name = function.name.clone();
                    let heads = grouped.entry(name.clone()).or_default();
                    if heads.is_empty() {
                        ordered.push(OrderedItem::Heads(name));
                    }
                    heads.push(function);
                }
                item => ordered.push(OrderedItem::Item(item)),
            }
        }

        let mut rebuilt = Vec::with_capacity(ordered.len());
        for item in ordered {
            match item {
                OrderedItem::Item(item) => rebuilt.push(item),
                OrderedItem::Heads(name) => {
                    let heads = grouped
                        .remove(&name)
                        .expect("multi-head placeholder has a group");
                    rebuilt.push(Item::Func(fold_multi_head_function(
                        heads, &variants, diags,
                    )));
                }
            }
        }
        module.items = rebuilt;
    }
}

fn fold_multi_head_function(
    heads: Vec<Func>,
    variants: &HashMap<String, Vec<(String, VariantPayload)>>,
    diags: &mut Vec<Diagnostic>,
) -> Func {
    let mut folded = heads
        .first()
        .cloned()
        .expect("multi-head group is non-empty");
    let mut enum_name: Option<String> = None;
    let mut invalid = false;
    let mut arms = Vec::with_capacity(heads.len());

    for head in &heads {
        let Some(pattern) = head.head_pattern.clone() else {
            invalid = true;
            continue;
        };
        let Pattern::Variant { variant, .. } = &pattern else {
            diags.push(Diagnostic::error(
                "E0305",
                "a multi-head declaration needs an enum variant head".to_string(),
                "multi-head coverage is proved by the enum pattern table".to_string(),
                "write a variant head such as `Circle(radius: Float)`".to_string(),
                Some(pattern.span()),
            ));
            invalid = true;
            continue;
        };
        let Some(candidates) = variants.get(variant) else {
            diags.push(Diagnostic::error(
                "E0305",
                format!("multi-head variant `{variant}` is not an enum variant"),
                "a multi-head table can only cover variants declared by an enum".to_string(),
                "check the variant spelling or declare the enum first".to_string(),
                Some(pattern.span()),
            ));
            invalid = true;
            continue;
        };
        let Some((candidate_enum, payload)) = candidates.first() else {
            invalid = true;
            continue;
        };
        if candidates
            .iter()
            .any(|(other_enum, _)| other_enum != candidate_enum)
        {
            diags.push(Diagnostic::error(
                "E0305",
                format!("multi-head variant `{variant}` is ambiguous"),
                "a bare head must identify one enum so one table subject can be typed".to_string(),
                "use one enum's variants for this function name".to_string(),
                Some(pattern.span()),
            ));
            invalid = true;
        }
        if let Some(previous) = &enum_name {
            if previous != candidate_enum {
                diags.push(Diagnostic::error(
                    "E0305",
                    format!(
                        "multi-head variants `{}` and `{variant}` belong to different enums",
                        previous
                    ),
                    "one function has one argument type and one coverage table".to_string(),
                    "put heads for one enum under one function name".to_string(),
                    Some(pattern.span()),
                ));
                invalid = true;
            }
        } else {
            enum_name = Some(candidate_enum.clone());
        }

        let expected = match payload {
            VariantPayload::Unit => Vec::new(),
            VariantPayload::Single(ty, _) => vec![ty.clone()],
            VariantPayload::Named(fields) => fields.iter().map(|field| field.ty.clone()).collect(),
        };
        for (param, expected_ty) in head.params.iter().zip(expected.iter()) {
            if param.ty != *expected_ty {
                diags.push(Diagnostic::error(
                    "E0305",
                    format!(
                        "multi-head binding `{}` has type `{}` but variant `{variant}` carries `{}`",
                        param.name,
                        param.ty.show(),
                        expected_ty.show()
                    ),
                    "head bindings must use the variant payload types".to_string(),
                    format!("write `{}` for this binding", expected_ty.show()),
                    Some(param.ty_span),
                ));
                invalid = true;
            }
        }

        let pattern_span = pattern.span();
        arms.push(SwitchArm {
            cond: Expr::PatternTest {
                subject: Box::new(Expr::Ident(
                    Syntax::INTERNAL_MULTI_HEAD_SUBJECT.to_string(),
                    folded.name_span,
                )),
                pattern,
                span: pattern_span,
            },
            body: head.body.clone(),
            span: pattern_span,
        });
    }

    if invalid || enum_name.is_none() {
        folded.head_pattern = None;
        return folded;
    }
    let enum_name = enum_name.expect("validated multi-head enum");
    let full_span = Span::new(
        folded.span.start,
        heads.last().map_or(folded.span.end, |head| head.span.end),
    );
    let subject = Expr::Ident(
        Syntax::INTERNAL_MULTI_HEAD_SUBJECT.to_string(),
        folded.name_span,
    );
    folded.params = vec![Param {
        convention: AccessConvention::Read,
        root: false,
        name: Syntax::INTERNAL_MULTI_HEAD_SUBJECT.to_string(),
        name_span: folded.name_span,
        public_label: None,
        zone: ParamZone::Either,
        ty: Type::Named(enum_name),
        ty_span: folded.name_span,
        default: None,
        variadic: false,
        variadic_bound_list: None,
        declared_view_from_names: None,
    }];
    folded.body = vec![Stmt::Switch {
        subject,
        arms,
        else_body: None,
        // Point coverage failures at the authored function name, not at a
        // compiler-created brace or switch span.
        span: folded.name_span,
    }];
    folded.span = full_span;
    folded.return_view_provenance = None;
    folded.declared_return_view_provenance = None;
    folded.head_pattern = None;
    folded
}

fn dedupe_unknown_names(diagnostics: &mut Vec<Diagnostic>) {
    let mut seen_unknown = HashSet::new();
    let mut seen_exact = HashSet::new();
    diagnostics.retain(|diagnostic| {
        if diagnostic.code == "E0107" {
            return diagnostic
                .span
                .is_none_or(|span| seen_unknown.insert((span.start, span.end)));
        }
        // E0119/E0354 can be raised once while materializing a marker and
        // again while checking its ordinary use-site. Keep distinct messages
        // at one span, but remove only byte-identical repeats.
        if matches!(diagnostic.code.as_str(), "E0119" | "E0354") {
            let Some(span) = diagnostic.span else {
                return true;
            };
            return seen_exact.insert((
                diagnostic.code.clone(),
                diagnostic.what.clone(),
                diagnostic.why.clone(),
                diagnostic.fix.clone(),
                span.start,
                span.end,
            ));
        }
        true
    });
}

/// A rejected operation must not also report its conversion failure at the
/// same source expression. Pair each primary with only its known cascade.
fn prune_conversion_cascades(diagnostics: &mut Vec<Diagnostic>) {
    let impure_spans = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "E3401")
        .filter_map(|diagnostic| diagnostic.span)
        .collect::<Vec<_>>();
    let vault_spans = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "E0510")
        .filter_map(|diagnostic| diagnostic.span)
        .collect::<Vec<_>>();
    if !impure_spans.is_empty() || !vault_spans.is_empty() {
        diagnostics.retain(|diagnostic| match diagnostic.code.as_str() {
            // A spanless report has no proven relationship to the primary;
            // retain it rather than treating `None` as a wildcard.
            "E2404" => diagnostic
                .span
                .is_none_or(|span| !impure_spans.contains(&span)),
            "E2402" => diagnostic
                .span
                .is_none_or(|span| !vault_spans.contains(&span)),
            _ => true,
        });
    }
    // One helper failure domain is one root report. Include the checker-known
    // declaration provenance so same-named helpers in separate scopes remain
    // independent roots.
    let mut seen_failure_domains = HashSet::new();
    diagnostics.retain(|diagnostic| {
        let Some(detail) = diagnostic.detail.as_deref() else {
            return true;
        };
        let Some(detail) = detail.strip_prefix("failure-domain:") else {
            return true;
        };
        let Some(domain) = detail.lines().next() else {
            return true;
        };
        let definition = detail
            .lines()
            .find_map(|line| line.strip_prefix("definition: "))
            .unwrap_or_default();
        let key = (domain.to_string(), definition.to_string());
        diagnostic.code != "E2404" || seen_failure_domains.insert(key)
    });
}

/// D-SHAPE-INTERNAL1: one resolved soft-public name use is one lint, even when
/// overlapping sema resolution paths report the same source occurrence.
fn dedupe_soft_public_lints(diagnostics: &mut Vec<Diagnostic>) {
    let mut seen = HashSet::new();
    diagnostics.retain(|diagnostic| {
        if diagnostic.code != "L0601" {
            return true;
        }
        let Some(span) = diagnostic.span else {
            return true;
        };
        seen.insert((
            diagnostic.code.clone(),
            diagnostic.what.clone(),
            span.start,
            span.end,
        ))
    });
}

fn check_fact_tags_and_states(
    bundle: &ProgramBundle,
    states: &[ModuleState],
    returns: &HashMap<String, super::Taint::TagSet>,
    return_types: &super::Taint::ReturnTypes,
    diags: &mut Vec<Diagnostic>,
) -> jet_foundation::Facts::FactRegistry {
    let mut facts = jet_foundation::Facts::FactRegistry::default();
    register_effect_facts(bundle, &mut facts);
    super::Taint::register_builtin_tag_facts(&mut facts);

    let mut scrubbers = HashMap::new();
    let mut field_tags = HashMap::new();
    let mut field_types = HashMap::new();
    let mut known_sources = std::collections::BTreeSet::new();
    for module in &bundle.modules {
        super::Taint::collect_function_paths(&module.alias, &module.items, &mut known_sources);
    }
    for module in &bundle.modules {
        super::Taint::collect_field_facts(&module.items, &mut field_tags, &mut field_types);
        super::Taint::collect_tag_facts(
            &module.items,
            &mut facts,
            &mut scrubbers,
            &known_sources,
            diags,
            true,
        );
    }
    for module in &bundle.modules {
        super::Taint::collect_tag_facts(
            &module.items,
            &mut facts,
            &mut scrubbers,
            &known_sources,
            diags,
            false,
        );
    }
    for (index, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            taint_check_item(
                item,
                &scrubbers,
                &facts,
                returns,
                return_types,
                &field_tags,
                &field_types,
                &states[index].core_imports,
                diags,
            );
        }
    }

    let mut state_table = crate::Sema::StateTable::with_facts(facts);
    for module in &bundle.modules {
        state_table.add_items(&module.items);
    }
    if !state_table.is_empty() {
        // Declaration facts are bundle-wide: an owner and its inherent impls
        // may be loaded from different source modules. Validate the one
        // erased `Type.State` row against one item view so graphs and marker
        // paths do not depend on file placement.
        let all_items: Vec<Item> = bundle
            .modules
            .iter()
            .flat_map(|module| module.items.iter().cloned())
            .collect();
        state_table.validate_declarations(&all_items, diags);
        for module in &bundle.modules {
            crate::Sema::check_items_state(&module.items, &state_table, diags);
        }
    }
    state_table.into_facts()
}

fn register_effect_facts(bundle: &ProgramBundle, facts: &mut jet_foundation::Facts::FactRegistry) {
    use jet_foundation::Facts::FactKind;
    for effect in jet_foundation::Authority::EFFECT_ROOTS.iter() {
        facts.declare(FactKind::Effect, (*effect).to_string(), std::iter::empty());
    }
    for name in crate::Syntax::BUILTIN_EFFECT_LEAVES {
        let root = super::effect_root(name);
        let member = name.strip_prefix(root).unwrap().trim_start_matches('.');
        facts.declare_member(FactKind::Effect, root.to_string(), member.to_string());
    }
    fn collect(items: &[Item], facts: &mut jet_foundation::Facts::FactRegistry) {
        for item in items {
            match item {
                Item::EffectDecl(declaration) => {
                    let root = super::effect_root(&declaration.name);
                    if let Some(member) = declaration
                        .name
                        .strip_prefix(root)
                        .and_then(|suffix| suffix.strip_prefix('.'))
                    {
                        facts.declare_member(
                            jet_foundation::Facts::FactKind::Effect,
                            root.to_string(),
                            member.to_string(),
                        );
                    }
                }
                Item::CodeModule(module) => {
                    if let Some(body) = &module.body {
                        collect(body, facts);
                    }
                }
                Item::GenericModule(module) => collect(&module.body, facts),
                _ => {}
            }
        }
    }
    for module in &bundle.modules {
        collect(&module.items, facts);
    }
}

fn validate_declared_effects(
    bundle: &ProgramBundle,
    facts: &jet_foundation::Facts::FactRegistry,
) -> Vec<Diagnostic> {
    fn check_name(
        name: &str,
        span: Span,
        facts: &jet_foundation::Facts::FactRegistry,
        diags: &mut Vec<Diagnostic>,
    ) {
        if let Err(suggestion) = super::resolve_effect_name(name, facts) {
            if super::parse_effect_name(name).is_some() {
                diags.push(super::undeclared_effect(
                    name,
                    suggestion.as_deref(),
                    Some(span),
                ));
            }
        }
    }
    fn check_type(
        ty: &Type,
        facts: &jet_foundation::Facts::FactRegistry,
        diags: &mut Vec<Diagnostic>,
    ) {
        match ty {
            Type::Fn {
                params,
                ret,
                effect_bound,
                ..
            } => {
                for parameter in params {
                    check_type(parameter, facts, diags);
                }
                if let Some(ret) = ret {
                    check_type(ret, facts, diags);
                }
                if let Some(names) = effect_bound {
                    for (name, span) in names {
                        if super::effect_row_var(name).is_none() {
                            check_name(name, *span, facts, diags);
                        }
                    }
                }
            }
            Type::List(inner)
            | Type::Option(inner)
            | Type::Shared(inner)
            | Type::Tagged { inner, .. }
            | Type::FixedList { elem: inner, .. } => check_type(inner, facts, diags),
            Type::Result { ok, err } => {
                check_type(ok, facts, diags);
                check_type(err, facts, diags);
            }
            Type::Apply { args, .. } | Type::Union(args) => {
                for argument in args {
                    check_type(argument, facts, diags);
                }
            }
            Type::Map { key, value, .. } => {
                check_type(key, facts, diags);
                check_type(value, facts, diags);
            }
            Type::Tuple(fields) => {
                for (_, field) in fields {
                    check_type(field, facts, diags);
                }
            }
            _ => {}
        }
    }
    fn check_stmts(
        body: &[Stmt],
        facts: &jet_foundation::Facts::FactRegistry,
        diags: &mut Vec<Diagnostic>,
    ) {
        for statement in body {
            match statement {
                Stmt::Val(binding) => {
                    if let Some(ty) = &binding.ty {
                        check_type(ty, facts, diags);
                    }
                }
                Stmt::CountedLoop { init, .. } => {
                    if let Some(ty) = &init.ty {
                        check_type(ty, facts, diags);
                    }
                }
                _ => {}
            }
            for nested in super::UnsafeObligations::nested_bodies(statement) {
                check_stmts(nested, facts, diags);
            }
        }
    }
    fn check_func(
        function: &Func,
        facts: &jet_foundation::Facts::FactRegistry,
        diags: &mut Vec<Diagnostic>,
    ) {
        if let Some(names) = &function.declared_effects {
            for (name, span) in names {
                let name = name.strip_prefix('!').unwrap_or(name);
                if super::effect_row_var(name).is_none() {
                    check_name(name, *span, facts, diags);
                }
            }
        }
        for parameter in &function.params {
            check_type(&parameter.ty, facts, diags);
        }
        if let Some(return_type) = &function.return_type {
            check_type(return_type, facts, diags);
        }
        check_stmts(&function.body, facts, diags);
    }
    fn check_items(
        items: &[Item],
        facts: &jet_foundation::Facts::FactRegistry,
        diags: &mut Vec<Diagnostic>,
    ) {
        for item in items {
            match item {
                Item::EffectDecl(declaration) => {
                    if super::parse_effect_name(&declaration.name).is_none() {
                        diags.push(super::unknown_effect(
                            &declaration.name,
                            declaration.name_span,
                        ));
                    } else if !declaration.name.contains('.') {
                        diags.push(super::effect_leaf_required(
                            &declaration.name,
                            Some(declaration.name_span),
                        ));
                    }
                }
                Item::Func(function) => check_func(function, facts, diags),
                Item::Struct(definition) => {
                    for field in &definition.fields {
                        check_type(&field.ty, facts, diags);
                    }
                    for function in &definition.methods {
                        check_func(function, facts, diags);
                    }
                    for implementation in &definition.trait_impls {
                        for function in &implementation.methods {
                            check_func(function, facts, diags);
                        }
                    }
                }
                Item::Enum(definition) => {
                    for variant in &definition.variants {
                        match &variant.payload {
                            crate::AST::VariantPayload::Unit => {}
                            crate::AST::VariantPayload::Single(ty, _) => {
                                check_type(ty, facts, diags);
                            }
                            crate::AST::VariantPayload::Named(fields) => {
                                for field in fields {
                                    check_type(&field.ty, facts, diags);
                                }
                            }
                        }
                    }
                    for function in &definition.methods {
                        check_func(function, facts, diags);
                    }
                    for implementation in &definition.trait_impls {
                        for function in &implementation.methods {
                            check_func(function, facts, diags);
                        }
                    }
                }
                Item::Impl(implementation) => {
                    for function in &implementation.methods {
                        check_func(function, facts, diags);
                    }
                }
                Item::Trait(definition) => {
                    for method in &definition.methods {
                        if let Some(names) = &method.declared_effects {
                            for (name, span) in names {
                                check_name(name, *span, facts, diags);
                            }
                        }
                        for parameter in &method.params {
                            check_type(&parameter.ty, facts, diags);
                        }
                        if let Some(return_type) = &method.return_type {
                            check_type(return_type, facts, diags);
                        }
                        if let Some(body) = &method.default_body {
                            check_stmts(body, facts, diags);
                        }
                    }
                }
                Item::Test(test) => check_stmts(&test.body, facts, diags),
                Item::CodeModule(module) => {
                    if let Some(body) = &module.body {
                        check_items(body, facts, diags);
                    }
                }
                Item::GenericModule(module) => check_items(&module.body, facts, diags),
                _ => {}
            }
        }
    }
    let mut diags = Vec::new();
    for module in &bundle.modules {
        check_items(&module.items, facts, &mut diags);
    }
    diags
}

#[derive(Default)]
pub struct IncrementalSemaCache {
    environment: Vec<u8>,
    module_interfaces: HashMap<String, Vec<u8>>,
    module_dependencies: HashMap<String, Vec<String>>,
    functions: HashMap<String, CachedFunctionBody>,
    hits: u64,
    recomputes: u64,
    measurement_recomputed_items: Vec<String>,
}

impl IncrementalSemaCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> IncrementalSemaStats {
        IncrementalSemaStats {
            hits: self.hits,
            recomputes: self.recomputes,
            live_items: self.functions.len(),
            live_item_bytes: self.environment.len()
                + self
                    .module_interfaces
                    .iter()
                    .map(|(module, fingerprint)| module.len() + fingerprint.len())
                    .sum::<usize>()
                + self
                    .module_dependencies
                    .iter()
                    .map(|(module, dependencies)| {
                        module.len() + dependencies.iter().map(String::len).sum::<usize>()
                    })
                    .sum::<usize>()
                + self
                    .functions
                    .iter()
                    .map(|(key, entry)| {
                        key.len()
                            + entry.input.len()
                            + format!("{:?}", entry.function).len()
                            + format!("{:?}", entry.diagnostics).len()
                            + format!("{:?}", entry.summaries).len()
                            + format!("{:?}", entry.comptime_inputs).len()
                            + format!("{:?}", entry.address_taken).len()
                            + format!("{:?}", entry.name_ledger).len()
                            + format!("{:?}", entry.pending_diagnostics).len()
                    })
                    .sum::<usize>(),
            recomputed_items: self.measurement_recomputed_items.clone(),
        }
    }

    /// Start one measured re-verdict window. The cumulative counters stay
    /// available for existing clients; this list names only work in the next
    /// window so a receipt can show the edit's actual cone.
    pub fn clear_measurement(&mut self) {
        self.measurement_recomputed_items.clear();
    }

    pub fn clear(&mut self) {
        self.environment.clear();
        self.module_interfaces.clear();
        self.module_dependencies.clear();
        self.functions.clear();
        self.measurement_recomputed_items.clear();
    }

    fn begin_bundle(
        &mut self,
        bundle: &ProgramBundle,
        name_ledger: &jet_foundation::Names::NameLedger,
    ) {
        // D-INCR-UNIT1=A: dirty the module interface and its reverse import
        // closure only. Private body edits are handled by the per-function
        // input below and do not fan out to unrelated modules.
        let environment = incremental_global_environment(bundle);
        let environment_changed = self.environment != environment;
        if environment_changed {
            self.environment = environment;
        }

        let interfaces = bundle
            .modules
            .iter()
            .map(|module| (module.display.clone(), incremental_module_interface(module)))
            .collect::<HashMap<_, _>>();
        let dependencies = incremental_module_dependencies_with_ledger(bundle, name_ledger);
        let mut dirty = self
            .module_interfaces
            .iter()
            .filter_map(|(module, _)| (!interfaces.contains_key(module)).then_some(module.clone()))
            .collect::<HashSet<_>>();
        dirty.extend(interfaces.iter().filter_map(|(module, fingerprint)| {
            (self.module_interfaces.get(module) != Some(fingerprint)).then_some(module.clone())
        }));
        // Resolved import targets are semantic input too. The source import
        // can stay unchanged while the ledger maps it to a different module
        // (for example after a file/module move), so invalidate that owner's
        // bodies before propagating the change through the current graph.
        dirty.extend(self.module_dependencies.iter().filter_map(|(module, previous)| {
            (dependencies.get(module) != Some(previous)).then_some(module.clone())
        }));
        dirty.extend(dependencies.iter().filter_map(|(module, current)| {
            (self.module_dependencies.get(module) != Some(current)).then_some(module.clone())
        }));
        if environment_changed {
            dirty.extend(interfaces.keys().cloned());
        }

        // An interface change invalidates the changed module and every module
        // that imports it. Body-only edits keep dependents warm: their effect
        // summaries are solved again below from the changed callee summary.
        loop {
            let newly_dirty = dependencies
                .iter()
                .filter_map(|(module, imported)| {
                    (!dirty.contains(module)
                        && imported.iter().any(|dependency| dirty.contains(dependency)))
                    .then_some(module.clone())
                })
                .collect::<Vec<_>>();
            if newly_dirty.is_empty() {
                break;
            }
            dirty.extend(newly_dirty);
        }

        if !dirty.is_empty() {
            self.functions.retain(|key, _| {
                !dirty
                    .iter()
                    .any(|module| key == module || key.starts_with(&format!("{module}::")))
            });
        }
        self.module_interfaces = interfaces;
        self.module_dependencies = dependencies;
    }

    pub(super) fn get(&mut self, key: &str, input: &[u8]) -> Option<CachedFunctionBody> {
        let hit = self
            .functions
            .get(key)
            .filter(|entry| entry.input == input)
            .cloned();
        if hit.is_some() {
            self.hits += 1;
        }
        hit
    }

    pub(super) fn store(&mut self, key: String, entry: CachedFunctionBody) {
        self.record_recompute(key.clone());
        self.functions.insert(key, entry);
    }

    pub(super) fn record_recompute(&mut self, key: String) {
        self.recomputes += 1;
        self.measurement_recomputed_items.push(key);
    }
}

fn incremental_global_environment(bundle: &ProgramBundle) -> Vec<u8> {
    // These package facts can change between batch requests without changing
    // the source modules. Keep the sema cache keyed by the inputs that affect
    // body checking. `required_effects` is deliberately absent: completion
    // derives it from the checked bodies and writes it back to the bundle.
    let package_policy = (
        &bundle.package_guarantees.contain,
        bundle.package_guarantees.harden,
        &bundle.package_guarantees.dependency_names,
        &bundle.package_guarantees.effects,
        &bundle.package_guarantees.unsafe_paths,
        &bundle.package_guarantees.expert,
        &bundle.package_guarantees.deps,
        &bundle.package_guarantees.lints_deny,
        &bundle.package_guarantees.memory_denials,
        &bundle.package_guarantees.application_authority.granted_effects,
        &bundle.package_guarantees.application_authority.denied_effects,
        &bundle.package_guarantees.application_authority.authority,
    );
    let mut out = crate::CanonicalAST::canonical_fragment(&(
        bundle.entry,
        &bundle.project_root,
        bundle.active_os,
        bundle.web_partition_enforced,
        &bundle.build_facts,
        bundle.layer_ceiling,
        &bundle.edition,
        package_policy,
    ));
    out.extend(format!("{:?}", bundle.project_root).into_bytes());
    out
}

fn incremental_module_interface(module: &crate::AST::LoadedModule) -> Vec<u8> {
    let mut out = Vec::new();
    let metadata = (
        &module.path,
        &module.display,
        &module.alias,
        &module.imports,
        module.web_target_ceiling,
        module.pub_file,
        module.no_prelude,
        &module.default_target,
        &module.html_path,
        &module.policy_declarations,
    );
    out.extend(crate::CanonicalAST::canonical_fragment(&metadata));
    out.extend(format!("{metadata:?}").into_bytes());
    let additional_metadata = (
        &module.user_policy_declarations,
        &module.rule_facts,
        &module.script_body,
    );
    out.extend(crate::CanonicalAST::canonical_fragment(
        &additional_metadata,
    ));
    out.extend(format!("{additional_metadata:?}").into_bytes());
    for item in &module.items {
        let mut item = item.clone();
        clear_callable_bodies(&mut item);
        out.extend(crate::CanonicalAST::canonical_fragment(&item));
        // Canonical AST deliberately omits locations. The exact signature
        // locations are part of IDE facts, so include the body-free Debug
        // form as a conservative span fingerprint.
        out.extend(format!("{item:?}").into_bytes());
    }
    out
}

#[cfg(test)]
fn incremental_module_dependencies(bundle: &ProgramBundle) -> HashMap<String, Vec<String>> {
    incremental_module_dependencies_with_ledger(bundle, &bundle.name_ledger)
}

fn incremental_module_dependencies_with_ledger(
    bundle: &ProgramBundle,
    name_ledger: &jet_foundation::Names::NameLedger,
) -> HashMap<String, Vec<String>> {
    bundle
        .modules
        .iter()
        .enumerate()
        .map(|(module_idx, module)| {
            // D-NAME-WALK1=A: an inline or generic module can import an
            // already-loaded file module without adding a top-level import.
            // Track every scope or its cached callable may outlive the
            // dependency interface that it resolves.
            let mut dependencies = crate::AST::walk_imports(module)
                .into_iter()
                .filter_map(|(_, import)| {
                    incremental_import_target(bundle, name_ledger, module_idx, module, import)
                })
                .collect::<Vec<_>>();
            dependencies.sort();
            dependencies.dedup();
            (module.display.clone(), dependencies)
        })
        .collect()
}

fn incremental_import_target(
    bundle: &ProgramBundle,
    name_ledger: &jet_foundation::Names::NameLedger,
    source_idx: usize,
    source: &crate::AST::LoadedModule,
    import: &crate::AST::ImportDecl,
) -> Option<String> {
    // Loader/sema already resolved top-level imports against the loaded
    // bundle. Preserve that exact edge for invalidation; aliases and display
    // paths are projections and can collide across packages. Nested imports
    // are not seeded in the ledger yet, so they use the structural fallback
    // below.
    if let Some(target) = name_ledger.import_target(source_idx, import.span) {
        return bundle
            .modules
            .get(target)
            .map(|module| module.display.clone());
    }
    let requested = match &import.kind {
        ImportKind::File(path, _) | ImportKind::Module(path, _) => path.as_str(),
        ImportKind::Unqualified { module_alias, .. } => module_alias.as_str(),
    };
    let requested_stem = Path::new(requested)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(requested);
    let requested_path = if matches!(&import.kind, ImportKind::File(_, _)) {
        let mut path = source
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(requested);
        if path.extension().is_none() {
            path.set_extension(Syntax::FILE_EXT.trim_start_matches('.'));
        }
        Some(normalize_incremental_path(&path))
    } else {
        None
    };
    bundle
        .modules
        .iter()
        .filter(|candidate| candidate.display != source.display)
        .find(|candidate| {
            let directory_module = candidate
                .path
                .file_name()
                .and_then(|name| name.to_str())
                == Some(Syntax::DEFAULT_ENTRY_FILE)
                && candidate
                    .path
                    .parent()
                    .and_then(|parent| parent.file_name())
                    .and_then(|name| name.to_str())
                    == Some(requested_stem);
            requested_path
                .as_ref()
                .is_some_and(|path| normalize_incremental_path(&candidate.path) == *path)
                || candidate.alias == requested
                || candidate.alias == requested_stem
                || candidate.display == requested
                || candidate.display.ends_with(&format!("/{requested}"))
                || candidate
                    .display
                    .ends_with(&format!("/{requested_stem}.jet"))
                || directory_module
        })
        .map(|candidate| candidate.display.clone())
}

fn normalize_incremental_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn clear_callable_bodies(item: &mut Item) {
    let clear = |function: &mut Func| {
        function.body.clear();
        // The declaration end includes the body. Normalize it so an edit in a
        // trailing function does not invalidate unrelated earlier functions.
        function.span = function.name_span;
    };
    match item {
        Item::Func(function) => clear(function),
        Item::Struct(definition) => {
            definition.methods.iter_mut().for_each(clear);
            for implementation in &mut definition.trait_impls {
                implementation.methods.iter_mut().for_each(clear);
            }
        }
        Item::Enum(definition) => {
            definition.methods.iter_mut().for_each(clear);
            for implementation in &mut definition.trait_impls {
                implementation.methods.iter_mut().for_each(clear);
            }
        }
        Item::Impl(implementation) => implementation.methods.iter_mut().for_each(clear),
        Item::CodeModule(module) => {
            if let Some(body) = &mut module.body {
                body.iter_mut().for_each(clear_callable_bodies);
            }
        }
        _ => {}
    }
}

fn builtin_type_registry() -> TypeRegistry {
    let zero = Span::new(0, 0);
    let variants = ["Less", "Equal", "Greater"]
        .into_iter()
        .map(|name| (name.to_string(), (zero, VariantPayload::Unit)))
        .collect::<HashMap<_, _>>();
    let mut types = HashMap::new();
    types.insert(
        Syntax::TYPE_ORDERING.to_string(),
        TypeDef::Enum {
            variants,
            variant_order: vec![
                "Less".to_string(),
                "Equal".to_string(),
                "Greater".to_string(),
            ],
            groups: HashMap::new(),
            methods: HashMap::new(),
            single_use: false,
            deprecation: None,
            must_use: false,
            c_layout_tag: None,
        },
    );
    let remove_by_variants = ["Val", "Slot"]
        .into_iter()
        .map(|name| (name.to_string(), (zero, VariantPayload::Unit)))
        .collect::<HashMap<_, _>>();
    types.insert(
        crate::Syntax::TYPE_REMOVE_BY.to_string(),
        TypeDef::Enum {
            variants: remove_by_variants,
            variant_order: vec!["Val".to_string(), "Slot".to_string()],
            groups: HashMap::new(),
            methods: HashMap::new(),
            deprecation: None,
            single_use: false,
            must_use: false,
            c_layout_tag: None,
        },
    );
    // D-UNITLIT1=A: literal facts enter a module only with an in-scope unit
    // family. Pipeline registration fills the fact registry after Prelude
    // injection; keeping it empty prevents `#NoPrelude` from gaining Time.
    let time_dimension = Some(crate::AST::Dimension::base("core.units::Time"));
    let time_package = PathBuf::from("core.units");
    let zero_ratio = crate::AST::UnitRatio::zero();
    let one_ratio = crate::AST::UnitRatio::integer(1);
    let unit_types = HashSet::from([
        crate::Syntax::DURATION_TYPE.to_string(),
        crate::Syntax::TYPE_INSTANT.to_string(),
    ]);
    let mut unit_facts = HashMap::new();
    unit_facts.insert(
        crate::Syntax::DURATION_TYPE.to_string(),
        UnitFact {
            package: time_package.clone(),
            family: "Time".to_string(),
            member: "ns".to_string(),
            dimension: time_dimension.clone(),
            scale: one_ratio.clone(),
            scale_provenance: crate::AST::UnitScaleProvenance::Rational,
            offset: zero_ratio.clone(),
            kind: QuantityKind::Delta,
        },
    );
    unit_facts.insert(
        crate::Syntax::TYPE_INSTANT.to_string(),
        UnitFact {
            package: time_package,
            family: "Time".to_string(),
            member: "ns".to_string(),
            dimension: time_dimension,
            scale: one_ratio,
            scale_provenance: crate::AST::UnitScaleProvenance::Rational,
            offset: zero_ratio,
            kind: QuantityKind::Point,
        },
    );
    TypeRegistry {
        types,
        error_types: HashSet::new(),
        unit_types,
        unit_facts,
        literal_facts: HashMap::new(),
        computed_fields: HashMap::new(),
        field_defaults: HashMap::new(),
    }
}

fn unit_fact(
    family: &crate::AST::UnitFamilyDef,
    type_name: &str,
    dimension: Option<crate::AST::Dimension>,
    package: PathBuf,
) -> Option<UnitFact> {
    let affine = family.base.is_some()
        && family
            .members
            .iter()
            .any(|member| member.offset != crate::AST::UnitRatio::zero());
    family.members.iter().find_map(|member| {
        let stem = crate::AST::UnitFamilyDef::type_name(&member.name);
        let kind = if affine {
            if type_name == format!("{stem}Point") {
                QuantityKind::Point
            } else if type_name == format!("{stem}Delta") {
                QuantityKind::Delta
            } else {
                return None;
            }
        } else if type_name == stem {
            QuantityKind::Linear
        } else {
            return None;
        };
        Some(UnitFact {
            package: package.clone(),
            family: family.family.clone(),
            member: member.name.clone(),
            dimension: dimension.clone(),
            scale: member.scale.clone(),
            scale_provenance: member.scale_provenance.clone(),
            offset: if kind == QuantityKind::Point {
                member.offset.clone()
            } else {
                crate::AST::UnitRatio::zero()
            },
            kind,
        })
    })
}

fn declare_name(
    ledger: &mut jet_foundation::Names::NameLedger,
    module: usize,
    name: impl Into<String>,
    path: impl Into<String>,
    kind: &str,
    span: Span,
    visibility: jet_foundation::Names::NameVisibility,
) {
    ledger.declare(
        module,
        name.into(),
        path.into(),
        kind.to_string(),
        span,
        visibility,
    );
}

fn scoped_name(prefix: Option<&str>, name: &str) -> String {
    prefix.map_or_else(
        || name.to_string(),
        |prefix| jet_foundation::Names::member_name(prefix, name),
    )
}

fn scoped_path(prefix: &str, name: &str) -> String {
    format!("{prefix}.{name}")
}

fn declare_method_names(
    ledger: &mut jet_foundation::Names::NameLedger,
    module: usize,
    owner: &str,
    owner_path: &str,
    methods: &[Func],
) {
    for method in methods {
        declare_name(
            ledger,
            module,
            format!("{owner}.{}", method.name),
            format!("{owner_path}.{}", method.name),
            "method",
            method.name_span,
            jet_foundation::Names::NameVisibility::from_flags(method.is_pub, method.is_package_pub),
        );
    }
}

fn declare_item_names_scoped(
    ledger: &mut jet_foundation::Names::NameLedger,
    module: usize,
    path_prefix: &str,
    name_prefix: Option<&str>,
    item: &Item,
) {
    use jet_foundation::Names::NameVisibility;

    match item {
        Item::Func(function) => declare_name(
            ledger,
            module,
            scoped_name(name_prefix, &function.name),
            scoped_path(path_prefix, &function.name),
            "function",
            function.name_span,
            NameVisibility::from_flags(function.is_pub, function.is_package_pub),
        ),
        Item::Struct(definition) => {
            let item_name = scoped_name(name_prefix, &definition.name);
            let item_path = scoped_path(path_prefix, &definition.name);
            declare_name(
                ledger,
                module,
                item_name.clone(),
                item_path.clone(),
                "type",
                definition.name_span,
                NameVisibility::from_flags(definition.is_pub, definition.is_package_pub),
            );
            for field in &definition.fields {
                declare_name(
                    ledger,
                    module,
                    format!("{item_name}.{}", field.name),
                    format!("{item_path}.{}", field.name),
                    "field",
                    field.name_span,
                    NameVisibility::from_flags(field.is_pub, field.is_package_pub),
                );
            }
            if let Some(state) = &definition.state {
                for (name, span) in &state.states {
                    declare_name(
                        ledger,
                        module,
                        format!("{item_name}.State.{name}"),
                        format!("{item_path}.State.{name}"),
                        "state",
                        *span,
                        NameVisibility::from_flags(
                            definition.is_pub,
                            definition.is_package_pub,
                        ),
                    );
                }
            }
            declare_method_names(ledger, module, &item_name, &item_path, &definition.methods);
            for implementation in &definition.trait_impls {
                declare_method_names(
                    ledger,
                    module,
                    &item_name,
                    &item_path,
                    &implementation.methods,
                );
            }
        }
        Item::Enum(definition) => {
            let item_name = scoped_name(name_prefix, &definition.name);
            let item_path = scoped_path(path_prefix, &definition.name);
            let visibility =
                NameVisibility::from_flags(definition.is_pub, definition.is_package_pub);
            declare_name(
                ledger,
                module,
                item_name.clone(),
                item_path.clone(),
                "type",
                definition.name_span,
                visibility,
            );
            for variant in &definition.variants {
                declare_name(
                    ledger,
                    module,
                    format!("{item_name}.{}", variant.name),
                    format!("{item_path}.{}", variant.name),
                    "variant",
                    variant.name_span,
                    visibility,
                );
            }
            declare_method_names(ledger, module, &item_name, &item_path, &definition.methods);
            for implementation in &definition.trait_impls {
                declare_method_names(
                    ledger,
                    module,
                    &item_name,
                    &item_path,
                    &implementation.methods,
                );
            }
        }
        Item::Distinct(definition) => declare_name(
            ledger,
            module,
            scoped_name(name_prefix, &definition.name),
            scoped_path(path_prefix, &definition.name),
            "type",
            definition.name_span,
            NameVisibility::from_flags(definition.is_pub, definition.is_package_pub),
        ),
        Item::TypeAlias(definition) => declare_name(
            ledger,
            module,
            scoped_name(name_prefix, &definition.name),
            scoped_path(path_prefix, &definition.name),
            "type",
            definition.name_span,
            NameVisibility::from_flags(definition.is_pub, definition.is_package_pub),
        ),
        Item::UnitFamily(family) => {
            for definition in family.distinct_defs() {
                declare_name(
                    ledger,
                    module,
                    scoped_name(name_prefix, &definition.name),
                    scoped_path(path_prefix, &definition.name),
                    "type",
                    definition.name_span,
                    NameVisibility::from_flags(definition.is_pub, definition.is_package_pub),
                );
            }
        }
        Item::Trait(definition) => {
            let item_name = scoped_name(name_prefix, &definition.name);
            let item_path = scoped_path(path_prefix, &definition.name);
            let visibility =
                NameVisibility::from_flags(definition.is_pub, definition.is_package_pub);
            declare_name(
                ledger,
                module,
                item_name.clone(),
                item_path.clone(),
                "trait",
                definition.name_span,
                visibility,
            );
            for method in &definition.methods {
                declare_name(
                    ledger,
                    module,
                    format!("{item_name}.{}", method.name),
                    format!("{item_path}.{}", method.name),
                    "method",
                    method.name_span,
                    visibility,
                );
            }
        }
        Item::Tag(definition) => declare_name(
            ledger,
            module,
            scoped_name(name_prefix, &definition.name),
            scoped_path(path_prefix, &definition.name),
            "tag",
            definition.name_span,
            NameVisibility::from_flags(definition.is_pub, definition.is_package_pub),
        ),
        Item::Impl(implementation) => {
            let owner = scoped_name(name_prefix, &implementation.type_name);
            let owner_path = scoped_path(path_prefix, &implementation.type_name);
            declare_method_names(ledger, module, &owner, &owner_path, &implementation.methods);
            for (name, span, _) in &implementation.assoc_type_impls {
                declare_name(
                    ledger,
                    module,
                    format!("{owner}.{name}"),
                    format!("{owner_path}.{name}"),
                    "associated_type",
                    *span,
                    NameVisibility::Private,
                );
            }
        }
        Item::Const(definition) => declare_name(
            ledger,
            module,
            scoped_name(name_prefix, &definition.name),
            scoped_path(path_prefix, &definition.name),
            "const",
            definition.name_span,
            NameVisibility::Private,
        ),
        Item::ExternRust(block) => {
            for function in &block.functions {
                declare_name(
                    ledger,
                    module,
                    scoped_name(name_prefix, &function.name),
                    scoped_path(path_prefix, &function.name),
                    "extern",
                    function.name_span,
                    NameVisibility::Private,
                );
            }
        }
        Item::CModule(module_def) => {
            for function in &module_def.functions {
                declare_name(
                    ledger,
                    module,
                    scoped_name(name_prefix, &function.name),
                    scoped_path(path_prefix, &function.name),
                    "extern",
                    function.name_span,
                    NameVisibility::Public,
                );
            }
        }
        Item::CodeModule(code_module) => {
            let item_name = scoped_name(name_prefix, &code_module.name);
            let item_path = scoped_path(path_prefix, &code_module.name);
            let visibility =
                NameVisibility::from_flags(code_module.is_pub, code_module.is_package_pub);
            declare_name(
                ledger,
                module,
                item_name.clone(),
                item_path.clone(),
                if code_module.body.is_some() {
                    "module"
                } else {
                    "file_module"
                },
                code_module.name_span,
                visibility,
            );
            if let Some(body) = &code_module.body {
                for nested in body {
                    declare_item_names_scoped(ledger, module, &item_path, Some(&item_name), nested);
                }
            }
        }
        Item::ProtocolDecl(protocol) => {
            let visibility = NameVisibility::from_flags(protocol.is_pub, protocol.is_package_pub);
            declare_name(
                ledger,
                module,
                scoped_name(name_prefix, &protocol.name),
                scoped_path(path_prefix, &protocol.name),
                "protocol",
                protocol.name_span,
                visibility,
            );
        }
        _ => {}
    }
}

fn declare_item_names(
    ledger: &mut jet_foundation::Names::NameLedger,
    module: usize,
    module_alias: &str,
    item: &Item,
) {
    declare_item_names_scoped(ledger, module, module_alias, None, item);
}

fn state_marker_path(expr: &Expr) -> Option<(String, Span)> {
    match expr {
        Expr::Ident(name, span) => Some((name.clone(), *span)),
        Expr::Field(base, member, span) => {
            Some((format!("{}.{}", state_marker_path(base)?.0, member), *span))
        }
        _ => None,
    }
}

fn record_state_marker_references_for_method(
    ledger: &mut jet_foundation::Names::NameLedger,
    module_idx: usize,
    source_module: &str,
    owner: &str,
    method: &Func,
) {
    for marker in &method.markers {
        let slots: &[usize] = match marker.name.as_str() {
            Syntax::KW_STATE => &[0],
            Syntax::KW_TRANSITION => &[0, 1],
            _ => &[],
        };
        for &slot in slots {
            let Some((raw, span)) = marker.args.get(slot).and_then(state_marker_path) else {
                continue;
            };
            if raw == crate::Syntax::STATE_ENTRY {
                continue;
            }
            let candidates = if raw.contains(".State.") {
                // A qualified marker names one exact owner. Do not fall back
                // to the current owner's leaf: that would make a misspelled
                // `Other.State.Ready` appear to reference `owner.State.Ready`.
                vec![raw.clone()]
            } else {
                vec![format!("{owner}.State.{raw}")]
            };
            let Some(candidate) = candidates
                .iter()
                .find(|candidate| ledger.declaration(module_idx, candidate).is_some())
            else {
                continue;
            };
            let Some((target_module, def_span, semantic_identity)) = ledger
                .declaration(module_idx, candidate)
                .map(|declaration| {
                    (
                        declaration.module,
                        declaration.span,
                        ledger.semantic_identity(declaration.module, candidate),
                    )
                })
            else {
                continue;
            };
            ledger.record_reference(
                source_module.to_string(),
                span.start,
                span.end,
                jet_foundation::Names::NameReference {
                    module_path: ledger
                        .module_path(target_module)
                        .unwrap_or(source_module)
                        .to_string(),
                    kind: "state".to_string(),
                    def_span,
                    semantic_identity,
                },
            );
        }
    }
}

fn record_state_marker_references_in_items(
    ledger: &mut jet_foundation::Names::NameLedger,
    module_idx: usize,
    source_module: &str,
    items: &[Item],
    prefix: Option<&str>,
) {
    for item in items {
        match item {
            Item::Struct(definition) => {
                let owner = scoped_name(prefix, &definition.name);
                for method in &definition.methods {
                    record_state_marker_references_for_method(
                        ledger,
                        module_idx,
                        source_module,
                        &owner,
                        method,
                    );
                }
                for implementation in &definition.trait_impls {
                    for method in &implementation.methods {
                        record_state_marker_references_for_method(
                            ledger,
                            module_idx,
                            source_module,
                            &owner,
                            method,
                        );
                    }
                }
            }
            Item::Impl(implementation) => {
                let owner = scoped_name(prefix, &implementation.type_name);
                for method in &implementation.methods {
                    record_state_marker_references_for_method(
                        ledger,
                        module_idx,
                        source_module,
                        &owner,
                        method,
                    );
                }
            }
            Item::Enum(definition) => {
                let owner = scoped_name(prefix, &definition.name);
                for method in &definition.methods {
                    record_state_marker_references_for_method(
                        ledger,
                        module_idx,
                        source_module,
                        &owner,
                        method,
                    );
                }
            }
            Item::CodeModule(module) => {
                let module_name = scoped_name(prefix, &module.name);
                if let Some(body) = &module.body {
                    record_state_marker_references_in_items(
                        ledger,
                        module_idx,
                        source_module,
                        body,
                        Some(&module_name),
                    );
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn record_state_marker_references(
    bundle: &ProgramBundle,
    ledger: &mut jet_foundation::Names::NameLedger,
) {
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        record_state_marker_references_in_items(
            ledger,
            module_idx,
            &module.display,
            &module.items,
            None,
        );
    }
}

fn populate_name_ledger(
    bundle: &ProgramBundle,
    states: &[ModuleState],
    ledger: &mut jet_foundation::Names::NameLedger,
) {
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        let package = jet_foundation::Names::package_scope_for(&module.path, &bundle.project_root);
        ledger.set_module(
            module_idx,
            module.alias.clone(),
            module.display.clone(),
            package,
        );
    }

    for (module_idx, module) in bundle.modules.iter().enumerate() {
        let state = &states[module_idx];
        for item in &module.items {
            declare_item_names(ledger, module_idx, &module.alias, item);
            if let Item::CodeModule(instance) = item {
                if instance.instance_identity.is_some() {
                    for (internal, display) in GenericModules::instance_display_paths(instance) {
                        ledger.record_display_path(
                            module_idx,
                            format!("{}.{}", module.alias, internal),
                            display,
                        );
                    }
                    for (internal, display) in
                        GenericModules::top_level_instance_display_paths(instance, &module.items)
                    {
                        ledger.record_display_path(
                            module_idx,
                            format!("{}.{}", module.alias, internal),
                            display,
                        );
                    }
                }
                // A plain inline module's member types are lifted with the same
                // member naming (`hoist_inline_module_member_types`), so project
                // them back to `module.Type` for every message and tool.
                if instance.instance_identity.is_none() && instance.body.is_some() {
                    for (internal, display) in
                        GenericModules::plain_inline_module_display_paths(instance, &module.items)
                    {
                        ledger.record_display_path(
                            module_idx,
                            format!("{}.{}", module.alias, internal),
                            display,
                        );
                    }
                }
            }
        }
        for import in &module.imports {
            let import_alias = import.import_alias();
            let alias_visibility = jet_foundation::Names::NameVisibility::from_flags(
                import.is_pub,
                import.is_package_pub,
            );
            let (import_target, target_module) =
                if let Some(target) = ledger.import_target(module_idx, import.span) {
                    (bundle.modules[target].alias.clone(), Some(target))
                } else if let Some(core_path) = import.core_module_path() {
                    (core_path, None)
                } else {
                    let target = match &import.kind {
                        ImportKind::File(path, _) | ImportKind::Module(path, _) => path.clone(),
                        ImportKind::Unqualified { module_alias, .. } => module_alias.clone(),
                    };
                    (target, None)
                };
            ledger.record_alias(
                module_idx,
                import_alias,
                import_target,
                target_module,
                import.alias_span,
                alias_visibility,
            );

            for binding in import.walk_bindings() {
                let Some(original) = binding.original else {
                    continue;
                };
                let local = binding.local.as_str();
                let target =
                    if let Some(core_prefix) = crate::AST::core_list_prefix(binding.module_alias) {
                        Some((format!("{core_prefix}.{original}"), None))
                    } else if let Some(target_module) = state.imports.get(local) {
                        Some((
                            bundle.modules[*target_module].alias.clone(),
                            Some(*target_module),
                        ))
                    } else if let Some((real, target_module)) = state.unqualified_file.get(local) {
                        Some((
                            format!("{}.{}", bundle.modules[*target_module].alias, real),
                            Some(*target_module),
                        ))
                    } else if let Some(resolved) = state.unqualified.get(local) {
                        Some((resolved.clone(), Some(module_idx)))
                    } else if let Some(target_module) = state.imports.get(binding.module_alias) {
                        Some((
                            format!("{}.{}", bundle.modules[*target_module].alias, original),
                            Some(*target_module),
                        ))
                    } else {
                        None
                    };
                if let Some((target, target_module)) = target {
                    ledger.record_alias(
                        module_idx,
                        local.to_string(),
                        target,
                        target_module,
                        binding.local_span,
                        alias_visibility,
                    );
                }
            }
        }
    }
}

/// D-MOD2: inside an inline `module math { … }`, a call to a sibling function
/// `helper(x)` must lower to the mangled `__jet_math__helper`. This pre-pass rewrites

pub fn check_bundle(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    pipeline_check_bundle_opts_for_output(
        bundle,
        mode,
        false,
        crate::Policy::GateSet::default(),
        None,
        None,
    )
    .0
}

fn validate_script_entries(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    validate_script_entry_bodies(bundle, &mut diags);
    diags
}

/// D-ENTRY-SCRIPT1=B: keep the script surface in the parser/formatter, then
/// validate the package seam's entry materialization. Imported scripts and
/// explicit-run conflicts are rejected before registration so their statements
/// can never become an accidental runtime side effect.
fn validate_script_entry_bodies(bundle: &mut ProgramBundle, diags: &mut Vec<Diagnostic>) {
    bundle.materialize_script_entries();
    for (module_idx, module) in bundle.modules.iter_mut().enumerate() {
        let body = std::mem::take(&mut module.script_body);
        if body.is_empty() {
            continue;
        }
        let script_span = Span::new(
            body.first().map_or(0, |stmt| stmt.span().start),
            body.last().map_or(0, |stmt| stmt.span().end),
        );
        let explicit_run = module.items.iter().find_map(|item| match item {
            Item::Func(function) if function.name == "run" => Some(function.clone()),
            _ => None,
        });

        if module_idx != bundle.entry {
            diags.push(Diagnostic::error(
                "E0620",
                format!("imported script `{}` has executable top-level statements", module.display),
                "imported files provide declarations; only the entry file may have a script body".to_string(),
                "move the statements into the entry file's `fn run`, or import a declaration-only file"
                    .to_string(),
                Some(script_span),
            ));
        } else if let Some(run) = explicit_run {
            let mut diagnostic = Diagnostic::error(
                "E0621",
                "a script cannot have loose statements and an explicit `fn run`".to_string(),
                "a script's loose statements already form its one `run` body".to_string(),
                "run `jet fix` to move the loose statements into `fn run`, or remove the explicit function"
                    .to_string(),
                Some(run.name_span),
            );
            if let Some(edit) = script_conflict_edit(&module.source, &body, &run) {
                diagnostic.set_structured_edit(edit);
            }
            diags.push(diagnostic);
        } else {
            module
                .items
                .push(Item::Func(Func::implicit_run(body, script_span)));
        }
    }
}

/// Produce the unified `jet fix`/LSP edit for the common explicit-run case.
/// The edit is deliberately conservative for unusual same-line layouts; the
/// diagnostic still remains actionable when no mechanical edit is safe.
fn script_conflict_edit(source: &str, body: &[Stmt], run: &Func) -> Option<TextEdit> {
    let statement_spans = body
        .iter()
        .map(|stmt| script_statement_span(source, stmt))
        .collect::<Option<Vec<_>>>()?;
    let mut spans = statement_spans.clone();
    spans.sort_by_key(|span| (span.start, span.end));
    spans.dedup_by_key(|span| (span.start, span.end));
    if spans.len() != statement_spans.len() {
        return None;
    }
    if spans.windows(2).any(|pair| pair[0].end > pair[1].start)
        || spans
            .iter()
            .any(|span| span.start < run.span.end && span.end > run.span.start)
    {
        return None;
    }

    let open = source.get(run.name_span.start..run.span.end)?.find('{')? + run.name_span.start;
    let close = matching_brace(source, open)?;
    let before = body
        .iter()
        .filter_map(|stmt| script_statement_span(source, stmt))
        .filter(|span| span.start < run.span.start)
        .filter_map(|span| source.get(span.start..span.end))
        .collect::<Vec<_>>()
        .join("\n");
    let after = body
        .iter()
        .filter_map(|stmt| script_statement_span(source, stmt))
        .filter(|span| span.start > run.span.end)
        .filter_map(|span| source.get(span.start..span.end))
        .collect::<Vec<_>>()
        .join("\n");
    let before_insert = (!before.is_empty()).then(|| {
        let text = indent_script(&before);
        if source.as_bytes().get(open + 1) == Some(&b'\n') {
            format!("\n{text}")
        } else {
            format!("\n{text}\n")
        }
    });
    let after_insert = (!after.is_empty()).then(|| {
        let text = indent_script(&after);
        if source.as_bytes().get(close.saturating_sub(1)) == Some(&b'\n') {
            format!("{text}\n")
        } else {
            format!("\n{text}\n")
        }
    });

    let mut edits = spans
        .into_iter()
        .map(|span| (span.start, span.end, String::new()))
        .collect::<Vec<_>>();
    if let Some(text) = before_insert {
        edits.push((open + 1, open + 1, text));
    }
    if let Some(text) = after_insert {
        edits.push((close, close, text));
    }
    edits.sort_by_key(|(start, end, _)| (*start, *end));

    let mut fixed = String::with_capacity(source.len());
    let mut cursor = 0;
    for (start, end, replacement) in edits {
        if start < cursor || end > source.len() || start > end {
            return None;
        }
        fixed.push_str(source.get(cursor..start)?);
        fixed.push_str(&replacement);
        cursor = end;
    }
    fixed.push_str(source.get(cursor..)?);
    Some(TextEdit {
        span: Span::new(0, source.len()),
        new_text: fixed,
    })
}

/// Return the source occupied by one parsed script statement.
///
/// `Stmt::span` is the semantic/debug span. Calls and method calls intentionally
/// retain only their callee span, so extend that AST anchor to the first
/// top-level statement terminator without consuming following declarations or
/// their trivia. This is source-boundary recovery for an existing AST node, not
/// another parser or desugaring path.
fn script_statement_span(source: &str, stmt: &Stmt) -> Option<Span> {
    let statement = stmt.span();
    let start = match stmt {
        Stmt::Expr(expr) => expression_source_start(expr),
        Stmt::Return(_, span) | Stmt::Yield(_, span) => span.start,
        _ => statement.start,
    };
    let end = source_statement_end(source, start, statement.end)?;
    let span = Span::new(start, end.max(statement.end));
    source.get(span.start..span.end)?;
    Some(span)
}

fn expression_source_start(expr: &Expr) -> usize {
    match expr {
        Expr::MethodCall { receiver, .. } => expression_source_start(receiver),
        _ => expr.span().start,
    }
}

fn source_statement_end(source: &str, start: usize, initial_end: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if start > bytes.len() {
        return None;
    }
    let mut index = start;
    let mut end = initial_end.min(bytes.len());
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut string_delimiter = 0u8;
    let mut character = false;
    let mut line_comment = false;
    let mut block_comment = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if line_comment {
            if byte == b'\n' {
                line_comment = false;
                if parens == 0 && brackets == 0 && braces == 0 {
                    return Some(end);
                }
            }
            index += 1;
            continue;
        }
        if block_comment {
            if bytes.get(index..index + 2) == Some(b"*/") {
                block_comment = false;
                index += 2;
            } else {
                if byte == b'\n' && parens == 0 && brackets == 0 && braces == 0 {
                    return Some(end);
                }
                index += 1;
            }
            continue;
        }
        if string_delimiter != 0 || character {
            if byte == b'\\' {
                index = index.saturating_add(2);
            } else if string_delimiter == 3 && bytes.get(index..index + 3) == Some(b"\"\"\"") {
                string_delimiter = 0;
                index += 3;
            } else {
                if (string_delimiter == 1 && byte == b'"') || (character && byte == b'\'') {
                    string_delimiter = 0;
                    character = false;
                }
                index += 1;
            }
            end = end.max(index.min(bytes.len()));
            continue;
        }

        if bytes.get(index..index + 2) == Some(b"//") {
            if parens == 0 && brackets == 0 && braces == 0 {
                return Some(end);
            }
            line_comment = true;
            index += 2;
            continue;
        }
        if bytes.get(index..index + 2) == Some(b"/*") {
            block_comment = true;
            index += 2;
            continue;
        }
        if bytes.get(index..index + 3) == Some(b"\"\"\"") {
            string_delimiter = 3;
            index += 3;
            end = end.max(index);
            continue;
        }

        match byte {
            b'"' => string_delimiter = 1,
            b'\'' => character = true,
            b'(' => parens += 1,
            b')' if parens > 0 => parens -= 1,
            b')' => return Some(end),
            b'[' => brackets += 1,
            b']' if brackets > 0 => brackets -= 1,
            b']' => return Some(end),
            b'{' => braces += 1,
            b'}' if braces > 0 => braces -= 1,
            b'}' => return Some(end),
            b';' if parens == 0 && brackets == 0 && braces == 0 => return Some(end),
            b'\n' if parens == 0 && brackets == 0 && braces == 0 => return Some(end),
            _ => {}
        }
        index += 1;
        if !byte.is_ascii_whitespace() {
            end = end.max(index);
        }
    }
    Some(end)
}

fn indent_script(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("    {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if source.ends_with('\n') { "\n" } else { "" }
}

fn matching_brace(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut index = open;
    let mut string = false;
    let mut line_comment = false;
    let mut block_comment = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if line_comment {
            if byte == b'\n' {
                line_comment = false;
            }
            index += 1;
            continue;
        }
        if block_comment {
            if bytes.get(index..index + 2) == Some(b"*/") {
                block_comment = false;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if string {
            if byte == b'\\' {
                index += 2;
            } else {
                string = byte != b'"';
                index += 1;
            }
            continue;
        }
        if bytes.get(index..index + 2) == Some(b"//") {
            line_comment = true;
            index += 2;
        } else if bytes.get(index..index + 2) == Some(b"/*") {
            block_comment = true;
            index += 2;
        } else if byte == b'"' {
            string = true;
            index += 1;
        } else if byte == b'{' {
            depth += 1;
            index += 1;
        } else if byte == b'}' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
            index += 1;
        } else {
            index += 1;
        }
    }
    None
}

/// Check one explicitly addressed runnable Output. Sema marks that resolved
/// callable as the sole runtime entry; lower tiers consume only the fact.
pub fn check_bundle_for_output(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    output: &str,
) -> Vec<Diagnostic> {
    pipeline_check_bundle_opts_for_output(
        bundle,
        mode,
        false,
        crate::Policy::GateSet::default(),
        Some(output),
        None,
    )
    .0
}

pub fn check_bundle_for_output_opts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    output: &str,
    freestanding: bool,
    gates: crate::Policy::GateSet,
) -> Vec<Diagnostic> {
    pipeline_check_bundle_opts_for_output(bundle, mode, freestanding, gates, Some(output), None).0
}
pub fn check_bundle_for_output_opts_with_effect_facts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    output: &str,
    freestanding: bool,
    gates: crate::Policy::GateSet,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    pipeline_check_bundle_opts_for_output(
        bundle,
        mode,
        freestanding,
        gates,
        Some(output),
        None,
    )
}


/// Like `check_bundle` but also returns effect facts for D-SEMINDEX1.
pub fn check_bundle_with_effect_facts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    pipeline_check_bundle_opts_for_output(
        bundle,
        mode,
        false,
        crate::Policy::GateSet::default(),
        None,
        None,
    )
}

/// Check the compiler-host build entry with the read-only compiler API enabled.
///
/// This is a separate authority from ordinary runtime/check sema. The caller
/// must have selected the package/workspace `fn build`; the checker still
/// limits the exception to that entry function in the entry module.
pub fn check_bundle_with_effect_facts_for_build(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    pipeline_check_bundle_opts_for_output_with_context(
        bundle,
        mode,
        false,
        crate::Policy::GateSet::default(),
        None,
        None,
        true,
    )
}

/// D-BUILDENTRY1: the selected root build entry's function name. Compiler-known
/// only while the build session runs; never part of the runtime program.
const BUILD_ENTRY_FN: &str = "build";

/// D-BUILDENTRY1: is this function the programmable-build root entry?
///
/// Identity is the name plus the one `BuildContext` parameter. `BuildContext`
/// is a compiler value with no runtime representation, so a function that takes
/// one is build-only whatever else its signature says — including a malformed
/// one, which must still be kept out of runtime codegen while `E3501` explains
/// it. The return clause is graded separately by
/// [`build_entry_signature_is_valid`].
///
/// The name alone is not enough: an ordinary `fn build(count: Int) Int ->` is a
/// normal runtime function, and dropping it would emit calls to a name that has
/// no definition.
pub fn is_build_entry(func: &Func) -> bool {
    func.name == BUILD_ENTRY_FN
        && func.params.len() == 1
        && func.params[0].ty == Type::Named(Syntax::TYPE_BUILD_CONTEXT.to_string())
}

/// D-BUILDENTRY1: does the build entry carry its one typed success contract,
/// `fn build(b: BuildContext) BuildPlan`? A bare return type uses the ordinary
/// implicit `!Err` failure route; an explicit result remains available only when
/// it names a non-default expert error domain. The retired `BuildPlan!` spelling
/// therefore stays invalid and reaches `E3501`. Build authority and graph
/// handoff are one contract, so a build entry with any other success type is
/// selected, rejected, and never emitted.
pub fn build_entry_signature_is_valid(func: &Func) -> bool {
    is_build_entry(func)
        && match func.return_type.as_ref() {
            Some(Type::Named(name)) => name == Syntax::TYPE_BUILD_PLAN,
            Some(Type::Result { ok, err }) => {
                **ok == Type::Named(Syntax::TYPE_BUILD_PLAN.to_string())
                    && !matches!(
                        err.as_ref(),
                        Type::Named(name) if name == Syntax::TYPE_ERR
                    )
            }
            _ => false,
        }
}

/// D-BUILDENTRY1 / I2 / I3 / I9: project a checked bundle down to the program
/// the user actually runs by removing every build-only entry.
///
/// `fn build` is compiler-host code. It is type-checked like any other
/// function, and `jet build` evaluates it in the comptime build interpreter,
/// but it is not runtime code: its `BuildContext` parameter and `BuildPlan`
/// result are compiler values with no runtime representation. A build entry
/// left in the program therefore reaches `Codegen::emit_func`, fails the typed
/// IR coverage gate, and raises an internal compiler error — a compiler bug by
/// I2, and by I3 a decision the front end owes codegen rather than one codegen
/// discovers (Tower card 2008).
///
/// The removal belongs to the front end, so it happens exactly once for every
/// consumer: AOT emit, the Cranelift JIT, and the interpreter all receive the
/// same runtime program instead of each engine having to know that one function
/// or build-only error conversion is not theirs (I9).
pub fn strip_build_only_entries(bundle: &mut ProgramBundle) {
    for module in &mut bundle.modules {
        module.items.retain(|item| match item {
            Item::Func(func) => !is_build_entry(func),
            // `BuildError` exists only in the compiler-host build interpreter.
            // Its Prelude conversion is needed while `fn build` runs, but the
            // conversion has no runtime source type after that entry is gone.
            Item::ErrorConv(conversion) => {
                conversion.from_ty != "BuildError" && conversion.to_ty != "BuildError"
            }
            _ => true,
        });
    }
}

/// Canonical batch sema entry with reusable per-function checking.
pub fn check_bundle_with_effect_facts_incremental(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    cache: &mut IncrementalSemaCache,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    pipeline_check_bundle_opts_for_output(
        bundle,
        mode,
        false,
        crate::Policy::GateSet::default(),
        None,
        Some(cache),
    )
}

/// Like `check_bundle` but with extra build options (E2-M15).
pub fn check_bundle_freestanding(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    pipeline_check_bundle_opts_for_output(
        bundle,
        mode,
        true,
        crate::Policy::GateSet::default(),
        None,
        None,
    )
    .0
}

pub fn check_bundle_freestanding_with_gates(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    gates: crate::Policy::GateSet,
) -> Vec<Diagnostic> {
    pipeline_check_bundle_opts_for_output(bundle, mode, true, gates, None, None).0
}

/// Check the audited-escape family with one invocation gate set.
pub fn check_bundle_gates(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    gates: crate::Policy::GateSet,
) -> Vec<Diagnostic> {
    pipeline_check_bundle_opts_for_output(bundle, mode, false, gates, None, None).0
}

#[cfg(test)]
mod structure_tests {
    use super::*;

    #[test]
    fn duplicate_unknown_name_at_one_span_is_reported_once() {
        let span = Span::new(4, 8);
        let unknown = || {
            Diagnostic::error(
                "E0107",
                "nothing named `x` exists here".to_string(),
                "a name must be declared before it is used".to_string(),
                "declare it first".to_string(),
                Some(span),
            )
        };
        let mut diagnostics = vec![unknown(), unknown()];
        dedupe_unknown_names(&mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn same_unknown_name_span_in_two_modules_is_preserved() {
        let unknown = || {
            Diagnostic::error(
                "E0107",
                "nothing named `x` exists here".to_string(),
                "a name must be declared before it is used".to_string(),
                "declare it first".to_string(),
                Some(Span::new(4, 8)),
            )
        };
        let mut first_module = vec![unknown(), unknown()];
        let mut second_module = vec![unknown(), unknown()];
        dedupe_unknown_names(&mut first_module);
        dedupe_unknown_names(&mut second_module);
        let diagnostics = [first_module, second_module].concat();
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn duplicate_soft_public_lint_at_one_occurrence_is_reported_once() {
        let span = Span::new(4, 8);
        let soft_public = || {
            Diagnostic::lint(
                "L0601",
                "`_x` is a soft-public API".to_string(),
                "a leading underscore allows outside use".to_string(),
                "use a public name".to_string(),
                Some(span),
            )
        };
        let mut diagnostics = vec![soft_public(), soft_public()];
        dedupe_soft_public_lints(&mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn distinct_soft_public_names_at_one_occurrence_are_preserved() {
        let span = Span::new(4, 8);
        let soft_public = |name: &str| {
            Diagnostic::lint(
                "L0601",
                format!("`{name}` is a soft-public API"),
                "a leading underscore allows outside use".to_string(),
                "use a public name".to_string(),
                Some(span),
            )
        };
        let mut diagnostics = vec![soft_public("_x"), soft_public("_y")];
        dedupe_soft_public_lints(&mut diagnostics);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].what, "`_x` is a soft-public API");
        assert_eq!(diagnostics[1].what, "`_y` is a soft-public API");
    }

    #[test]
    fn soft_public_lints_at_distinct_occurrences_are_preserved() {
        let soft_public = |span| {
            Diagnostic::lint(
                "L0601",
                "`_x` is a soft-public API".to_string(),
                "a leading underscore allows outside use".to_string(),
                "use a public name".to_string(),
                Some(span),
            )
        };
        let mut diagnostics = vec![soft_public(Span::new(4, 8)), soft_public(Span::new(12, 16))];
        dedupe_soft_public_lints(&mut diagnostics);
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn script_conflict_edit_splices_statement_not_shared_line() {
        let source = "print(\"before\"); fn helper() {}\nfn run() { print(\"middle\") }\n";
        let (tokens, lexer_diagnostics) = crate::Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let mut program = crate::Parser::parse(&tokens).unwrap();
        let body = std::mem::take(&mut program.script_body);
        let run = program
            .items
            .iter()
            .find_map(|item| match item {
                Item::Func(function) if function.name == "run" => Some(function.clone()),
                _ => None,
            })
            .unwrap();

        let edit = script_conflict_edit(source, &body, &run).expect("structured script edit");
        let fixed = edit.new_text;
        let (fixed_tokens, fixed_lexer_diagnostics) = crate::Lexer::lex(&fixed);
        assert!(
            fixed_lexer_diagnostics.is_empty(),
            "{fixed_lexer_diagnostics:?}"
        );
        let fixed_program = crate::Parser::parse(&fixed_tokens).unwrap();
        assert_eq!(
            fixed_program
                .items
                .iter()
                .filter(|item| matches!(item, Item::Func(function) if function.name == "run"))
                .count(),
            1
        );
        assert_eq!(
            fixed_program
                .items
                .iter()
                .filter(|item| matches!(item, Item::Func(function) if function.name == "helper"))
                .count(),
            1,
            "a declaration sharing the script line must survive outside run"
        );
        let fixed_run = fixed_program
            .items
            .iter()
            .find_map(|item| match item {
                Item::Func(function) if function.name == "run" => Some(function),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            fixed_run
                .body
                .iter()
                .filter(|stmt| matches!(stmt, Stmt::Expr(Expr::Call(call)) if call.name == "print"))
                .count(),
            2,
            "the fix must retain the loose and explicit run statements"
        );
        assert!(fixed_program.script_body.is_empty());
    }
    fn incremental_bundle(source: &str) -> ProgramBundle {
        let (tokens, lexer_diagnostics) = crate::Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let mut program = crate::Parser::parse(&tokens).unwrap();
        ProgramBundle {
            entry: 0,
            project_root: std::path::PathBuf::from("."),
            modules: vec![crate::AST::LoadedModule {
                path: std::path::PathBuf::from("cache-accounting.jet"),
                display: "cache-accounting.jet".to_string(),
                source: source.to_string(),
                alias: "main".to_string(),
                imports: std::mem::take(&mut program.imports),
                items: std::mem::take(&mut program.items),
                script_body: std::mem::take(&mut program.script_body),
                block_spans: std::mem::take(&mut program.block_spans),
                web_target_ceiling: program.web_target_ceiling,
                pub_file: program.pub_file,
                no_prelude: program.no_prelude,
                default_target: program.default_target,
                html_path: program.html_path,
                policy_declarations: program.policy_declarations,
                user_policy_declarations: program.user_policy_declarations,
                rule_facts: program.rule_facts,
            }],
            parse_teaching: Vec::new(),
            used_core: HashSet::new(),
            ffi_callback_fns: HashSet::new(),
            cffi: crate::AST::CFfi::default(),
            comptime_inputs: Vec::new(),
            name_ledger: jet_foundation::Names::NameLedger::default(),
            layer_ceiling: None,
            inferred_layer: crate::Syntax::RuntimeLayer::Core,
            web_partitions: HashMap::new(),
            web_partition_enforced: false,
            web_partition_report: None,
            dep_roots: HashMap::new(),
            package_guarantees: Default::default(),
            program_allocator: Default::default(),
            active_os: crate::Syntax::OSTarget::host(),
            build_facts: Default::default(),
            edition: "2027".to_string(),
        }
    }

    #[test]
    fn ordinary_units_prelude_supplies_standard_axes_and_provenance() {
        let mut bundle = incremental_bundle(
            "#UnitFamily(Token, dimension, base: token) { token }\n\
             #UnitFamily(TokenRate, dimension: Token / Time, base: token_per_second) { token_per_second }\n\
             fn touch_mass() { _ :: 1dalton }\n",
        );
        assert!(inject_units_prelude(&mut bundle).is_empty());
        assert!(resolve_unit_dimensions(&mut bundle).is_empty());

        let families = bundle.modules[0]
            .items
            .iter()
            .filter_map(|item| match item {
                Item::UnitFamily(family) => Some(family),
                _ => None,
            })
            .collect::<Vec<_>>();
        let token = families
            .iter()
            .find(|family| family.family == "Token")
            .unwrap();
        let time = families
            .iter()
            .find(|family| family.family == "Time")
            .unwrap();
        let rate = families
            .iter()
            .find(|family| family.family == "TokenRate")
            .unwrap();
        assert_eq!(
            rate.resolved_dimension,
            token
                .resolved_dimension
                .as_ref()
                .unwrap()
                .divide(time.resolved_dimension.as_ref().unwrap())
        );

        let mass = families
            .iter()
            .find(|family| family.family == "Mass")
            .unwrap();
        let dalton = mass
            .members
            .iter()
            .find(|member| member.name == "dalton")
            .unwrap();
        assert!(matches!(
            dalton.scale_provenance,
            crate::AST::UnitScaleProvenance::Measured { .. }
        ));
    }

    #[test]
    fn imported_public_dimension_participates_in_derived_claims() {
        let mut bundle = incremental_bundle(
            "use dep\n\
             #UnitFamily(InventoryRate, dimension: Inventory / Time, base: item_per_second) { item_per_second }\n",
        );
        let import_span = bundle.modules[0].imports[0].span;
        let mut dependency =
            incremental_bundle("pub #UnitFamily(Inventory, dimension, base: item) { item }\n")
                .modules
                .remove(0);
        dependency.path = "deps/dep.jet".into();
        dependency.display = "deps/dep.jet".to_string();
        dependency.alias = "dep".to_string();
        bundle.modules.push(dependency);
        bundle.name_ledger.record_import_target(0, import_span, 1);

        assert!(inject_units_prelude(&mut bundle).is_empty());
        assert!(resolve_unit_dimensions(&mut bundle).is_empty());
        let dimension = |module: usize, family: &str| {
            bundle.modules[module]
                .items
                .iter()
                .find_map(|item| match item {
                    Item::UnitFamily(definition) if definition.family == family => {
                        definition.resolved_dimension.clone()
                    }
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(
            dimension(0, "InventoryRate"),
            dimension(1, "Inventory")
                .divide(&dimension(0, "Time"))
                .unwrap()
        );
    }

    #[test]
    fn incremental_dependencies_include_nested_module_imports() {
        let mut bundle = incremental_bundle(
            "module api {\n    use dep.[value]\n    pub fn call() Int -> { return value() }\n}\n",
        );
        let mut dependency = incremental_bundle("pub fn value() Int -> { return 1 }\n")
            .modules
            .remove(0);
        dependency.path = "dep.jet".into();
        dependency.display = "dep.jet".to_string();
        dependency.alias = "dep".to_string();
        bundle.modules.push(dependency);

        let dependencies = incremental_module_dependencies(&bundle);
        assert_eq!(dependencies["cache-accounting.jet"], vec!["dep.jet"]);
    }

    #[test]
    fn incremental_dependencies_match_directory_module_imports() {
        let mut bundle = incremental_bundle("use archive\nfn run() Int -> { return archive.value() }\n");
        let mut dependency = incremental_bundle("pub fn value() Int -> { return 1 }\n")
            .modules
            .remove(0);
        dependency.path = "archive/run.jet".into();
        dependency.display = "archive/run.jet".to_string();
        dependency.alias = "run".to_string();
        bundle.modules.push(dependency);

        let dependencies = incremental_module_dependencies(&bundle);
        assert_eq!(dependencies["cache-accounting.jet"], vec!["archive/run.jet"]);
    }

    #[test]
    fn pending_diagnostics_have_exact_retained_byte_cost() {
        let mut bundle = incremental_bundle("fn run() {}\n");
        let mut cache = IncrementalSemaCache::new();
        let (diagnostics, _) =
            check_bundle_with_effect_facts_incremental(&mut bundle, CompileMode::Check, &mut cache);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(cache.functions.len(), 1);

        let before = cache.stats().live_item_bytes;
        let pending = PendingFunctionDiagnostic {
            function_key: "run".to_string(),
            function_span: Span::new(0, 11),
            diagnostic: Diagnostic::error(
                "E2702",
                "crypto API misuse".to_string(),
                "known nonce length".to_string(),
                "pass the exact nonce length".to_string(),
                Some(Span::new(3, 6)),
            ),
        };
        let expected_delta = format!("{:?}", vec![pending.clone()]).len()
            - format!("{:?}", Vec::<PendingFunctionDiagnostic>::new()).len();
        cache
            .functions
            .values_mut()
            .next()
            .unwrap()
            .pending_diagnostics
            .push(pending);

        assert_eq!(cache.stats().live_item_bytes - before, expected_delta);
    }

    #[test]
    fn bundle_stays_split_without_reordering_passes() {
        const MAX_MODULE_LINES: usize = 2500;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |relative: &str| std::fs::read_to_string(root.join(relative)).unwrap();
        let bundle = read("src/Sema/Bundle.rs");
        let comptime = read("src/Sema/Bundle/Comptime.rs");
        let units = read("src/Sema/Bundle/Units.rs");
        let generic = read("src/Sema/Bundle/GenericModules.rs");
        let substitution = read("src/Sema/Bundle/GenericModules/Substitution.rs");
        let outputs = read("src/Sema/Bundle/Outputs.rs");
        let inline_calls = read("src/Sema/Bundle/InlineCalls.rs");
        let pipeline = read("src/Sema/Bundle/Pipeline.rs");
        let inline_imports = read("src/Sema/Bundle/Pipeline/InlineImports.rs");
        let completion = read("src/Sema/Bundle/Pipeline/Completion.rs");
        let validation = read("src/Sema/Bundle/Validation.rs");
        let core_usage = read("src/Sema/Bundle/Validation/CoreUsage.rs");
        let production = bundle
            .split("#[cfg(test)]\nmod structure_tests")
            .next()
            .unwrap();
        for (relative, source) in [
            ("src/Sema/Bundle.rs", production),
            ("src/Sema/Bundle/Comptime.rs", comptime.as_str()),
            ("src/Sema/Bundle/Units.rs", units.as_str()),
            ("src/Sema/Bundle/GenericModules.rs", generic.as_str()),
            (
                "src/Sema/Bundle/GenericModules/Substitution.rs",
                substitution.as_str(),
            ),
            ("src/Sema/Bundle/InlineCalls.rs", inline_calls.as_str()),
            ("src/Sema/Bundle/Outputs.rs", outputs.as_str()),
            ("src/Sema/Bundle/Pipeline.rs", pipeline.as_str()),
            (
                "src/Sema/Bundle/Pipeline/InlineImports.rs",
                inline_imports.as_str(),
            ),
            (
                "src/Sema/Bundle/Pipeline/Completion.rs",
                completion.as_str(),
            ),
            ("src/Sema/Bundle/Validation.rs", validation.as_str()),
            (
                "src/Sema/Bundle/Validation/CoreUsage.rs",
                core_usage.as_str(),
            ),
        ] {
            assert!(
                source.lines().count() < MAX_MODULE_LINES,
                "{relative} must stay below the card #510 module boundary"
            );
            assert!(!source.contains("include!("));
            assert!(!source.contains("#[path"));
        }
        assert!(production.contains(
            "\nmod GenericModules;\nmod InlineCalls;\nmod Outputs;\nmod Pipeline;\nmod Validation;\n"
        ));
        assert!(generic.contains("\nmod Substitution;\n"));
        assert!(validation.contains("\nmod CoreUsage;\n"));

        let ordered = [
            "expand_generic_module_aliases(bundle, &mut diags);",
            "mangle_inline_sibling_calls(bundle);",
            "super::super::Registration::expand_builtin_derive_items(&mut module.items, &mut diags);",
            "super::super::Registration::expand_builtin_serde_items(&mut module.items, &mut diags);",
            "register_type_methods(&module.items, &mut st.registry, &mut diags);",
            "register_impl_methods(&module.items, &mut st.registry, &mut diags);",
            "let mut module_diags = check_module_bodies(",
            "collect_used_core(bundle, &states)",
            "apply_helper_layer_inference(bundle, &states, &usage_spans, &mut diags);",
        ];
        let ordered_source = format!("{production}\n{pipeline}\n{inline_imports}\n{completion}");
        let positions: Vec<usize> = ordered
            .iter()
            .map(|needle| ordered_source.find(needle).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
