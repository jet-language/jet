use super::*;
use crate::Diagnostics::{Diagnostic, TextEdit};
use crate::Syntax;
use crate::Traits::TraitRegistry;
use crate::AST::{
    AccessConvention, CodeModule, ConstAttr, EnumDef, EnumLitArg, Expr, ForKind, Func,
    GenericModuleDef, GenericModuleParam, ImportKind, Item, LValue, LambdaBody, ModuleAliasDef,
    ModuleArg, OrFallback, Param, ParamZone, Pattern, ProgramBundle, RustConstKind, Stmt, StrPart,
    SwitchArm, Type, VariantPayload,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod Comptime;
mod Units;

mod GenericModules;
mod InlineCalls;
mod Outputs;
mod Pipeline;
mod Validation;

pub use Comptime::bundle_has_comptime_evaluation;
use Comptime::stmts_have_comptime_evaluation;
use Units::{inject_units_prelude, resolve_unit_dimensions};

pub(crate) use InlineCalls::{mangle_inline_sibling_calls, rewrite_inline_calls_stmts};

pub(crate) use GenericModules::expand_generic_module_aliases;
pub use GenericModules::specialize_function_types;
use GenericModules::{clone_enum, clone_struct};
use Outputs::{
    cli_entry_param_shape, is_fallible_void_entry_return, no_run_error, resolve_outputs,
    CLIEntryShape,
};
use Pipeline::{
    check_bundle_opts_for_output as pipeline_check_bundle_opts_for_output,
    check_bundle_opts_for_output_with_context as pipeline_check_bundle_opts_for_output_with_context,
};
#[allow(unused_imports)]
pub(crate) use Validation::{
    check_func_body_bundle, check_module_bodies, collect_core_expr, collect_core_lvalue,
    collect_core_stmts, collect_used_core, expand_core_reachable_closure, fn_types_compatible,
    func_sig_to_fn_type, register_func_item,
};
use Validation::{
    apply_helper_layer_inference, qualified_effect_facts, taint_check_item,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IncrementalSemaStats {
    pub hits: u64,
    pub recomputes: u64,
    pub live_items: usize,
    pub live_item_bytes: usize,
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
        let Some(span) = diagnostic.span else { continue };
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
    let mut seen = HashSet::new();
    diagnostics.retain(|diagnostic| {
        diagnostic.code != "E0107"
            || diagnostic
                .span
                .is_none_or(|span| seen.insert((span.start, span.end)))
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
        seen.insert((diagnostic.code.clone(), diagnostic.what.clone(), span.start, span.end))
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
        super::Taint::collect_function_paths(
            &module.alias,
            &module.items,
            &mut known_sources,
        );
    }
    for module in &bundle.modules {
        super::Taint::collect_field_facts(
            &module.items,
            &mut field_tags,
            &mut field_types,
        );
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
        for module in &bundle.modules {
            state_table.validate_declarations(&module.items, diags);
            crate::Sema::check_items_state(&module.items, &state_table, diags);
        }
    }
    state_table.into_facts()
}

fn register_effect_facts(
    bundle: &ProgramBundle,
    facts: &mut jet_foundation::Facts::FactRegistry,
) {
    use jet_foundation::Facts::FactKind;
    for effect in jet_foundation::Facts::EFFECT_ROOTS.iter() {
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
                Item::Bench(bench) => check_stmts(&bench.body, facts, diags),
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
    functions: HashMap<String, CachedFunctionBody>,
    hits: u64,
    recomputes: u64,
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
        }
    }

    pub fn clear(&mut self) {
        self.environment.clear();
        self.functions.clear();
    }

    fn begin_bundle(&mut self, bundle: &ProgramBundle) {
        let environment = incremental_environment(bundle);
        if self.environment != environment {
            self.environment = environment;
            self.functions.clear();
        }
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
        self.recomputes += 1;
        self.functions.insert(key, entry);
    }
}

fn incremental_environment(bundle: &ProgramBundle) -> Vec<u8> {
    let mut out = crate::CanonicalAST::canonical_fragment(&(
        bundle.entry,
        &bundle.project_root,
        bundle.active_os,
        bundle.layer_ceiling,
    ));
    out.extend(format!("{:?}", bundle.project_root).into_bytes());
    for module in &bundle.modules {
        let metadata = (
            &module.path,
            &module.display,
            &module.alias,
            &module.imports,
            module.web_target_ceiling,
            module.pub_file,
            module.no_prelude,
            &module.html_path,
            &module.policy_declarations,
        );
        out.extend(crate::CanonicalAST::canonical_fragment(&metadata));
        out.extend(format!("{metadata:?}").into_bytes());
        for item in &module.items {
            let mut item = item.clone();
            clear_callable_bodies(&mut item);
            out.extend(crate::CanonicalAST::canonical_fragment(&item));
            // Canonical AST deliberately omits locations. The exact signature
            // locations are part of IDE facts, so include the body-free Debug
            // form as a conservative span fingerprint.
            out.extend(format!("{item:?}").into_bytes());
        }
    }
    out
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
    let variants = ["Less", "Equal", "Greater"].into_iter().map(|name| {
        (name.to_string(), (zero, VariantPayload::Unit))
    }).collect::<HashMap<_, _>>();
    let mut types = HashMap::new();
    types.insert(Syntax::TYPE_ORDERING.to_string(), TypeDef::Enum {
        variants, variant_order: vec!["Less".to_string(), "Equal".to_string(), "Greater".to_string()],
        groups: HashMap::new(), methods: HashMap::new(), single_use: false,
        must_use: false, c_layout_tag: None,
    });
    let remove_by_variants = ["Val", "Slot"].into_iter().map(|name| {
        (name.to_string(), (zero, VariantPayload::Unit))
    }).collect::<HashMap<_, _>>();
    types.insert(crate::Syntax::TYPE_REMOVE_BY.to_string(), TypeDef::Enum {
        variants: remove_by_variants,
        variant_order: vec!["Val".to_string(), "Slot".to_string()],
        groups: HashMap::new(),
        methods: HashMap::new(),
        single_use: false,
        must_use: false,
        c_layout_tag: None,
    });
    TypeRegistry {
        types,
        unit_types: HashSet::new(),
        unit_facts: HashMap::new(),
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


fn package_scope_for(path: &Path, project_root: &Path) -> PathBuf {
    let norm_path = normalize_sem_path(path);
    let norm_root = normalize_sem_path(project_root);
    if norm_path.starts_with(&norm_root) {
        return norm_root;
    }
    norm_path
        .parent()
        .map(normalize_sem_path)
        .unwrap_or(norm_path)
}

fn normalize_sem_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
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
    prefix.map_or_else(|| name.to_string(), |prefix| jet_foundation::Names::member_name(prefix, name))
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
            jet_foundation::Names::NameVisibility::from_flags(
                method.is_pub,
                method.is_package_pub,
            ),
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
            declare_method_names(
                ledger,
                module,
                &item_name,
                &item_path,
                &definition.methods,
            );
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
            let visibility = NameVisibility::from_flags(definition.is_pub, definition.is_package_pub);
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
            declare_method_names(
                ledger,
                module,
                &item_name,
                &item_path,
                &definition.methods,
            );
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
            let visibility = NameVisibility::from_flags(definition.is_pub, definition.is_package_pub);
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
            declare_method_names(
                ledger,
                module,
                &owner,
                &owner_path,
                &implementation.methods,
            );
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
            let visibility = NameVisibility::from_flags(
                code_module.is_pub,
                code_module.is_package_pub,
            );
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
                    declare_item_names_scoped(
                        ledger,
                        module,
                        &item_path,
                        Some(&item_name),
                        nested,
                    );
                }
            }
        }
        Item::StateDecl(state) => {
            let visibility = NameVisibility::from_flags(state.is_pub, state.is_package_pub);
            let state_name = scoped_name(name_prefix, &state.type_name);
            let state_path = scoped_path(path_prefix, &state.type_name);
            declare_name(
                ledger,
                module,
                state_name.clone(),
                state_path.clone(),
                "state",
                state.type_name_span,
                visibility,
            );
            for (name, span) in &state.states {
                declare_name(
                    ledger,
                    module,
                    format!("{state_name}.State.{name}"),
                    format!("{state_path}.State.{name}"),
                    "state",
                    *span,
                    visibility,
                );
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

fn populate_name_ledger(
    bundle: &ProgramBundle,
    states: &[ModuleState],
    ledger: &mut jet_foundation::Names::NameLedger,
) {
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        let package = package_scope_for(&module.path, &bundle.project_root)
            .to_string_lossy()
            .into_owned();
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
        }
        for import in &module.imports {
            let import_alias = import.import_alias();
            let alias_visibility = jet_foundation::Names::NameVisibility::from_flags(
                import.is_pub,
                import.is_package_pub,
            );
            let (import_target, target_module) = if let Some(target) =
                ledger.import_target(module_idx, import.span)
            {
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

            let ImportKind::Unqualified {
                module_alias,
                items,
                ..
            } = &import.kind
            else {
                continue;
            };
            for (original, local_alias) in items {
                let local = crate::AST::import_item_alias(original, local_alias.as_deref());
                let target = if let Some(core_prefix) = crate::AST::core_list_prefix(module_alias) {
                    Some((format!("{core_prefix}.{original}"), None))
                } else if let Some((real, target_module)) = state.unqualified_file.get(local) {
                    Some((
                        format!("{}.{}", bundle.modules[*target_module].alias, real),
                        Some(*target_module),
                    ))
                } else if let Some(resolved) = state.unqualified.get(local) {
                    Some((resolved.clone(), Some(module_idx)))
                } else if let Some(target_module) = state.imports.get(module_alias) {
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
                        import.alias_span,
                        alias_visibility,
                    );
                }
            }
        }
    }
}

/// D-MOD2: inside an inline `module math { … }`, a call to a sibling function
/// `helper(x)` must lower to the mangled `math__helper`. This pre-pass rewrites

pub fn check_bundle(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    pipeline_check_bundle_opts_for_output(bundle, mode, false, false, None, None).0
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
            diagnostic.edit = script_conflict_edit(&module.source, &body, &run);
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
    if spans
        .windows(2)
        .any(|pair| pair[0].end > pair[1].start)
        || spans
            .iter()
            .any(|span| span.start < run.span.end && span.end > run.span.start)
    {
        return None;
    }

    let open = source
        .get(run.name_span.start..run.span.end)?
        .find('{')?
        + run.name_span.start;
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
            } else if string_delimiter == 3
                && bytes.get(index..index + 3) == Some(b"\"\"\"")
            {
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
    pipeline_check_bundle_opts_for_output(bundle, mode, false, false, Some(output), None).0
}

pub fn check_bundle_for_output_opts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    output: &str,
    freestanding: bool,
    allow_impure: bool,
) -> Vec<Diagnostic> {
    pipeline_check_bundle_opts_for_output(
        bundle,
        mode,
        freestanding,
        allow_impure,
        Some(output),
        None,
    )
    .0
}

/// Like `check_bundle` but also returns effect facts for D-SEMINDEX1.
pub fn check_bundle_with_effect_facts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    pipeline_check_bundle_opts_for_output(bundle, mode, false, false, None, None)
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
        false,
        None,
        None,
        true,
    )
}

pub fn check_bundle_with_effect_facts_incremental(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    cache: &mut IncrementalSemaCache,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    pipeline_check_bundle_opts_for_output(bundle, mode, false, false, None, Some(cache))
}

/// Like `check_bundle` but with extra build options (E2-M15).
pub fn check_bundle_freestanding(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    pipeline_check_bundle_opts_for_output(bundle, mode, true, false, None, None).0
}

/// Like `check_bundle` but with D-CTEFFECT1 `--allow-impure` flag.
pub fn check_bundle_allow_impure(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    pipeline_check_bundle_opts_for_output(bundle, mode, false, true, None, None).0
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
        assert!(fixed_lexer_diagnostics.is_empty(), "{fixed_lexer_diagnostics:?}");
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
                html_path: program.html_path,
                no_alloc_policy: program.no_alloc_policy,
                policy_declarations: program.policy_declarations,
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
            active_os: crate::Syntax::OSTarget::host(),
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
        let token = families.iter().find(|family| family.family == "Token").unwrap();
        let time = families.iter().find(|family| family.family == "Time").unwrap();
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

        let mass = families.iter().find(|family| family.family == "Mass").unwrap();
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
    fn pending_diagnostics_have_exact_retained_byte_cost() {
        let mut bundle = incremental_bundle("fn run() {}\n");
        let mut cache = IncrementalSemaCache::new();
        let (diagnostics, _) = check_bundle_with_effect_facts_incremental(
            &mut bundle,
            CompileMode::Check,
            &mut cache,
        );
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
            ("src/Sema/Bundle/Validation.rs", validation.as_str()),
            ("src/Sema/Bundle/Validation/CoreUsage.rs", core_usage.as_str()),
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
        let ordered_source = format!("{production}\n{pipeline}");
        let positions: Vec<usize> = ordered
            .iter()
            .map(|needle| ordered_source.find(needle).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
