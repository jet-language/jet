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
    check_bundle_opts_for_output(bundle, mode, false, false, None, None).0
}

/// Prepare script entries for entry-swap callers (`jet dev` and named tasks)
/// before they install their ordinary forwarding `run` wrapper.
pub fn prepare_script_entries(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    materialize_script_entries(bundle, &mut diags);
    diags
}

/// D-ENTRY-SCRIPT1=B: keep the script surface in the parser/formatter, then
/// lower only the entry file's loose statements to the ordinary `run` path.
/// Imported scripts are rejected before registration so their statements can
/// never become an accidental runtime side effect.
fn materialize_script_entries(bundle: &mut ProgramBundle, diags: &mut Vec<Diagnostic>) {
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
    let statement_ranges = body
        .iter()
        .map(|stmt| statement_line_range(source, stmt.span()))
        .collect::<Vec<_>>();
    let mut ranges = statement_ranges.clone();
    ranges.sort_by_key(|(start, _)| *start);
    ranges.dedup();
    if ranges.len() != statement_ranges.len() {
        return None;
    }
    if ranges
        .windows(2)
        .any(|pair| pair[0].1 > pair[1].0)
        || ranges
            .iter()
            .any(|(start, end)| *start < run.span.end && *end > run.span.start)
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
        .filter(|stmt| stmt.span().start < run.span.start)
        .map(|stmt| source_slice_for_statement(source, stmt.span()))
        .collect::<Vec<_>>()
        .join("");
    let after = body
        .iter()
        .filter(|stmt| stmt.span().start > run.span.end)
        .map(|stmt| source_slice_for_statement(source, stmt.span()))
        .collect::<Vec<_>>()
        .join("");
    let before_insert = (!before.is_empty()).then(|| {
        let text = indent_script(&before);
        if source.as_bytes().get(open + 1) == Some(&b'\n') {
            text
        } else {
            format!("\n{text}")
        }
    });
    let after_insert = (!after.is_empty()).then(|| {
        let text = indent_script(&after);
        if source.as_bytes().get(close.saturating_sub(1)) == Some(&b'\n') {
            text
        } else {
            format!("\n{text}")
        }
    });

    let mut edits = ranges
        .into_iter()
        .map(|(start, end)| (start, end, String::new()))
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

fn statement_line_range(source: &str, span: Span) -> (usize, usize) {
    let start = source[..span.start.min(source.len())]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let end = source[span.end.min(source.len())..]
        .find('\n')
        .map_or(source.len(), |index| span.end.min(source.len()) + index + 1);
    (start, end)
}

fn source_slice_for_statement(source: &str, span: Span) -> &str {
    let (start, end) = statement_line_range(source, span);
    &source[start..end]
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
    check_bundle_opts_for_output(bundle, mode, false, false, Some(output), None).0
}

pub fn check_bundle_for_output_opts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    output: &str,
    freestanding: bool,
    allow_impure: bool,
) -> Vec<Diagnostic> {
    check_bundle_opts_for_output(
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
    check_bundle_opts_for_output(bundle, mode, false, false, None, None)
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
    check_bundle_opts_for_output_with_context(
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
    check_bundle_opts_for_output(bundle, mode, false, false, None, Some(cache))
}

/// Like `check_bundle` but with extra build options (E2-M15).
pub fn check_bundle_freestanding(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts_for_output(bundle, mode, true, false, None, None).0
}

/// Like `check_bundle` but with D-CTEFFECT1 `--allow-impure` flag.
pub fn check_bundle_allow_impure(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts_for_output(bundle, mode, false, true, None, None).0
}

fn check_bundle_opts_for_output(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    freestanding: bool,
    allow_impure: bool,
    explicit_output: Option<&str>,
    incremental: Option<&mut IncrementalSemaCache>,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    check_bundle_opts_for_output_with_context(
        bundle,
        mode,
        freestanding,
        allow_impure,
        explicit_output,
        incremental,
        false,
    )
}

fn check_bundle_opts_for_output_with_context(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    freestanding: bool,
    allow_impure: bool,
    explicit_output: Option<&str>,
    incremental: Option<&mut IncrementalSemaCache>,
    allow_compiler_api: bool,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    let edition = bundle.edition.clone();
    super::Edition::with_package_edition(&edition, || {
        check_bundle_opts_for_output_inner(
            bundle,
            mode,
            freestanding,
            allow_impure,
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
    allow_impure: bool,
    explicit_output: Option<&str>,
    mut incremental: Option<&mut IncrementalSemaCache>,
    allow_compiler_api: bool,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    let mut diags = Vec::new();
    diags.extend(prepare_script_entries(bundle));
    diags.extend(inject_units_prelude(bundle));
    super::Prelude::inject(bundle);
    diags.extend(super::Casing::validate_bundle(bundle));
    diags.extend(resolve_unit_dimensions(bundle));
    // D-OSTARGET2=B (ratified 2026-07-03): fold every `$if build.os == {
    // … }` switch to the arm matching this build's active OS *before* any other
    // pass sees a body — so OS-gating checks, the type-checker, and codegen only
    // meet the taken arm. Rewrites into a `$if` chain (reuses D-WHEN1).
    diags.extend(super::desugar_os_switches(bundle));
    // D-MIGRATE4: desugar each `change … via { (old) => … }` converter on a
    // decodable `#PublishedSchema` type into a synthetic top-level converter
    // function, so the runtime migration step (codegen) can call it. Runs before
    // registration/checking so those synthetic functions are type-checked and
    // lowered through the normal pipeline. Sets `conv_fn` on the `change` op.
    super::desugar_migrations(bundle);
    // D-SPREAD1=A: expand `prefix.[a, b]` to field lists (spliced in list
    // position) before inference sees bodies.
    super::desugar_member_spreads(bundle);
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
    diags.extend(super::UnsafeObligations::check_and_strip(bundle));
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

    // D-MARK-VOCAB1 (card #518): the dynamic half of the `#Rule` vocabulary
    // vocabulary — every `derive T.Name { … }` provider in the bundle, not
    // just this module's own, per the same bundle-wide orphan-rule view as
    // `derive_providers` above.
    let marker_vocabulary = jet_foundation::Policy::MarkerVocabulary::with_derives(
        derive_providers.iter().map(|(_, name, _, _, _)| name.clone()),
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
                    items,
                    ..
                } = &import.kind
                else {
                    continue;
                };
                let Some(core_prefix) = crate::AST::core_list_prefix(module_alias) else {
                    continue;
                };
                for (original, alias) in items {
                    let local = crate::AST::import_item_alias(original, alias.as_deref());
                    let full = format!("{core_prefix}.{original}");
                    if crate::Syntax::is_known_core_module(&full) {
                        imports.insert(local.to_string(), full);
                    }
                }
            }
            imports
        })
        .collect();
    let mut top_level_embed_inputs = Vec::new();

    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        super::Protocol::expand_module_protocols(&mut module.items, &mut diags);
        // D-DOTSCOPE1: validate contextual `.member { … }` scope statements
        // against each marker's declared vocabulary (E0614/E0615/E0616/E0617/E0618).
        diags.extend(super::ScopeMembers::check(&module.items));
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
        super::CheckerMarkers::resolve_static_rule_products(
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
        for item in &module.items {
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
                        // (`math__double`) so call-site sema can check them.
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
                // D-META-NAME1/FORM1: registration is #1457's/#1458's job; no
                // registry row for #1456's declaration-side parse.
                Item::EffectDecl(_)
                | Item::MarkerDecl(_)
                | Item::FactDecl(_)
                | Item::GenericModule(_)
                | Item::ModuleAlias(_) => {}
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
                        let type_info =
                            crate::Comptime::build_struct_type_info_with_states(s, &states);

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
                                            super::Registration::parse_generated_fragment(
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
                                diags.push(Diagnostic::error(
                                    "E2710",
                                    format!(
                                        "`derive T.{}` body failed while expanding `#{}` on `{}`",
                                        derive_name, derive_name, s.name
                                    ),
                                    why,
                                    fix,
                                    Some(*derive_span),
                                ));
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

        st.consts.extend(comptime_types);
        // D-ONCE-DERIVE1=A / I3: built-in capability requests re-enter as
        // ordinary Jet impl blocks before the final trait registration pass.
        super::Registration::expand_builtin_derive_items(&mut module.items, &mut diags);
        // D-SERDE2=A/R11: built-in codecs re-enter as ordinary Jet source in
        // bundle builds too; this is the production multi-file path.
        super::Registration::expand_builtin_serde_items(&mut module.items, &mut diags);

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
        diags.extend(check_marker_vocabulary(&module.items, &marker_vocabulary));
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
    let bundle_auto_derives = TraitRegistry::bundle_auto_derives(bundle, &name_ledger);
    for (state, auto_derives) in states.iter_mut().zip(&bundle_auto_derives) {
        state.trait_reg.merge_auto_derives(auto_derives);
    }
    bundle.comptime_inputs.extend(top_level_embed_inputs);
    diags.extend(super::BudgetSpecs::validate_bundle(bundle));

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
            // Unqualified imports are handled in the dedicated pass below.
            if matches!(&imp.kind, ImportKind::Unqualified { .. }) {
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
            // S59 (E2-M14): C `use` forms bind to a synthetic merged module
            // resolved by `CFFI::assemble` (E3204 already reported there).
            if imp.is_c_import() {
                if let Some(target) = bundle.cffi.target_for(idx, &alias) {
                    st.imports.insert(alias, target);
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
                items,
                ..
            } = &imp.kind
            else {
                continue;
            };
            let st = &mut states[idx];
            if let Some(canonical) = st.code_modules.get(module_alias.as_str()) {
                // Inline module: items are mangled as `{alias}__{item}`.
                for (orig, alias_opt) in items {
                    let local = crate::AST::import_item_alias(orig, alias_opt.as_deref());
                    let mangled = jet_foundation::Names::member_name(canonical, orig);
                    if !st.funcs.contains_key(&mangled) {
                        diags.push(Diagnostic::error(
                            "E0611",
                            format!("`{}` is not defined in module `{}`", orig, module_alias),
                            "check the module body for the item you're importing".to_string(),
                            "make sure the name is spelled correctly".to_string(),
                            Some(*module_alias_span),
                        ));
                    } else if !name_ledger.exported(idx, &mangled)
                    {
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
                        st.unqualified.insert(local.to_string(), mangled.clone());
                        if imp.is_pub {
                            st.reexports.insert(local.to_string(), (mangled, idx));
                        }
                    }
                }
            } else if let Some(core_prefix) = crate::AST::core_list_prefix(module_alias) {
                // D-CORE-USELIST1=A: a list member may name either a Core
                // submodule (`core.encoding.[json]`) or an item in the
                // longest known module prefix (`core.math.[abs]`).
                let st = &mut states[idx];
                for (orig, alias_opt) in items {
                    let local = crate::AST::import_item_alias(orig, alias_opt.as_deref());
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
                    if st.core_imports.contains_key(local) {
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
                        st.core_imports.insert(local.to_string(), module);
                        if let Some(item) = item {
                            st.core_item_imports.insert(local.to_string(), item);
                        }
                    }
                }
            } else if st.imports.contains_key(module_alias.as_str()) {
                // File module: look up items in the target module's state.
                let target_idx = st.imports[module_alias.as_str()];
                let is_reexport = imp.is_pub;
                for (orig, alias_opt) in items {
                    let local = crate::AST::import_item_alias(orig, alias_opt.as_deref());
                    let is_pub = name_ledger.visible(idx, target_idx, orig);
                    let file_module_target = states[target_idx]
                        .imports
                        .get(orig.as_str())
                        .copied()
                        .filter(|_| {
                            name_ledger
                                .declaration(target_idx, orig)
                                .is_some_and(|declaration| declaration.kind == "file_module")
                        });
                    if let Some(file_module_target) = file_module_target {
                        if !is_pub {
                            diags.push(Diagnostic::error(
                                "E0609",
                                format!("`{}` is private in module `{}`", orig, module_alias),
                                "only public modules can be brought into scope with `use`".to_string(),
                                format!("add `pub` before `module {}` in the imported file", orig),
                                Some(*module_alias_span),
                            ));
                        } else {
                            states[idx]
                                .imports
                                .insert(local.to_string(), file_module_target);
                        }
                        continue;
                    }
                    let exists = states[target_idx].funcs.contains_key(orig.as_str());
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
                            .insert(local.to_string(), (orig.clone(), target_idx));
                        if is_reexport {
                            states[idx]
                                .reexports
                                .insert(local.to_string(), (orig.clone(), target_idx));
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
                    states[idx].inline_core_imports.insert(
                        (inline_name.clone(), imp.import_alias()),
                        module,
                    );
                    continue;
                }
                let ImportKind::Unqualified {
                    module_alias,
                    module_alias_span,
                    items,
                    ..
                } = imp.kind
                else {
                    // Inline bodies inherit file/module aliases; a second
                    // module-loading declaration has no loader target.
                    continue;
                };
                for (orig, alias_opt) in items {
                    let local = crate::AST::import_item_alias(&orig, alias_opt.as_deref()).to_string();
                    enum Target {
                        Inline { alias: String, mangled: String },
                        File { name: String, module_idx: usize },
                        Core { module: String, item: Option<String> },
                    }
                    let resolved = {
                        let st = &states[idx];
                        if let Some(canonical) = st.code_modules.get(&module_alias) {
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
                                        alias: module_alias.clone(),
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
                        } else if let Some(&target_idx) = st.imports.get(&module_alias) {
                            let target = &states[target_idx];
                            let visible = name_ledger.visible(idx, target_idx, &orig);
                            if !target.funcs.contains_key(&orig) {
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
                                    name: orig.clone(),
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
                            if imp.is_pub {
                                st.inline_reexport_inline.insert(
                                    (inline_name.clone(), local),
                                    (alias, mangled),
                                );
                            }
                        }
                        Target::File { name, module_idx } => {
                            st.inline_unqualified_file.insert(
                                (inline_name.clone(), local.clone()),
                                (name.clone(), module_idx),
                            );
                            if imp.is_pub {
                                st.inline_reexport_file
                                    .insert((inline_name.clone(), local), (name, module_idx));
                            }
                        }
                        Target::Core { module, item } => {
                            let key = (inline_name.clone(), local.clone());
                            st.inline_core_imports
                                .insert(key.clone(), module.clone());
                            if let Some(item) = item {
                                st.inline_core_items
                                    .insert(key.clone(), item.clone());
                                if imp.is_pub {
                                    st.inline_reexport_core
                                        .insert(key, (module, item));
                                }
                            }
                        }
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

    let entry = &states[bundle.entry];
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
            // S12/D-S80-RUN1/D-CLIFLAG1: `run` is the only program entry name.
            // It is zero-arg (optionally `-> () ?`), or one typed CLI-spec
            // parameter (`#[CLI]` struct / enum).
            if run_fn.params.is_empty() {
                if mode == CompileMode::Run
                    && run_fn
                        .return_type
                        .as_ref()
                        .is_some_and(|ret| !is_fallible_void_entry_return(ret, entry))
                {
                    diags.push(Diagnostic::error(
                        "E0122",
                        "`run` returns the wrong kind of value".to_string(),
                        "`run` is where running starts; it either returns nothing or reports top-level errors with `() ?`"
                            .to_string(),
                        "write `fn run() { ... }`, or `fn run() => () ? { ... }` if the entry uses `?`"
                            .to_string(),
                        Some(run_fn.name_span),
                    ));
                }
            } else if run_fn.params.len() == 1 {
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
            } else {
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
            allow_impure,
            &mut local_summaries,
            &mut embed_inputs,
            &mut global_addr_taken,
            &mut name_ledger,
            &mut local_pending_diagnostics,
            incremental.as_deref_mut(),
        );
        dedupe_unknown_names(&mut module_diags);
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
        super::Taint::collect_return_tag_facts(
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
    super::Effects::check_autodiff_purity(
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
        super::Effects::check_inferred_purity(
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
    for pending in module_pending_diagnostics.into_iter().flatten() {
        if !failed_diagnostic_phases.contains(&pending.function_key) {
            diags.push(pending.diagnostic);
        }
    }

    // D-WASM1=A (c123 M1): JS/WASM partition inference and boundary checks.
    // D-MEM-FACTS1: module `#Policy(no_alloc)` declarations are checked only
    // after the same qualified, dependency-complete graph is projected.
    // #657 feeds the other scope levels and the two remaining fact values into
    // this declaration surface; reachability itself stays single-mechanism.
    let (memory_summaries, memory_declarations) =
        super::MemoryFacts::bundle_memory_inputs(bundle, &public_summaries);
    let memory_projections = memory_declarations
        .iter()
        .flat_map(|declaration| {
            declaration.roots.iter().map(|root| {
                (
                    (root.clone(), declaration.fact),
                    super::MemoryFacts::project_memory_fact(
                        declaration.fact,
                        root,
                        &memory_summaries,
                    ),
                )
            })
        })
        .collect::<HashMap<_, _>>();
    diags.extend(super::MemoryFacts::check_memory_facts(
        &memory_declarations,
        &memory_summaries,
    ));
    diags.extend(check_web_partition(
        bundle,
        &public_summaries,
        &public_solved,
    ));

    // D-WEBAPP1=D / D-WEBAUTHOR1=D (Tower #438): one sema-known application graph.
    let (web_app_graph, web_app_diags) = super::WebApp::extract_web_app_graph(bundle);
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
    diags.extend(super::MemoryFacts::annotate_scoped_gc_promotions(bundle));
    apply_helper_layer_inference(bundle, &states, &usage_spans, &mut diags);
    bundle.name_ledger = name_ledger.clone();
    (
        diags,
        super::Effects::SemIndexEffectFacts {
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
        assert!(production.contains("\nmod GenericModules;\nmod InlineCalls;\nmod Outputs;\nmod Validation;\n"));
        assert!(generic.contains("\nmod Substitution;\n"));
        assert!(validation.contains("\nmod CoreUsage;\n"));

        let ordered = [
            "expand_generic_module_aliases(bundle, &mut diags);",
            "mangle_inline_sibling_calls(bundle);",
            "super::Registration::expand_builtin_derive_items(&mut module.items, &mut diags);",
            "super::Registration::expand_builtin_serde_items(&mut module.items, &mut diags);",
            "register_type_methods(&module.items, &mut st.registry, &mut diags);",
            "register_impl_methods(&module.items, &mut st.registry, &mut diags);",
            "let mut module_diags = check_module_bodies(",
            "collect_used_core(bundle, &states)",
            "apply_helper_layer_inference(bundle, &states, &usage_spans, &mut diags);",
        ];
        let positions: Vec<usize> = ordered
            .iter()
            .map(|needle| production.find(needle).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
