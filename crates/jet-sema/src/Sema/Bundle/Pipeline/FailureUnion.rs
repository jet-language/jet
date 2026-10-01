//! D-FAIL-INFER-UNION1=A / D-ERR-CASES1=A: the failure union of functions
//! that write no failure contract.
//!
//! Before any real body check, each unannotated function that can meet a
//! typed failure is probed on a clone: the checker records every failure
//! that reaches its default `Err` route instead of reporting it. The rows
//! are joined over the call graph (least fixed point), function-owned cases
//! (`Err(.NotFound(n))`) are minted into a closed `#Error enum` named after
//! the function (`find` gives `FindError`), and every function whose set
//! holds a typed member is projected onto `T (A | B)!` with no written span.
//! The real check, every caller and every execution tier then read that one
//! contract; `FailureContract::Inferred` reports its provenance.
//!
//! A set with no typed member keeps the default `Err` route (Core error
//! families still convert into it there); the #3708 solve decides whether
//! that route is empty.

use super::*;
use crate::Sema::{FailureCase, FailureUnionProbe};
use crate::Syntax;
use crate::AST::{Expr, Func, Item, ProgramBundle, Type};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Rounds of the call-graph join before a still-growing row widens to the
/// general `Err` (D-FAIL-INFER-UNION1 rule 5).
const JOIN_ROUND_CAP: usize = 64;

fn module_origin(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> std::sync::Arc<jet_foundation::Diagnostics::DiagnosticOrigin> {
    let module = &bundle.modules[module_idx];
    std::sync::Arc::new(jet_foundation::Diagnostics::DiagnosticOrigin::new(
        module.display.clone(),
        module.path.to_string_lossy().into_owned(),
        module.source.clone(),
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_failure_unions(
    bundle: &mut ProgramBundle,
    states: &mut [ModuleState],
    plugin_interfaces: &PluginInterfaceRegistry,
    devtools_registry: &jet_foundation::AST::DevtoolsRegistry,
    effect_facts: &jet_foundation::Facts::FactRegistry,
    no_os: bool,
    gates: crate::Policy::GateSet,
    name_ledger: &jet_foundation::Names::NameLedger,
    diags: &mut Vec<Diagnostic>,
) {
    let probed = select_probed_functions(bundle);
    if probed.is_empty() {
        return;
    }
    let probes = probe_functions(
        bundle,
        states,
        &probed,
        plugin_interfaces,
        devtools_registry,
        effect_facts,
        no_os,
        gates,
        name_ledger,
    );
    let minted = mint_case_types(bundle, states, &probes, diags);
    let solved = join_failure_rows(&probes, &minted);
    for ((module_idx, name), error) in solved {
        project_inferred_contract(bundle, states, module_idx, &name, error);
    }
}

/// A written contract whose error side is a typed failure a caller could
/// match (not the general `Err`, a message, `Never`, or a Core error family
/// that converts into `Err`).
fn typed_failure(ty: &Type) -> bool {
    match ty {
        Type::Union(members) => members.iter().any(typed_failure),
        Type::Named(name) | Type::Apply { name, .. } => {
            name != Syntax::TYPE_ERR
                && name != Syntax::TYPE_NEVER
                && !crate::Sema::Diagnostics::is_core_error_family_type(name)
        }
        Type::String => false,
        _ => true,
    }
}

/// The declaration facts a minted error type copies from its function.
struct ProbeOwner {
    span: crate::Diagnostics::Span,
    name_span: crate::Diagnostics::Span,
    is_pub: bool,
    is_package_pub: bool,
}

fn written_typed_failure(function: &Func) -> bool {
    matches!(
        function.failure_contract(),
        crate::AST::FailureContract::Explicit { error, .. } if typed_failure(&error)
    )
}

fn last_segment(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// The unannotated functions worth probing: those that may meet a typed
/// failure (an `Err(value)` that is not a message, or a call to a name whose
/// contract, written or seeded, is typed) plus every unannotated function
/// they call, whose own failures join their rows. Names are matched by their
/// last segment, which over-approximates the call graph and never misses an
/// edge.
/// The walks only read the tree; the mutable visitor avoids cloning bodies.
fn select_probed_functions(bundle: &mut ProgramBundle) -> BTreeMap<usize, BTreeSet<String>> {
    let mut typed_names: HashSet<String> = HashSet::new();
    for module in &bundle.modules {
        for item in &module.items {
            let methods: &[Func] = match item {
                Item::Func(function) => std::slice::from_ref(function),
                Item::Struct(definition) => &definition.methods,
                Item::Enum(definition) => &definition.methods,
                Item::Impl(implementation) => &implementation.methods,
                _ => &[],
            };
            for function in methods {
                if written_typed_failure(function) {
                    typed_names.insert(function.name.clone());
                }
            }
        }
    }
    struct Candidate {
        module_idx: usize,
        name: String,
        calls: BTreeSet<String>,
        seeded: bool,
    }
    let mut candidates = Vec::new();
    for (module_idx, module) in bundle.modules.iter_mut().enumerate() {
        for item in &mut module.items {
            let Item::Func(function) = item else {
                continue;
            };
            if function.name.contains('.')
                || !crate::Sema::failure_inference_candidate(function, false)
            {
                continue;
            }
            let mut calls = BTreeSet::new();
            let mut seeded = false;
            for stmt in &mut function.body {
                stmt.for_each_expr_mut(|expr| match expr {
                    Expr::Err(inner, _) => {
                        seeded |= !matches!(inner.without_parens(), Expr::Str(..));
                    }
                    // The parser writes `Err(value)` as a call; the checker
                    // turns it into `Expr::Err` only while inferring it.
                    Expr::Call(call) if call.name == Syntax::LIT_ERR => {
                        seeded |= !matches!(
                            call.args.as_slice(),
                            [arg] if matches!(arg.expr.without_parens(), Expr::Str(..))
                        );
                    }
                    Expr::Call(call) => {
                        calls.insert(last_segment(&call.name).to_string());
                    }
                    Expr::MethodCall { method, .. } => {
                        calls.insert(method.clone());
                    }
                    _ => {}
                });
            }
            seeded |= calls.iter().any(|call| typed_names.contains(call));
            candidates.push(Candidate {
                module_idx,
                name: function.name.clone(),
                calls,
                seeded,
            });
        }
    }
    // A caller of a seeded function may receive its typed failures.
    loop {
        let seeded_names: HashSet<String> = candidates
            .iter()
            .filter(|candidate| candidate.seeded)
            .map(|candidate| candidate.name.clone())
            .collect();
        let mut changed = false;
        for candidate in &mut candidates {
            if !candidate.seeded && candidate.calls.iter().any(|call| seeded_names.contains(call)) {
                candidate.seeded = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Every unannotated callee of a probed function is probed too, so its
    // general `Err` (or its proven absence) joins the caller's row.
    let mut probed: HashSet<usize> = (0..candidates.len())
        .filter(|&index| candidates[index].seeded)
        .collect();
    let mut work: Vec<usize> = probed.iter().copied().collect();
    while let Some(index) = work.pop() {
        for (other, candidate) in candidates.iter().enumerate() {
            if !probed.contains(&other) && candidates[index].calls.contains(&candidate.name) {
                probed.insert(other);
                work.push(other);
            }
        }
    }
    let mut out: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    for index in probed {
        let candidate = &candidates[index];
        out.entry(candidate.module_idx)
            .or_default()
            .insert(candidate.name.clone());
    }
    out
}

fn has_propagating_lambda(body: &mut [crate::AST::Stmt]) -> bool {
    let mut found = false;
    for stmt in body {
        stmt.for_each_expr_mut(|expr| {
            if let Expr::Lambda(lambda) = expr {
                found |= lambda.meta.fallible_propagation;
            }
        });
        if found {
            break;
        }
    }
    found
}

/// Check a clone of each selected function with the probe enabled. The
/// clone, its diagnostics and every other analysis product are discarded;
/// only the failure row survives.
#[allow(clippy::too_many_arguments)]
fn probe_functions(
    bundle: &ProgramBundle,
    states: &[ModuleState],
    probed: &BTreeMap<usize, BTreeSet<String>>,
    plugin_interfaces: &PluginInterfaceRegistry,
    devtools_registry: &jet_foundation::AST::DevtoolsRegistry,
    effect_facts: &jet_foundation::Facts::FactRegistry,
    no_os: bool,
    gates: crate::Policy::GateSet,
    name_ledger: &jet_foundation::Names::NameLedger,
) -> BTreeMap<(usize, String), (FailureUnionProbe, ProbeOwner)> {
    let mut probes = BTreeMap::new();
    let ct_checked_funcs = HashMap::new();
    for (&module_idx, names) in probed {
        let module = &bundle.modules[module_idx];
        let (ct_funcs, ct_externs, ct_globals) = comptime_context_from_items(&module.items);
        let ct_base_dir = module
            .path
            .parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        for item in &module.items {
            let Item::Func(function) = item else {
                continue;
            };
            if !names.contains(&function.name) {
                continue;
            }
            let mut clone = function.clone();
            let mut ledger = name_ledger.body_snapshot();
            let mut checker = checker_for_module(
                module_idx,
                states,
                plugin_interfaces,
                devtools_registry,
                effect_facts,
                &ct_funcs,
                &ct_checked_funcs,
                states[module_idx].items.as_slice(),
                &ct_externs,
                &ct_base_dir,
                &ct_globals,
                no_os,
                gates,
                module.no_prelude,
                &mut ledger,
                Some(&clone),
                false,
                true,
            );
            checker.canonicalize_function_return_type(&mut clone);
            checker.ret = crate::Sema::checked_body_return_type(&clone, false);
            checker.failure_union_probe = Some(FailureUnionProbe::default());
            checker.check_params_and_body(&mut clone, None);
            let mut probe = checker.failure_union_probe.take().unwrap_or_default();
            probe.general |= checker.failure_direct_source;
            drop(checker);
            probe.general |= has_propagating_lambda(&mut clone.body);
            probes.insert(
                (module_idx, function.name.clone()),
                (
                    probe,
                    ProbeOwner {
                        span: function.span,
                        name_span: function.name_span,
                        is_pub: function.is_pub,
                        is_package_pub: function.is_package_pub,
                    },
                ),
            );
        }
    }
    probes
}

/// `find` gives `FindError`; `parse_age` gives `ParseAgeError`; known
/// acronyms keep their capitals (`read_http` gives `ReadHTTPError`).
fn owned_error_type_name(function: &str) -> String {
    format!("{}Error", Syntax::to_pascal_acronym(function))
}

fn case_payload(case: &FailureCase) -> crate::AST::VariantPayload {
    match case.payload.as_slice() {
        [] => crate::AST::VariantPayload::Unit,
        [(None, ty)] => crate::AST::VariantPayload::Single(ty.clone(), case.span),
        fields => crate::AST::VariantPayload::Named(
            fields
                .iter()
                .enumerate()
                .map(|(index, (label, ty))| crate::AST::VariantField {
                    name: label
                        .clone()
                        .unwrap_or_else(|| format!("value{}", index + 1)),
                    name_span: case.span,
                    ty: ty.clone(),
                    ty_span: case.span,
                })
                .collect(),
        ),
    }
}

/// D-ERR-CASES1=A: mint each function's own closed `#Error enum` from the
/// cases its body creates, register it in the owning module and add it to
/// the module's items so every tier emits it. Returns the minted type per
/// function.
fn mint_case_types(
    bundle: &mut ProgramBundle,
    states: &mut [ModuleState],
    probes: &BTreeMap<(usize, String), (FailureUnionProbe, ProbeOwner)>,
    diags: &mut Vec<Diagnostic>,
) -> HashMap<(usize, String), Type> {
    let mut minted = HashMap::new();
    for ((module_idx, name), (probe, function)) in probes {
        if probe.cases.is_empty() {
            continue;
        }
        let type_name = owned_error_type_name(name);
        let state = &mut states[*module_idx];
        if state.registry.contains(&type_name) {
            diags.push(
                Diagnostic::error(
                    "E0105",
                    format!(
                        "`{name}` creates error cases, but the type `{type_name}` already exists"
                    ),
                    format!(
                        "cases written as `Err(.Case(…))` in `{name}` form its own error type, named `{type_name}` after the function"
                    ),
                    format!(
                        "write the failure contract of `{name}` (for example `-> T {type_name}!`), or rename the declared `{type_name}`"
                    ),
                    Some(probe.cases[0].span),
                )
                .with_origin(module_origin(bundle, *module_idx)),
            );
            continue;
        }
        let mut variants: Vec<crate::AST::Variant> = Vec::new();
        for case in &probe.cases {
            let payload = case_payload(case);
            if let Some(existing) = variants.iter().find(|variant| variant.name == case.name) {
                if !same_payload(&existing.payload, &payload) {
                    diags.push(
                        Diagnostic::error(
                            "E0105",
                            format!(
                                "case `.{}` of `{type_name}` is created with two different payloads",
                                case.name
                            ),
                            format!(
                                "every `Err(.{}(…))` in `{name}` adds to the one type `{type_name}`, so each case has one payload",
                                case.name
                            ),
                            format!(
                                "give the two failures different case names, or pass the same payload types to `.{}`",
                                case.name
                            ),
                            Some(case.span),
                        )
                        .with_origin(module_origin(bundle, *module_idx)),
                    );
                }
                continue;
            }
            variants.push(crate::AST::Variant {
                name: case.name.clone(),
                name_span: case.span,
                payload,
                discriminant: None,
                discriminant_expr: None,
                serde_markers: Vec::new(),
            });
        }
        let marker_span = function.name_span;
        let definition = crate::AST::EnumDef {
            span: function.span,
            is_pub: function.is_pub,
            is_package_pub: function.is_package_pub,
            name: type_name.clone(),
            name_span: function.name_span,
            type_params: Vec::new(),
            variants,
            methods: Vec::new(),
            trait_impls: Vec::new(),
            derives: Vec::new(),
            auto_derive_default: false,
            is_single_use: false,
            single_use_span: None,
            is_must_use: false,
            must_use_span: None,
            serde_markers: Vec::new(),
            type_markers: vec![crate::AST::Marker {
                name: Syntax::MARKER_ERROR.to_string(),
                negated: false,
                name_span: marker_span,
                args: Vec::new(),
                arg_labels: Vec::new(),
                span: marker_span,
                ct: None,
            }],
            groups: Vec::new(),
        };
        let before = diags.len();
        register_enum(
            &definition,
            &mut state.registry,
            diags,
            &state.funcs,
            &state.consts,
        );
        if diags.len() != before {
            continue;
        }
        state.items.push(Item::Enum(definition.clone()));
        bundle.modules[*module_idx].items.push(Item::Enum(definition));
        minted.insert((*module_idx, name.clone()), Type::Named(type_name));
    }
    minted
}

fn same_payload(left: &crate::AST::VariantPayload, right: &crate::AST::VariantPayload) -> bool {
    use crate::AST::VariantPayload;
    match (left, right) {
        (VariantPayload::Unit, VariantPayload::Unit) => true,
        (VariantPayload::Single(left, _), VariantPayload::Single(right, _)) => left == right,
        (VariantPayload::Named(left), VariantPayload::Named(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| left.name == right.name && left.ty == right.ty)
        }
        _ => false,
    }
}

/// Least fixed point of `row(f) = own(f) ∪ row(g)` over the probed call
/// edges. An edge to an unannotated callee that was not probed counts as the
/// general `Err`. Returns the inferred error type of every function whose
/// row holds a typed member.
fn join_failure_rows(
    probes: &BTreeMap<(usize, String), (FailureUnionProbe, ProbeOwner)>,
    minted: &HashMap<(usize, String), Type>,
) -> Vec<((usize, String), Type)> {
    let mut rows: BTreeMap<(usize, String), (Vec<Type>, bool)> = probes
        .iter()
        .map(|(key, (probe, _))| {
            let mut members = probe.members.clone();
            if let Some(owned) = minted.get(key) {
                members.push(owned.clone());
            }
            (key.clone(), (members, probe.general))
        })
        .collect();
    let mut settled = false;
    for _ in 0..JOIN_ROUND_CAP {
        let mut changed = false;
        for (key, (probe, _)) in probes {
            for edge in &probe.edges {
                if edge == key {
                    continue;
                }
                let (incoming, incoming_general) = match rows.get(edge) {
                    Some((members, general)) => (members.clone(), *general),
                    None => (Vec::new(), true),
                };
                let row = rows.get_mut(key).expect("every probed function has a row");
                for member in incoming {
                    if !row.0.contains(&member) {
                        row.0.push(member);
                        changed = true;
                    }
                }
                if incoming_general && !row.1 {
                    row.1 = true;
                    changed = true;
                }
            }
        }
        if !changed {
            settled = true;
            break;
        }
    }
    let mut solved = Vec::new();
    for (key, (mut members, general)) in rows {
        if !members.iter().any(typed_failure) {
            continue;
        }
        if general || !settled {
            members.push(Type::Named(Syntax::TYPE_ERR.to_string()));
        }
        solved.push((key, crate::AST::canonicalize_union(members)));
    }
    solved
}

/// Project the inferred set onto the declaration, its registry signature
/// and the checker's copy of the module items: `T (A | B)!` with no written
/// span, which `Func::failure_contract` reports as `Inferred`.
fn project_inferred_contract(
    bundle: &mut ProgramBundle,
    states: &mut [ModuleState],
    module_idx: usize,
    name: &str,
    error: Type,
) {
    let project = |function: &mut Func| -> bool {
        let success = match function.failure_contract() {
            crate::AST::FailureContract::Default { success, .. } => success,
            _ => return false,
        };
        function.return_type = Some(Type::Result {
            ok: Box::new(success),
            err: Box::new(error.clone()),
        });
        function.return_type_span = None;
        true
    };
    let mut projected = None;
    for item in &mut bundle.modules[module_idx].items {
        if let Item::Func(function) = item {
            if function.name == name && project(function) {
                projected = function.return_type.clone();
            }
        }
    }
    let Some(return_type) = projected else {
        return;
    };
    let state = &mut states[module_idx];
    for item in &mut state.items {
        if let Item::Func(function) = item {
            if function.name == name {
                project(function);
            }
        }
    }
    if let Some(signature) = state.funcs.get_mut(name) {
        signature.return_type = Some(return_type);
    }
}
